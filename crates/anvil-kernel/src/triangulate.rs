//! Ear-clipping triangulation of simple polygons with holes.
//! Holes are merged into the outer boundary with bridge edges (Eberly's method).

use crate::math::Vec2;
use crate::profile::polygon_signed_area;

/// Triangulates `outer` minus `holes`. Returns the merged vertex list and triangle
/// indices into it. Triangles are counter-clockwise.
pub fn triangulate(outer: &[Vec2], holes: &[Vec<Vec2>]) -> (Vec<Vec2>, Vec<[usize; 3]>) {
    let mut pts: Vec<Vec2> = Vec::new();
    let mut ring: Vec<usize> = Vec::new();

    let mut o: Vec<Vec2> = outer.to_vec();
    if polygon_signed_area(&o) < 0.0 {
        o.reverse();
    }
    for p in o {
        ring.push(pts.len());
        pts.push(p);
    }

    // Merge holes in order of decreasing max-x so each bridge stays to the right of later holes.
    let mut hole_rings: Vec<Vec<usize>> = Vec::new();
    for h in holes {
        if h.len() < 3 {
            continue;
        }
        let mut h = h.clone();
        if polygon_signed_area(&h) > 0.0 {
            h.reverse();
        }
        let idx: Vec<usize> = h
            .iter()
            .map(|&p| {
                pts.push(p);
                pts.len() - 1
            })
            .collect();
        hole_rings.push(idx);
    }
    let max_x = |r: &Vec<usize>| r.iter().map(|&i| pts[i].x).fold(f64::NEG_INFINITY, f64::max);
    hole_rings.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    for h in &hole_rings {
        ring = bridge_hole(&pts, ring, h);
    }

    let tris = ear_clip(&pts, &ring);
    (pts, tris)
}

fn bridge_hole(pts: &[Vec2], ring: Vec<usize>, hole: &[usize]) -> Vec<usize> {
    // Rightmost hole vertex M.
    let Some((mi, &m_idx)) = hole.iter().enumerate().max_by(|a, b| pts[*a.1].x.total_cmp(&pts[*b.1].x))
    else {
        return ring;
    };
    let m = pts[m_idx];

    // Cast a ray to +x and find the closest crossing of the outer ring.
    let n = ring.len();
    let mut best: Option<(f64, usize)> = None; // (x of hit, ring position of candidate P)
    for i in 0..n {
        let (a, b) = (pts[ring[i]], pts[ring[(i + 1) % n]]);
        // Edge must cross the ray's line, approaching from below on a CCW ring.
        if (a.y <= m.y && m.y <= b.y) || (b.y <= m.y && m.y <= a.y) {
            if (a.y - b.y).abs() < 1e-15 {
                // Horizontal edge on the ray line: take the nearer endpoint.
                for (x, pos) in [(a.x, i), (b.x, (i + 1) % n)] {
                    if x >= m.x && best.is_none_or(|(bx, _)| x < bx) {
                        best = Some((x, pos));
                    }
                }
                continue;
            }
            let t = (m.y - a.y) / (b.y - a.y);
            let x = a.x + t * (b.x - a.x);
            if x >= m.x - 1e-12 && best.is_none_or(|(bx, _)| x < bx) {
                let pos = if a.x > b.x { i } else { (i + 1) % n };
                best = Some((x, pos));
            }
        }
    }

    let p_pos = match best {
        Some((hit_x, pos)) => {
            let i = crate::math::v2(hit_x, m.y);
            let p = pts[ring[pos]];
            if p.distance(i) < 1e-12 {
                pos
            } else {
                // Any ring vertex inside triangle (M, I, P) blocks visibility; pick the one
                // with the smallest angle to the ray instead.
                let mut chosen = pos;
                let mut best_angle = f64::INFINITY;
                for k in 0..n {
                    if k == pos {
                        continue;
                    }
                    let r = pts[ring[k]];
                    if point_in_triangle(r, m, i, p) || point_in_triangle(r, m, p, i) {
                        let d = r - m;
                        let ang = d.y.abs().atan2(d.x);
                        if ang < best_angle - 1e-12
                            || ((ang - best_angle).abs() <= 1e-12
                                && d.length() < (pts[ring[chosen]] - m).length())
                        {
                            best_angle = ang;
                            chosen = k;
                        }
                    }
                }
                chosen
            }
        }
        None => {
            // Degenerate input; fall back to the nearest ring vertex.
            (0..n).min_by(|&a, &b| pts[ring[a]].distance(m).total_cmp(&pts[ring[b]].distance(m))).unwrap_or(0)
        }
    };

    let mut out = Vec::with_capacity(ring.len() + hole.len() + 2);
    out.extend_from_slice(&ring[..=p_pos]);
    for k in 0..=hole.len() {
        out.push(hole[(mi + k) % hole.len()]);
    }
    out.push(ring[p_pos]);
    out.extend_from_slice(&ring[p_pos + 1..]);
    out
}

fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = (b - a).cross(p - a);
    let d2 = (c - b).cross(p - b);
    let d3 = (a - c).cross(p - c);
    d1 >= -1e-12 && d2 >= -1e-12 && d3 >= -1e-12
}

fn ear_clip(pts: &[Vec2], ring: &[usize]) -> Vec<[usize; 3]> {
    let mut tris = Vec::new();
    let mut v: Vec<usize> = ring.to_vec();
    let mut guard = 0usize;
    let mut i = 0usize;
    while v.len() > 3 {
        let n = v.len();
        let (ia, ib, ic) = (v[(i + n - 1) % n], v[i % n], v[(i + 1) % n]);
        let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
        let cross = (b - a).cross(c - b);
        let scale = (b - a).length() * (c - b).length();
        if cross.abs() <= 1e-12 * scale.max(1e-300) && (b - a).dot(c - b) > 0.0 {
            // Straight-through vertex: drop it without emitting a sliver.
            v.remove(i % n);
            guard = 0;
            continue;
        }
        let is_ear = cross > 0.0
            && !v.iter().any(|&k| {
                let p = pts[k];
                // Ignore vertices that coincide with the triangle corners (bridge duplicates).
                p != a && p != b && p != c && point_in_triangle(p, a, b, c)
            });
        // After two full passes without an ear the input is degenerate; clip anyway so the
        // loop always terminates.
        if is_ear || guard > 2 * n {
            if cross > 0.0 {
                tris.push([ia, ib, ic]);
            }
            v.remove(i % n);
            guard = 0;
            i = i.saturating_sub(1);
            continue;
        }
        i = (i + 1) % n;
        guard += 1;
    }
    if v.len() == 3 {
        let (a, b, c) = (pts[v[0]], pts[v[1]], pts[v[2]]);
        if (b - a).cross(c - a) > 0.0 {
            tris.push([v[0], v[1], v[2]]);
        }
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v2;

    fn area(pts: &[Vec2], tris: &[[usize; 3]]) -> f64 {
        tris.iter().map(|t| 0.5 * (pts[t[1]] - pts[t[0]]).cross(pts[t[2]] - pts[t[0]])).sum()
    }

    #[test]
    fn square_with_hole() {
        let outer = vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0), v2(0.0, 10.0)];
        let hole = vec![v2(3.0, 3.0), v2(7.0, 3.0), v2(7.0, 7.0), v2(3.0, 7.0)];
        let (pts, tris) = triangulate(&outer, &[hole]);
        assert!((area(&pts, &tris) - 84.0).abs() < 1e-9);
    }

    #[test]
    fn concave_with_two_holes() {
        // L-shape with two square holes.
        let outer =
            vec![v2(0.0, 0.0), v2(20.0, 0.0), v2(20.0, 5.0), v2(5.0, 5.0), v2(5.0, 20.0), v2(0.0, 20.0)];
        let h1 = vec![v2(1.0, 1.0), v2(2.0, 1.0), v2(2.0, 2.0), v2(1.0, 2.0)];
        let h2 = vec![v2(15.0, 1.0), v2(17.0, 1.0), v2(17.0, 3.0), v2(15.0, 3.0)];
        let (pts, tris) = triangulate(&outer, &[h1, h2]);
        let expected = 20.0 * 5.0 + 5.0 * 15.0 - 1.0 - 4.0;
        assert!((area(&pts, &tris) - expected).abs() < 1e-9);
    }

    #[test]
    fn circle_hole() {
        let outer = vec![v2(-5.0, -5.0), v2(5.0, -5.0), v2(5.0, 5.0), v2(-5.0, 5.0)];
        let hole: Vec<Vec2> =
            (0..36).map(|i| Vec2::from_angle(i as f64 * std::f64::consts::TAU / 36.0) * 2.0).collect();
        let hole_area = crate::profile::polygon_signed_area(&hole);
        let (pts, tris) = triangulate(&outer, &[hole]);
        assert!((area(&pts, &tris) - (100.0 - hole_area)).abs() < 1e-9);
    }

    #[test]
    fn empty_hole_is_ignored() {
        // An empty hole ring used to panic while looking for its rightmost vertex.
        let outer = vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0), v2(0.0, 10.0)];
        let (pts, tris) = triangulate(&outer, &[vec![]]);
        assert!((area(&pts, &tris) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn nan_hole_does_not_panic() {
        // Sorting holes by a NaN coordinate used to panic.
        let outer = vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0), v2(0.0, 10.0)];
        let nan = vec![v2(f64::NAN, 1.0), v2(f64::NAN, 2.0), v2(f64::NAN, 3.0)];
        let hole = vec![v2(3.0, 3.0), v2(7.0, 3.0), v2(7.0, 7.0), v2(3.0, 7.0)];
        let _ = triangulate(&outer, &[nan, hole]);
    }
}
