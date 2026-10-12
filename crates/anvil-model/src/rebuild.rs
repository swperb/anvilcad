//! Regenerates the part from its feature tree.
//!
//! A failing feature records an error and is skipped; later features still rebuild where
//! they can, so one bad dimension never destroys the whole model.

use crate::document::*;
use anvil_kernel::blend::{blend_edges, EdgeTreatment};
use anvil_kernel::build::{extrude, revolve};
use anvil_kernel::shell::shell;
use anvil_kernel::{FaceId, FaceTag, Frame, MassProperties, Polygon, Solid, SurfaceKind, Vec3};
use anvil_sketch::{Geom, SolveResult};
use std::collections::{BTreeMap, HashMap};

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
        let mut st = BuildState::default();
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
                apply(&mut st.body, &tool, ef.op)?;
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
                apply(&mut st.body, &tool, rf.op)?;
                st.tools.insert(id, (tool, rf.op));
            }
            FeatureKind::LinearPattern(pf) => {
                let spacing = num(&pf.spacing, "spacing")?;
                let count = num(&pf.count, "count")?.round().max(1.0) as usize;
                let dir = pf.direction.dir();
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
                st.body = imported_solid(id, &ib)?;
            }
            FeatureKind::Fillet(f) => {
                let radius = num(&f.radius, "radius")?;
                st.body = blend_edges(&st.body, &f.edges, EdgeTreatment::Fillet { radius }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Chamfer(c) => {
                let distance = num(&c.distance, "distance")?;
                st.body = blend_edges(&st.body, &c.edges, EdgeTreatment::Chamfer { distance }, id)
                    .map_err(|e| e.to_string())?;
            }
            FeatureKind::Shell(s) => {
                let thickness = num(&s.thickness, "thickness")?;
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
