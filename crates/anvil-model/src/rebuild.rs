//! Regenerates the part from its feature tree.
//!
//! A failing feature records an error and is skipped; later features still rebuild where
//! they can, so one bad dimension never destroys the whole model.
//!
//! Two kernels work together. While every feature so far is one the exact kernel
//! ([`anvil_brep`]) can build (extrudes, cuts, patterns, fillets and chamfers on straight
//! edges between flat faces), the part is built exactly and the faceted body is
//! regenerated from the exact result. The first feature the exact kernel cannot build
//! (or declines) hands over to the faceted kernel, which carries on as before. A feature
//! is never built worse than the faceted kernel alone would build it.

use crate::document::*;
use crate::exact;
use anvil_kernel::blend::{blend_edges, EdgeTreatment};
use anvil_kernel::build::revolve;
use anvil_kernel::shell::shell;
use anvil_kernel::{FaceId, FaceTag, Frame, MassProperties, Polygon, Solid, SurfaceKind, Vec3};
use anvil_sketch::{Geom, SolveResult};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default, Clone)]
pub struct BuildState {
    pub body: Solid,
    /// Placement of every sketch and reference plane, keyed by feature id.
    pub frames: HashMap<u32, Frame>,
    pub sketch_status: HashMap<u32, SolveResult>,
    /// Rebuild errors keyed by feature id.
    pub errors: HashMap<u32, String>,
    pub variables: BTreeMap<String, f64>,
    pub variable_errors: BTreeMap<String, String>,
    /// The tool body each solid feature contributed, used by patterns.
    pub(crate) tools: HashMap<u32, (Solid, BodyOp)>,
    /// Exact bodies, while every feature so far was built exactly.
    pub exact: Option<Vec<anvil_brep::Solid>>,
    /// Features that were built on the exact kernel.
    pub exact_features: BTreeSet<u32>,
    /// The first feature that moved the part to the faceted kernel, and why.
    pub faceted_from: Option<(u32, String)>,
    /// The exact tool bodies each solid feature contributed, used by patterns.
    pub(crate) exact_tools: HashMap<u32, (Vec<anvil_brep::Solid>, BodyOp)>,
    /// Reference axes (a point and a unit direction), keyed by feature id.
    pub axes: HashMap<u32, (Vec3, Vec3)>,
    /// Reference points, keyed by feature id.
    pub points: HashMap<u32, Vec3>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassReport {
    pub props: MassProperties,
    /// Grams, when a material is assigned.
    pub mass: Option<f64>,
    /// Inertia tensor about the centre of mass, g mm^2, when a material is assigned.
    pub inertia: Option<[[f64; 3]; 3]>,
    /// Principal moments of inertia about the centre of mass, g mm^2, smallest first.
    pub principal: Option<[f64; 3]>,
}

impl BuildState {
    pub fn mass_report(&self, doc: &Document) -> MassReport {
        let props = self.body.mass_properties();
        let material = doc.material.as_ref().map(|m| m.resolved());
        let inertia = material.as_ref().map(|m| {
            let unit = second_moments(&self.body, props.centroid);
            // g/cm^3 to g/mm^3.
            let rho = m.density / 1000.0;
            unit.map(|row| row.map(|v| v * rho))
        });
        MassReport {
            props,
            mass: material.as_ref().map(|m| m.mass_of(props.volume)),
            inertia,
            principal: inertia.map(principal_moments),
        }
    }
}

/// Inertia tensor of the body about `c` for unit density, by summing the signed
/// tetrahedra each boundary triangle makes with `c`.
fn second_moments(body: &Solid, c: Vec3) -> [[f64; 3]; 3] {
    // Integrals of x^2, y^2, z^2, xy, yz, zx over the volume, relative to c.
    let mut m = [0.0f64; 6];
    for p in &body.polygons {
        let v0 = p.verts[0] - c;
        for i in 1..p.verts.len() - 1 {
            let (a, b, d) = (v0, p.verts[i] - c, p.verts[i + 1] - c);
            let vol = a.dot(b.cross(d)) / 6.0;
            let (x, y, z) = ([a.x, b.x, d.x], [a.y, b.y, d.y], [a.z, b.z, d.z]);
            let sq = |u: [f64; 3]| {
                u[0] * u[0] + u[1] * u[1] + u[2] * u[2] + u[0] * u[1] + u[0] * u[2] + u[1] * u[2]
            };
            let pr = |u: [f64; 3], w: [f64; 3]| {
                2.0 * (u[0] * w[0] + u[1] * w[1] + u[2] * w[2])
                    + u[0] * w[1]
                    + u[1] * w[0]
                    + u[0] * w[2]
                    + u[2] * w[0]
                    + u[1] * w[2]
                    + u[2] * w[1]
            };
            m[0] += vol / 10.0 * sq(x);
            m[1] += vol / 10.0 * sq(y);
            m[2] += vol / 10.0 * sq(z);
            m[3] += vol / 20.0 * pr(x, y);
            m[4] += vol / 20.0 * pr(y, z);
            m[5] += vol / 20.0 * pr(z, x);
        }
    }
    let [xx, yy, zz, xy, yz, zx] = m;
    // Products that are zero by symmetry come out as rounding noise; show them as zero.
    let scale = xx.abs().max(yy.abs()).max(zz.abs());
    let clean = |v: f64| if v.abs() < 1e-9 * scale { 0.0 } else { v };
    let (xy, yz, zx) = (clean(xy), clean(yz), clean(zx));
    [[yy + zz, -xy, -zx], [-xy, xx + zz, -yz], [-zx, -yz, xx + yy]]
}

/// Eigenvalues of a symmetric 3x3 matrix by Jacobi rotations, smallest first.
fn principal_moments(t: [[f64; 3]; 3]) -> [f64; 3] {
    let mut a = t;
    for _ in 0..50 {
        let (mut p, mut q, mut big) = (0, 1, 0.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > big {
                (p, q, big) = (i, j, a[i][j].abs());
            }
        }
        let scale = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if big <= 1e-15 * scale.max(1e-300) {
            break;
        }
        let theta = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (s, c) = theta.sin_cos();
        let mut r = [[0.0; 3]; 3];
        for (k, row) in r.iter_mut().enumerate() {
            row[k] = 1.0;
        }
        r[p][p] = c;
        r[q][q] = c;
        r[p][q] = s;
        r[q][p] = -s;
        // a = r^T a r
        let mut ar = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                ar[i][j] = (0..3).map(|k| a[i][k] * r[k][j]).sum();
            }
        }
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] = (0..3).map(|k| r[k][i] * ar[k][j]).sum();
            }
        }
    }
    let mut e = [a[0][0], a[1][1], a[2][2]];
    e.sort_by(f64::total_cmp);
    e
}

/// Builds a solid from imported triangles. Each imported face keeps its own face id so
/// later sketches can attach to it.
fn imported_solid(id: u32, ib: &ImportedBodyFeature) -> Result<Solid, String> {
    let v = |i: u32| -> Result<Vec3, String> {
        ib.vertices
            .get(i as usize)
            .map(|p| Vec3 { x: p[0], y: p[1], z: p[2] })
            .ok_or_else(|| "imported body references a missing vertex".to_string())
    };
    let mut polys = Vec::new();
    for (fi, face) in ib.faces.iter().enumerate() {
        let tag = FaceTag {
            id: FaceId { feature: id, index: fi as u32 },
            surface: if face.planar { SurfaceKind::Plane } else { SurfaceKind::Other },
        };
        for t in &face.triangles {
            polys.extend(Polygon::new(vec![v(t[0])?, v(t[1])?, v(t[2])?], tag));
        }
    }
    if polys.is_empty() {
        return Err("imported body is empty".into());
    }
    Ok(Solid::new(polys))
}

impl BuildState {
    /// Runs one feature on the exact kernel. On success the faceted body is regenerated
    /// from the exact result and `true` is returned. On failure the part moves to the
    /// faceted kernel for good and the caller builds the feature there.
    pub(crate) fn exact_step(
        &mut self,
        id: u32,
        build: impl FnOnce(&[anvil_brep::Solid]) -> Result<Vec<anvil_brep::Solid>, String>,
    ) -> bool {
        let Some(bodies) = self.exact.as_ref() else {
            return false;
        };
        let result = build(bodies).and_then(|new| {
            let body = exact::faceted(&new)?;
            Ok((new, body))
        });
        match result {
            Ok((new, body)) => {
                self.exact = Some(new);
                self.body = body;
                self.exact_features.insert(id);
                true
            }
            Err(reason) => {
                self.leave_exact(id, reason);
                false
            }
        }
    }

    /// Why feature `id`, which only the exact kernel builds, failed: the exact kernel's
    /// own reason when it was this feature that left it, or that an earlier one did.
    fn needs_exact(&self, id: u32, what: &str) -> String {
        match &self.faceted_from {
            Some((from, reason)) if *from == id => reason.clone(),
            _ => format!("{what} need the exact kernel, and this part is faceted from an earlier feature on"),
        }
    }

    fn leave_exact(&mut self, id: u32, reason: String) {
        if self.exact.take().is_some() {
            self.faceted_from = Some((id, reason));
        }
    }
}

pub(crate) fn apply(body: &mut Solid, tool: &Solid, op: BodyOp) -> Result<(), String> {
    match op {
        BodyOp::Boss => *body = body.union(tool),
        BodyOp::Cut => {
            if body.is_empty() {
                return Err("there is no body to cut".into());
            }
            *body = body.subtract(tool);
        }
    }
    Ok(())
}

impl Document {
    pub(crate) fn resolve_plane(&self, p: &PlaneRef, st: &BuildState) -> Result<Frame, String> {
        if let Some(f) = p.standard_frame() {
            return Ok(f);
        }
        match *p {
            PlaneRef::Face(fid) => st
                .body
                .planar_face_frame(fid)
                .ok_or_else(|| "the referenced face no longer exists or is not planar".to_string()),
            PlaneRef::Feature(id) => {
                st.frames.get(&id).copied().ok_or_else(|| "the referenced plane is missing".to_string())
            }
            _ => unreachable!(),
        }
    }

    /// Rebuilds every feature up to the rollback bar. Sketches are solved in place.
    pub fn rebuild(&mut self) -> BuildState {
        let mut st = BuildState { exact: Some(Vec::new()), ..BuildState::default() };
        let (vars, verrs) = self.evaluate_variables();
        st.variables = vars;
        st.variable_errors = verrs.into_iter().map(|(k, v)| (k, v.to_string())).collect();

        let limit = self.rollback.unwrap_or(usize::MAX).min(self.features.len());
        for i in 0..limit {
            if self.features[i].suppressed {
                continue;
            }
            let id = self.features[i].id;
            if let Err(e) = self.rebuild_feature(i, &mut st) {
                st.errors.insert(id, e);
            }
        }
        st
    }

    fn rebuild_feature(&mut self, i: usize, st: &mut BuildState) -> Result<(), String> {
        let id = self.features[i].id;
        let kind = self.features[i].kind.clone();
        let num = |d: &Dim, what: &str| -> Result<f64, String> {
            d.eval(&st.variables).map_err(|e| format!("{what}: {e}"))
        };
        match kind {
            FeatureKind::Sketch(sf) => {
                let frame = self.resolve_plane(&sf.plane, st)?;
                let mut sketch = sf.sketch.clone();
                for d in &sf.driven {
                    let v = crate::equations::eval(&d.expr, &|n| st.variables.get(n).copied())
                        .map_err(|e| format!("dimension equation: {e}"))?;
                    if let Some(c) = sketch.constraints.get_mut(d.constraint) {
                        c.set_value(v);
                    }
                }
                let res = sketch.solve();
                let ok = res.converged;
                st.sketch_status.insert(id, res);
                if let Some(s) = self.sketch_mut(id) {
                    s.sketch = sketch;
                }
                if !ok {
                    return Err("sketch is over defined or cannot be solved".into());
                }
                st.frames.insert(id, frame);
            }
            FeatureKind::Plane(pf) => self.rebuild_plane(id, &pf, st)?,
            FeatureKind::Axis(a) => self.rebuild_axis(id, &a, st)?,
            FeatureKind::Point(p) => self.rebuild_point(id, &p, st)?,
            FeatureKind::CoordSys(c) => self.rebuild_coordsys(id, &c, st)?,
            FeatureKind::Extrude(ef) => {
                let plan = self.plan_extrude(&ef, st)?;
                let tool = plan.faceted(id)?;
                let mut exact_tools = None;
                let done = st.exact_step(id, |bodies| {
                    let tools = plan.exact(id)?;
                    let mut out = bodies.to_vec();
                    for t in &tools {
                        out = exact::combine(&out, t, ef.op)?;
                    }
                    exact_tools = Some(tools);
                    Ok(out)
                });
                if done {
                    if let Some(t) = exact_tools {
                        st.exact_tools.insert(id, (t, ef.op));
                    }
                } else {
                    apply(&mut st.body, &tool, ef.op)?;
                }
                st.tools.insert(id, (tool, ef.op));
            }
            FeatureKind::Draft(d) => self.rebuild_draft(id, &d, st)?,
            FeatureKind::Dome(d) => self.rebuild_dome(id, &d, st)?,
            FeatureKind::Sweep(sw) => {
                let plan = self.plan_sweep(&sw, st)?;
                let tool = plan.faceted(id)?;
                let mut exact_tools = None;
                let done = st.exact_step(id, |bodies| {
                    let tools = plan.exact(id)?;
                    let mut out = bodies.to_vec();
                    for t in &tools {
                        out = exact::combine(&out, t, sw.op)?;
                    }
                    exact_tools = Some(tools);
                    Ok(out)
                });
                if done {
                    if let Some(t) = exact_tools {
                        st.exact_tools.insert(id, (t, sw.op));
                    }
                } else {
                    apply(&mut st.body, &tool, sw.op)?;
                }
                st.tools.insert(id, (tool, sw.op));
            }
            FeatureKind::Revolve(rf) => {
                if rf.flip_side {
                    return Err("flip side to cut is not available for revolves yet".into());
                }
                let plan = self.plan_revolve(&rf, st)?;
                let tool = plan.faceted(id)?;
                let mut exact_tools = None;
                let done = st.exact_step(id, |bodies| {
                    let tools = plan.exact(id)?;
                    let mut out = bodies.to_vec();
                    for t in &tools {
                        out = exact::combine(&out, t, rf.op)?;
                    }
                    exact_tools = Some(tools);
                    Ok(out)
                });
                if done {
                    if let Some(t) = exact_tools {
                        st.exact_tools.insert(id, (t, rf.op));
                    }
                } else {
                    apply(&mut st.body, &tool, rf.op)?;
                }
                st.tools.insert(id, (tool, rf.op));
            }
            FeatureKind::LinearPattern(pf) => self.rebuild_linear_pattern(id, &pf, st)?,
            FeatureKind::CircularPattern(pf) => self.rebuild_circular_pattern(id, &pf, st)?,
            FeatureKind::Mirror(m) => self.rebuild_mirror(id, &m, st)?,
            FeatureKind::SketchPattern(sp) => self.rebuild_sketch_pattern(id, &sp, st)?,
            FeatureKind::ImportedBody(ib) => {
                let mesh = imported_solid(id, &ib)?;
                match &ib.exact {
                    Some(exact) => {
                        let mut body = exact.clone();
                        body.set_owner(id);
                        let adds = ib.adds;
                        if st.exact_step(id, |bodies| {
                            Ok(if adds { bodies.iter().cloned().chain([body]).collect() } else { vec![body] })
                        }) {
                            return Ok(());
                        }
                    }
                    None => st.leave_exact(id, "imported bodies without exact geometry are meshes".into()),
                }
                // Bodies from one file stay separate, as in a multibody part, and sheets
                // are not closed, so they are appended rather than combined by a boolean.
                st.body = if ib.adds && !st.body.is_empty() {
                    let mut polys = st.body.polygons.clone();
                    polys.extend(mesh.polygons);
                    Solid::new(polys)
                } else {
                    mesh
                };
            }
            FeatureKind::Fillet(f) => {
                use anvil_brep::blend::{Blend, Profile};
                let radius = num(&f.radius, "radius")?;
                let fillet = match &f.profile {
                    FilletProfile::Circular => Blend::Fillet(radius),
                    FilletProfile::ConicRho { rho } => {
                        Blend::Profiled { radius, profile: Profile::ConicRho(num(rho, "rho")?) }
                    }
                    FilletProfile::ConicRadius { radius: r } => {
                        Blend::Profiled { radius, profile: Profile::ConicRadius(num(r, "conic radius")?) }
                    }
                    FilletProfile::CurvatureContinuous => {
                        Blend::Profiled { radius, profile: Profile::CurvatureContinuous }
                    }
                };
                if st.exact_step(id, |b| {
                    exact::blend(b, &f.edges, exact::Treatment::Plain(fillet), f.tangent_propagation, id)
                }) {
                    return Ok(());
                }
                if f.profile != FilletProfile::Circular {
                    return Err(st.needs_exact(id, &format!("{} fillets", f.profile.label().to_lowercase())));
                }
                st.body = blend_edges(&st.body, &f.edges, EdgeTreatment::Fillet { radius }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Chamfer(c) => {
                use anvil_brep::blend::Blend;
                let distance = num(&c.distance, "distance")?;
                let treatment = match &c.kind {
                    ChamferKind::EqualDistance => exact::Treatment::Plain(Blend::Chamfer(distance)),
                    ChamferKind::OffsetFace => exact::Treatment::Plain(Blend::ChamferOffset(distance)),
                    ChamferKind::AngleDistance { angle, flip } => exact::Treatment::AngleChamfer {
                        distance,
                        angle: num(angle, "angle")?.to_radians(),
                        flip: *flip,
                    },
                };
                if st.exact_step(id, |b| exact::blend(b, &c.edges, treatment, c.tangent_propagation, id)) {
                    return Ok(());
                }
                if c.kind != ChamferKind::EqualDistance {
                    return Err(st.needs_exact(id, &format!("{} chamfers", c.kind.label().to_lowercase())));
                }
                st.body = blend_edges(&st.body, &c.edges, EdgeTreatment::Chamfer { distance }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Shell(s) => {
                let thickness = num(&s.thickness, "thickness")?;
                if !st.exact_step(id, |bodies| exact::shell(bodies, &s.remove, thickness, id)) {
                    st.body = shell(&st.body, &s.remove, thickness, id).map_err(|e| e.to_string())?;
                }
            }
            FeatureKind::Hole(h) => self.rebuild_hole(id, &h, st)?,
        }
        Ok(())
    }

    /// Cuts one hole at each free point of the hole's sketch. Each hole is its section
    /// turned about the hole's axis; see `docs/re/hole-wizard.md`.
    fn rebuild_hole(&self, id: u32, h: &crate::hole::HoleFeature, st: &mut BuildState) -> Result<(), String> {
        let sf = self.sketch(h.sketch).ok_or("the sketch this feature uses is missing")?;
        let frame = self.sketch_profiles_frame(h.sketch, st)?;
        let centers = crate::hole::positions(&sf.sketch);
        if centers.is_empty() {
            return Err("the sketch has no points to place holes at; add sketch points".into());
        }
        let section = h.resolve(&st.variables)?;
        if st.body.is_empty() {
            return Err("there is no body to cut".into());
        }
        let b = st.body.bounds();
        let through = b.diagonal() + (b.center() - frame.origin).length() + 1.0;
        let profile =
            anvil_kernel::Profile::new(anvil_kernel::Loop2::polygon(&section.outline(through), 0), vec![]);
        let profiles = [profile];
        // The section lies in a plane through the axis: x across it, y out of the face.
        let out = if h.reverse { -frame.z } else { frame.z };
        let place = |c: anvil_kernel::Vec2| {
            let x = frame.x;
            Frame { origin: frame.origin + frame.x * c.x + frame.y * c.y, x, y: out, z: x.cross(out) }
        };
        let axis = (anvil_kernel::v2(0.0, 0.0), anvil_kernel::v2(0.0, 1.0));
        let full = std::f64::consts::TAU;

        let mut faceted = Vec::new();
        for &c in &centers {
            faceted.push(revolve(&profiles, &place(c), axis.0, axis.1, full, id).map_err(|e| e.to_string())?);
        }
        let mut exact_tools = Vec::new();
        let done = st.exact_step(id, |bodies| {
            let mut out = bodies.to_vec();
            for &c in &centers {
                for t in exact::revolve(&profiles, &place(c), axis.0, axis.1, full, id)? {
                    out = exact::combine(&out, &t, BodyOp::Cut)?;
                    exact_tools.push(t);
                }
            }
            Ok(out)
        });
        if done {
            st.exact_tools.insert(id, (exact_tools, BodyOp::Cut));
        } else {
            for t in &faceted {
                apply(&mut st.body, t, BodyOp::Cut)?;
            }
        }
        let mut polys = Vec::new();
        for t in faceted {
            polys.extend(t.polygons);
        }
        st.tools.insert(id, (Solid::new(polys), BodyOp::Cut));
        Ok(())
    }

    fn sketch_profiles_frame(&self, sketch_id: u32, st: &BuildState) -> Result<Frame, String> {
        st.frames
            .get(&sketch_id)
            .copied()
            .ok_or_else(|| "the sketch this feature uses failed or is suppressed".into())
    }
}

/// If the sketch's non-construction geometry has exactly two loose ends and both lie on the
/// axis line (a, b), returns a copy with a closing segment between them.
pub(crate) fn close_along_axis(
    sk: &anvil_sketch::Sketch,
    a: anvil_kernel::Vec2,
    b: anvil_kernel::Vec2,
) -> Option<anvil_sketch::Sketch> {
    let key = |p: anvil_kernel::Vec2| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
    let mut uses: HashMap<(i64, i64), (usize, usize)> = HashMap::new();
    for e in sk.entities.iter().filter(|e| !e.construction) {
        let ends = match e.geom {
            Geom::Line { a, b } | Geom::Arc { a, b, .. } => [a, b],
            Geom::Circle { .. } => continue,
        };
        for p in ends {
            uses.entry(key(sk.pos(p))).or_insert((0, p)).0 += 1;
        }
    }
    let d = b - a;
    let on_axis = |p: anvil_kernel::Vec2| (d.cross(p - a) / d.length().max(1e-12)).abs() < 1e-6;
    let loose: Vec<usize> = uses.values().filter(|(n, _)| *n == 1).map(|&(_, p)| p).collect();
    if loose.len() != 2 || !loose.iter().all(|&p| on_axis(sk.pos(p))) {
        return None;
    }
    let mut s = sk.clone();
    s.add_line(loose[0], loose[1]);
    Some(s)
}
