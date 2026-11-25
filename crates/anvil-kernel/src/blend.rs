//! Constant-radius fillets and equal-distance chamfers on model edges.
//!
//! Each edge is named by the two faces it separates. For every edge we build the exact
//! "corner" region between the two faces and the rounding surface:
//! - straight edges between two planar faces: a cross-section of corner minus circle,
//!   extruded along the edge (the rounding surface is a cylinder);
//! - circular edges between a plane and a cylinder (boss tops, hole rims, boss roots): a
//!   corner minus quarter-circle profile revolved about the cylinder's own axis (the
//!   rounding surface is a torus).
//!
//! Convex edges subtract the region, concave edges add it. The region's flat sides are
//! lifted slightly off the body's faces (into air for cuts, into material for additions) so
//! the booleans never have to resolve exactly coincident faces.

use crate::build::{extrude, revolve, CHORD_TOL};
use crate::math::{v2, Frame, Vec2, Vec3};
use crate::profile::{point_in_polygon, Loop2, Profile, Seg2};
use crate::solid::{EdgeRef, FaceId, Polygon, Solid, SurfaceKind};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeTreatment {
    Fillet { radius: f64 },
    Chamfer { distance: f64 },
}

impl EdgeTreatment {
    fn size(&self) -> f64 {
        match *self {
            EdgeTreatment::Fillet { radius } => radius,
            EdgeTreatment::Chamfer { distance } => distance,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlendError {
    EdgeNotFound(EdgeRef),
    Unsupported(String),
    /// The blend is too wide for a face next to each of `edges`. `size` is the radius or
    /// distance asked for; `limit` is the largest of the same measure that every one of
    /// those edges allows, so any size under it fits them.
    TooLarge {
        edges: Vec<EdgeRef>,
        size: f64,
        limit: f64,
    },
    Invalid(String),
}

/// A number for a message: up to four decimals, without trailing zeros.
fn short(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl std::fmt::Display for BlendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlendError::EdgeNotFound(e) => write!(
                f,
                "an edge between faces <{}:{}> and <{}:{}> no longer exists",
                e.a.feature, e.a.index, e.b.feature, e.b.index
            ),
            BlendError::Unsupported(s) => write!(f, "{s} are not supported yet"),
            BlendError::TooLarge { edges, size, limit } => {
                let which =
                    if edges.len() == 1 { "an edge".to_string() } else { format!("{} edges", edges.len()) };
                write!(
                    f,
                    "a size of {} does not fit on the faces next to {which}; at most {} fits",
                    short(*size),
                    short(*limit)
                )
            }
            BlendError::Invalid(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for BlendError {}

/// How far region faces are lifted off the body's faces. Larger than the chord tolerance
/// so faceted curved walls are fully covered.
const LIFT: f64 = 2.0 * CHORD_TOL;

/// Applies a fillet or chamfer to the given edges. New faces are tagged with `feature`.
pub fn blend_edges(
    solid: &Solid,
    edges: &[EdgeRef],
    t: EdgeTreatment,
    feature: u32,
) -> Result<Solid, BlendError> {
    if t.size().is_nan() || t.size() <= 0.0 {
        return Err(BlendError::Invalid("the size must be greater than zero".into()));
    }
    if edges.is_empty() {
        return Err(BlendError::Invalid("select at least one edge".into()));
    }
    let shared = solid.shared_segments();
    let mut removes: Vec<Solid> = Vec::new();
    let mut adds: Vec<Solid> = Vec::new();
    // Edges the blend is too wide for, with the size each allows. Every edge is checked,
    // so the error can name all of them rather than only the first.
    let mut too_large: Vec<(EdgeRef, f64)> = Vec::new();
    for (k, e) in edges.iter().enumerate() {
        // Segments of this edge as (segment, polygon on face a, polygon on face b).
        let segs: Vec<([Vec3; 2], usize, usize)> = shared
            .iter()
            .filter_map(|&(s, i, j)| {
                let (ti, tj) = (solid.polygons[i].tag.id, solid.polygons[j].tag.id);
                if ti == e.a && tj == e.b {
                    Some((s, i, j))
                } else if ti == e.b && tj == e.a {
                    Some((s, j, i))
                } else {
                    None
                }
            })
            .collect();
        if segs.is_empty() || e.a == e.b {
            return Err(BlendError::EdgeNotFound(*e));
        }
        let base = k as u32 * 8;
        let (sa, sb) = (solid.polygons[segs[0].1].tag.surface, solid.polygons[segs[0].2].tag.surface);
        let (region, convex) = match (sa, sb) {
            (SurfaceKind::Plane, SurfaceKind::Plane) => {
                let regions = match linear_regions(solid, &segs, t, feature, base) {
                    Err(Fit::TooLarge(limit)) => {
                        too_large.push((*e, limit));
                        continue;
                    }
                    Err(Fit::Failed(err)) => return Err(err),
                    Ok(r) => r,
                };
                for r in regions {
                    if r.1 {
                        removes.push(r.0);
                    } else {
                        adds.push(r.0);
                    }
                }
                continue;
            }
            (SurfaceKind::Plane, SurfaceKind::Cylinder { .. })
            | (SurfaceKind::Cylinder { .. }, SurfaceKind::Plane) => {
                let plane_first = matches!(sa, SurfaceKind::Plane);
                let segs: Vec<_> =
                    segs.iter().map(|&(s, a, b)| if plane_first { (s, a, b) } else { (s, b, a) }).collect();
                match circular_region(solid, &segs, t, feature, base) {
                    Err(Fit::TooLarge(limit)) => {
                        too_large.push((*e, limit));
                        continue;
                    }
                    Err(Fit::Failed(err)) => return Err(err),
                    Ok(r) => r,
                }
            }
            _ => {
                return Err(BlendError::Unsupported(
                    "fillets and chamfers between these kinds of curved faces".into(),
                ))
            }
        };
        if convex {
            removes.push(region);
        } else {
            adds.push(region);
        }
    }
    if !too_large.is_empty() {
        let limit = too_large.iter().map(|&(_, l)| l).fold(f64::INFINITY, f64::min);
        return Err(BlendError::TooLarge {
            edges: too_large.into_iter().map(|(e, _)| e).collect(),
            size: t.size(),
            limit,
        });
    }
    let mut body = solid.clone();
    for c in &removes {
        body = body.subtract(c);
    }
    for c in &adds {
        body = body.union(c);
    }
    Ok(body)
}

/// Why one edge could not be blended: too wide for its faces, allowing at most the size
/// given, or any other blend error.
enum Fit {
    TooLarge(f64),
    Failed(BlendError),
}

impl From<BlendError> for Fit {
    fn from(e: BlendError) -> Fit {
        Fit::Failed(e)
    }
}

fn centroid(p: &Polygon) -> Vec3 {
    p.verts.iter().fold(Vec3::ZERO, |s, &v| s + v) / p.verts.len() as f64
}

/// Builds the closed 2D outline of a corner region.
/// `k` is the corner, `a`/`b` the points where the treatment meets each face, `na`/`nb`
/// the face normals (2D) used to lift the flat sides by `lift`.
#[allow(clippy::too_many_arguments)]
fn corner_loop(
    k: Vec2,
    a: Vec2,
    b: Vec2,
    arc_center: Option<Vec2>,
    na: Vec2,
    nb: Vec2,
    lift: f64,
    base: u32,
) -> Loop2 {
    let k2 = k + (na + nb) * lift;
    let a2 = a + na * lift;
    let b2 = b + nb * lift;
    let line = |p: Vec2, q: Vec2, s: u32| Seg2::Line { a: p, b: q, source: base + s };
    let middle = match arc_center {
        Some(c) => {
            let r = a.distance(c);
            let start = (a - c).angle();
            // The rounding arc is the short way round, facing the corner.
            let mut sweep = (b - c).angle() - start;
            while sweep > PI {
                sweep -= TAU;
            }
            while sweep < -PI {
                sweep += TAU;
            }
            Seg2::Arc { center: c, radius: r, start, sweep, source: base + 2 }
        }
        None => line(a, b, 2),
    };
    let mut segs = vec![line(k2, a2, 0)];
    if lift.abs() > 0.0 {
        segs.push(line(a2, a, 1));
    }
    segs.push(middle);
    if lift.abs() > 0.0 {
        segs.push(line(b, b2, 3));
    }
    segs.push(line(b2, k2, 4));
    Loop2::new(segs)
}

/// Straight edges between two planar faces. Returns (region, convex) per run of the edge.
fn linear_regions(
    solid: &Solid,
    segs: &[([Vec3; 2], usize, usize)],
    t: EdgeTreatment,
    feature: u32,
    base: u32,
) -> Result<Vec<(Solid, bool)>, Fit> {
    let (pa, pb) = (&solid.polygons[segs[0].1], &solid.polygons[segs[0].2]);
    let (na, nb) = (pa.normal, pb.normal);
    let e = na.cross(nb);
    if e.length() < 1e-9 {
        return Err(BlendError::Invalid("the two faces are parallel".into()).into());
    }
    let e = e.normalized();
    let p0 = segs[0].0[0];
    // In-face directions perpendicular to the edge, pointing into each face.
    let into = |n: Vec3, poly: &Polygon| {
        let u = e.cross(n).normalized();
        if u.dot(centroid(poly) - p0) >= 0.0 {
            u
        } else {
            -u
        }
    };
    let (ua, ub) = (into(na, pa), into(nb, pb));
    let theta = ua.dot(ub).clamp(-1.0, 1.0).acos();
    if !(1e-3..PI - 1e-3).contains(&theta) {
        return Err(BlendError::Unsupported("edges between tangent faces".into()).into());
    }
    // The wedge between the faces is material when face b dips behind face a.
    let convex = na.dot(ub) < 0.0;
    let lift = if convex { LIFT } else { -LIFT };

    // Merge the edge's segments into continuous runs along the edge line.
    let mut spans: Vec<(f64, f64)> = segs
        .iter()
        .map(|(s, _, _)| {
            let (t0, t1) = ((s[0] - p0).dot(e), (s[1] - p0).dot(e));
            (t0.min(t1), t0.max(t1))
        })
        .collect();
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut runs: Vec<(f64, f64)> = Vec::new();
    for s in spans {
        match runs.last_mut() {
            Some(r) if s.0 <= r.1 + 1e-6 => r.1 = r.1.max(s.1),
            _ => runs.push(s),
        }
    }

    // The blend must fit on both faces, as on the exact kernel: the point it sets back
    // to, from the middle of each run, has to lie on the face. Without this a blend wider
    // than a face cuts or adds material far from the edge (issue #172).
    // The setback is how far from the edge the blend meets each face; `per` converts it
    // back to the treatment's own size (a fillet's radius, a chamfer's distance).
    let per = match t {
        EdgeTreatment::Fillet { .. } => (theta / 2.0).tan(),
        EdgeTreatment::Chamfer { .. } => 1.0,
    };
    let setback = t.size() / per;
    let fits = |face: FaceId, n: Vec3, at: Vec3| face_contains(solid, face, n, e, at);
    let sides = [(pa.tag.id, na, ua), (pb.tag.id, nb, ub)];
    let too_wide = runs.iter().any(|&(lo, hi)| {
        let mid = p0 + e * ((lo + hi) / 2.0);
        sides.iter().any(|&(face, n, u)| !fits(face, n, mid + u * (setback * 0.999)))
    });
    if too_wide {
        // The largest setback every run's faces allow, found by bisection from the edge
        // (which always lies on the face) out to the setback asked for.
        let mut most = setback;
        for &(lo, hi) in &runs {
            let mid = p0 + e * ((lo + hi) / 2.0);
            for &(face, n, u) in &sides {
                let (mut inside, mut outside) = (0.0, most);
                if fits(face, n, mid + u * outside) {
                    continue;
                }
                for _ in 0..60 {
                    let m = (inside + outside) / 2.0;
                    if fits(face, n, mid + u * m) {
                        inside = m;
                    } else {
                        outside = m;
                    }
                }
                most = most.min(inside);
            }
        }
        return Err(Fit::TooLarge(most * per));
    }

    // Cross-section frame: x along face a, z along the edge.
    let to2 = |v: Vec3, x: Vec3, y: Vec3| v2(v.dot(x), v.dot(y));
    let mut out = Vec::new();
    for (lo, hi) in runs {
        let origin = p0 + e * lo;
        let (x, z) = (ua, e);
        let y = z.cross(x);
        let frame = Frame { origin, x, y, z };
        let ub2 = to2(ub, x, y);
        let (na2, nb2) = (to2(na, x, y), to2(nb, x, y));
        let (a, b, center) = match t {
            EdgeTreatment::Fillet { radius } => {
                let d = radius / (theta / 2.0).tan();
                let bis = (v2(1.0, 0.0) + ub2).normalized();
                (v2(d, 0.0), ub2 * d, Some(bis * (radius / (theta / 2.0).sin())))
            }
            EdgeTreatment::Chamfer { distance } => (v2(distance, 0.0), ub2 * distance, None),
        };
        let lp = corner_loop(Vec2::ZERO, a, b, center, na2, nb2, lift, base);
        let region = extrude(&[Profile::new(lp, vec![])], &frame, 0.0, hi - lo, feature)
            .map_err(|e| BlendError::Invalid(e.to_string()))?;
        out.push((region, convex));
    }
    Ok(out)
}

/// Whether point `p`, on the plane of face `face` (outward normal `n`), lies on that face:
/// inside, or within a hair of the edge of, one of its polygons. `along` is any direction
/// in the plane, used to lay the polygons flat.
fn face_contains(solid: &Solid, face: FaceId, n: Vec3, along: Vec3, p: Vec3) -> bool {
    const TOL: f64 = 1e-7;
    let (x, y) = (along, n.cross(along));
    let flat = |v: Vec3| v2(v.dot(x), v.dot(y));
    let q = flat(p);
    solid.polygons.iter().filter(|poly| poly.tag.id == face && poly.normal.dot(n) > 1.0 - 1e-6).any(|poly| {
        let pts: Vec<Vec2> = poly.verts.iter().map(|&v| flat(v)).collect();
        if point_in_polygon(q, &pts) {
            return true;
        }
        // On the polygon's boundary: closer than TOL to one of its sides.
        (0..pts.len()).any(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let ab = b - a;
            let t = ((q - a).dot(ab) / ab.dot(ab).max(1e-300)).clamp(0.0, 1.0);
            (a + ab * t - q).length() < TOL
        })
    })
}

/// Circular edge between a plane (first polygon of each pair) and a cylinder (second).
fn circular_region(
    solid: &Solid,
    segs: &[([Vec3; 2], usize, usize)],
    t: EdgeTreatment,
    feature: u32,
    base: u32,
) -> Result<(Solid, bool), Fit> {
    let (pp, pc) = (&solid.polygons[segs[0].1], &solid.polygons[segs[0].2]);
    let SurfaceKind::Cylinder { axis_origin: o, axis_dir, radius: big_r } = pc.tag.surface else {
        unreachable!()
    };
    let dir = axis_dir.normalized();
    let np = pp.normal;
    if np.dot(dir).abs() < 1.0 - 1e-6 {
        return Err(BlendError::Unsupported(
            "edges between a cylinder and a plane not square to its axis".into(),
        )
        .into());
    }
    let axial = |p: Vec3| (p - o).dot(dir);
    let radial = |p: Vec3| {
        let d = p - o;
        (d - dir * d.dot(dir)).length()
    };
    let h = axial(pp.verts[0]);
    let sign = |x: f64| if x >= 0.0 { 1.0 } else { -1.0 };
    // Which side of the edge each face lies on, judged by its farthest vertex.
    let far = |poly: &Polygon, f: &dyn Fn(Vec3) -> f64| {
        poly.verts.iter().map(|&v| f(v)).fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m })
    };
    let sigma_s = sign(far(pc, &|v| axial(v) - h));
    // Cap triangles can have every vertex on the edge circle, so judge the plane side by
    // centroids, which lie strictly inside their face. Prefer the clearest one.
    let sigma_r =
        sign(segs.iter().map(|s| radial(centroid(&solid.polygons[s.1])) - big_r).fold(0.0f64, |m, x| {
            if x.abs() > m.abs() {
                x
            } else {
                m
            }
        }));
    let plane_up = sign(np.dot(dir));
    // Material lies opposite the plane's outward normal; the corner between the faces is
    // material (convex edge) when the cylinder wall runs into it.
    let convex = sigma_s == -plane_up;
    // Outward normal of the cylinder wall in the (radial, axial) half-plane.
    let c0 = centroid(pc);
    let rdir = {
        let d = c0 - o;
        (d - dir * d.dot(dir)).normalized()
    };
    let wall_out = sign(pc.normal.dot(rdir));

    let size = t.size();
    // Inside a hole or around a boss's foot the blend runs on the plane towards the axis,
    // so it must stay under the cylinder's radius.
    if sigma_r < 0.0 && size >= big_r - 1e-9 {
        return Err(Fit::TooLarge(big_r));
    }

    // Angular extent of the edge around the axis.
    let u_ref = dir.any_perpendicular();
    let w_ref = u_ref.cross(dir);
    let ang = |p: Vec3| {
        let d = p - o;
        d.dot(w_ref).atan2(d.dot(u_ref)).rem_euclid(TAU)
    };
    let mut angles: Vec<f64> = segs.iter().flat_map(|(s, _, _)| [ang(s[0]), ang(s[1])]).collect();
    angles.sort_by(|a, b| a.total_cmp(b));
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let (start, span) = if angles.len() < 2 {
        (0.0, TAU)
    } else {
        let mut gap = (angles[0] + TAU - angles[angles.len() - 1], 0usize);
        for i in 1..angles.len() {
            let g = angles[i] - angles[i - 1];
            if g > gap.0 {
                gap = (g, i);
            }
        }
        // A full circle has only facet-sized gaps.
        if gap.0 < 0.5 {
            (0.0, TAU)
        } else {
            (angles[gap.1], TAU - gap.0)
        }
    };
    let u0 = u_ref * start.cos() + w_ref * start.sin();
    let frame = Frame { origin: o, x: u0, y: dir, z: u0.cross(dir) };

    let k = v2(big_r, h);
    let (a, b, center) = match t {
        EdgeTreatment::Fillet { radius } => (
            k + v2(sigma_r * radius, 0.0),
            k + v2(0.0, sigma_s * radius),
            Some(k + v2(sigma_r * radius, sigma_s * radius)),
        ),
        EdgeTreatment::Chamfer { distance } => {
            (k + v2(sigma_r * distance, 0.0), k + v2(0.0, sigma_s * distance), None)
        }
    };
    let lift = if convex { LIFT } else { -LIFT };
    let lp = corner_loop(k, a, b, center, v2(0.0, plane_up), v2(wall_out, 0.0), lift, base);
    let region = revolve(&[Profile::new(lp, vec![])], &frame, Vec2::ZERO, v2(0.0, 1.0), span, feature)
        .map_err(|e| BlendError::Invalid(e.to_string()))?;
    Ok((region, convex))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{block, cylinder};
    use crate::math::v3;
    use crate::solid::FaceId;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// The box from `block` has faces: 0 bottom (z=min), 1 top, sides 2..5 from the profile's
    /// edges: 2 (y=min), 3 (x=max), 4 (y=max), 5 (x=min).
    fn face(feature: u32, index: u32) -> FaceId {
        FaceId { feature, index }
    }

    /// Issue #172: a blend wider than a face next to its edge is refused, not built into a
    /// broken body. The 20 x 10 x 10 box's edge between its top and its x = max side is
    /// 10 mm long, with 20 mm of top and 10 mm of side next to it.
    #[test]
    fn a_blend_that_does_not_fit_its_faces_is_refused() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let edge = EdgeRef::new(face(1, 1), face(1, 3));
        let blend = |t| blend_edges(&b, &[edge], t, 9);
        // The 10 mm side is the narrower face, so it sets the limit for both: a right
        // angle sets a fillet back by its radius and a chamfer by its distance.
        let too_large = |size: f64| Some(BlendError::TooLarge { edges: vec![edge], size, limit: 10.0 });
        let near = |e: Option<BlendError>, want: Option<BlendError>| match (e, want) {
            (
                Some(BlendError::TooLarge { edges, size, limit }),
                Some(BlendError::TooLarge { edges: we, size: ws, limit: wl }),
            ) => edges == we && size == ws && (limit - wl).abs() < 1e-6,
            _ => false,
        };
        let got = blend(EdgeTreatment::Fillet { radius: 100.0 }).err();
        assert!(near(got.clone(), too_large(100.0)), "{got:?}");
        assert!(near(blend(EdgeTreatment::Chamfer { distance: 10.5 }).err(), too_large(10.5)));
        assert_eq!(
            blend(EdgeTreatment::Chamfer { distance: 10.5 }).err().map(|e| e.to_string()).as_deref(),
            Some("a size of 10.5 does not fit on the faces next to an edge; at most 10 fits")
        );
        // Just inside the 10 mm side still fits, and removes what it should.
        let out = blend(EdgeTreatment::Fillet { radius: 9.9 }).expect("fits");
        let removed = (1.0 - PI / 4.0) * 9.9 * 9.9 * 10.0;
        assert!(rel(out.mass_properties().volume, 2000.0 - removed) < 1e-3);
    }

    #[test]
    fn fillet_one_box_edge() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let edge = EdgeRef::new(face(1, 1), face(1, 3)); // top meets x = max
        let r = 2.0;
        let out = blend_edges(&b, &[edge], EdgeTreatment::Fillet { radius: r }, 9).unwrap();
        let removed = (1.0 - PI / 4.0) * r * r * 10.0;
        let v = out.mass_properties().volume;
        assert!(rel(v, 2000.0 - removed) < 1e-3, "{v}");
        assert_eq!(out.open_edges(), 0);
        // The rounding face is tagged with the fillet feature.
        assert!(out.polygons.iter().any(|p| p.tag.id.feature == 9));
    }

    #[test]
    fn chamfer_one_box_edge() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let edge = EdgeRef::new(face(1, 1), face(1, 3));
        let out = blend_edges(&b, &[edge], EdgeTreatment::Chamfer { distance: 3.0 }, 9).unwrap();
        let v = out.mass_properties().volume;
        assert!(rel(v, 2000.0 - 0.5 * 9.0 * 10.0) < 1e-4, "{v}");
    }

    #[test]
    fn fillet_inside_corner_adds_material() {
        // L-shape: a 20x10x2 plate with a 2x10x10 wall at x = 0..2.
        let plate = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 2.0), 1).unwrap();
        let wall = block(v3(0.0, 0.0, 0.0), v3(2.0, 10.0, 12.0), 2).unwrap();
        let l = plate.union(&wall);
        // Concave edge: plate top (feature 1, top cap) meets the wall's x = max side.
        let edge = EdgeRef::new(face(1, 1), face(2, 3));
        let r = 3.0;
        let before = l.mass_properties().volume;
        let out = blend_edges(&l, &[edge], EdgeTreatment::Fillet { radius: r }, 9).unwrap();
        let added = (1.0 - PI / 4.0) * r * r * 10.0;
        let v = out.mass_properties().volume;
        assert!(rel(v, before + added) < 1e-3, "{v} vs {}", before + added);
    }

    #[test]
    fn fillet_boss_top_and_hole_rim() {
        // Boss: cylinder r = 10, h = 20; fillet its top edge (convex torus cut).
        let c = cylinder(v3(0.0, 0.0, 0.0), 10.0, 20.0, 1).unwrap();
        let top = EdgeRef::new(face(1, 1), face(1, 2));
        let r = 2.0;
        let out = blend_edges(&c, &[top], EdgeTreatment::Fillet { radius: r }, 9).unwrap();
        // Removed volume: corner area times the path length of its centroid (Pappus).
        let area = (1.0 - PI / 4.0) * r * r;
        // The corner region's centroid sits (5/6 - pi/4) / (1 - pi/4) * r in from the edge.
        let cx = 10.0 - r * (5.0 / 6.0 - PI / 4.0) / (1.0 - PI / 4.0);
        let removed = area * TAU * cx;
        let v0 = c.mass_properties().volume;
        let v = out.mass_properties().volume;
        assert!(rel(v0 - v, removed) < 0.03, "removed {} vs {removed}", v0 - v);

        // Plate with a hole: fillet the hole's top rim.
        let plate = block(v3(-20.0, -20.0, 0.0), v3(20.0, 20.0, 5.0), 3).unwrap();
        let hole = cylinder(v3(0.0, 0.0, -1.0), 6.0, 7.0, 4).unwrap();
        let p = plate.subtract(&hole);
        let rim = EdgeRef::new(face(3, 1), face(4, 2));
        let before = p.mass_properties().volume;
        let out = blend_edges(&p, &[rim], EdgeTreatment::Fillet { radius: 1.0 }, 9).unwrap();
        let v = out.mass_properties().volume;
        assert!(v < before && v > before - 10.0, "{before} -> {v}");
    }

    #[test]
    fn fillet_all_box_edges() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let faces: Vec<FaceId> = (0..6).map(|i| face(1, i)).collect();
        let opposite = |i: u32, j: u32| matches!((i.min(j), i.max(j)), (0, 1) | (2, 4) | (3, 5));
        let mut edges = Vec::new();
        for i in 0..6u32 {
            for j in i + 1..6 {
                if !opposite(i, j) {
                    edges.push(EdgeRef::new(faces[i as usize], faces[j as usize]));
                }
            }
        }
        assert_eq!(edges.len(), 12);
        let r = 2.0;
        let out = blend_edges(&b, &edges, EdgeTreatment::Fillet { radius: r }, 9).unwrap();
        let v = out.mass_properties().volume;
        // Edges remove (1 - pi/4) r^2 per unit length; the corners, where three fillets
        // meet, keep a little more than a sphere would. Bound it between the two.
        let edge_len = 4.0 * (20.0 + 10.0 + 10.0) - 24.0 * r;
        let edges_only = (1.0 - PI / 4.0) * r * r * edge_len;
        let sphere_corners = 8.0 * (1.0 - PI / 6.0) * r * r * r;
        let lo = 2000.0 - edges_only - sphere_corners;
        let hi = 2000.0 - edges_only;
        assert!(v > lo - 1.0 && v < hi, "{lo} < {v} < {hi}");
        assert_eq!(out.open_edges(), 0);
    }

    #[test]
    fn errors() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 1).unwrap();
        let missing = EdgeRef::new(face(1, 0), face(1, 1)); // top and bottom never meet
        assert!(matches!(
            blend_edges(&b, &[missing], EdgeTreatment::Fillet { radius: 1.0 }, 9),
            Err(BlendError::EdgeNotFound(_))
        ));
        let c = cylinder(v3(0.0, 0.0, 0.0), 2.0, 5.0, 1).unwrap();
        assert!(matches!(
            blend_edges(
                &c,
                &[EdgeRef::new(face(1, 1), face(1, 2))],
                EdgeTreatment::Fillet { radius: 3.0 },
                9
            ),
            Err(BlendError::TooLarge { size: 3.0, limit: 2.0, .. })
        ));
    }

    /// Every edge a blend is too wide for is named, and the limit is the tightest one.
    /// The 20 x 10 x 6 box's top meets its 10 mm long x = max side and its 6 mm tall
    /// y = min side; an 8 mm fillet fits on neither, but does on the upright edge between
    /// those sides, which are 20 and 10 mm wide.
    #[test]
    fn a_blend_too_large_for_several_edges_names_them_all() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 6.0), 1).unwrap();
        let x_side = EdgeRef::new(face(1, 1), face(1, 3));
        let y_side = EdgeRef::new(face(1, 1), face(1, 2));
        let fits = EdgeRef::new(face(1, 2), face(1, 3));
        match blend_edges(&b, &[x_side, fits, y_side], EdgeTreatment::Fillet { radius: 8.0 }, 9) {
            Err(BlendError::TooLarge { edges, size, limit }) => {
                assert_eq!(edges, vec![x_side, y_side]);
                assert_eq!(size, 8.0);
                // The y = min side is only 6 mm tall.
                assert!((limit - 6.0).abs() < 1e-6, "{limit}");
            }
            other => panic!("{other:?}"),
        }
    }
}
