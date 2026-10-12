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
use anvil_kernel::build::{extrude, revolve};
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
    tools: HashMap<u32, (Solid, BodyOp)>,
    /// Exact bodies, while every feature so far was built exactly.
    pub exact: Option<Vec<anvil_brep::Solid>>,
    /// Features that were built on the exact kernel.
    pub exact_features: BTreeSet<u32>,
    /// The first feature that moved the part to the faceted kernel, and why.
    pub faceted_from: Option<(u32, String)>,
    /// The exact tool bodies each solid feature contributed, used by patterns.
    exact_tools: HashMap<u32, (Vec<anvil_brep::Solid>, BodyOp)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassReport {
    pub props: MassProperties,
    /// Grams, when a material is assigned.
    pub mass: Option<f64>,
}

impl BuildState {
    pub fn mass_report(&self, doc: &Document) -> MassReport {
        let props = self.body.mass_properties();
        MassReport {
            props,
            // mm^3 * g/cm^3 / 1000 = g
            mass: doc.material.as_ref().map(|m| props.volume * m.density / 1000.0),
        }
    }
}

/// Rotation by `angle` about the line through `origin` along `axis`.
fn rotation_frame(origin: Vec3, axis: Vec3, angle: f64) -> Frame {
    let rot = |v: Vec3| {
        let (s, c) = angle.sin_cos();
        v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
    };
    Frame { origin: origin - rot(origin), x: rot(Vec3::X), y: rot(Vec3::Y), z: rot(Vec3::Z) }
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
    fn exact_step(
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

    fn leave_exact(&mut self, id: u32, reason: String) {
        if self.exact.take().is_some() {
            self.faceted_from = Some((id, reason));
        }
    }
}

fn apply(body: &mut Solid, tool: &Solid, op: BodyOp) -> Result<(), String> {
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
    fn resolve_plane(&self, p: &PlaneRef, st: &BuildState) -> Result<Frame, String> {
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
            FeatureKind::Plane(pf) => {
                let base = self.resolve_plane(&pf.base, st)?;
                let off = num(&pf.offset, "offset")?;
                let mut f = base.offset(if pf.flip { -off } else { off });
                if pf.flip {
                    f = Frame { z: -f.z, x: -f.x, ..f };
                }
                st.frames.insert(id, f);
            }
            FeatureKind::Extrude(ef) => {
                let (frame, profiles) = self.sketch_profiles(ef.sketch, st)?;
                let dir = match (ef.op, ef.reverse) {
                    (BodyOp::Boss, false) | (BodyOp::Cut, true) => 1.0,
                    _ => -1.0,
                };
                let (z0, z1) = match ef.end {
                    EndCondition::Blind => (0.0, dir * num(&ef.depth, "depth")?),
                    EndCondition::MidPlane => {
                        let d = num(&ef.depth, "depth")?;
                        (-d / 2.0, d / 2.0)
                    }
                    EndCondition::ThroughAll | EndCondition::ThroughAllBoth => {
                        let b = st.body.bounds();
                        let reach = if b.is_empty() {
                            100.0
                        } else {
                            b.diagonal() + (b.center() - frame.origin).length() + 1.0
                        };
                        if ef.end == EndCondition::ThroughAll {
                            (0.0, dir * reach)
                        } else {
                            (-reach, reach)
                        }
                    }
                };
                let tool = extrude(&profiles, &frame, z0, z1, id).map_err(|e| e.to_string())?;
                let mut exact_tools = None;
                let done = st.exact_step(id, |bodies| {
                    let tools = exact::extrude(&profiles, &frame, z0, z1, id)?;
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
            FeatureKind::Revolve(rf) => {
                let sk = &self.sketch(rf.sketch).ok_or("the sketch this feature uses is missing")?.sketch;
                let (a, b) = match sk.entities.get(rf.axis).map(|e| e.geom) {
                    Some(Geom::Line { a, b }) => (sk.pos(a), sk.pos(b)),
                    _ => return Err("the axis of revolution must be a sketch line".into()),
                };
                // Like SOLIDWORKS, an open profile may be closed along the axis of revolution.
                let (frame, profiles) = match self.sketch_profiles(rf.sketch, st) {
                    Ok(r) => r,
                    Err(e) => match close_along_axis(sk, a, b).map(|s| s.profiles()) {
                        Some(Ok(p)) => (self.sketch_profiles_frame(rf.sketch, st)?, p),
                        _ => return Err(e),
                    },
                };
                let ang = num(&rf.angle, "angle")?.to_radians();
                let tool = revolve(&profiles, &frame, a, b - a, ang, id).map_err(|e| e.to_string())?;
                st.leave_exact(id, "revolves use the faceted kernel".into());
                apply(&mut st.body, &tool, rf.op)?;
                st.tools.insert(id, (tool, rf.op));
            }
            FeatureKind::LinearPattern(pf) => {
                let spacing = num(&pf.spacing, "spacing")?;
                let count = num(&pf.count, "count")?.round().max(1.0) as usize;
                let dir = pf.direction.dir();
                let moves: Vec<Frame> = (1..count)
                    .map(|k| Frame { origin: dir * (spacing * k as f64), ..Frame::WORLD })
                    .collect();
                let seeds = pattern_seeds(&pf.features, &st.exact_tools);
                if st.exact_step(id, |b| exact_pattern(b, &seeds?, &moves)) {
                    return Ok(());
                }
                for src in &pf.features {
                    let (tool, op) =
                        st.tools.get(src).cloned().ok_or("a seed feature is missing or failed")?;
                    for k in 1..count {
                        let f = Frame { origin: dir * (spacing * k as f64), ..Frame::WORLD };
                        apply(&mut st.body, &tool.transformed(&f), op)?;
                    }
                }
            }
            FeatureKind::CircularPattern(pf) => {
                let total = num(&pf.total_angle, "angle")?;
                let count = num(&pf.count, "count")?.round().max(1.0) as usize;
                let step = if (total.abs() - 360.0).abs() < 1e-9 {
                    total / count as f64
                } else {
                    total / (count.max(2) - 1) as f64
                };
                let axis = pf.axis.dir();
                let moves: Vec<Frame> = (1..count)
                    .map(|k| rotation_frame(pf.axis.origin(), axis, (step * k as f64).to_radians()))
                    .collect();
                let seeds = pattern_seeds(&pf.features, &st.exact_tools);
                if st.exact_step(id, |b| exact_pattern(b, &seeds?, &moves)) {
                    return Ok(());
                }
                for src in &pf.features {
                    let (tool, op) =
                        st.tools.get(src).cloned().ok_or("a seed feature is missing or failed")?;
                    for k in 1..count {
                        let f = rotation_frame(pf.axis.origin(), axis, (step * k as f64).to_radians());
                        apply(&mut st.body, &tool.transformed(&f), op)?;
                    }
                }
            }
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
                st.body = if ib.adds && !st.body.is_empty() { st.body.union(&mesh) } else { mesh };
            }
            FeatureKind::Fillet(f) => {
                let radius = num(&f.radius, "radius")?;
                let fillet = anvil_brep::blend::Blend::Fillet(radius);
                if st.exact_step(id, |b| exact::blend(b, &f.edges, fillet, id)) {
                    return Ok(());
                }
                st.body = blend_edges(&st.body, &f.edges, EdgeTreatment::Fillet { radius }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Chamfer(c) => {
                let distance = num(&c.distance, "distance")?;
                let chamfer = anvil_brep::blend::Blend::Chamfer(distance);
                if st.exact_step(id, |b| exact::blend(b, &c.edges, chamfer, id)) {
                    return Ok(());
                }
                st.body = blend_edges(&st.body, &c.edges, EdgeTreatment::Chamfer { distance }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Shell(s) => {
                let thickness = num(&s.thickness, "thickness")?;
                st.leave_exact(id, "shells use the faceted kernel".into());
                st.body = shell(&st.body, &s.remove, thickness, id).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn sketch_profiles_frame(&self, sketch_id: u32, st: &BuildState) -> Result<Frame, String> {
        st.frames
            .get(&sketch_id)
            .copied()
            .ok_or_else(|| "the sketch this feature uses failed or is suppressed".into())
    }

    fn sketch_profiles(
        &self,
        sketch_id: u32,
        st: &BuildState,
    ) -> Result<(Frame, Vec<anvil_kernel::Profile>), String> {
        let sf = self.sketch(sketch_id).ok_or("the sketch this feature uses is missing")?;
        let frame =
            *st.frames.get(&sketch_id).ok_or("the sketch this feature uses failed or is suppressed")?;
        let profiles = sf.sketch.profiles().map_err(|e| e.to_string())?;
        Ok((frame, profiles))
    }
}

type ExactSeeds = Vec<(Vec<anvil_brep::Solid>, BodyOp)>;

/// The exact tool bodies of a pattern's seed features.
fn pattern_seeds(
    seeds: &[u32],
    tools: &HashMap<u32, (Vec<anvil_brep::Solid>, BodyOp)>,
) -> Result<ExactSeeds, String> {
    seeds
        .iter()
        .map(|s| tools.get(s).cloned().ok_or_else(|| "a seed feature was not built exactly".to_string()))
        .collect()
}

/// Applies a copy of every seed tool moved by each frame, in order.
fn exact_pattern(
    bodies: &[anvil_brep::Solid],
    seeds: &ExactSeeds,
    moves: &[Frame],
) -> Result<Vec<anvil_brep::Solid>, String> {
    let mut out = bodies.to_vec();
    for (tools, op) in seeds {
        for m in moves {
            let m = exact::frame(m);
            for t in tools {
                out = exact::combine(&out, &t.transformed(&m), *op)?;
            }
        }
    }
    Ok(out)
}

/// If the sketch's non-construction geometry has exactly two loose ends and both lie on the
/// axis line (a, b), returns a copy with a closing segment between them.
fn close_along_axis(
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
