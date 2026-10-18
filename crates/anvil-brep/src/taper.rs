//! Extrusion with draft: the profile grows or shrinks steadily with height.
//!
//! The cross-section at height `z` is the profile offset outwards by `z * taper`
//! (inwards when that is negative), so the sketch plane (`z = 0`) keeps the profile
//! as drawn. Each line sweeps a plane leaning by `atan(taper)`, each arc a cone of that
//! half angle about the arc's axis. Where two lines meet, or a curve runs smoothly into
//! the next, the leaning walls meet in a straight line, which becomes the side edge.
//! Where an arc meets anything at a corner the walls meet in a conic, which this module
//! does not build; it refuses, and callers fall back to the faceted kernel.

use crate::extrude::segment_curve;
use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Region, Segment, build_regions};
use crate::topo::{Coedge, Edge, EdgeId, Face, FaceTag, Loop, Solid};
use crate::{KernelError, Result, check, tol};
use glam::DVec2;

fn refuse<T>(msg: &str) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

/// Unit direction of travel at the start (`at_end` false) or end of a segment.
fn travel(s: &Segment, at_end: bool) -> DVec2 {
    match *s {
        Segment::Line { a, b } => (b - a).normalize_or_zero(),
        Segment::Arc { start, sweep, .. } => {
            let t = if at_end { start + sweep } else { start };
            DVec2::from_angle(t).perp() * sweep.signum()
        }
    }
}

/// Outward normal (right of travel) of a line.
fn right(d: DVec2) -> DVec2 {
    DVec2::new(d.y, -d.x)
}

/// The loop moved `d` outwards (to the right of travel). Corners must be between two
/// lines or smooth; segments must not vanish or turn over.
pub fn offset_loop(l: &Loop2, d: f64) -> Result<Loop2> {
    let n = l.segments.len();
    if d == 0.0 {
        return Ok(l.clone());
    }
    // The moved segment's supporting line or circle.
    let moved: Vec<Segment> = l
        .segments
        .iter()
        .map(|s| match *s {
            Segment::Line { a, b } => {
                let off = right((b - a).normalize_or_zero()) * d;
                Ok(Segment::Line { a: a + off, b: b + off })
            }
            Segment::Arc { center, radius, start, sweep } => {
                let r = radius + d * sweep.signum();
                if r <= tol::LINEAR {
                    return refuse("the draft closes an arc of the profile to a point");
                }
                Ok(Segment::Arc { center, radius: r, start, sweep })
            }
        })
        .collect::<Result<_>>()?;
    if n == 1 {
        return Ok(Loop2::with_tags(moved, l.tags.clone()));
    }
    // Corner k is where segment k-1 ends and segment k starts.
    let mut corners = Vec::with_capacity(n);
    for k in 0..n {
        let (prev, next) = (&l.segments[(k + n - 1) % n], &l.segments[k]);
        let (u, w) = (travel(prev, true), travel(next, false));
        let smooth = u.perp_dot(w).abs() < 1e-9 && u.dot(w) > 0.0;
        let p = next.start();
        let corner = if smooth {
            p + right(w) * d
        } else {
            match (moved[(k + n - 1) % n], moved[k]) {
                (Segment::Line { a: a0, .. }, Segment::Line { a: a1, .. }) => {
                    let den = u.perp_dot(w);
                    let t = (a1 - a0).perp_dot(w) / den;
                    a0 + u * t
                }
                _ => return refuse("drafting a sharp corner at an arc needs conic edges"),
            }
        };
        corners.push(corner);
    }
    let mut segs = Vec::with_capacity(n);
    for k in 0..n {
        let (a, b) = (corners[k], corners[(k + 1) % n]);
        segs.push(match (l.segments[k], moved[k]) {
            (Segment::Line { a: oa, b: ob }, Segment::Line { .. }) => {
                if (b - a).dot(ob - oa) <= tol::LINEAR * (ob - oa).length() {
                    return refuse("the draft shrinks a side of the profile away");
                }
                Segment::Line { a, b }
            }
            (_, arc @ Segment::Arc { .. }) => arc,
            _ => return Err(KernelError::InvalidInput("segment kinds changed".into())),
        });
    }
    Ok(Loop2::with_tags(segs, l.tags.clone()))
}

/// The region with every contour moved `d` outwards, checked to be one region still.
fn offset_region(region: &Region, d: f64) -> Result<Region> {
    let outer = offset_loop(&region.outer, d)?;
    let holes = region.holes.iter().map(|h| offset_loop(h, d)).collect::<Result<Vec<_>>>()?;
    let mut all = vec![outer.clone()];
    all.extend(holes.iter().cloned());
    let check = build_regions(&all)
        .map_err(|_| KernelError::Unsupported("the draft makes the profile run into itself".into()))?;
    if check.len() != 1 || check[0].holes.len() != holes.len() {
        return refuse("the draft makes the profile run into itself");
    }
    Ok(Region { outer, holes })
}

/// Extrudes `region` between `z0` and `z1` along `frame.z`, the cross-section at
/// height `z` being the region moved `z * taper` outwards. With `taper` zero this is a
/// plain extrusion. The returned solid has passed [`check::validate`].
pub fn extrude_drafted(frame: &Frame, region: &Region, z0: f64, z1: f64, taper: f64) -> Result<Solid> {
    if !z0.is_finite() || !z1.is_finite() || !taper.is_finite() {
        return Err(KernelError::InvalidDistance);
    }
    let (z0, z1) = (z0.min(z1), z0.max(z1));
    if z1 - z0 < tol::LINEAR {
        return Err(KernelError::InvalidDistance);
    }
    if taper == 0.0 {
        return crate::extrude::extrude(frame, region, z0, z1);
    }
    let lo = offset_region(region, z0 * taper)?;
    let hi = offset_region(region, z1 * taper)?;
    let bottom = frame.offset(z0);
    let top = frame.offset(z1);
    let height = z1 - z0;

    let mut solid = Solid::default();
    let pairs: Vec<(&Loop2, &Loop2)> =
        std::iter::once((&lo.outer, &hi.outer)).chain(lo.holes.iter().zip(hi.holes.iter())).collect();

    struct Rings {
        bottom: Vec<EdgeId>,
        top: Vec<EdgeId>,
        side: Vec<EdgeId>,
    }
    let mut rings = Vec::with_capacity(pairs.len());
    for (b, t) in &pairs {
        let n = b.segments.len();
        let vb: Vec<_> = b.segments.iter().map(|s| solid.add_vertex(bottom.to_world(s.start()))).collect();
        let vt: Vec<_> = t.segments.iter().map(|s| solid.add_vertex(top.to_world(s.start()))).collect();
        let mut r =
            Rings { bottom: Vec::with_capacity(n), top: Vec::with_capacity(n), side: Vec::with_capacity(n) };
        for k in 0..n {
            let next = (k + 1) % n;
            let (cb, b0, b1) = segment_curve(&bottom, &b.segments[k]);
            let (ct, t0, t1) = segment_curve(&top, &t.segments[k]);
            r.bottom.push(solid.add_edge(Edge { curve: cb, start: vb[k], end: vb[next], t0: b0, t1: b1 }));
            r.top.push(solid.add_edge(Edge { curve: ct, start: vt[k], end: vt[next], t0, t1 }));
            let (pb, pt) = (bottom.to_world(b.segments[k].start()), top.to_world(t.segments[k].start()));
            let len = (pt - pb).length();
            r.side.push(solid.add_edge(Edge {
                curve: Curve::Line { origin: pb, dir: (pt - pb) / len },
                start: vb[k],
                end: vt[k],
                t0: 0.0,
                t1: len,
            }));
        }
        rings.push(r);
    }

    let fwd = |edge| Coedge { edge, reversed: false };
    let rev = |edge| Coedge { edge, reversed: true };
    let mut top_loops = rings.iter().map(|r| Loop { coedges: r.top.iter().copied().map(fwd).collect() });
    solid.add_face(Face {
        surface: Surface::Plane { frame: top },
        reversed: false,
        outer: top_loops.next().unwrap_or_default(),
        inners: top_loops.collect(),
        tag: FaceTag::EndCap,
        owner: 0,
    });
    let mut bottom_loops =
        rings.iter().map(|r| Loop { coedges: r.bottom.iter().rev().copied().map(rev).collect() });
    solid.add_face(Face {
        surface: Surface::Plane { frame: bottom },
        reversed: true,
        outer: bottom_loops.next().unwrap_or_default(),
        inners: bottom_loops.collect(),
        tag: FaceTag::StartCap,
        owner: 0,
    });

    for (ci, ((b, t), r)) in pairs.iter().zip(&rings).enumerate() {
        let n = b.segments.len();
        for k in 0..n {
            let next = (k + 1) % n;
            let (surface, reversed) = match (b.segments[k], t.segments[k]) {
                (Segment::Line { a, b: e }, Segment::Line { a: ta, .. }) => {
                    let pa = bottom.to_world(a);
                    let d = bottom.to_world(e) - pa;
                    let slant = top.to_world(ta) - pa;
                    (Surface::Plane { frame: Frame::new(pa, d.cross(slant), d)? }, false)
                }
                (Segment::Arc { center, radius: rb, sweep, .. }, Segment::Arc { radius: rt, .. }) => {
                    let axis_b = bottom.to_world(center);
                    let axis_t = top.to_world(center);
                    let surface = if (rt - rb).abs() < 1e-12 {
                        Surface::Cylinder { frame: bottom.with_origin(axis_b), radius: rb }
                    } else {
                        let half_angle = ((rt - rb).abs() / height).atan();
                        // Framed at the narrow end, opening along its z.
                        let frame = if rt > rb {
                            bottom.with_origin(axis_b)
                        } else {
                            Frame { origin: axis_t, x: top.x, y: -top.y, z: -top.z }
                        };
                        Surface::Cone { frame, radius: rb.min(rt), half_angle, apex: false }
                    };
                    (surface, sweep < 0.0)
                }
                _ => return Err(KernelError::InvalidInput("segment kinds changed".into())),
            };
            solid.add_face(Face {
                surface,
                reversed,
                outer: Loop {
                    coedges: vec![fwd(r.bottom[k]), fwd(r.side[next]), rev(r.top[k]), rev(r.side[k])],
                },
                inners: Vec::new(),
                tag: FaceTag::Side { contour: ci as u32, segment: b.tag(k) },
                owner: 0,
            });
        }
    }
    check::validate(&solid)?;
    Ok(solid)
}

/// Extrudes every region with draft, one solid per region.
pub fn extrude_regions_drafted(
    frame: &Frame,
    regions: &[Region],
    z0: f64,
    z1: f64,
    taper: f64,
) -> Result<Vec<Solid>> {
    regions.iter().map(|r| extrude_drafted(frame, r, z0, z1, taper)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mass, tess};
    use glam::DVec3;
    use std::f64::consts::PI;

    fn polygon(pts: &[(f64, f64)]) -> Loop2 {
        let p: Vec<DVec2> = pts.iter().map(|&(x, y)| DVec2::new(x, y)).collect();
        Loop2::new((0..p.len()).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect())
    }

    fn volume(s: &Solid) -> f64 {
        let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        assert!(m.is_watertight(), "not watertight");
        mass::properties(&m).volume
    }

    fn region(loops: &[Loop2]) -> Region {
        build_regions(loops).expect("region").remove(0)
    }

    #[test]
    fn drafted_box_is_a_frustum() {
        // 20 x 10, 10 high, sides leaning in at 5 degrees: each side moves in by
        // 10 tan 5 at the top. Volume of a prismatoid: h/6 (A0 + 4 Am + A1).
        let t = 5f64.to_radians().tan();
        let r = region(&[polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)])]);
        let s = extrude_drafted(&Frame::world(), &r, 0.0, 10.0, -t).expect("drafted");
        let area = |z: f64| (20.0 - 2.0 * z * t) * (10.0 - 2.0 * z * t);
        let exact = 10.0 / 6.0 * (area(0.0) + 4.0 * area(5.0) + area(10.0));
        assert!((volume(&s) - exact).abs() < 1e-6 * exact, "{} vs {exact}", volume(&s));
        let top = s.vertices.iter().filter(|v| (v.point.z - 10.0).abs() < 1e-9).count();
        assert_eq!(top, 4);
        assert!(s.vertices.iter().any(|v| (v.point - DVec3::new(10.0 * t, 10.0 * t, 10.0)).length() < 1e-9));
    }

    #[test]
    fn drafted_cylinder_is_a_cone_frustum() {
        let t = 10f64.to_radians().tan();
        let r = region(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 5.0)])]);
        // Outward draft: radius 5 at the sketch plane, 5 + 8 t at the top.
        let s = extrude_drafted(&Frame::world(), &r, 0.0, 8.0, t).expect("drafted");
        let (r0, r1) = (5.0, 5.0 + 8.0 * t);
        let exact = PI * 8.0 / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
        assert!((volume(&s) - exact).abs() < 1e-4 * exact, "{} vs {exact}", volume(&s));
        assert!(s.faces.iter().any(|f| matches!(f.surface, Surface::Cone { .. })));
        // Inward, going down from the sketch plane: narrow at the bottom.
        let s = extrude_drafted(&Frame::world(), &r, -8.0, 0.0, t).expect("drafted");
        let r1 = 5.0 - 8.0 * t;
        let exact = PI * 8.0 / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
        assert!((volume(&s) - exact).abs() < 1e-4 * exact, "{} vs {exact}", volume(&s));
    }

    #[test]
    fn plate_with_a_hole_drafts_both_ways() {
        // Inward draft on the outside, and the hole opens out as the outline closes in.
        let t = 3f64.to_radians().tan();
        let outer = polygon(&[(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)]);
        let hole = Loop2::new(vec![Segment::circle(DVec2::ZERO, 6.0)]);
        let r = region(&[outer, hole]);
        let h = 6.0;
        let s = extrude_drafted(&Frame::world(), &r, 0.0, h, -t).expect("drafted");
        let area = |z: f64| (40.0 - 2.0 * z * t).powi(2) - PI * (6.0 + z * t).powi(2);
        let exact = h / 6.0 * (area(0.0) + 4.0 * area(h / 2.0) + area(h));
        assert!((volume(&s) - exact).abs() < 1e-4 * exact, "{} vs {exact}", volume(&s));
    }

    #[test]
    fn slot_with_smooth_ends_drafts() {
        // An obround: two lines joined smoothly by half circles.
        let l = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, -5.0), b: DVec2::new(20.0, -5.0) },
            Segment::Arc { center: DVec2::new(20.0, 0.0), radius: 5.0, start: -0.5 * PI, sweep: PI },
            Segment::Line { a: DVec2::new(20.0, 5.0), b: DVec2::new(0.0, 5.0) },
            Segment::Arc { center: DVec2::new(0.0, 0.0), radius: 5.0, start: 0.5 * PI, sweep: PI },
        ]);
        let t = 2f64.to_radians().tan();
        let r = region(&[l]);
        let s = extrude_drafted(&Frame::world(), &r, 0.0, 10.0, -t).expect("drafted");
        let area = |z: f64| {
            let rr = 5.0 - z * t;
            20.0 * 2.0 * rr + PI * rr * rr
        };
        let exact = 10.0 / 6.0 * (area(0.0) + 4.0 * area(5.0) + area(10.0));
        assert!((volume(&s) - exact).abs() < 1e-4 * exact, "{} vs {exact}", volume(&s));
    }

    #[test]
    fn drafted_boss_unites_with_a_plate() {
        use crate::boolean::{BoolOp, boolean};
        let plate = region(&[polygon(&[(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)])]);
        let base = crate::extrude::extrude(&Frame::world(), &plate, -5.0, 0.0).expect("plate");
        let t = 5f64.to_radians().tan();
        let boss = region(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 8.0)])]);
        let b = extrude_drafted(&Frame::world(), &boss, 0.0, 10.0, -t).expect("boss");
        let u = boolean(&base, &b, BoolOp::Union).expect("union");
        assert_eq!(u.len(), 1);
        let r1 = 8.0 - 10.0 * t;
        let exact = 40.0 * 40.0 * 5.0 + PI * 10.0 / 3.0 * (64.0 + 8.0 * r1 + r1 * r1);
        assert!((volume(&u[0]) - exact).abs() < 1e-4 * exact, "{} vs {exact}", volume(&u[0]));
    }

    #[test]
    fn refuses_sharp_corners_at_arcs_and_overdrafts() {
        // A D shape: an arc meeting a line at sharp corners.
        let d = Loop2::new(vec![
            Segment::Arc { center: DVec2::ZERO, radius: 5.0, start: -0.5 * PI, sweep: PI },
            Segment::Line { a: DVec2::new(0.0, 5.0), b: DVec2::new(0.0, -5.0) },
        ]);
        let r = region(&[d]);
        assert!(matches!(
            extrude_drafted(&Frame::world(), &r, 0.0, 5.0, -0.1),
            Err(KernelError::Unsupported(_))
        ));
        // A 4 mm square drafted in by 45 degrees over 3 mm closes up.
        let sq = region(&[polygon(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)])]);
        assert!(extrude_drafted(&Frame::world(), &sq, 0.0, 3.0, -1.0).is_err());
    }
}
