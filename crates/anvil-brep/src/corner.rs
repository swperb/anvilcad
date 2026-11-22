//! Ball corners: closing the corner where fillets of one radius meet at a vertex.
//!
//! Where every edge at a vertex is rounded with the same radius, the rolling ball that
//! rounds them comes to rest touching all the faces there, and a patch of its sphere
//! closes the corner, with each round running into it tangentially. Rounding the edges
//! one after another does not make that patch: the rounds' cylinders run on into each
//! other and leave the corner bulging (or, at a concave corner, short of material).
//!
//! The patch is made with one more exact boolean per corner, after the edges are rounded.
//! With `c` the ball's centre (at the radius from every face, on the material's side at a
//! convex corner and on the empty side at a concave one) and `n_i` the faces' normals
//! pointing out of the side being rounded, the points whose nearest point of the faces
//! moved in by the radius is `c` itself form the cone at `c` spanned by the `n_i`. Its
//! sides are the planes through `c` square to each edge, where each round's cylinder
//! meets the ball. Inside that cone the rounded part is the ball and nothing else, so the
//! tool is the cone (cut off well beyond the vertex) less the ball: cut from the part at
//! a convex corner, added to it at a concave one.
//!
//! Only corners whose faces are all flat, whose edges are all straight, all selected and
//! all bending the same way are closed this way; other corners are left as the rounds
//! make them, as before. Such a corner whose faces have no common ball (the apex of a
//! pyramid on a rectangle) is refused rather than built wrong. The behaviour is specified in
//! `docs/re/fillet-corners.md`.

use crate::boolean::{self, BoolOp};
use crate::geom::{Curve, Frame, Surface};
use crate::loft::{Section, loft_ruled};
use crate::profile::{Loop2, Segment, build_regions};
use crate::topo::{EdgeId, Solid, VertexId};
use crate::{KernelError, Result};
use glam::{DVec2, DVec3};
use std::f64::consts::{PI, TAU};

/// A corner to close with a patch of a ball.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// Where the edges met.
    pub vertex: DVec3,
    /// The ball's centre.
    pub centre: DVec3,
    pub radius: f64,
    /// The faces' normals, pointing out of the side being rounded (out of the part at a
    /// convex corner, into it at a concave one), in order round the corner.
    pub normals: Vec<DVec3>,
    /// The edges that meet there, and their directions away from the vertex.
    pub edges: Vec<(EdgeId, DVec3)>,
    /// True where the edges are convex (the patch is cut away), false where they are
    /// concave (it is added).
    pub convex: bool,
}

fn unsupported<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

/// The corners of `solid` that rounding all of `edges` with `radius` closes with a ball:
/// the vertices at which every edge is among `edges`, at least three of them, all of
/// them straight creases bending the same way between flat faces. Other vertices are
/// left to the rounds alone, as before: those with an edge left sharp, a smooth join
/// (the end of a rounded outline's arc), a curved edge or face, or both convex and
/// concave edges, whose corner has more than one shape.
///
/// Refuses a corner that should be a ball's but that the kernel cannot close: flat faces
/// that no one ball touches (a pyramid's apex on a rectangle), or patches at the two ends
/// of an edge that would overlap.
pub fn ball_corners(solid: &Solid, edges: &[EdgeId], radius: f64) -> Result<Vec<Corner>> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(KernelError::InvalidInput("the radius must be a positive number".into()));
    }
    let mut vertices: Vec<VertexId> = Vec::new();
    for &id in edges {
        if let Some(e) = solid.edge(id) {
            for v in [e.start, e.end] {
                if !vertices.contains(&v) {
                    vertices.push(v);
                }
            }
        }
    }
    let mut out = Vec::new();
    for v in vertices {
        let at: Vec<EdgeId> = (0..solid.edges.len() as u32)
            .map(EdgeId)
            .filter(|id| solid.edge(*id).is_some_and(|e| (e.start == v || e.end == v) && e.start != e.end))
            .collect();
        if at.len() < 3 || !at.iter().all(|id| edges.contains(id)) {
            continue;
        }
        if let Some(c) = corner_at(solid, v, &at, radius)? {
            out.push(c);
        }
    }
    // Each round ends where its edge meets a patch: square to the edge through the ball's
    // centre, `(c - v) . e` along it from the vertex. The patches at an edge's two ends
    // must leave room for each other.
    for &id in edges {
        let Some(e) = solid.edge(id) else { continue };
        let len = (e.curve.eval(e.t1) - e.curve.eval(e.t0)).length();
        let taken: f64 = out
            .iter()
            .flat_map(|c| {
                c.edges.iter().filter(|(i, _)| *i == id).map(|(_, d)| (c.centre - c.vertex).dot(*d))
            })
            .sum();
        if taken > len * (1.0 - 1e-9) {
            return Err(KernelError::InvalidInput(format!(
                "a fillet of {radius} does not fit: the corners at the ends of an edge {len:.3} long would overlap"
            )));
        }
    }
    Ok(out)
}

/// The corner at vertex `v`, where the edges `at` meet, or `None` where it is not a
/// ball's (see [`ball_corners`]).
fn corner_at(solid: &Solid, v: VertexId, at: &[EdgeId], radius: f64) -> Result<Option<Corner>> {
    let p = solid.vertex(v).ok_or_else(|| KernelError::InvalidInput("missing vertex".into()))?.point;
    let mut convex = None;
    let mut dirs = Vec::new();
    for &id in at {
        let e = solid.edge(id).ok_or_else(|| KernelError::InvalidInput("missing edge".into()))?;
        let Curve::Line { dir, .. } = e.curve else {
            return Ok(None);
        };
        // The edge's direction away from the vertex.
        let along = if e.start == v { (e.t1 - e.t0).signum() } else { (e.t0 - e.t1).signum() };
        dirs.push((id, dir * along));
        let Some(c) = crate::blend::crease(solid, id) else {
            return Ok(None);
        };
        if convex.is_some_and(|k| k != c) {
            return Ok(None);
        }
        convex = Some(c);
    }
    let convex = convex.unwrap_or(true);
    // The faces at the corner, by their planes' outward normals.
    let mut outward: Vec<DVec3> = Vec::new();
    for &id in at {
        for f in solid.edge_faces(id) {
            let face = &solid.faces[f.index()];
            let Surface::Plane { .. } = face.surface else {
                return Ok(None);
            };
            let n = face.normal_at(p);
            if !outward.iter().any(|m| m.dot(n) > 1.0 - 1e-9) {
                outward.push(n);
            }
        }
    }
    if outward.len() != at.len() {
        return Ok(None);
    }
    // The side being rounded: the material at a convex corner, the empty side at a
    // concave one. Its normals point out of it.
    let sign = if convex { 1.0 } else { -1.0 };
    let normals: Vec<DVec3> = outward.iter().map(|n| *n * sign).collect();
    // The ball's centre: the radius in from every face (least squares over them, then
    // checked against each).
    let centre = p + solve_offset(&normals, -radius)?;
    if normals.iter().any(|n| ((centre - p).dot(*n) + radius).abs() > 1e-9 * (1.0 + radius)) {
        return unsupported(format!(
            "rounding every edge at the corner at {}: no one ball touches all its faces, so the \
             corner would have to split, which is not supported yet; leave one of its edges \
             sharp or round them in separate fillets",
            fmt_point(p)
        ));
    }
    // Order the normals round the corner, about the direction from the centre to the
    // vertex.
    let m = (p - centre).normalize();
    if normals.iter().any(|n| n.dot(m) <= 1e-6) {
        return unsupported(no_patch(p));
    }
    let x = (normals[0] - m * normals[0].dot(m)).normalize();
    let y = m.cross(x);
    let angle = |n: &DVec3| {
        let a = n.dot(y).atan2(n.dot(x));
        if a < 0.0 { a + TAU } else { a }
    };
    let mut normals = normals;
    normals.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    // The vertex must lie in the cone the normals span (between the planes through the
    // centre square to the edges), or the patch would not be where the rounds end.
    for i in 0..normals.len() {
        let (a, b) = (normals[i], normals[(i + 1) % normals.len()]);
        let side = a.cross(b);
        if side.dot(p - centre) < -1e-9 {
            return unsupported(no_patch(p));
        }
    }
    Ok(Some(Corner { vertex: p, centre, radius, normals, edges: dirs, convex }))
}

fn fmt_point(p: DVec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)
}

fn no_patch(p: DVec3) -> String {
    format!(
        "rounding every edge at the corner at {}: the ball cannot close a corner of this shape; \
         leave one of its edges sharp or round them in separate fillets",
        fmt_point(p)
    )
}

/// The offset `d` with `d . n_i = target` for every normal, by least squares.
fn solve_offset(normals: &[DVec3], target: f64) -> Result<DVec3> {
    // (sum n n^T) d = sum n target.
    let mut a = glam::DMat3::ZERO;
    let mut b = DVec3::ZERO;
    for n in normals {
        a += glam::DMat3::from_cols(*n * n.x, *n * n.y, *n * n.z);
        b += *n * target;
    }
    if a.determinant().abs() < 1e-12 {
        return unsupported("rounding every edge at a corner whose faces do not meet at one point");
    }
    Ok(a.inverse() * b)
}

/// The tool that closes `corner`: the cone at the ball's centre spanned by the normals,
/// cut off beyond the vertex, less the ball. Its sphere face is the patch.
pub fn corner_tool(corner: &Corner, tag: u32) -> Result<Solid> {
    let c = corner.centre;
    let m = (corner.vertex - c).normalize();
    // The cone is cut off by a plane square to `m`, just past the vertex. Nothing of the
    // corner lies further: a point of the cone is `c + sum a_i n_i` with every `a_i` at
    // least 0, it lies on the rounded side of every face where `G a <= r` (`G` the normals'
    // Gram matrix), and its distance along `m` is `r sum a_i / |v - c|`, since every
    // `n_i . m` is `r / |v - c|`. The vertex's own coefficients, which solve `G a = r`,
    // also solve the dual of maximising `sum a_i`, so the vertex is the furthest point.
    // A cap further out would reach parts of the solid beyond the corner.
    let reach = (corner.vertex - c).length() + 0.1 * corner.radius;
    let frame = Frame::new(c + m * reach, -m, corner.normals[0] - m * corner.normals[0].dot(m))?;
    let pts: Vec<DVec2> = corner
        .normals
        .iter()
        .map(|n| {
            let q = c + *n * (reach / n.dot(m));
            let l = frame.to_local(q);
            DVec2::new(l.x, l.y)
        })
        .collect();
    let k = pts.len();
    let outline = Loop2::new((0..k).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % k] }).collect());
    // The loop runs counter-clockwise about the frame's normal, which points back at the
    // centre; build_regions orients it.
    let region = build_regions(&[outline])?;
    let outline =
        region.first().ok_or_else(|| KernelError::InvalidInput("degenerate corner".into()))?.outer.clone();
    let cone = loft_ruled(&Section::Loop { frame, outline }, &Section::Point(c), [None, None])?;
    let ball = ball(c, corner.radius, tag)?;
    let mut tool = boolean::boolean(&cone, &ball, BoolOp::Subtract)?;
    if tool.len() != 1 {
        return Err(KernelError::InvalidInput("the corner's patch did not come out in one piece".into()));
    }
    Ok(tool.remove(0))
}

/// The ball of `radius` about `c`, its sphere face tagged `tag`.
fn ball(c: DVec3, radius: f64, tag: u32) -> Result<Solid> {
    let plane = Frame { origin: c, x: DVec3::X, y: DVec3::Z, z: DVec3::NEG_Y };
    let half = Loop2::new(vec![
        Segment::Arc { center: DVec2::ZERO, radius, start: -0.5 * PI, sweep: PI },
        Segment::Line { a: DVec2::new(0.0, radius), b: DVec2::new(0.0, -radius) },
    ]);
    let r = build_regions(&[half])?;
    let region = r.first().ok_or_else(|| KernelError::InvalidInput("degenerate ball".into()))?;
    let mut b = crate::revolve::revolve(&plane, region, DVec2::ZERO, DVec2::Y, TAU)?;
    for f in &mut b.faces {
        if matches!(f.surface, Surface::Sphere { .. }) {
            f.tag = crate::topo::FaceTag::Side { contour: 0, segment: tag };
        }
    }
    Ok(b)
}

/// `solid` (its edges already rounded) with `corner` closed by a patch of the ball, the
/// patch's face owned by `owner` and tagged `tag`.
pub fn close_corner(solid: &Solid, corner: &Corner, owner: u32, tag: u32) -> Result<Vec<Solid>> {
    let mut tool = corner_tool(corner, tag)?;
    tool.set_owner(owner);
    let patch = tool
        .faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Sphere { .. }))
        .ok_or_else(|| KernelError::InvalidInput("the corner's tool has no sphere".into()))?;
    // The rounds' cylinders touch the ball along the circles where they end, without
    // crossing it.
    let contacts: Vec<(usize, usize)> = solid
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| match f.surface {
            Surface::Cylinder { frame, radius } => {
                let d = corner.centre - frame.origin;
                (radius - corner.radius).abs() < 1e-9 * (1.0 + radius)
                    && (d - frame.z * d.dot(frame.z)).length() < 1e-7 * (1.0 + radius)
            }
            _ => false,
        })
        .map(|(i, _)| (i, patch))
        .collect();
    let op = if corner.convex { BoolOp::Subtract } else { BoolOp::Union };
    boolean::boolean_with_contacts(solid, &tool, op, &contacts)
}

#[cfg(test)]
mod tests;
