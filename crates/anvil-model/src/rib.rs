//! Ribs: a sketch contour thickened into a strip and run out until it meets the part.
//! The behaviour is specified in `docs/re/rib-draft-dome.md` ("Rib"), whose Decision this
//! follows: extend the contour's open ends, thicken it, sweep the strip far past the part,
//! take the part away, keep the pieces that touch the contour, and join them on. A kept
//! piece that runs out to the end of the sweep has missed the part, which is an error.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use crate::thin::chains;
use anvil_brep as brep;
use anvil_brep::boolean::{boolean, contains_point, BoolOp};
use anvil_kernel::offset::thin_wall;
use anvil_kernel::{Loop2, Profile, Seg2, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Which side of the contour a rib's thickness goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RibSides {
    #[default]
    Both,
    /// To the left of the contour's direction (for a rib normal to the sketch), or along
    /// the sketch normal (for a rib parallel to it).
    First,
    Second,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibFeature {
    pub sketch: u32,
    pub thickness: Dim,
    #[serde(default)]
    pub sides: RibSides,
    /// Run the rib along the sketch normal; otherwise within the sketch plane, square to
    /// the contour's first segment.
    #[serde(default)]
    pub normal_to_sketch: bool,
    /// Run the other way from the one found automatically.
    #[serde(default)]
    pub flip: bool,
    /// Carry open ends on along their own curve (arcs round their circle) instead of
    /// straight on.
    #[serde(default)]
    pub natural: bool,
}

fn d3(v: Vec3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

/// The chain with its open ends carried on `reach` beyond.
fn extended(segs: &[Seg2], natural: bool, reach: f64) -> (Vec<Seg2>, bool) {
    if natural {
        if let [Seg2::Arc { center, radius, source, .. }] = segs {
            return (
                vec![Seg2::Arc {
                    center: *center,
                    radius: *radius,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                    source: *source,
                }],
                true,
            );
        }
    }
    let mut out = segs.to_vec();
    if let Some(first) = segs.first() {
        let t = anvil_kernel::offset::travel(first, false);
        let p = first.start_point();
        out.insert(0, Seg2::Line { a: p - t * reach, b: p, source: first.source() });
    }
    if let Some(last) = segs.last() {
        let t = anvil_kernel::offset::travel(last, true);
        let p = last.end_point();
        out.push(Seg2::Line { a: p, b: p + t * reach, source: last.source() });
    }
    (out, false)
}

impl Document {
    pub(crate) fn rebuild_rib(&self, id: u32, rf: &RibFeature, st: &mut BuildState) -> Result<(), String> {
        let t = rf.thickness.eval(&st.variables).map_err(|e| format!("thickness: {e}"))?;
        if t <= 0.0 {
            return Err("the rib thickness must be greater than zero".into());
        }
        let sf = self.sketch(rf.sketch).ok_or("the rib's sketch is missing")?;
        let frame = *st.frames.get(&rf.sketch).ok_or("the rib's sketch failed or is suppressed")?;
        let bodies = st
            .exact
            .clone()
            .ok_or("ribs need the exact kernel, and the part is faceted from an earlier feature on")?;
        if bodies.is_empty() {
            return Err("there is no part for the rib to meet".into());
        }
        let b = st.body.bounds();
        let reach = 2.0 * (b.diagonal() + (b.center() - frame.origin).length()) + 10.0;
        let all = chains(&sf.sketch)?;
        if all.is_empty() {
            return Err("the sketch has no contour for the rib".into());
        }

        let mut pieces_all: Vec<brep::Solid> = Vec::new();
        for chain in &all {
            let mid_seg = chain.segs[chain.segs.len() / 2];
            let probe2 = match mid_seg {
                Seg2::Line { a, b, .. } => (a + b) * 0.5,
                Seg2::Arc { center, radius, start, sweep, .. } => {
                    center + Vec2::from_angle(start + 0.5 * sweep) * radius
                }
            };
            let (segs, closed) = if chain.closed {
                (chain.segs.clone(), true)
            } else {
                extended(&chain.segs, rf.natural, reach)
            };
            let (left, right) = match rf.sides {
                RibSides::Both => (0.5 * t, 0.5 * t),
                RibSides::First => (t, 0.0),
                RibSides::Second => (0.0, t),
            };
            // Each candidate: the slab and a point just inside it next to the contour.
            let mut candidates: Vec<(Vec<brep::Solid>, Vec3, Vec3)> = Vec::new();
            for s in [1.0, -1.0] {
                if rf.normal_to_sketch {
                    let walls = thin_wall(&segs, closed, left, right, 1 << 21, 1 << 20)
                        .map_err(|e| format!("rib: {e}"))?;
                    let mut it = walls.into_iter();
                    let Some(first) = it.next() else { continue };
                    let profile = match it.next() {
                        Some(second) => {
                            let (o, i) = if first.signed_area().abs() >= second.signed_area().abs() {
                                (first, second)
                            } else {
                                (second, first)
                            };
                            Profile::new(o, vec![i])
                        }
                        None => Profile::new(first, vec![]),
                    };
                    let slab = exact::extrude(&[profile], &frame, 0.0, s * reach, id)?;
                    // A point in the strip beside the contour, a little way in.
                    let side = match rf.sides {
                        RibSides::Both => 0.0,
                        RibSides::First => 0.5 * t,
                        RibSides::Second => -0.5 * t,
                    };
                    let tangent = match mid_seg {
                        Seg2::Line { a, b, .. } => (b - a).normalized(),
                        Seg2::Arc { start, sweep, .. } => {
                            Vec2::from_angle(start + 0.5 * sweep).perp() * sweep.signum()
                        }
                    };
                    let p2 = probe2 + tangent.perp() * side;
                    let p = frame.to_world(p2) + frame.z * (s * 1e-3);
                    candidates.push((slab, p, frame.z * s));
                } else {
                    // The strip runs within the sketch plane, square to the first segment.
                    let first = chain.segs[0];
                    let dir = (first.end_point() - first.start_point()).normalized();
                    let m = dir.perp() * s;
                    let mut pts: Vec<Vec2> = Vec::new();
                    for seg in &segs {
                        let mut buf = Vec::new();
                        seg.sample(0.01, &mut buf);
                        pts.extend(buf);
                    }
                    if let Some(last) = segs.last() {
                        pts.push(last.end_point());
                    }
                    let mut poly = pts.clone();
                    poly.extend(pts.iter().rev().map(|p| *p + m * reach));
                    let region = Profile::new(Loop2::polygon(&poly, 0), vec![]);
                    let (z0, z1) = match rf.sides {
                        RibSides::Both => (-0.5 * t, 0.5 * t),
                        RibSides::First => (0.0, t),
                        RibSides::Second => (-t, 0.0),
                    };
                    let slab = exact::extrude(&[region], &frame, z0, z1, id)?;
                    let p = frame.to_world(probe2 + m * 1e-3) + frame.z * (0.5 * (z0 + z1));
                    let dir3 = frame.x * m.x + frame.y * m.y;
                    candidates.push((slab, p, dir3));
                }
            }
            // Each sense either meets the part (the kept pieces) or does not.
            let mut senses: Vec<Option<Vec<brep::Solid>>> = Vec::new();
            for (slab, probe, dir) in candidates {
                let mut pieces = slab;
                for body in &bodies {
                    let mut next = Vec::new();
                    for p in &pieces {
                        next.extend(boolean(p, body, BoolOp::Subtract).map_err(|e| e.to_string())?);
                    }
                    pieces = next;
                }
                let kept: Vec<brep::Solid> =
                    pieces.into_iter().filter(|p| contains_point(p, d3(probe)).unwrap_or(false)).collect();
                // A piece running out to the far end of the sweep never met the part.
                let far = kept.iter().any(|p| {
                    p.vertices.iter().any(|v| {
                        (Vec3 { x: v.point.x, y: v.point.y, z: v.point.z } - probe).dot(dir) > 0.5 * reach
                    })
                });
                senses.push((!kept.is_empty() && !far).then_some(kept));
            }
            // The automatic sense is the first that meets the part; Flip takes the other.
            let auto = senses.iter().position(Option::is_some).unwrap_or(0);
            let pick = if rf.flip { 1 - auto } else { auto };
            let found = senses.get_mut(pick).and_then(Option::take);
            pieces_all.extend(found.ok_or("the rib does not meet the part")?);
        }
        let ok = st.exact_step(id, |bodies| {
            let mut out = bodies.to_vec();
            for p in &pieces_all {
                out = exact::combine(&out, p, BodyOp::Boss)?;
            }
            Ok(out)
        });
        if !ok {
            return Err(st
                .faceted_from
                .as_ref()
                .map(|f| f.1.clone())
                .unwrap_or_else(|| "the rib could not be joined".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Rib statements 1, 3, 4 and 5 of `docs/re/rib-draft-dome.md`.
    use super::*;
    use anvil_kernel::{v2, FaceId};

    /// The open box: [0,100] x [0,60] x [0,30], 5 mm walls and floor, open at the top.
    fn open_box() -> Document {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(100.0, 60.0));
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(30.0),
            ..Default::default()
        }));
        doc.add_feature(FeatureKind::Shell(ShellFeature {
            remove: vec![FaceId { feature: e, index: FaceId::END_CAP }],
            thickness: Dim::Value(5.0),
        }));
        doc
    }

    fn line_sketch(doc: &mut Document, plane: PlaneRef, a: (f64, f64), b: (f64, f64)) -> u32 {
        let mut sf = SketchFeature::new(plane);
        let (p, q) = (sf.sketch.add_point(v2(a.0, a.1)), sf.sketch.add_point(v2(b.0, b.1)));
        sf.sketch.add_line(p, q);
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn rib(doc: &mut Document, sketch: u32, sides: RibSides, normal: bool, t: f64) -> u32 {
        doc.add_feature(FeatureKind::Rib(RibFeature {
            sketch,
            thickness: Dim::Value(t),
            sides,
            normal_to_sketch: normal,
            flip: false,
            natural: false,
        }))
    }

    fn volume(doc: &mut Document) -> f64 {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st.body.mass_properties().volume
    }

    #[test]
    fn s01_s02_s03_parallel_to_the_sketch() {
        for (a, b, sides) in [
            ((5.0, -30.0), (95.0, -30.0), RibSides::Both),
            ((20.0, -30.0), (80.0, -30.0), RibSides::Both),
            ((5.0, -30.0), (95.0, -30.0), RibSides::First),
        ] {
            let mut doc = open_box();
            let base = doc.add_feature(FeatureKind::Plane(PlaneFeature {
                base: PlaneRef::Top,
                offset: Dim::Value(30.0),
                ..Default::default()
            }));
            let s = line_sketch(&mut doc, PlaneRef::Feature(base), a, b);
            rib(&mut doc, s, sides, false, 4.0);
            assert!((volume(&mut doc) - 76500.0).abs() < 1e-6, "{sides:?}");
        }
    }

    #[test]
    fn s04_normal_to_the_sketch() {
        let mut doc = open_box();
        let top = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(30.0),
            ..Default::default()
        }));
        let s = line_sketch(&mut doc, PlaneRef::Feature(top), (20.0, 30.0), (80.0, 30.0));
        let r = rib(&mut doc, s, RibSides::Both, true, 4.0);
        assert!((volume(&mut doc) - 76500.0).abs() < 1e-6);
        if let Some(FeatureKind::Rib(x)) = doc.feature_mut(r).map(|f| &mut f.kind) {
            x.flip = true;
        }
        assert!(doc.rebuild().errors.contains_key(&r));
    }

    #[test]
    fn s05_closed_ring() {
        let mut doc = open_box();
        let top = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(30.0),
            ..Default::default()
        }));
        let mut sf = SketchFeature::new(PlaneRef::Feature(top));
        let c = sf.sketch.add_point(v2(50.0, 30.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        rib(&mut doc, s, RibSides::Both, true, 2.0);
        let v = volume(&mut doc);
        let want = 67500.0 + 1000.0 * std::f64::consts::PI;
        assert!((v - want).abs() < 1e-4 * want, "{v}");
    }
}
