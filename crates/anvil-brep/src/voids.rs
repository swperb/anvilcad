//! Solids with sealed internal voids.
//!
//! A solid's faces may form more than one closed shell. Exactly one of them is the
//! outer shell, facing out; every other one bounds a void (a sealed cavity) inside it
//! and faces into the void, away from the material. Shells are not stored: they are the
//! sets of faces joined through shared edges, so a solid with no void is unchanged.
//!
//! The divergence theorem gives an outer shell a positive volume and a void's shell a
//! negative one, which is how the two are told apart. Shells must not touch, so each
//! lies wholly inside or wholly outside each other one.

use crate::boolean;
use crate::topo::{Coedge, EdgeId, Face, Loop, Solid, VertexId};
use crate::{KernelError, Result, mass, tess};
use glam::DVec3;
use std::collections::HashMap;

fn fail<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Topology(msg.into()))
}

/// The indices of each shell's faces: the faces joined through shared edges, in face
/// order, the shell holding face 0 first.
pub fn shell_faces(solid: &Solid) -> Vec<Vec<usize>> {
    let n = solid.faces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut first_user: HashMap<EdgeId, usize> = HashMap::new();
    for (fi, f) in solid.faces.iter().enumerate() {
        for c in f.loops().flat_map(|l| &l.coedges) {
            match first_user.get(&c.edge) {
                Some(&other) => {
                    let (a, b) = (find(&mut parent, fi), find(&mut parent, other));
                    parent[a.max(b)] = a.min(b);
                }
                None => {
                    first_user.insert(c.edge, fi);
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut slot: HashMap<usize, usize> = HashMap::new();
    for fi in 0..n {
        let r = find(&mut parent, fi);
        let k = *slot.entry(r).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[k].push(fi);
    }
    groups
}

/// The faces at `faces` as a solid of their own, with only the vertices and edges they
/// use, in the order first used.
pub fn extract(solid: &Solid, faces: &[usize]) -> Result<Solid> {
    let mut out = Solid::default();
    let mut vids: HashMap<VertexId, VertexId> = HashMap::new();
    let mut eids: HashMap<EdgeId, EdgeId> = HashMap::new();
    for &fi in faces {
        let Some(face) = solid.faces.get(fi) else {
            return fail(format!("face {fi} does not exist"));
        };
        let mut remap = |l: &Loop, out: &mut Solid| -> Result<Loop> {
            let mut coedges = Vec::with_capacity(l.coedges.len());
            for c in &l.coedges {
                let id = match eids.get(&c.edge) {
                    Some(&id) => id,
                    None => {
                        let Some(e) = solid.edge(c.edge) else {
                            return fail(format!("face {fi} references a missing edge"));
                        };
                        let mut vertex = |v: VertexId, out: &mut Solid| -> Result<VertexId> {
                            if let Some(&id) = vids.get(&v) {
                                return Ok(id);
                            }
                            let Some(p) = solid.vertex(v) else {
                                return fail(format!("face {fi} references a missing vertex"));
                            };
                            let id = out.add_vertex(p.point);
                            vids.insert(v, id);
                            Ok(id)
                        };
                        let start = vertex(e.start, out)?;
                        let end = vertex(e.end, out)?;
                        let id = out.add_edge(crate::topo::Edge { start, end, ..e.clone() });
                        eids.insert(c.edge, id);
                        id
                    }
                };
                coedges.push(Coedge { edge: id, reversed: c.reversed });
            }
            Ok(Loop { coedges })
        };
        let outer = remap(&face.outer, &mut out)?;
        let inners = face.inners.iter().map(|l| remap(l, &mut out)).collect::<Result<Vec<_>>>()?;
        out.add_face(Face { outer, inners, ..face.clone() });
    }
    Ok(out)
}

/// Each shell of `solid` as a solid of its own (see [`shell_faces`]).
pub fn split(solid: &Solid) -> Result<Vec<Solid>> {
    shell_faces(solid).iter().map(|faces| extract(solid, faces)).collect()
}

/// The shells `parts` gathered into one solid, in order, their faces, edges and vertices
/// kept apart.
pub fn join(parts: &[Solid]) -> Solid {
    let mut out = Solid::default();
    for p in parts {
        let (v0, e0) = (out.vertices.len() as u32, out.edges.len() as u32);
        out.vertices.extend(p.vertices.iter().cloned());
        out.edges.extend(p.edges.iter().map(|e| crate::topo::Edge {
            start: VertexId(e.start.0 + v0),
            end: VertexId(e.end.0 + v0),
            ..e.clone()
        }));
        let shift = |l: &Loop| Loop {
            coedges: l
                .coedges
                .iter()
                .map(|c| Coedge { edge: EdgeId(c.edge.0 + e0), reversed: c.reversed })
                .collect(),
        };
        out.faces.extend(p.faces.iter().map(|f| Face {
            outer: shift(&f.outer),
            inners: f.inners.iter().map(shift).collect(),
            ..f.clone()
        }));
    }
    out
}

/// `solid` turned inside out: every face's outward normal reversed and its loops run the
/// other way, so the volume changes sign.
pub fn flipped(solid: &Solid) -> Solid {
    let flip = |l: &Loop| Loop {
        coedges: l.coedges.iter().rev().map(|c| Coedge { edge: c.edge, reversed: !c.reversed }).collect(),
    };
    Solid {
        vertices: solid.vertices.clone(),
        edges: solid.edges.clone(),
        faces: solid
            .faces
            .iter()
            .map(|f| Face {
                reversed: !f.reversed,
                outer: flip(&f.outer),
                inners: f.inners.iter().map(flip).collect(),
                ..f.clone()
            })
            .collect(),
    }
}

/// The volume one closed shell bounds, positive when it faces out and negative when it
/// faces in (a void's shell). Integrated on the exact surfaces, or on a tessellation if
/// that fails.
pub fn signed_volume(shell: &Solid) -> Result<f64> {
    match mass::exact_properties(shell) {
        Ok(m) if m.volume.is_finite() => Ok(m.volume),
        _ => {
            let mesh = tess::tessellate(shell, &tess::TessParams::default())?;
            Ok(mass::properties(&mesh).volume)
        }
    }
}

/// A closed shell with its signed volume and a point on its surface.
struct Part {
    shell: Solid,
    volume: f64,
    probe: DVec3,
}

fn parts(shells: Vec<Solid>) -> Result<Vec<Part>> {
    shells
        .into_iter()
        .map(|shell| {
            let volume = signed_volume(&shell)?;
            let Some(probe) = boolean::shell_probe(&shell) else {
                return fail("a shell has no face with an inside to probe");
            };
            Ok(Part { shell, volume, probe })
        })
        .collect()
}

/// True when the point lies strictly inside the region `shell` bounds, whichever way the
/// shell faces.
fn encloses(shell: &Solid, p: DVec3) -> Result<bool> {
    boolean::contains_point(shell, p)
}

/// The outer shell and the voids of a solid, each as a solid of its own; the voids face
/// into the void. A solid of one shell has no voids.
pub fn outer_and_voids(solid: &Solid) -> Result<(Solid, Vec<Solid>)> {
    let mut shells = split(solid)?;
    if shells.len() == 1 {
        return Ok((shells.remove(0), Vec::new()));
    }
    let mut outer = None;
    let mut voids = Vec::new();
    for s in shells {
        if signed_volume(&s)? > 0.0 {
            if outer.is_some() {
                return fail("solid has more than one outer shell");
            }
            outer = Some(s);
        } else {
            voids.push(s);
        }
    }
    let Some(outer) = outer else {
        return fail("solid has no outer shell: every shell faces inwards");
    };
    Ok((outer, voids))
}

/// Checks that the shells of a solid with several of them make one body: one outer shell
/// facing out, and voids facing in that lie inside it and outside one another.
pub(crate) fn check_layout(solid: &Solid) -> Result<()> {
    let ps = parts(split(solid)?)?;
    let outers: Vec<&Part> = ps.iter().filter(|p| p.volume > 0.0).collect();
    let [outer] = outers[..] else {
        return fail(if outers.is_empty() {
            "solid has no outer shell: every shell faces inwards".to_string()
        } else {
            format!("solid has {} separate outer shells; each must be a body of its own", outers.len())
        });
    };
    let voids: Vec<&Part> = ps.iter().filter(|p| p.volume <= 0.0).collect();
    for (i, v) in voids.iter().enumerate() {
        if !encloses(&outer.shell, v.probe)? {
            return fail("a void's shell lies outside the solid's outer shell");
        }
        for (j, w) in voids.iter().enumerate() {
            if i != j && encloses(&w.shell, v.probe)? {
                return fail("a void's shell lies inside another void");
            }
        }
    }
    Ok(())
}

/// Closed shells gathered into bodies: each outer shell (facing out) with the voids it
/// holds. A void belongs to the smallest outer shell around it; an outer shell inside
/// another one's void is a body of its own, as a ball inside a hollow ball is.
pub(crate) fn bodies(shells: Vec<Solid>) -> Result<Vec<Solid>> {
    if shells.len() < 2 {
        return Ok(shells);
    }
    let ps = parts(shells)?;
    let outer_ix: Vec<usize> = (0..ps.len()).filter(|&i| ps[i].volume > 0.0).collect();
    let mut held: Vec<Vec<usize>> = vec![Vec::new(); ps.len()];
    for (vi, v) in ps.iter().enumerate().filter(|(_, p)| p.volume <= 0.0) {
        let mut best: Option<usize> = None;
        for &oi in &outer_ix {
            if encloses(&ps[oi].shell, v.probe)? && best.is_none_or(|b| ps[oi].volume < ps[b].volume) {
                best = Some(oi);
            }
        }
        let Some(b) = best else {
            return Err(KernelError::Boolean(
                "the result has a shell facing inwards with nothing around it".into(),
            ));
        };
        held[b].push(vi);
    }
    let mut out = Vec::with_capacity(outer_ix.len());
    for &oi in &outer_ix {
        let mut members = vec![ps[oi].shell.clone()];
        members.extend(held[oi].iter().map(|&vi| ps[vi].shell.clone()));
        out.push(join(&members));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
