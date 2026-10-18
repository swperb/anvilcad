//! Translates a SOLIDWORKS part, feature by feature, into an AnvilCAD document.
//!
//! Every translated feature is checked against SOLIDWORKS: the rollback bar is moved to
//! just after the feature and SOLIDWORKS' exact volume is compared with AnvilCAD's rebuilt
//! volume. Features that cannot be translated, or whose translation does not reproduce the
//! SOLIDWORKS result, are carried over as SOLIDWORKS' own geometry in an imported body, and
//! translation continues parametrically after it.

use crate::com::{Arg, ComError, ComResult, Disp};
use crate::report::{FileReport, FileStatus, Outcome};
use crate::sw::*;
use anvil_kernel::{v2, Frame, Vec2, Vec3};
use anvil_model::*;
use anvil_sketch::{Constraint, Geom, Sketch, ORIGIN};
use std::collections::HashMap;

pub struct Options {
    /// Verify each feature against SOLIDWORKS (needs rollback; slower but trustworthy).
    pub verify_each: bool,
    /// Tessellate imported geometry at 0.01 mm (accurate but slow over COM).
    pub precise_geometry: bool,
}

/// Relative volume tolerance for a translation to count as matching.
const VOLUME_TOL: f64 = 0.01;

/// Feature types that carry no geometry of their own.
const STRUCTURAL: &[&str] = &[
    "FavoriteFolder",
    "HistoryFolder",
    "SelectionSetFolder",
    "SensorFolder",
    "DocsFolder",
    "DetailCabinet",
    "InkMarkupFolder",
    "EnvFolder",
    "SolidBodyFolder",
    "SurfaceBodyFolder",
    "CommentsFolder",
    "EqnFolder",
    "MaterialFolder",
    "OriginProfileFeature",
    "ConfigTableFolder",
    "MateReferenceGroupFolder",
    "FtrFolder",
    "BlockFolder",
    "CutListFolder",
    "SubWeldFolder",
];

const REFERENCE: &[&str] = &["RefAxis", "RefPoint", "CoordSys"];

#[derive(Clone, Copy, Debug)]
enum DimTarget {
    Sketch { feature: u32, constraint: usize },
    ExtrudeDepth(u32),
    RevolveAngle(u32),
    PatternSpacing(u32),
    PatternCount(u32),
}

struct SketchInfo {
    id: u32,
    /// SOLIDWORKS segment id -> AnvilCAD entity index.
    segments: HashMap<(i64, i64), usize>,
}

/// Reference to a sketch item from a SOLIDWORKS relation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ref {
    Point(usize),
    Line(usize),
    Round(usize),
    External,
}

struct Ctx<'a> {
    session: &'a Session,
    model: Disp,
    can_rollback: bool,
    verify_each: bool,
    precise_geometry: bool,
    doc: Document,
    st: BuildState,
    report: FileReport,
    sketches: HashMap<String, SketchInfo>,
    solids: HashMap<String, u32>,
    planes: HashMap<String, PlaneRef>,
    dims: HashMap<String, DimTarget>,
    /// Unsupported geometric features since the last imported body: (name, type, reason).
    pending: Vec<(String, String, String)>,
    default_planes: usize,
    units_mm: bool,
    /// A note produced while building candidates (for example a converted end condition).
    note: std::cell::RefCell<Option<String>>,
}

fn err(e: ComError) -> String {
    e.0
}

fn volume_matches(ours: f64, theirs: f64) -> bool {
    let scale = theirs.abs().max(ours.abs()).max(1e-3);
    (ours - theirs).abs() / scale <= VOLUME_TOL
}

pub fn migrate_part(session: &Session, path: &str, opts: &Options) -> (Option<Document>, FileReport) {
    let start = std::time::Instant::now();
    let mut report = FileReport::new(path);
    let open = match session.open_part(path) {
        Ok(o) => o,
        Err(e) => {
            report.warnings.push(e.0);
            report.seconds = start.elapsed().as_secs_f64();
            return (None, report);
        }
    };
    let mut ctx = Ctx {
        session,
        model: open.model.clone(),
        can_rollback: open.opened_by_us,
        verify_each: opts.verify_each && open.opened_by_us,
        precise_geometry: opts.precise_geometry,
        doc: Document::new(),
        st: BuildState::default(),
        report,
        sketches: HashMap::new(),
        solids: HashMap::new(),
        planes: HashMap::new(),
        dims: HashMap::new(),
        pending: Vec::new(),
        default_planes: 0,
        units_mm: true,
        note: Default::default(),
    };
    if !open.opened_by_us {
        ctx.report.warnings.push(
            "The file was already open in SOLIDWORKS, so per-feature verification was skipped to \
             leave your session untouched."
                .into(),
        );
    }
    let result = ctx.run();
    if ctx.can_rollback {
        ctx.rollback_to_end();
    }
    session.close(&open);
    let mut report = ctx.report;
    report.seconds = start.elapsed().as_secs_f64();
    match result {
        Ok(()) => {
            report.status =
                if report.count(Outcome::Imported) > 0 { FileStatus::Hybrid } else { FileStatus::Parametric };
            (Some(ctx.doc), report)
        }
        Err(e) => {
            report.status = FileStatus::Failed;
            report.warnings.push(e);
            (None, report)
        }
    }
}

impl Ctx<'_> {
    fn run(&mut self) -> Result<(), String> {
        let unit = self.model.i64("LengthUnit", &[]).unwrap_or(0);
        self.units_mm = unit == 0;
        self.read_properties();

        let mut feat = self.model.opt_obj("FirstFeature", &[]).map_err(err)?;
        while let Some(f) = feat {
            let ty = f.string("GetTypeName2", &[]).map_err(err)?;
            let name = f.string("Name", &[]).map_err(err)?;
            let suppressed = f.bool("IsSuppressed", &[]).unwrap_or(false);
            self.handle(&f, &ty, &name, suppressed)?;
            feat = f.opt_obj("GetNextFeature", &[]).map_err(err)?;
        }
        if !self.pending.is_empty() {
            self.flush(false)?;
        }
        self.read_equations();
        self.rebuild();
        self.verify_final();
        Ok(())
    }

    fn handle(&mut self, f: &Disp, ty: &str, name: &str, suppressed: bool) -> Result<(), String> {
        if STRUCTURAL.contains(&ty) || ty.ends_with("Folder") {
            return Ok(());
        }
        if ty == "RefPlane" {
            return self.ref_plane(f, name);
        }
        if REFERENCE.contains(&ty) {
            self.report.add(
                name,
                ty,
                Outcome::Skipped,
                "reference geometry, used only to place other features",
            );
            return Ok(());
        }
        if suppressed {
            self.report.add(name, ty, Outcome::Skipped, "suppressed in SOLIDWORKS");
            return Ok(());
        }
        match ty {
            "ProfileFeature" => {
                if !self.pending.is_empty() {
                    self.flush(true)?;
                }
                match self.sketch(f, name) {
                    Ok(detail) => self.report.add(name, ty, Outcome::Parametric, detail),
                    Err(e) => self.report.add(name, ty, Outcome::Skipped, e),
                }
            }
            "3DProfileFeature" => {
                self.report.add(name, ty, Outcome::Skipped, "3D sketches are not supported yet")
            }
            "Extrusion" | "Boss" | "Cut" | "ICE" | "Revolution" | "RevCut" | "LPattern" | "CirPattern"
            | "Fillet" | "Chamfer" | "Shell" => {
                if !self.pending.is_empty() {
                    if !self.can_rollback {
                        self.pending.push((name.into(), ty.into(), "follows imported geometry".into()));
                        return Ok(());
                    }
                    self.flush(true)?;
                }
                match self.solid_feature(f, ty, name) {
                    Ok(detail) => self.report.add(name, ty, Outcome::Parametric, detail),
                    Err(reason) => self.pending.push((name.into(), ty.into(), reason)),
                }
            }
            _ => {
                self.pending.push((name.into(), ty.into(), format!("'{ty}' features are not supported yet")))
            }
        }
        Ok(())
    }

    fn rebuild(&mut self) {
        self.st = self.doc.rebuild();
    }

    fn rollback_after(&self, feature: &str) -> ComResult<()> {
        let fm = self.model.obj("FeatureManager", &[])?;
        fm.call("EditRollback", &[SW_ROLLBACK_AFTER_FEATURE.into(), feature.into()])?;
        Ok(())
    }

    fn rollback_to_end(&self) {
        if let Ok(fm) = self.model.obj("FeatureManager", &[]) {
            let _ = fm.call("EditRollback", &[SW_ROLLBACK_TO_END.into(), "".into()]);
        }
    }

    // ----- properties -----------------------------------------------------------------

    fn read_properties(&mut self) {
        // Custom properties (file level).
        let props = (|| -> ComResult<Vec<CustomProperty>> {
            let ext = self.model.obj("Extension", &[])?;
            let mgr = ext.obj("CustomPropertyManager", &["".into()])?;
            let mut out = Vec::new();
            for n in mgr.call("GetNames", &[])?.list() {
                let Some(name) = n.str() else { continue };
                let (_, outs) = mgr.call_out(
                    "Get6",
                    &[name.into(), false.into(), Arg::OutStr, Arg::OutStr, Arg::OutBool, Arg::OutBool],
                )?;
                let raw = outs.first().and_then(|v| v.str()).unwrap_or("").to_string();
                let resolved = outs.get(1).and_then(|v| v.str()).unwrap_or("").to_string();
                let kind = match mgr.i64("GetType2", &[name.into()]).unwrap_or(30) {
                    3 | 5 => PropertyKind::Number,
                    11 => PropertyKind::YesNo,
                    64 => PropertyKind::Date,
                    _ => PropertyKind::Text,
                };
                if raw != resolved && !raw.is_empty() {
                    self.report.warnings.push(format!(
                        "Property '{name}' is linked in SOLIDWORKS ({raw}); its current value '{resolved}' was copied."
                    ));
                }
                out.push(CustomProperty { name: name.to_string(), kind, value: resolved });
            }
            Ok(out)
        })();
        match props {
            Ok(p) => self.doc.custom_properties = p,
            Err(e) => self.report.warnings.push(format!("custom properties: {e}")),
        }

        // Material and density.
        if let Ok((name, _)) = self.model.call_out("GetMaterialPropertyName2", &["".into(), Arg::OutStr]) {
            if let Some(n) = name.str().filter(|n| !n.is_empty()) {
                let density = sw_density(&self.model).unwrap_or(1.0);
                self.doc.material = Some(Material::simple(n, density).resolved());
            }
        }
    }

    fn read_equations(&mut self) {
        let Ok(em) = self.model.obj("GetEquationMgr", &[]) else { return };
        let n = em.i64("GetCount", &[]).unwrap_or(0);
        let mut bindings = Vec::new();
        for i in 0..n as i32 {
            let Ok(eq) = em.string("Equation", &[i.into()]) else { continue };
            if em.bool("Suppression", &[i.into()]).unwrap_or(false) {
                continue;
            }
            let Some((lhs, rhs)) = split_equation(&eq) else { continue };
            let global = em.bool("GlobalVariable", &[i.into()]).unwrap_or(false);
            if !self.units_mm {
                self.report.warnings.push(format!(
                    "Equation {eq} kept as a plain value: the part's units are not millimetres."
                ));
                continue;
            }
            match convert_expression(&rhs) {
                Some(expr) if global => {
                    self.doc.variables.push(GlobalVariable { name: lhs, expr, comment: String::new() })
                }
                Some(expr) => bindings.push((lhs, expr, eq)),
                None => self.report.warnings.push(format!(
                    "Equation {eq} uses functions or references AnvilCAD cannot express yet; kept as a value."
                )),
            }
        }
        let (vars, errs) = self.doc.evaluate_variables();
        for (name, e) in errs {
            self.report.warnings.push(format!("Global variable '{name}' could not be evaluated: {e}"));
        }
        for (lhs, expr, eq) in bindings {
            let Some(target) = self.dims.get(&lhs).copied() else {
                self.report.warnings.push(format!(
                    "Equation {eq} drives a dimension that was not carried over; kept as a value."
                ));
                continue;
            };
            if Dim::Expr(expr.clone()).eval(&vars).is_err() {
                self.report.warnings.push(format!("Equation {eq} could not be evaluated; kept as a value."));
                continue;
            }
            let set = |f: &mut Feature| match (target, &mut f.kind) {
                (DimTarget::Sketch { constraint, .. }, FeatureKind::Sketch(s)) => {
                    s.set_driven(constraint, Some(expr.clone()))
                }
                (DimTarget::ExtrudeDepth(_), FeatureKind::Extrude(e)) => e.depth = Dim::Expr(expr.clone()),
                (DimTarget::RevolveAngle(_), FeatureKind::Revolve(r)) => r.angle = Dim::Expr(expr.clone()),
                (DimTarget::PatternSpacing(_), FeatureKind::LinearPattern(p)) => {
                    p.spacing = Dim::Expr(expr.clone())
                }
                (DimTarget::PatternCount(_), FeatureKind::LinearPattern(p)) => {
                    p.count = Dim::Expr(expr.clone())
                }
                (DimTarget::PatternCount(_), FeatureKind::CircularPattern(p)) => {
                    p.count = Dim::Expr(expr.clone())
                }
                _ => {}
            };
            let id = match target {
                DimTarget::Sketch { feature, .. } => feature,
                DimTarget::ExtrudeDepth(i)
                | DimTarget::RevolveAngle(i)
                | DimTarget::PatternSpacing(i)
                | DimTarget::PatternCount(i) => i,
            };
            if let Some(f) = self.doc.feature_mut(id) {
                set(f);
            }
        }
    }

    // ----- planes and sketches -----------------------------------------------------

    fn ref_plane(&mut self, f: &Disp, name: &str) -> Result<(), String> {
        let frame = f
            .obj("GetSpecificFeature2", &[])
            .and_then(|p| p.obj("Transform", &[]))
            .and_then(|t| t.call("ArrayData", &[]))
            .map_err(err)
            .map(|d| local_to_model(&d.floats()))?
            .ok_or("could not read the plane's position")?;
        if self.default_planes < 3 {
            // The first three planes are SOLIDWORKS' Front, Top and Right.
            self.default_planes += 1;
            let n = frame.z;
            let std = if n.z.abs() > 0.99 {
                PlaneRef::Front
            } else if n.y.abs() > 0.99 {
                PlaneRef::Top
            } else {
                PlaneRef::Right
            };
            self.planes.insert(name.to_string(), std);
            return Ok(());
        }
        let id = self.doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Fixed(frame),
            offset: Dim::Value(0.0),
            flip: false,
        }));
        self.name_feature(
            id,
            name,
            Some("Placed at its SOLIDWORKS position (not linked to its references)."),
        );
        self.planes.insert(name.to_string(), PlaneRef::Feature(id));
        self.rebuild();
        self.report.add(name, "RefPlane", Outcome::Parametric, "fixed placement");
        Ok(())
    }

    fn name_feature(&mut self, id: u32, name: &str, note: Option<&str>) {
        if let Some(f) = self.doc.feature_mut(id) {
            f.name = name.to_string();
            f.note = note.map(|n| n.to_string());
        }
    }

    /// Chooses the most associative reference for a sketch frame.
    fn resolve_plane(&self, frame: &Frame) -> PlaneRef {
        for p in [PlaneRef::Front, PlaneRef::Top, PlaneRef::Right] {
            if frames_equal(&p.standard_frame().unwrap(), frame) {
                return p;
            }
        }
        for p in self.planes.values() {
            if let PlaneRef::Feature(id) = p {
                if self.st.frames.get(id).is_some_and(|f| frames_equal(f, frame)) {
                    return *p;
                }
            }
        }
        for fid in self.st.body.face_ids() {
            if self.st.body.planar_face_frame(fid).is_some_and(|f| frames_equal(&f, frame)) {
                return PlaneRef::Face(fid);
            }
        }
        PlaneRef::Fixed(*frame)
    }

    fn sketch(&mut self, f: &Disp, name: &str) -> Result<String, String> {
        let sk = f.obj("GetSpecificFeature2", &[]).map_err(err)?;
        if sk.bool("Is3D", &[]).unwrap_or(false) {
            return Err("3D sketches are not supported yet".into());
        }
        let frame = sk
            .obj("ModelToSketchTransform", &[])
            .and_then(|t| t.call("ArrayData", &[]))
            .map_err(err)
            .map(|d| model_to_local(&d.floats()))?
            .ok_or("could not read the sketch plane")?;
        let plane = self.resolve_plane(&frame);

        let mut s = Sketch::new();
        let mut points: HashMap<(i64, i64), usize> = HashMap::new();
        let mut segments: HashMap<(i64, i64), usize> = HashMap::new();
        let mut notes: Vec<String> = Vec::new();

        let pos =
            |p: &Disp| -> ComResult<Vec2> { Ok(v2(p.f64("X", &[])? * M_TO_MM, p.f64("Y", &[])? * M_TO_MM)) };
        let mut point = |s: &mut Sketch, p: &Disp| -> ComResult<usize> {
            let key = id_key(p)?;
            if let Some(&i) = points.get(&key) {
                return Ok(i);
            }
            let i = s.add_point(pos(p)?);
            points.insert(key, i);
            Ok(i)
        };

        let mut unsupported_segments = 0;
        for seg in sk.call("GetSketchSegments", &[]).map_err(err)?.objects() {
            let kind = seg.i64("GetType", &[]).map_err(err)?;
            let construction = seg.bool("ConstructionGeometry", &[]).unwrap_or(false);
            let entity = match kind {
                0 => {
                    let a = point(&mut s, &seg.obj("GetStartPoint2", &[]).map_err(err)?).map_err(err)?;
                    let b = point(&mut s, &seg.obj("GetEndPoint2", &[]).map_err(err)?).map_err(err)?;
                    Some(s.add_line(a, b))
                }
                1 => {
                    let c = point(&mut s, &seg.obj("GetCenterPoint2", &[]).map_err(err)?).map_err(err)?;
                    if seg.i64("IsCircle", &[]).unwrap_or(0) != 0 {
                        let r = seg.f64("GetRadius", &[]).map_err(err)? * M_TO_MM;
                        Some(s.add_circle(c, r))
                    } else {
                        let a = point(&mut s, &seg.obj("GetStartPoint2", &[]).map_err(err)?).map_err(err)?;
                        let b = point(&mut s, &seg.obj("GetEndPoint2", &[]).map_err(err)?).map_err(err)?;
                        // AnvilCAD arcs run counter-clockwise from their first point.
                        if seg.i64("GetRotationDir", &[]).unwrap_or(1) < 0 {
                            Some(s.add_arc(c, b, a))
                        } else {
                            Some(s.add_arc(c, a, b))
                        }
                    }
                }
                _ => {
                    unsupported_segments += 1;
                    None
                }
            };
            if let Some(e) = entity {
                s.entities[e].construction = construction;
                segments.insert(id_key(&seg).map_err(err)?, e);
            }
        }
        if unsupported_segments > 0 {
            notes.push(format!("{unsupported_segments} ellipse/spline/text segment(s) not carried over"));
        }

        // Relations and dimensions.
        let id = self.doc.next_id; // the id this sketch will get
        let mut skipped_relations = 0;
        let mut dims: Vec<(String, usize)> = Vec::new();
        if let Ok(rm) = sk.obj("RelationManager", &[]) {
            for rel in rm.call("GetRelations", &[0.into()]).map_err(err)?.objects() {
                let ty = rel.i64("GetRelationType", &[]).unwrap_or(0);
                let ents = rel.call("GetEntities", &[]).map(|v| v.objects()).unwrap_or_default();
                let types: Vec<i64> = rel
                    .call("GetEntitiesType", &[])
                    .map(|v| v.list())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|v| v.i64())
                    .collect();
                let refs: Vec<Ref> = ents
                    .iter()
                    .zip(types.iter().chain(std::iter::repeat(&0)))
                    .map(|(e, &t)| resolve_ref(&s, e, t, &points, &segments))
                    .collect();
                match relation(&s, ty, &refs) {
                    Some(cs) if !cs.is_empty() => {
                        for c in cs {
                            s.add_constraint(c);
                        }
                    }
                    _ if matches!(ty, 1 | 2 | 3 | 15) => match dimension(&s, &rel, ty, &refs) {
                        Ok(Some((c, full))) => {
                            let ci = s.add_constraint(c);
                            dims.push((full, ci));
                        }
                        Ok(None) => {}
                        Err(_) => skipped_relations += 1,
                    },
                    _ => skipped_relations += 1,
                }
            }
        }

        // The geometry is already solved in SOLIDWORKS; drop anything we mapped wrongly.
        let mut res = s.solve();
        let mut dropped = 0;
        for _ in 0..8 {
            if res.converged {
                break;
            }
            let mut bad = res.unsatisfied.clone();
            bad.sort_unstable();
            bad.dedup();
            for ci in bad.into_iter().rev() {
                if ci != 0 {
                    s.remove_constraint(ci);
                    dims.retain(|(_, c)| *c != ci);
                    for d in &mut dims {
                        if d.1 > ci {
                            d.1 -= 1;
                        }
                    }
                    dropped += 1;
                }
            }
            res = s.solve();
        }
        if skipped_relations > 0 {
            notes.push(format!(
                "{skipped_relations} relation(s) to external or unsupported geometry not carried over"
            ));
        }
        if dropped > 0 {
            notes.push(format!("{dropped} relation(s) dropped as inconsistent"));
        }

        let sf = SketchFeature { plane, sketch: s, driven: Vec::new() };
        let new_id = self.doc.add_feature(FeatureKind::Sketch(sf));
        debug_assert_eq!(new_id, id);
        let note = matches!(plane, PlaneRef::Fixed(_)).then_some(
            "Placed at its SOLIDWORKS position (the face or plane it was drawn on was not carried over).",
        );
        self.name_feature(new_id, name, note);
        for (full, ci) in dims {
            self.dims.insert(full, DimTarget::Sketch { feature: new_id, constraint: ci });
        }
        self.sketches.insert(name.to_string(), SketchInfo { id: new_id, segments });
        // Later features read this sketch's plane from the build state.
        self.rebuild();
        let mut detail = format!("{} DOF", res.dof);
        if res.dof == 0 {
            detail = "fully defined".into();
        }
        if !notes.is_empty() {
            detail = format!("{detail}; {}", notes.join("; "));
        }
        Ok(detail)
    }

    // ----- solid features ------------------------------------------------------------

    /// The sketch a feature consumes (its absorbed sub-feature).
    fn consumed_sketch(&self, f: &Disp) -> Result<&SketchInfo, String> {
        let mut sub = f.opt_obj("GetFirstSubFeature", &[]).map_err(err)?;
        while let Some(s) = sub {
            if s.string("GetTypeName2", &[]).unwrap_or_default() == "ProfileFeature" {
                let n = s.string("Name", &[]).map_err(err)?;
                return self.sketches.get(&n).ok_or_else(|| format!("its sketch '{n}' was not carried over"));
            }
            sub = s.opt_obj("GetNextSubFeature", &[]).map_err(err)?;
        }
        Err("it has no sketch".into())
    }

    fn seed_ids(&self, data: &Disp) -> Result<Vec<u32>, String> {
        let seeds = data.call("PatternFeatureArray", &[]).map_err(err)?.objects();
        if seeds.is_empty() {
            return Err("only feature patterns are supported (not face or body patterns)".into());
        }
        seeds
            .iter()
            .map(|s| {
                let n = s.string("Name", &[]).map_err(err)?;
                self.solids.get(&n).copied().ok_or_else(|| format!("seed feature '{n}' was not carried over"))
            })
            .collect()
    }

    /// Builds candidate translations; the first that reproduces SOLIDWORKS wins.
    fn candidates(&self, f: &Disp, ty: &str) -> Result<Vec<FeatureKind>, String> {
        let data = f.obj("GetDefinition", &[]).map_err(err)?;
        match ty {
            "Extrusion" | "Boss" | "Cut" | "ICE" => {
                // "ICE" extrusions can be bosses or cuts; the API does not say which, so both
                // are tried (cut first when the name suggests it) and verification decides.
                let op = match ty {
                    "Cut" => BodyOp::Cut,
                    "ICE" if name_hint_cut(f) => BodyOp::Cut,
                    _ => BodyOp::Boss,
                };
                let ops = if ty == "ICE" {
                    vec![op, if op == BodyOp::Cut { BodyOp::Boss } else { BodyOp::Cut }]
                } else {
                    vec![op]
                };
                if data.bool("IsThinFeature", &[]).unwrap_or(false) {
                    return Err("thin features are not supported yet".into());
                }
                if data.bool("GetDraftWhileExtruding", &[true.into()]).unwrap_or(false) {
                    return Err("drafted extrusions are not supported yet".into());
                }
                if data.i64("FromType", &[]).unwrap_or(0) != 0 {
                    return Err("extrusions starting away from the sketch plane are not supported yet".into());
                }
                if op == BodyOp::Cut && data.bool("FlipSideToCut", &[]).unwrap_or(false) {
                    return Err("'flip side to cut' is not supported yet".into());
                }
                if op == BodyOp::Boss && !data.bool("Merge", &[]).unwrap_or(true) {
                    return Err("multibody extrusions (merge result off) are not supported yet".into());
                }
                let sketch = self.consumed_sketch(f)?.id;
                let both = data.bool("BothDirections", &[]).unwrap_or(false);
                let e1 = data.i64("GetEndCondition", &[true.into()]).map_err(err)?;
                let e2 = data.i64("GetEndCondition", &[false.into()]).unwrap_or(0);
                let d1 = data.f64("GetDepth", &[true.into()]).map_err(err)? * M_TO_MM;
                let d2 = data.f64("GetDepth", &[false.into()]).unwrap_or(0.0) * M_TO_MM;
                let (end, depth) = match (e1, both, e2) {
                    (0, false, _) => (EndCondition::Blind, d1),
                    (6, _, _) => (EndCondition::MidPlane, d1),
                    (1, false, _) => (EndCondition::ThroughAll, 0.0),
                    (9, _, _) | (1, true, 1) => (EndCondition::ThroughAllBoth, 0.0),
                    (0, true, 0) if (d1 - d2).abs() < 1e-6 => (EndCondition::MidPlane, 2.0 * d1),
                    (2 | 3 | 4 | 5 | 7 | 10 | 11, false, _) => {
                        let d = self.measured_depth(f, sketch)?;
                        *self.note.borrow_mut() = Some(format!(
                            "{} converted to Blind {} mm (measured from the SOLIDWORKS result)",
                            end_condition_name(e1),
                            crate::export::fmt_mm(d)
                        ));
                        (EndCondition::Blind, d)
                    }
                    _ => return Err("this end condition is not supported yet".into()),
                };
                let reverse = data.bool("ReverseDirection", &[]).unwrap_or(false);
                let mut out = Vec::new();
                for op in ops {
                    for r in [reverse, !reverse] {
                        out.push(FeatureKind::Extrude(ExtrudeFeature {
                            sketch,
                            end,
                            depth: Dim::Value(depth),
                            reverse: r,
                            op,
                        }));
                    }
                }
                Ok(out)
            }
            "Revolution" | "RevCut" => {
                let op = if ty == "RevCut" { BodyOp::Cut } else { BodyOp::Boss };
                if data.bool("IsThinFeature", &[]).unwrap_or(false) {
                    return Err("thin features are not supported yet".into());
                }
                let kind = data.i64("Type", &[]).unwrap_or(0);
                let angle = match kind {
                    0 => data.f64("GetRevolutionAngle", &[true.into()]).map_err(err)?.to_degrees(),
                    3 => 360.0,
                    _ => return Err("mid-plane and two-direction revolves are not supported yet".into()),
                };
                let info = self.consumed_sketch(f)?;
                let axis = with_selections(&data, &self.model, || {
                    let ax = data.obj("Axis", &[]).map_err(err)?;
                    let key = id_key(&ax).map_err(|_| "the revolve axis is not a sketch line".to_string())?;
                    info.segments
                        .get(&key)
                        .copied()
                        .ok_or_else(|| "the revolve axis is not in its sketch".to_string())
                })?;
                let sketch = info.id;
                Ok([angle, -angle]
                    .into_iter()
                    .map(|a| FeatureKind::Revolve(RevolveFeature { sketch, axis, angle: Dim::Value(a), op }))
                    .collect())
            }
            "LPattern" => with_selections(&data, &self.model, || {
                let features = self.seed_ids(&data)?;
                if data.i64("D2TotalInstances", &[]).unwrap_or(1) > 1 {
                    return Err("two-direction linear patterns are not supported yet".into());
                }
                let ax = data.obj("D1Axis", &[]).map_err(err)?;
                let (origin, dir) = axis_of(&ax).ok_or("the pattern direction could not be read")?;
                let spacing = data.f64("D1Spacing", &[]).map_err(err)? * M_TO_MM;
                let count = data.i64("D1TotalInstances", &[]).map_err(err)? as f64;
                let sign = if data.bool("D1ReverseDirection", &[]).unwrap_or(false) { -1.0 } else { 1.0 };
                Ok([sign, -sign]
                    .into_iter()
                    .map(|s| {
                        FeatureKind::LinearPattern(LinearPatternFeature {
                            features: features.clone(),
                            direction: Axis::Line { origin, dir: dir * s },
                            spacing: Dim::Value(spacing),
                            count: Dim::Value(count),
                        })
                    })
                    .collect())
            }),
            "CirPattern" => with_selections(&data, &self.model, || {
                let features = self.seed_ids(&data)?;
                if data.bool("Direction2", &[]).unwrap_or(false) {
                    return Err("two-direction circular patterns are not supported yet".into());
                }
                let ax = data.obj("Axis", &[]).map_err(err)?;
                let (origin, dir) = axis_of(&ax).ok_or("the pattern axis could not be read")?;
                let spacing = data.f64("Spacing", &[]).map_err(err)?.to_degrees();
                let count = data.i64("TotalInstances", &[]).map_err(err)?.max(1) as f64;
                let equal = data.bool("EqualSpacing", &[]).unwrap_or(true);
                let total = if equal { spacing } else { spacing * (count - 1.0) };
                let mut totals = vec![total];
                if equal && (total - 360.0).abs() > 1e-9 && count > 1.0 {
                    // Some releases divide a partial angle by the instance count.
                    totals.push(total * (count - 1.0) / count);
                }
                let mut out = Vec::new();
                for t in totals {
                    for s in [1.0, -1.0] {
                        out.push(FeatureKind::CircularPattern(CircularPatternFeature {
                            features: features.clone(),
                            axis: Axis::Line { origin, dir: dir * s },
                            total_angle: Dim::Value(t),
                            count: Dim::Value(count),
                        }));
                    }
                }
                Ok(out)
            }),
            "Fillet" => with_selections(&data, &self.model, || {
                if data.i64("Type", &[]).unwrap_or(0) != 0 {
                    return Err("face, full-round and variable-radius fillets are not supported yet".into());
                }
                if data.bool("AsymmetricFillet", &[]).unwrap_or(false) {
                    return Err("asymmetric fillets are not supported yet".into());
                }
                if data.i64("ConicTypeForCrossSectionProfile", &[]).unwrap_or(0) != 0 {
                    return Err("conic and curvature-continuous fillets are not supported yet".into());
                }
                let radius = data.f64("DefaultRadius", &[]).map_err(err)? * M_TO_MM;
                // FilletItemsCount is -1 when the selection is a plain edge list.
                let n = data.i64("FilletItemsCount", &[]).unwrap_or(-1);
                let mut items: Vec<Disp> = (0..n.max(0) as i32)
                    .filter_map(|i| data.opt_obj("GetFilletItemAtIndex", &[i.into()]).ok().flatten())
                    .collect();
                if items.is_empty() {
                    items = data.call("Edges", &[]).map(|v| v.objects()).unwrap_or_default();
                }
                let edges = self.map_edges(&items)?;
                Ok(vec![FeatureKind::Fillet(FilletFeature { edges, radius: Dim::Value(radius) })])
            }),
            "Chamfer" => with_selections(&data, &self.model, || {
                let kind = data.i64("Type", &[]).unwrap_or(0);
                let d1 = data.f64("GetEdgeChamferDistance", &[0.into()]).unwrap_or(0.0) * M_TO_MM;
                let equal = match kind {
                    16 => true,
                    2 => data.bool("EqualDistance", &[]).unwrap_or(false),
                    1 => (data.f64("EdgeChamferAngle", &[]).unwrap_or(0.0).to_degrees() - 45.0).abs() < 1e-6,
                    _ => false,
                };
                if !equal || d1 <= 0.0 {
                    return Err("only equal-distance chamfers are supported yet".into());
                }
                let items = data.call("Edges", &[]).map_err(err)?.objects();
                let edges = self.map_edges(&items)?;
                Ok(vec![FeatureKind::Chamfer(ChamferFeature {
                    edges,
                    distance: Dim::Value(d1),
                    kind: ChamferKind::EqualDistance,
                })])
            }),
            "Shell" => with_selections(&data, &self.model, || {
                if data.i64("GetMultipleThicknessFacesCount", &[]).unwrap_or(0) > 0 {
                    return Err("multi-thickness shells are not supported yet".into());
                }
                let thickness = data.f64("Thickness", &[]).map_err(err)? * M_TO_MM;
                let faces = data.call("FacesRemoved", &[]).map(|v| v.objects()).unwrap_or_default();
                let mut remove = Vec::new();
                for f in &faces {
                    let p = face_point(f).ok_or("a removed face could not be read")?;
                    let id = self
                        .st
                        .body
                        .face_at(p, 0.1)
                        .ok_or("a removed face has no counterpart in the translated model")?;
                    if !remove.contains(&id) {
                        remove.push(id);
                    }
                }
                Ok(vec![FeatureKind::Shell(ShellFeature { remove, thickness: Dim::Value(thickness) })])
            }),
            _ => Err(format!("'{ty}' features are not supported yet")),
        }
    }

    /// Maps SOLIDWORKS edges (or every edge of selected faces) onto edges of the translated
    /// model, matching by position.
    fn map_edges(&self, items: &[Disp]) -> Result<Vec<anvil_kernel::EdgeRef>, String> {
        let mut sw_edges = Vec::new();
        for it in items {
            if it.call("GetTwoAdjacentFaces2", &[]).is_ok() {
                sw_edges.push(it.clone());
            } else if let Ok(es) = it.call("GetEdges", &[]) {
                sw_edges.extend(es.objects());
            } else {
                return Err("fillets on whole features are not supported yet".into());
            }
        }
        if sw_edges.is_empty() {
            return Err("no edges could be read".into());
        }
        let mut out = Vec::new();
        for e in &sw_edges {
            let p = edge_midpoint(e).ok_or("an edge could not be read")?;
            let r = self
                .st
                .body
                .edge_near(p, 0.05)
                .ok_or("an edge has no counterpart in the translated model")?;
            if !out.contains(&r) {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Depth of an extrusion measured from its planar end face in SOLIDWORKS, used to turn
    /// "up to" end conditions into a blind depth.
    fn measured_depth(&self, f: &Disp, sketch: u32) -> Result<f64, String> {
        let frame = self.st.frames.get(&sketch).copied().ok_or("its sketch plane is unknown")?;
        let mut best: Option<f64> = None;
        for face in f.call("GetFaces", &[]).map_err(err)?.objects() {
            let Ok(s) = face.obj("GetSurface", &[]) else { continue };
            if !s.bool("IsPlane", &[]).unwrap_or(false) {
                continue;
            }
            let p = s.call("PlaneParams", &[]).map(|v| v.floats()).unwrap_or_default();
            if p.len() < 6 {
                continue;
            }
            let n = Vec3 { x: p[0], y: p[1], z: p[2] };
            if n.cross(frame.z).length() > 1e-6 {
                continue;
            }
            let pt = Vec3 { x: p[3], y: p[4], z: p[5] } * M_TO_MM;
            let d = (pt - frame.origin).dot(frame.z);
            if d.abs() > 1e-6 && best.is_none_or(|b| d.abs() > b.abs()) {
                best = Some(d);
            }
        }
        best.map(f64::abs).ok_or_else(|| "its end face is not planar".to_string())
    }

    fn solid_feature(&mut self, f: &Disp, ty: &str, name: &str) -> Result<String, String> {
        // Roll SOLIDWORKS to just after this feature first, so its faces and volume are current.
        let target = if self.verify_each {
            self.rollback_after(name).map_err(err)?;
            Some(sw_volume(&self.model).map_err(err)?)
        } else {
            None
        };
        *self.note.borrow_mut() = None;
        let candidates = self.candidates(f, ty)?;
        let note = self.note.borrow_mut().take();
        let saved = self.doc.clone();
        let mut last_problem = String::from("it could not be rebuilt");
        for (i, kind) in candidates.into_iter().enumerate() {
            self.doc = saved.clone();
            let id = self.doc.add_feature(kind);
            self.name_feature(id, name, note.as_deref());
            self.rebuild();
            if let Some(e) = self.st.errors.get(&id) {
                last_problem = format!("rebuild failed: {e}");
                continue;
            }
            let ours = self.st.body.mass_properties().volume;
            if let Some(theirs) = target {
                if !volume_matches(ours, theirs) {
                    last_problem =
                        format!("result did not match SOLIDWORKS (volume {ours:.1} vs {theirs:.1} mm³)");
                    continue;
                }
            }
            self.solids.insert(name.to_string(), id);
            self.record_feature_dims(f, id);
            let mut detail =
                if target.is_some() { "verified".to_string() } else { "not verified".to_string() };
            if i > 0 {
                detail.push_str(" (direction resolved by verification)");
            }
            if let Some(n) = &note {
                detail = format!("{detail}; {n}");
            }
            return Ok(detail);
        }
        self.doc = saved;
        self.rebuild();
        Err(last_problem)
    }

    /// Maps a feature's own dimensions (such as D1@Boss-Extrude1) so equations can drive them.
    fn record_feature_dims(&mut self, f: &Disp, id: u32) {
        let Some(feat) = self.doc.feature(id) else { return };
        let kind = feat.kind.clone();
        let mut dd = f.opt_obj("GetFirstDisplayDimension", &[]).ok().flatten();
        while let Some(d) = dd {
            if let Ok(dim) = d.obj("GetDimension2", &[0.into()]) {
                let full = dim.string("FullName", &[]).unwrap_or_default();
                let v = dim.f64("GetSystemValue2", &["".into()]).unwrap_or(f64::NAN);
                let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
                let target = match &kind {
                    FeatureKind::Extrude(e) => match e.depth {
                        Dim::Value(x) if near(v * M_TO_MM, x) || near(2.0 * v * M_TO_MM, x) => {
                            Some(DimTarget::ExtrudeDepth(id))
                        }
                        _ => None,
                    },
                    FeatureKind::Revolve(_) => Some(DimTarget::RevolveAngle(id)),
                    FeatureKind::LinearPattern(p) => match p.spacing {
                        Dim::Value(x) if near(v * M_TO_MM, x) => Some(DimTarget::PatternSpacing(id)),
                        _ => Some(DimTarget::PatternCount(id)),
                    },
                    FeatureKind::CircularPattern(_) if d.i64("GetType", &[]).unwrap_or(0) != 3 => {
                        Some(DimTarget::PatternCount(id))
                    }
                    _ => None,
                };
                if let Some(t) = target {
                    self.dims.insert(full, t);
                }
            }
            dd = f.opt_obj("GetNextDisplayDimension", &[(&d).into()]).ok().flatten();
        }
    }

    /// Replaces the pending unsupported features with SOLIDWORKS' geometry as it stands
    /// after the last of them (or at the end of the tree).
    fn flush(&mut self, mid_tree: bool) -> Result<(), String> {
        let last = self.pending.last().map(|p| p.0.clone()).unwrap_or_default();
        if self.can_rollback {
            if mid_tree {
                self.rollback_after(&last).map_err(err)?;
            } else {
                self.rollback_to_end();
            }
        }
        let body =
            capture_bodies(&self.model, &self.session.release_name(), self.precise_geometry).map_err(err)?;
        let names: Vec<String> = self.pending.iter().map(|p| p.0.clone()).collect();
        let pending = std::mem::take(&mut self.pending);
        let Some(mut body) = body else {
            for (n, t, why) in pending {
                self.report.add(&n, &t, Outcome::Skipped, format!("{why}; no solid geometry at this point"));
            }
            return Ok(());
        };
        body.replaces = names.clone();
        let id = self.doc.add_feature(FeatureKind::ImportedBody(body));
        let note = format!(
            "Exact SOLIDWORKS geometry standing in for: {}. Edits to features above it will not flow through it.",
            names.join(", ")
        );
        self.name_feature(id, &format!("Imported ({})", names.join(", ")), Some(&note));
        self.rebuild();
        for (n, t, why) in pending {
            self.report.add(&n, &t, Outcome::Imported, why);
        }
        Ok(())
    }

    fn verify_final(&mut self) {
        let theirs = match sw_volume(&self.model) {
            Ok(v) => v,
            Err(e) => {
                self.report.warnings.push(format!("could not read SOLIDWORKS volume: {e}"));
                return;
            }
        };
        let ours = self.st.body.mass_properties().volume;
        self.report.sw_volume_mm3 = Some(theirs);
        self.report.anvil_volume_mm3 = Some(ours);
        if theirs.abs() > 1e-9 {
            self.report.volume_error_pct = Some((ours - theirs) / theirs * 100.0);
        }
        self.report.verified = volume_matches(ours, theirs) && self.st.errors.is_empty();
        for (id, e) in &self.st.errors {
            let n = self.doc.feature(*id).map(|f| f.name.clone()).unwrap_or_default();
            self.report.warnings.push(format!("AnvilCAD rebuild error in {n}: {e}"));
        }
    }
}

/// Whether a feature's name suggests a cut (used only to order candidates).
fn name_hint_cut(f: &Disp) -> bool {
    f.string("Name", &[]).map(|n| n.to_ascii_lowercase().contains("cut")).unwrap_or(false)
}

fn end_condition_name(code: i64) -> &'static str {
    match code {
        2 => "Through Next",
        3 => "Up To Vertex",
        4 => "Up To Surface",
        5 => "Offset From Surface",
        7 => "Up To Body",
        10 => "Up To Selection",
        11 => "Up To Next",
        _ => "End condition",
    }
}

pub(crate) fn fmt_mm(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Runs `f` with the feature definition's selections accessible, always releasing them.
fn with_selections<T>(data: &Disp, model: &Disp, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let ok = data.bool("AccessSelections", &[model.into(), Arg::Disp(None)]).unwrap_or(false);
    let r = f();
    if ok {
        let _ = data.call("ReleaseSelectionAccess", &[]);
    }
    r
}

fn sw_density(model: &Disp) -> ComResult<f64> {
    let mp = model.obj("Extension", &[])?.obj("CreateMassProperty", &[])?;
    // kg/m^3 -> g/cm^3
    Ok(mp.f64("Density", &[])? / 1000.0)
}

/// Collects triangles with shared, de-duplicated vertices (millimetres).
#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<[f64; 3]>,
    index: HashMap<(i64, i64, i64), u32>,
    faces: Vec<ImportedFace>,
}

impl MeshBuilder {
    fn vertex(&mut self, metres: &[f64]) -> u32 {
        let p = [metres[0] * M_TO_MM, metres[1] * M_TO_MM, metres[2] * M_TO_MM];
        let key = ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64);
        let next = self.vertices.len() as u32;
        *self.index.entry(key).or_insert_with(|| {
            self.vertices.push(p);
            next
        })
    }

    fn face(&mut self, planar: bool, tris: Vec<[u32; 3]>) {
        let tris: Vec<[u32; 3]> =
            tris.into_iter().filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).collect();
        if !tris.is_empty() {
            self.faces.push(ImportedFace { planar, triangles: tris });
        }
    }
}

/// Tessellates the current solid bodies into an imported body.
fn capture_bodies(model: &Disp, source: &str, precise: bool) -> ComResult<Option<ImportedBodyFeature>> {
    let bodies = model.call("GetBodies2", &[SW_SOLID_BODY.into(), true.into()])?.objects();
    // Display tessellation is coarse by default; use the finest setting while capturing.
    let old_quality = model.i64("GetTessellationQuality", &[]).ok();
    let _ = model.call("SetTessellationQuality", &[100.into()]);
    let mut mesh = MeshBuilder::default();
    let mut result = Ok(());
    for body in &bodies {
        let faces = match body.call("GetFaces", &[]) {
            Ok(f) => f.objects(),
            Err(e) => {
                result = Err(e);
                break;
            }
        };
        if !(precise && tessellate_precise(body, &faces, &mut mesh).unwrap_or(false)) {
            if let Err(e) = tessellate_display(&faces, &mut mesh) {
                result = Err(e);
                break;
            }
        }
    }
    if let Some(q) = old_quality {
        let _ = model.call("SetTessellationQuality", &[(q as i32).into()]);
    }
    result?;
    if mesh.faces.is_empty() {
        return Ok(None);
    }
    let MeshBuilder { vertices, mut faces, .. } = mesh;
    // Make sure triangles face outward (positive enclosed volume).
    let p =
        |i: u32| Vec3 { x: vertices[i as usize][0], y: vertices[i as usize][1], z: vertices[i as usize][2] };
    let vol: f64 =
        faces.iter().flat_map(|f| f.triangles.iter()).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2])))).sum();
    if vol < 0.0 {
        for f in &mut faces {
            for t in &mut f.triangles {
                t.swap(1, 2);
            }
        }
    }
    Ok(Some(ImportedBodyFeature {
        source: source.to_string(),
        replaces: Vec::new(),
        vertices,
        faces,
        exact: None,
        adds: false,
    }))
}

fn is_planar(face: &Disp) -> bool {
    face.obj("GetSurface", &[]).and_then(|s| s.bool("IsPlane", &[])).unwrap_or(false)
}

/// Watertight tessellation at a controlled tolerance (0.01 mm chord, like AnvilCAD's own).
fn tessellate_precise(body: &Disp, faces: &[Disp], mesh: &mut MeshBuilder) -> ComResult<bool> {
    let Some(tess) = body.opt_obj("GetTessellation", &[Arg::Empty])? else { return Ok(false) };
    tess.put("NeedFaceFacetMap", Arg::Bool(true))?;
    tess.put("NeedVertexNormal", Arg::Bool(false))?;
    tess.put("NeedVertexParams", Arg::Bool(false))?;
    tess.put("NeedEdgeFinMap", Arg::Bool(false))?;
    tess.put("NeedErrorList", Arg::Bool(false))?;
    tess.put("MatchType", Arg::Int(0))?; // match facet topology: shared edge vertices
    tess.put("CurveChordTolerance", Arg::Float(1e-5))?;
    tess.put("CurveChordAngleTolerance", Arg::Float(10f64.to_radians()))?;
    tess.put("SurfacePlaneTolerance", Arg::Float(1e-5))?;
    tess.put("SurfacePlaneAngleTolerance", Arg::Float(10f64.to_radians()))?;
    if !tess.bool("Tessellate", &[])? {
        return Ok(false);
    }
    let mut vmap: HashMap<i64, u32> = HashMap::new();
    for face in faces {
        let mut tris = Vec::new();
        for facet in tess.call("GetFaceFacets", &[face.into()])?.list() {
            let Some(facet) = facet.i64() else { continue };
            let fins = tess.call("GetFacetFins", &[(facet as i32).into()])?.list();
            let mut tri = [0u32; 3];
            if fins.len() != 3 {
                continue;
            }
            for (k, fin) in fins.iter().enumerate() {
                let fin = fin.i64().unwrap_or(-1) as i32;
                let v = tess.call("GetFinVertices", &[fin.into()])?.list();
                let vid = v.first().and_then(|x| x.i64()).unwrap_or(-1);
                tri[k] = match vmap.get(&vid) {
                    Some(&i) => i,
                    None => {
                        let pt = tess.call("GetVertexPoint", &[(vid as i32).into()])?.floats();
                        if pt.len() < 3 {
                            return Ok(false);
                        }
                        let i = mesh.vertex(&pt);
                        vmap.insert(vid, i);
                        i
                    }
                };
            }
            tris.push(tri);
        }
        mesh.face(is_planar(face), tris);
    }
    Ok(true)
}

/// Fallback: SOLIDWORKS' display tessellation (coarser, per face).
fn tessellate_display(faces: &[Disp], mesh: &mut MeshBuilder) -> ComResult<()> {
    for face in faces {
        let t = face.call("GetTessTriangles", &[true.into()])?.floats();
        let tris = t
            .chunks_exact(9)
            .map(|tri| [mesh.vertex(&tri[0..3]), mesh.vertex(&tri[3..6]), mesh.vertex(&tri[6..9])])
            .collect();
        mesh.face(is_planar(face), tris);
    }
    Ok(())
}

fn resolve_ref(
    s: &Sketch,
    e: &Disp,
    ty: i64,
    points: &HashMap<(i64, i64), usize>,
    segments: &HashMap<(i64, i64), usize>,
) -> Ref {
    let key = id_key(e).ok();
    match ty {
        2 => {
            if let Some(&i) = key.as_ref().and_then(|k| points.get(k)) {
                return Ref::Point(i);
            }
            // The sketch origin is not a sketch point in SOLIDWORKS.
            let x = e.f64("X", &[]).unwrap_or(f64::NAN);
            let y = e.f64("Y", &[]).unwrap_or(f64::NAN);
            if x.abs() < 1e-12 && y.abs() < 1e-12 {
                Ref::Point(ORIGIN)
            } else {
                Ref::External
            }
        }
        3 | 4 => match key.as_ref().and_then(|k| segments.get(k)) {
            Some(&i) if s.entities[i].is_line() => Ref::Line(i),
            Some(&i) => Ref::Round(i),
            None => Ref::External,
        },
        _ => Ref::External,
    }
}

fn line_points(s: &Sketch, l: usize) -> (usize, usize) {
    match s.entities[l].geom {
        Geom::Line { a, b } => (a, b),
        _ => (0, 0),
    }
}

fn center_of(s: &Sketch, e: usize) -> usize {
    match s.entities[e].geom {
        Geom::Circle { c, .. } | Geom::Arc { c, .. } => c,
        Geom::Line { a, .. } => a,
    }
}

/// Maps a geometric relation. Returns None for dimensions and unsupported relations.
fn relation(s: &Sketch, ty: i64, r: &[Ref]) -> Option<Vec<Constraint>> {
    use Constraint as C;
    use Ref::*;
    if r.contains(&External) {
        return None;
    }
    let fix_point = |p: usize| C::Fix { p, x: s.pos(p).x, y: s.pos(p).y };
    Some(match (ty, r) {
        (4, [Line(l)]) => vec![C::Horizontal { line: *l }],
        (5, [Line(l)]) => vec![C::Vertical { line: *l }],
        (4 | 25, [Point(p), Point(q)]) => vec![C::HorizontalPoints { p: *p, q: *q }],
        (5 | 26, [Point(p), Point(q)]) => vec![C::VerticalPoints { p: *p, q: *q }],
        (6, [Line(l), Round(c)]) | (6, [Round(c), Line(l)]) => vec![C::Tangent { line: *l, entity: *c }],
        (7, [Line(a), Line(b)]) => vec![C::Parallel { a: *a, b: *b }],
        (8, [Line(a), Line(b)]) => vec![C::Perpendicular { a: *a, b: *b }],
        (9 | 42, [Point(p), Point(q)]) if p != q => vec![C::Coincident { p: *p, q: *q }],
        (9 | 42, [Point(_), Point(_)]) => vec![],
        (9, [Point(p), Line(l)]) | (9, [Line(l), Point(p)]) => vec![C::PointOnLine { p: *p, line: *l }],
        (9, [Point(p), Round(c)]) | (9, [Round(c), Point(p)]) => vec![C::PointOnCircle { p: *p, entity: *c }],
        (10, [Round(a), Round(b)]) => vec![C::Concentric { a: *a, b: *b }],
        (10, [Round(a), Point(p)]) | (10, [Point(p), Round(a)]) => {
            vec![C::Coincident { p: *p, q: center_of(s, *a) }]
        }
        (12, [Point(p), Line(l)]) | (12, [Line(l), Point(p)]) => vec![C::Midpoint { p: *p, line: *l }],
        (14, [a, b]) => {
            let (x, y) = match (a, b) {
                (Line(x) | Round(x), Line(y) | Round(y)) => (*x, *y),
                _ => return None,
            };
            vec![C::Equal { a: x, b: y }]
        }
        (17, [Point(p)]) => vec![fix_point(*p)],
        (17, [Line(e) | Round(e)]) => s.entities[*e].points().into_iter().map(fix_point).collect(),
        (27, [Line(a), Line(b)]) => {
            vec![C::Parallel { a: *a, b: *b }, C::PointOnLine { p: line_points(s, *b).0, line: *a }]
        }
        (28, [Round(a), Round(b)]) => vec![C::Concentric { a: *a, b: *b }, C::Equal { a: *a, b: *b }],
        _ => return None,
    })
}

/// Maps a driving dimension (relation types distance, angle, radius, diameter). Values are
/// taken from the solved geometry, so they are consistent with AnvilCAD's conventions.
fn dimension(s: &Sketch, rel: &Disp, ty: i64, r: &[Ref]) -> ComResult<Option<(Constraint, String)>> {
    use Constraint as C;
    use Ref::*;
    let Some(dd) = rel.opt_obj("GetDisplayDimension", &[])? else { return Ok(None) };
    let dim = dd.obj("GetDimension2", &[0.into()])?;
    // Driven (reference) dimensions do not constrain anything.
    if dim.i64("DrivenState", &[]).unwrap_or(2) == 1 {
        return Ok(None);
    }
    let full = dim.string("FullName", &[])?;
    let dtype = dd.i64("GetType", &[]).unwrap_or(0);
    let pt = |x: &Ref| -> Option<usize> {
        match *x {
            Point(p) => Some(p),
            Round(e) => Some(center_of(s, e)),
            _ => None,
        }
    };
    let pl_dist = |p: usize, l: usize| {
        let (a, b) = line_points(s, l);
        let (pa, pb) = (s.pos(a), s.pos(b));
        let d = pb - pa;
        (d.cross(s.pos(p) - pa) / d.length().max(1e-12)).abs()
    };
    let c = match (ty, r) {
        (3, [Round(e)]) => C::Radius { entity: *e, value: s.radius(*e) },
        (15, [Round(e)]) => C::Diameter { entity: *e, value: 2.0 * s.radius(*e) },
        (1, [Line(l)]) => {
            let (a, b) = line_points(s, *l);
            match dtype {
                11 => C::HDistance { p: a, q: b, value: s.pos(b).x - s.pos(a).x },
                12 => C::VDistance { p: a, q: b, value: s.pos(b).y - s.pos(a).y },
                _ => C::Length { line: *l, value: s.line_length(*l) },
            }
        }
        (1, [Line(l), x]) | (1, [x, Line(l)]) if pt(x).is_some() => {
            let p = pt(x).unwrap();
            C::PointLineDistance { p, line: *l, value: pl_dist(p, *l) }
        }
        (1, [Line(a), Line(b)]) => {
            let p = line_points(s, *b).0;
            C::PointLineDistance { p, line: *a, value: pl_dist(p, *a) }
        }
        (1, [x, y]) if pt(x).is_some() && pt(y).is_some() => {
            let (p, q) = (pt(x).unwrap(), pt(y).unwrap());
            match dtype {
                11 => C::HDistance { p, q, value: s.pos(q).x - s.pos(p).x },
                12 => C::VDistance { p, q, value: s.pos(q).y - s.pos(p).y },
                _ => C::Distance { p, q, value: s.pos(p).distance(s.pos(q)) },
            }
        }
        (2, [Line(a), Line(b)]) => {
            let dir = |l: usize| {
                let (p, q) = line_points(s, l);
                s.pos(q) - s.pos(p)
            };
            let (u, w) = (dir(*a), dir(*b));
            C::Angle { a: *a, b: *b, degrees: u.cross(w).atan2(u.dot(w)).to_degrees() }
        }
        _ => return Err(ComError("unsupported dimension".into())),
    };
    Ok(Some((c, full)))
}

/// Splits `"name" = expression` into its parts, unquoting the name.
fn split_equation(eq: &str) -> Option<(String, String)> {
    let mut in_quote = false;
    for (i, ch) in eq.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '=' if !in_quote => {
                let lhs = eq[..i].trim().trim_matches('"').to_string();
                let rhs = eq[i + 1..].trim().to_string();
                return (!lhs.is_empty() && !rhs.is_empty()).then_some((lhs, rhs));
            }
            _ => {}
        }
    }
    None
}

/// Converts a SOLIDWORKS equation expression into AnvilCAD's syntax, or None if it uses
/// constructs AnvilCAD cannot evaluate yet (dimension references, conditionals, unsupported
/// functions, units other than millimetres and degrees).
fn convert_expression(rhs: &str) -> Option<String> {
    const KNOWN: &[&str] = &["sin", "cos", "tan", "sqrt", "abs", "min", "max", "round", "pi"];
    // Only text outside quotes is rewritten; quoted names are kept as they are.
    let convert_plain = |text: &str| -> Option<String> {
        let t = text.replace("sqr(", "sqrt(");
        let mut out = String::new();
        let mut word = String::new();
        let mut prev_digit = false;
        let flush = |word: &mut String, out: &mut String, after_number: bool| -> bool {
            if word.is_empty() {
                return true;
            }
            let w = word.to_ascii_lowercase();
            let ok = if after_number {
                // Unit suffix on a number: millimetres and degrees match AnvilCAD.
                w == "mm" || w == "deg"
            } else {
                KNOWN.contains(&w.as_str())
            };
            if ok && !after_number {
                out.push_str(word);
            }
            word.clear();
            ok
        };
        let mut word_after_number = false;
        for c in t.chars() {
            if c.is_ascii_alphabetic() || c == '_' {
                if word.is_empty() {
                    word_after_number = prev_digit;
                }
                word.push(c);
            } else {
                if !flush(&mut word, &mut out, word_after_number) {
                    return None;
                }
                out.push(c);
            }
            prev_digit = c.is_ascii_digit() || c == '.' || (prev_digit && c == ' ');
        }
        if !flush(&mut word, &mut out, word_after_number) {
            return None;
        }
        Some(out)
    };

    let mut out = String::new();
    for (i, part) in rhs.split('"').enumerate() {
        if i % 2 == 1 {
            if part.contains('@') {
                return None; // reference to another dimension
            }
            out.push('"');
            out.push_str(part);
            out.push('"');
        } else {
            out.push_str(&convert_plain(part)?);
        }
    }
    Some(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equations() {
        assert_eq!(split_equation("\"width\"= 80"), Some(("width".into(), "80".into())));
        assert_eq!(
            split_equation("\"D1@Sketch1\" = \"width\" / 2"),
            Some(("D1@Sketch1".into(), "\"width\" / 2".into()))
        );
        assert_eq!(convert_expression("\"width\" / 2"), Some("\"width\" / 2".into()));
        assert_eq!(convert_expression("80mm"), Some("80".into()));
        assert_eq!(convert_expression("sqr(\"a\") + 1"), Some("sqrt(\"a\") + 1".into()));
        assert_eq!(convert_expression("\"D2@Sketch1\" * 2"), None);
        assert_eq!(convert_expression("IIF(\"a\" > 1, 2, 3)"), None);
    }

    #[test]
    fn transforms() {
        // Identity model-to-sketch transform is the Front plane.
        let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        assert!(frames_equal(&model_to_local(&id).unwrap(), &Frame::WORLD));
        // A sketch whose local origin sits at model z = 10 mm: local = model + (0,0,-0.01).
        let mut t = id;
        t[11] = -0.01;
        let f = model_to_local(&t).unwrap();
        assert!((f.origin.z - 10.0).abs() < 1e-9);
    }
}
