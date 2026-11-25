//! Topological and geometric consistency checks.

use crate::topo::Solid;
use crate::{KernelError, Result, tol};

fn fail<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Topology(msg.into()))
}

/// Verifies that `solid` is a closed, orientable 2-manifold whose geometry agrees with
/// its topology: one connected shell, or one outer shell holding sealed voids (see
/// [`crate::voids`]).
pub fn validate(solid: &Solid) -> Result<()> {
    if solid.faces.is_empty() {
        return fail("solid has no faces");
    }
    for v in &solid.vertices {
        if !tol::is_finite3(v.point) {
            return Err(KernelError::NonFinite);
        }
    }
    for (i, e) in solid.edges.iter().enumerate() {
        let (Some(a), Some(b)) = (solid.vertex(e.start), solid.vertex(e.end)) else {
            return fail(format!("edge {i} references a missing vertex"));
        };
        for (t, p) in [(e.t0, a.point), (e.t1, b.point)] {
            let tol = tol::LINEAR * 10.0 + 1e-12 * p.length();
            if (e.curve.eval(t) - p).length() > tol {
                return fail(format!("edge {i} does not end on its vertex"));
            }
        }
    }

    let mut forward = vec![0u32; solid.edges.len()];
    let mut backward = vec![0u32; solid.edges.len()];
    let mut edge_faces: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (fi, f) in solid.faces.iter().enumerate() {
        for l in f.loops() {
            if l.coedges.is_empty() {
                return fail(format!("face {fi} has an empty loop"));
            }
            for (k, c) in l.coedges.iter().enumerate() {
                let Some((_, end)) = solid.coedge_vertices(*c) else {
                    return fail(format!("face {fi} references a missing edge"));
                };
                let next = l.coedges[(k + 1) % l.coedges.len()];
                let Some((start, _)) = solid.coedge_vertices(next) else {
                    return fail(format!("face {fi} references a missing edge"));
                };
                if end != start {
                    return fail(format!("face {fi} has an open loop"));
                }
                let ei = c.edge.index();
                if c.reversed {
                    backward[ei] += 1;
                } else {
                    forward[ei] += 1;
                }
                edge_faces[ei].push(fi);
            }
        }
    }
    for i in 0..solid.edges.len() {
        if forward[i] != 1 || backward[i] != 1 {
            return fail(format!(
                "edge {i} is used {} times forwards and {} times backwards",
                forward[i], backward[i]
            ));
        }
    }

    // Connectivity: one shell, or an outer shell holding voids.
    let shells = crate::voids::shell_faces(solid);
    if shells.len() > 1 {
        for faces in &shells {
            shell_genus(&crate::voids::extract(solid, faces)?)?;
        }
        return crate::voids::check_layout(solid);
    }

    shell_genus(solid).map(|_| ())
}

/// Genus from the Euler-Poincare formula V - E + F - R = 2(S - G), summed over the
/// solid's shells. Each through-hole adds one to the genus; a void adds nothing.
pub fn genus(solid: &Solid) -> Result<u32> {
    let shells = crate::voids::shell_faces(solid);
    if shells.len() > 1 {
        let mut total = 0;
        for faces in &shells {
            total += shell_genus(&crate::voids::extract(solid, faces)?)?;
        }
        return Ok(total);
    }
    shell_genus(solid)
}

/// Genus of a solid of one shell.
fn shell_genus(solid: &Solid) -> Result<u32> {
    let v = solid.vertices.len() as i64;
    let e = solid.edges.len() as i64;
    let f = solid.faces.len() as i64;
    let r: i64 = solid.faces.iter().map(|f| f.inners.len() as i64).sum();
    let chi = v - e + f - r;
    let twice_genus = 2 - chi;
    if twice_genus < 0 || twice_genus % 2 != 0 {
        return fail(format!("Euler characteristic {chi} is impossible for one shell"));
    }
    Ok((twice_genus / 2) as u32)
}
