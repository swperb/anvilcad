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
//! Where the faces moved in by the radius do not meet at one point (the four faces at the
//! apex of a pyramid on a rectangle), the ball cannot touch them all, and the corner
//! splits. Moved in, the faces meet along a short ridge instead, from a point where three
//! of them meet to another; the rounded corner is that ridge grown by the radius: a
//! cylinder about the ridge between the two faces it lies on, and a patch of the ball at
//! each end. Each piece is closed by its own tool of the same kind: the region whose
//! nearest point of the moved-in faces is on that piece (a wedge along the ridge, a cone
//! at each end), less the round.
//!
//! Only corners whose faces are all flat, whose edges are all straight, all selected and
//! all bending the same way are closed this way; other corners are left as the rounds
//! make them, as before. A corner whose moved-in faces meet in anything more involved than
//! one point or one ridge is refused rather than built wrong. The behaviour is specified
//! in `docs/re/fillet-corners.md`.

use crate::boolean::{self, BoolOp};
use crate::geom::{Curve, Frame, Surface};
use crate::loft::{Section, loft_ruled};
use crate::profile::{Loop2, Segment, build_regions};
use crate::topo::{EdgeId, Solid, VertexId};
use crate::{KernelError, Result};
use glam::{DVec2, DVec3};
use std::f64::consts::{PI, TAU};

/// A corner to close: one patch of a ball, or (where the corner splits) a patch at each
/// end of a ridge round.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// Where the edges met.
    pub vertex: DVec3,
    pub radius: f64,
    /// The edges that meet there: each with its direction away from the vertex and its two
    /// faces' normals (pointing out of the side being rounded).
    pub edges: Vec<(EdgeId, DVec3, [DVec3; 2])>,
    /// True where the edges are convex (the corner is cut away), false where they are
    /// concave (it is filled).
    pub convex: bool,
    /// All the faces at the corner, by their normals pointing out of the side being
    /// rounded.
    pub faces: Vec<DVec3>,
    /// The balls' patches: one for a corner the ball closes, two for a split corner.
    pub patches: Vec<Patch>,
    /// The ridge round between a split corner's two patches.
    pub ridges: Vec<Ridge>,
}

/// A patch of a ball of the corner's radius.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub centre: DVec3,
    /// The faces the ball touches, by their normals pointing out of the side being
    /// rounded, in order round the patch.
    pub normals: Vec<DVec3>,
}

/// A round of the corner's radius about the segment from `from` to `to`, between the two
/// faces with these normals.
#[derive(Clone, Debug, PartialEq)]
pub struct Ridge {
    pub from: DVec3,
    pub to: DVec3,
    pub normals: [DVec3; 2],
}

impl Corner {
    /// The faces it adds: its patches, then its ridges.
    pub fn face_count(&self) -> usize {
        self.patches.len() + self.ridges.len()
    }
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
    // centre, `(c - v) . e` along it from the vertex, with `c` the patch of the ball that
    // touches both the edge's faces. The patches at an edge's two ends must leave room for
    // each other.
    for &id in edges {
        let Some(e) = solid.edge(id) else { continue };
        let len = (e.curve.eval(e.t1) - e.curve.eval(e.t0)).length();
        let mut taken = 0.0;
        for c in &out {
            for (i, d, faces) in &c.edges {
                if *i == id {
                    taken += c.setback(*d, faces);
                }
            }
        }
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
    for &id in at {
        let Some(c) = crate::blend::crease(solid, id) else {
            return Ok(None);
        };
        if convex.is_some_and(|k| k != c) {
            return Ok(None);
        }
        convex = Some(c);
    }
    let convex = convex.unwrap_or(true);
    // The side being rounded: the material at a convex corner, the empty side at a
    // concave one. Its normals point out of it.
    let sign = if convex { 1.0 } else { -1.0 };
    let mut normals: Vec<DVec3> = Vec::new();
    let mut edges = Vec::new();
    for &id in at {
        let e = solid.edge(id).ok_or_else(|| KernelError::InvalidInput("missing edge".into()))?;
        let Curve::Line { dir, .. } = e.curve else {
            return Ok(None);
        };
        // The edge's direction away from the vertex.
        let along = if e.start == v { (e.t1 - e.t0).signum() } else { (e.t0 - e.t1).signum() };
        let mut faces = Vec::new();
        for f in solid.edge_faces(id) {
            let face = &solid.faces[f.index()];
            let Surface::Plane { .. } = face.surface else {
                return Ok(None);
            };
            let n = face.normal_at(p) * sign;
            faces.push(n);
            if !normals.iter().any(|m| m.dot(n) > 1.0 - 1e-9) {
                normals.push(n);
            }
        }
        let [a, b] = faces[..] else {
            return Ok(None);
        };
        edges.push((id, dir * along, [a, b]));
    }
    if normals.len() != at.len() {
        return Ok(None);
    }
    let normals_all = normals.clone();
    // The faces moved in by the radius: where they meet in one point, the ball there closes
    // the corner.
    let offset = |x: DVec3, n: &DVec3| (x - p).dot(*n) + radius;
    let tol = 1e-9 * (1.0 + radius + p.length());
    let centre = p + solve_offset(&normals, -radius)?;
    let (patches, ridges) = if normals.iter().all(|n| offset(centre, n).abs() <= tol) {
        (vec![patch(p, centre, normals)?], Vec::new())
    } else {
        // Otherwise they may meet along a ridge: the points where three of them meet that
        // lie inside the rest, and the edge between two such points.
        let mut points: Vec<(DVec3, Vec<DVec3>)> = Vec::new();
        let k = normals.len();
        for i in 0..k {
            for j in i + 1..k {
                for l in j + 1..k {
                    let three = [normals[i], normals[j], normals[l]];
                    let Ok(d) = solve_offset(&three, -radius) else { continue };
                    let x = p + d;
                    if normals.iter().any(|n| offset(x, n) > tol) {
                        continue;
                    }
                    if points.iter().any(|(q, _)| (*q - x).length() <= tol) {
                        continue;
                    }
                    let active = normals.iter().copied().filter(|n| offset(x, n).abs() <= tol).collect();
                    points.push((x, active));
                }
            }
        }
        let split = match &points[..] {
            [(a, na), (b, nb)] if na.len() == 3 && nb.len() == 3 => {
                let shared: Vec<DVec3> =
                    na.iter().copied().filter(|n| nb.iter().any(|m| m.dot(*n) > 1.0 - 1e-9)).collect();
                match shared[..] {
                    [s0, s1] => Some((
                        vec![patch(p, *a, na.clone())?, patch(p, *b, nb.clone())?],
                        vec![Ridge { from: *a, to: *b, normals: [s0, s1] }],
                    )),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(split) = split else {
            return unsupported(format!(
                "rounding every edge at the corner at {}: its faces, moved in by the radius, meet \
                 in a shape the corner cannot be closed for yet; leave one of its edges sharp or \
                 round them in separate fillets",
                fmt_point(p)
            ));
        };
        split
    };
    Ok(Some(Corner { vertex: p, radius, edges, convex, faces: normals_all, patches, ridges }))
}

impl Corner {
    /// Whether the edge `id` of `solid` lies wholly inside the corner: on the line of one of
    /// the corner's edges, between the vertex and where that edge's round meets the corner.
    /// Rounding an edge can cut a neighbouring edge in two there; that piece is not rounded
    /// on its own, as closing the corner takes it away.
    pub fn holds_edge(&self, solid: &Solid, id: EdgeId) -> bool {
        let Some(e) = solid.edge(id) else { return false };
        let (a, b) = (e.curve.eval(e.t0), e.curve.eval(e.t1));
        let tol = 1e-7 * (1.0 + self.radius + self.vertex.length());
        self.edges.iter().any(|(_, d, faces)| {
            let along = |q: DVec3| (q - self.vertex).dot(*d);
            let off = |q: DVec3| {
                let v = q - self.vertex;
                (v - *d * v.dot(*d)).length()
            };
            let reach = self.setback(*d, faces);
            off(a) < tol
                && off(b) < tol
                && along(a) > -tol
                && along(b) > -tol
                && along(a).max(along(b)) <= reach + tol
        })
    }

    /// How far along an edge (direction `d` from the vertex, faces `faces`) its round runs
    /// before it meets the corner: to the plane square to it through the centre of the
    /// patch that touches both its faces.
    fn setback(&self, d: DVec3, faces: &[DVec3; 2]) -> f64 {
        let touches = |p: &Patch, n: &DVec3| p.normals.iter().any(|m| m.dot(*n) > 1.0 - 1e-9);
        self.patches
            .iter()
            .filter(|p| touches(p, &faces[0]) && touches(p, &faces[1]))
            .map(|p| (p.centre - self.vertex).dot(d))
            .fold(0.0, f64::max)
    }
}

/// The patch of the ball centred at `centre` touching the faces with `normals`, at the
/// corner at vertex `p`, its normals put in order round it.
fn patch(p: DVec3, centre: DVec3, normals: Vec<DVec3>) -> Result<Patch> {
    // Order the normals round the patch, about the direction at the same angle to all of
    // them.
    let m = axis(&normals)?;
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
    Ok(Patch { centre, normals })
}

/// The direction at the same angle to every normal (by least squares where there are more
/// than three): for a corner one ball closes, the direction from its centre to the
/// vertex.
fn axis(normals: &[DVec3]) -> Result<DVec3> {
    let m = solve_offset(normals, 1.0)?.normalize_or_zero();
    if normals.iter().any(|n| n.dot(m) <= 1e-6) {
        return unsupported("rounding every edge at a corner this shape");
    }
    Ok(m)
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

/// The tool that closes one patch of a corner at `vertex`, whose faces have the normals
/// `faces`: the cone at the ball's centre spanned by the patch's normals, cut off beyond
/// the corner, less the ball. Its sphere face is the patch.
fn patch_tool(vertex: DVec3, faces: &[DVec3], patch: &Patch, radius: f64, tag: u32) -> Result<Solid> {
    let c = patch.centre;
    let m = axis(&patch.normals)?;
    // The cone is cut off by a plane square to `m`, just past the furthest point of the
    // corner in it. A point of the cone is `c + sum a_i n_i` with every `a_i` at least 0;
    // it lies on the rounded side of every face `n_j` where
    // `(c - v) . n_j + sum a_i n_i . n_j <= 0`. Those points form a small polytope (no
    // direction of the cone keeps to the rounded side of all its own faces), and the
    // furthest along `m` is one of its vertices. A cap further out would reach parts of the
    // solid beyond the corner.
    let reach = furthest(vertex, faces, patch, m)? + 0.1 * radius;
    let frame = Frame::new(c + m * reach, -m, patch.normals[0] - m * patch.normals[0].dot(m))?;
    let pts: Vec<DVec2> = patch
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
    let ball = ball(c, radius, tag, -m)?;
    let mut tool = boolean::boolean(&cone, &ball, BoolOp::Subtract)?;
    if tool.len() != 1 {
        return Err(KernelError::InvalidInput("the corner's patch did not come out in one piece".into()));
    }
    Ok(tool.remove(0))
}

/// How far along `m` from the patch's centre the corner reaches inside the patch's cone
/// (see [`patch_tool`]): the largest `m . sum a_i n_i` over the vertices of the polytope
/// `a_i >= 0`, `(c - v) . n_j + sum a_i n_i . n_j <= 0` for every face `n_j`.
fn furthest(vertex: DVec3, faces: &[DVec3], patch: &Patch, m: DVec3) -> Result<f64> {
    let ns = &patch.normals;
    let k = ns.len();
    // Rows `row . a <= rhs`: the faces, then `-a_i <= 0`.
    let mut rows: Vec<(Vec<f64>, f64)> = faces
        .iter()
        .map(|nj| (ns.iter().map(|ni| ni.dot(*nj)).collect(), -(patch.centre - vertex).dot(*nj)))
        .collect();
    for i in 0..k {
        let mut row = vec![0.0; k];
        row[i] = -1.0;
        rows.push((row, 0.0));
    }
    let tol = 1e-9 * (1.0 + (patch.centre - vertex).length());
    let mut best: Option<f64> = None;
    // Each vertex of the polytope is where `k` of the constraints hold with equality.
    let mut pick = vec![0usize; k];
    fn next(pick: &mut [usize], n: usize) -> bool {
        let k = pick.len();
        for i in (0..k).rev() {
            if pick[i] < n - k + i {
                pick[i] += 1;
                for j in i + 1..k {
                    pick[j] = pick[j - 1] + 1;
                }
                return true;
            }
        }
        false
    }
    for (i, p) in pick.iter_mut().enumerate() {
        *p = i;
    }
    loop {
        if let Some(a) = solve_rows(&pick.iter().map(|&r| rows[r].clone()).collect::<Vec<_>>())
            && rows.iter().all(|(row, rhs)| row.iter().zip(&a).map(|(x, y)| x * y).sum::<f64>() <= rhs + tol)
        {
            let q: DVec3 = ns.iter().zip(&a).map(|(n, ai)| *n * *ai).sum();
            let d = q.dot(m);
            best = Some(best.map_or(d, |b: f64| b.max(d)));
        }
        if !next(&mut pick, rows.len()) {
            break;
        }
    }
    best.ok_or_else(|| KernelError::Unsupported("the corner's patch has no room".into()))
}

/// Solves the square system `row . a = rhs` given as rows, by Gaussian elimination with
/// partial pivoting; `None` where it is singular.
fn solve_rows(rows: &[(Vec<f64>, f64)]) -> Option<Vec<f64>> {
    let k = rows.len();
    let mut m: Vec<Vec<f64>> = rows.iter().map(|(r, b)| r.iter().copied().chain([*b]).collect()).collect();
    for col in 0..k {
        let piv = (col..k).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[piv][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, piv);
        let pivot = m[col].clone();
        for (i, row) in m.iter_mut().enumerate() {
            if i != col {
                let f = row[col] / pivot[col];
                for (x, p) in row.iter_mut().zip(&pivot).skip(col) {
                    *x -= f * p;
                }
            }
        }
    }
    Some((0..k).map(|i| m[i][k] / m[i][i]).collect())
}

/// The ball of `radius` about `c`, its sphere face tagged `tag`, with the seam the revolve
/// leaves on the side of `away`, so that a patch on the far side is one face.
fn ball(c: DVec3, radius: f64, tag: u32, away: DVec3) -> Result<Solid> {
    let x = away.normalize_or(DVec3::X);
    let y = x.any_orthonormal_vector();
    let plane = Frame { origin: c, x, y, z: x.cross(y) };
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

/// The tool that closes a split corner's ridge: the wedge along the ridge between its two
/// faces' normals, less the cylinder of the radius about it. Its cylinder face is the
/// ridge round, tagged `tag`.
fn ridge_tool(ridge: &Ridge, radius: f64, tag: u32) -> Result<Solid> {
    let along = ridge.to - ridge.from;
    let len = along.length();
    let [n1, n2] = ridge.normals;
    let frame = Frame::new(ridge.from, along / len, n1)?;
    let to2 = |v: DVec3| {
        let l = frame.to_local(ridge.from + v);
        DVec2::new(l.x, l.y)
    };
    // A point `a n1 + b n2` out from the ridge lies on the rounded side of both faces
    // where `a + b c <= r` and `a c + b <= r` (`c = n1 . n2`), so never further than
    // `a + b = 2 r / (1 + c)`: the wedge is cut off just past that.
    let c = n1.dot(n2);
    if c <= -1.0 + 1e-6 {
        return unsupported("rounding a corner whose ridge lies between faces this far apart");
    }
    let reach = 2.0 * radius / (1.0 + c) + 0.1 * radius;
    let (a, b, cc, d) = (to2(n1 * radius), to2(n1 * reach), to2(n2 * reach), to2(n2 * radius));
    // The round, from the second face's contact back to the first's, the short way.
    let mid = to2((n1 + n2).normalize() * radius);
    let round = [true, false]
        .into_iter()
        .filter_map(|ccw| Segment::arc_from_center(DVec2::ZERO, d, a, ccw))
        .find(|s| {
            matches!(s, Segment::Arc { start, sweep, radius, center } if {
                let m = center + DVec2::from_angle(start + 0.5 * sweep) * radius;
                (m - mid).length() < 1e-6 * (1.0 + radius)
            })
        })
        .ok_or_else(|| KernelError::InvalidInput("degenerate ridge".into()))?;
    let section = Loop2::with_tags(
        vec![Segment::Line { a, b }, Segment::Line { a: b, b: cc }, Segment::Line { a: cc, b: d }, round],
        vec![tag + 1, tag + 1, tag + 1, tag],
    );
    let regions = build_regions(&[section])?;
    let region = regions.first().ok_or_else(|| KernelError::InvalidInput("degenerate ridge".into()))?;
    crate::extrude::extrude(&frame, region, 0.0, len)
}

/// `solid` (its edges already rounded) with `corner` closed: its patches and ridge rounds
/// are faces owned by `owner`, tagged from `tags` in turn (patches first), each falling
/// back to the last tag plus its place when `tags` runs short.
pub fn close_corner(solid: &Solid, corner: &Corner, owner: u32, tags: &[u32]) -> Result<Vec<Solid>> {
    let tag = |i: usize| tags.get(i).copied().unwrap_or_else(|| tags.last().copied().unwrap_or(0) + i as u32);
    let r = corner.radius;
    let op = if corner.convex { BoolOp::Subtract } else { BoolOp::Union };
    let mut bodies = vec![solid.clone()];
    let mut steps: Vec<(Solid, DVec3)> = Vec::new();
    for (i, p) in corner.patches.iter().enumerate() {
        steps.push((patch_tool(corner.vertex, &corner.faces, p, r, tag(i))?, p.centre));
    }
    for (i, ridge) in corner.ridges.iter().enumerate() {
        let mid = (ridge.from + ridge.to) * 0.5;
        steps.push((ridge_tool(ridge, r, tag(corner.patches.len() + i))?, mid));
    }
    for (mut tool, centre) in steps {
        tool.set_owner(owner);
        // The body the tool reaches: around the round's centre at a convex corner (it lies
        // in the part), at the vertex at a concave one (the rounds filled it).
        let probe = if corner.convex { centre } else { corner.vertex };
        let Some(b) = bodies.iter().position(|s| boolean::contains_point(s, probe).unwrap_or(false)) else {
            return Err(KernelError::InvalidInput("the corner is no longer in the solid".into()));
        };
        let contacts = touching(&bodies[b], &tool, r);
        let result = boolean::boolean_with_contacts(&bodies[b], &tool, op, &contacts)?;
        let result: Vec<Solid> = result.into_iter().map(|s| join_split_faces(s, owner)).collect();
        bodies.splice(b..=b, result);
    }
    Ok(bodies)
}

/// `solid` with the pieces of each of `owner`'s faces put back together where the boolean
/// left them split along a curve with the same face on both sides (a patch crossed where a
/// round it cut away used to meet it, or the ball's own seam). Two pieces join where they
/// lie on the same surface, carry the same name, have no holes and meet along one chain of
/// edges; anything else is left as it is.
fn join_split_faces(mut solid: Solid, owner: u32) -> Solid {
    loop {
        let Some((fa, fb)) = (0..solid.faces.len()).find_map(|a| {
            let fa = &solid.faces[a];
            if fa.owner != owner || !fa.inners.is_empty() {
                return None;
            }
            (a + 1..solid.faces.len())
                .find(|&b| {
                    let fb = &solid.faces[b];
                    fb.owner == owner
                        && fb.inners.is_empty()
                        && fb.tag == fa.tag
                        && fb.reversed == fa.reversed
                        && same_surface(&fb.surface, &fa.surface)
                        && fa.outer.coedges.iter().any(|c| fb.outer.coedges.iter().any(|d| d.edge == c.edge))
                })
                .map(|b| (a, b))
        }) else {
            return solid;
        };
        match joined(&solid.faces[fa].outer.coedges, &solid.faces[fb].outer.coedges) {
            Some(coedges) => {
                // A sphere's parameters are laid out from a point in the face's middle: for
                // the joined face, the middle of both.
                if let Surface::Sphere { frame, radius } = solid.faces[fa].surface {
                    let mid: DVec3 = coedges
                        .iter()
                        .filter_map(|c| solid.edge(c.edge))
                        .map(|e| (e.curve.eval(0.5 * (e.t0 + e.t1)) - frame.origin).normalize_or_zero())
                        .sum();
                    if let Ok(f) = Frame::new(frame.origin, mid, mid.any_orthonormal_vector()) {
                        solid.faces[fa].surface = Surface::Sphere { frame: f, radius };
                    }
                }
                solid.faces[fa].outer.coedges = coedges;
                solid.faces.remove(fb);
                drop_unused(&mut solid);
            }
            None => return solid,
        }
    }
}

/// Whether two surfaces are the same: equal, or spheres with the same centre and radius
/// (each piece of a sphere face lays its parameters out from its own middle).
fn same_surface(a: &Surface, b: &Surface) -> bool {
    match (a, b) {
        (Surface::Sphere { frame: fa, radius: ra }, Surface::Sphere { frame: fb, radius: rb }) => {
            (fa.origin - fb.origin).length() <= 1e-12 * (1.0 + fa.origin.length())
                && (ra - rb).abs() <= 1e-12 * ra
        }
        _ => a == b,
    }
}

/// The loop round two faces that meet along one chain of edges, `a`'s loop running one way
/// along it and `b`'s the other: `a`'s loop without the chain, then `b`'s.
fn joined(a: &[crate::topo::Coedge], b: &[crate::topo::Coedge]) -> Option<Vec<crate::topo::Coedge>> {
    let shared =
        |c: &crate::topo::Coedge, other: &[crate::topo::Coedge]| other.iter().any(|d| d.edge == c.edge);
    // Rotate each loop to start just after its shared run, which must be one run.
    let rotate =
        |l: &[crate::topo::Coedge], other: &[crate::topo::Coedge]| -> Option<Vec<crate::topo::Coedge>> {
            let n = l.len();
            let starts: Vec<usize> =
                (0..n).filter(|&i| shared(&l[i], other) && !shared(&l[(i + n - 1) % n], other)).collect();
            let [s] = starts[..] else { return None };
            let rest: Vec<crate::topo::Coedge> =
                (0..n).map(|k| l[(s + k) % n]).skip_while(|c| shared(c, other)).collect();
            (!rest.is_empty()).then_some(rest)
        };
    let ra = rotate(a, b)?;
    let rb = rotate(b, a)?;
    if ra.iter().any(|c| shared(c, b)) || rb.iter().any(|c| shared(c, a)) {
        return None;
    }
    Some(ra.into_iter().chain(rb).collect())
}

/// Removes the edges no face uses and the vertices no edge uses, renumbering the rest.
fn drop_unused(solid: &mut Solid) {
    use crate::topo::{Coedge, VertexId};
    let used: std::collections::BTreeSet<u32> =
        solid.faces.iter().flat_map(|f| f.loops()).flat_map(|l| &l.coedges).map(|c| c.edge.0).collect();
    let mut edge_map = vec![None; solid.edges.len()];
    let mut edges = Vec::new();
    for (i, e) in solid.edges.iter().enumerate() {
        if used.contains(&(i as u32)) {
            edge_map[i] = Some(EdgeId(edges.len() as u32));
            edges.push(e.clone());
        }
    }
    let vused: std::collections::BTreeSet<u32> = edges.iter().flat_map(|e| [e.start.0, e.end.0]).collect();
    let mut vmap = vec![None; solid.vertices.len()];
    let mut vertices = Vec::new();
    for (i, v) in solid.vertices.iter().enumerate() {
        if vused.contains(&(i as u32)) {
            vmap[i] = Some(VertexId(vertices.len() as u32));
            vertices.push(v.clone());
        }
    }
    for e in &mut edges {
        e.start = vmap[e.start.index()].unwrap_or(e.start);
        e.end = vmap[e.end.index()].unwrap_or(e.end);
    }
    let remap = |c: &mut Coedge| c.edge = edge_map[c.edge.index()].unwrap_or(c.edge);
    for f in &mut solid.faces {
        f.outer.coedges.iter_mut().for_each(remap);
        for l in &mut f.inners {
            l.coedges.iter_mut().for_each(remap);
        }
    }
    solid.edges = edges;
    solid.vertices = vertices;
}

/// Faces of `solid` and `tool` that touch without crossing: a cylinder and a sphere of
/// radius `r` whose centre lies on its axis (a round running into a patch), or a cylinder
/// of radius `r` and a plane parallel to its axis at that distance (a round on a face).
fn touching(solid: &Solid, tool: &Solid, r: f64) -> Vec<(usize, usize)> {
    let tol = 1e-7 * (1.0 + r);
    let on_axis = |frame: &Frame, q: DVec3| {
        let d = q - frame.origin;
        (d - frame.z * d.dot(frame.z)).length() < tol
    };
    let mut out = Vec::new();
    for (j, t) in tool.faces.iter().enumerate() {
        for (i, f) in solid.faces.iter().enumerate() {
            let touch = match (&f.surface, &t.surface) {
                (Surface::Cylinder { frame, radius }, Surface::Sphere { frame: s, .. })
                | (Surface::Sphere { frame: s, .. }, Surface::Cylinder { frame, radius }) => {
                    (radius - r).abs() < tol && on_axis(frame, s.origin)
                }
                (Surface::Plane { frame: pl }, Surface::Cylinder { frame, radius })
                | (Surface::Cylinder { frame, radius }, Surface::Plane { frame: pl }) => {
                    (radius - r).abs() < tol
                        && pl.z.dot(frame.z).abs() < 1e-9
                        && ((frame.origin - pl.origin).dot(pl.z).abs() - r).abs() < tol
                }
                _ => false,
            };
            if touch {
                out.push((i, j));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
