//! Bridge to the exact kernel ([`anvil_brep`]), and its implementation of
//! [`ExactKernel`] ([`Brep`]).
//!
//! While every feature so far is one the exact kernel can build, the rebuild keeps
//! exact bodies alongside the faceted body and regenerates the faceted body from them
//! after each feature. Face names match the faceted kernel's: start cap 0, end cap 1,
//! side faces `2 + sketch entity`, and blend faces `2 + 8k + 2` for the k-th edge.

use crate::document::BodyOp;
use crate::kernel::{self, ExactKernel};
use anvil_brep as brep;
use anvil_brep::boolean::{self, BoolOp};
use anvil_brep::topo::{Face, FaceTag as BrepTag};
use anvil_kernel::{
    EdgeRef, FaceId, FaceTag, Frame, Loop2, Polygon, Profile, Seg2, Solid, SurfaceKind, Vec3,
};
use glam::{DVec2, DVec3};

fn d3(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

fn v3(d: DVec3) -> Vec3 {
    Vec3 { x: d.x, y: d.y, z: d.z }
}

pub fn frame(f: &Frame) -> brep::Frame {
    brep::Frame { origin: d3(f.origin), x: d3(f.x), y: d3(f.y), z: d3(f.z) }
}

pub(crate) fn exact_loop(l: &Loop2) -> brep::Loop2 {
    let mut segs = Vec::with_capacity(l.segs.len());
    let mut tags = Vec::with_capacity(l.segs.len());
    for s in &l.segs {
        segs.push(match *s {
            Seg2::Line { a, b, .. } => {
                brep::Segment::Line { a: DVec2::new(a.x, a.y), b: DVec2::new(b.x, b.y) }
            }
            Seg2::Arc { center, radius, start, sweep, .. } => {
                brep::Segment::Arc { center: DVec2::new(center.x, center.y), radius, start, sweep }
            }
        });
        tags.push(s.source());
    }
    brep::Loop2::with_tags(segs, tags)
}

/// Exact regions for sketch profiles, validated by the exact kernel.
pub fn regions(profiles: &[Profile]) -> Result<Vec<brep::Region>, String> {
    let loops: Vec<brep::Loop2> = profiles.iter().flat_map(|p| p.loops()).map(exact_loop).collect();
    brep::profile::build_regions(&loops).map_err(|e| e.to_string())
}

/// Extruded tool bodies, one per region, owned by `feature`.
pub fn extrude(
    profiles: &[Profile],
    f: &Frame,
    z0: f64,
    z1: f64,
    feature: u32,
) -> Result<Vec<brep::Solid>, String> {
    let regions = regions(profiles)?;
    let mut tools = brep::extrude::extrude_regions(&frame(f), &regions, z0, z1).map_err(|e| e.to_string())?;
    for t in &mut tools {
        t.set_owner(feature);
    }
    Ok(tools)
}

/// Extruded tool bodies with draft (see [`anvil_brep::taper`]), one per region.
pub fn extrude_drafted(
    profiles: &[Profile],
    f: &Frame,
    z0: f64,
    z1: f64,
    taper: f64,
    feature: u32,
) -> Result<Vec<brep::Solid>, String> {
    let regions = regions(profiles)?;
    let mut tools = brep::taper::extrude_regions_drafted(&frame(f), &regions, z0, z1, taper)
        .map_err(|e| e.to_string())?;
    for t in &mut tools {
        t.set_owner(feature);
    }
    Ok(tools)
}

/// Revolved tool bodies, one per region, owned by `feature`: the profiles turned by
/// `angle` radians about the sketch line through `axis_point` along `axis_dir`.
pub fn revolve(
    profiles: &[Profile],
    f: &Frame,
    axis_point: anvil_kernel::Vec2,
    axis_dir: anvil_kernel::Vec2,
    angle: f64,
    feature: u32,
) -> Result<Vec<brep::Solid>, String> {
    let regions = regions(profiles)?;
    let mut tools = brep::revolve::revolve_regions(
        &frame(f),
        &regions,
        DVec2::new(axis_point.x, axis_point.y),
        DVec2::new(axis_dir.x, axis_dir.y),
        angle,
    )
    .map_err(|e| e.to_string())?;
    for t in &mut tools {
        t.set_owner(feature);
    }
    Ok(tools)
}

/// Shells the bodies: hollowed to a wall of `thickness`, open through the faces named in
/// `remove`. Cavity faces belong to `feature`, numbered as the faceted kernel numbers
/// them (2 plus the rank of the face they follow among all the part's face names).
pub fn shell(
    bodies: &[brep::Solid],
    remove: &[FaceId],
    thickness: f64,
    feature: u32,
) -> Result<Vec<brep::Solid>, String> {
    let names: std::collections::BTreeSet<FaceId> =
        bodies.iter().flat_map(|b| b.faces.iter().map(face_id)).collect();
    if let Some(r) = remove.iter().find(|r| !names.contains(r)) {
        return Err(format!("face <{}:{}> chosen for removal no longer exists", r.feature, r.index));
    }
    let rank: std::collections::HashMap<FaceId, u32> =
        names.iter().enumerate().map(|(i, id)| (*id, 2 + i as u32)).collect();
    let mut out = Vec::new();
    for body in bodies {
        let open: Vec<usize> =
            (0..body.faces.len()).filter(|&f| remove.contains(&face_id(&body.faces[f]))).collect();
        let mut cavity = brep::shell::cavity(body, &open, thickness).map_err(|e| e.to_string())?;
        for (f, face) in cavity.faces.iter_mut().enumerate() {
            let index = rank.get(&face_id(&body.faces[f])).copied().ok_or("a face has no name")?;
            face.tag = BrepTag::Imported { index };
            face.owner = feature;
        }
        out.extend(boolean::boolean(body, &cavity, BoolOp::Subtract).map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn boxes_touch(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> bool {
    let pad = 1e-6;
    a.0.cmple(b.1 + pad).all() && b.0.cmple(a.1 + pad).all()
}

/// Applies a tool to the bodies: a boss merges with every body it touches, a cut is
/// subtracted from every body.
pub fn combine(bodies: &[brep::Solid], tool: &brep::Solid, op: BodyOp) -> Result<Vec<brep::Solid>, String> {
    let touches = |a: &brep::Solid, b: &brep::Solid| match (a.bounds(), b.bounds()) {
        (Some(x), Some(y)) => boxes_touch(x, y),
        _ => false,
    };
    match op {
        BodyOp::Boss => {
            let mut merged = tool.clone();
            let mut rest = Vec::new();
            for body in bodies {
                if !touches(body, &merged) {
                    rest.push(body.clone());
                    continue;
                }
                let r = boolean::boolean(body, &merged, BoolOp::Union).map_err(|e| e.to_string())?;
                if r.len() == 1 {
                    merged = r.into_iter().next().ok_or("empty union")?;
                } else {
                    rest.push(body.clone());
                }
            }
            rest.push(merged);
            Ok(rest)
        }
        BodyOp::Cut => {
            if bodies.is_empty() {
                return Err("there is no body to cut".into());
            }
            let mut out = Vec::new();
            for body in bodies {
                if touches(body, tool) {
                    out.extend(boolean::boolean(body, tool, BoolOp::Subtract).map_err(|e| e.to_string())?);
                } else {
                    out.push(body.clone());
                }
            }
            Ok(out)
        }
    }
}

/// The faceted kernel's name for an exact face.
pub fn face_id(face: &Face) -> FaceId {
    let index = match face.tag {
        BrepTag::StartCap => FaceId::START_CAP,
        BrepTag::EndCap => FaceId::END_CAP,
        BrepTag::Side { segment, .. } => 2 + segment,
        BrepTag::Imported { index } => index,
    };
    FaceId { feature: face.owner, index }
}

fn surface_kind(face: &Face) -> SurfaceKind {
    match face.surface {
        brep::Surface::Plane { .. } => SurfaceKind::Plane,
        brep::Surface::Cylinder { frame, radius } => {
            SurfaceKind::Cylinder { axis_origin: v3(frame.origin), axis_dir: v3(frame.z), radius }
        }
        brep::Surface::Sphere { .. } | brep::Surface::BSpline(_) => SurfaceKind::Other,
        brep::Surface::Cone { frame, .. } | brep::Surface::Torus { frame, .. } => {
            SurfaceKind::Revolved { axis_origin: v3(frame.origin), axis_dir: v3(frame.z) }
        }
    }
}

/// Edges of the exact bodies that separate the two named faces.
fn find_edges(bodies: &[brep::Solid], e: &EdgeRef) -> Vec<(usize, brep::EdgeId)> {
    let mut out = Vec::new();
    for (bi, solid) in bodies.iter().enumerate() {
        for ei in 0..solid.edges.len() {
            let id = brep::EdgeId(ei as u32);
            let faces = solid.edge_faces(id);
            let names: Vec<FaceId> = faces.iter().filter_map(|f| solid.face(*f)).map(face_id).collect();
            if names.len() == 2 && EdgeRef::new(names[0], names[1]) == *e {
                out.push((bi, id));
            }
        }
    }
    out
}

/// The exact kernel's blend for a blend feature's, for edges other than an
/// angle-distance chamfer's (whose face to measure along depends on the edge).
fn plain_blend(b: kernel::Blend) -> Option<brep::blend::Blend> {
    use brep::blend::{Blend, Profile};
    use kernel::Round;
    Some(match b {
        kernel::Blend::Fillet { radius, round: Round::Circular } => Blend::Fillet(radius),
        kernel::Blend::Fillet { radius, round: Round::ConicRho(rho) } => {
            Blend::Profiled { radius, profile: Profile::ConicRho(rho) }
        }
        kernel::Blend::Fillet { radius, round: Round::ConicRadius(r) } => {
            Blend::Profiled { radius, profile: Profile::ConicRadius(r) }
        }
        kernel::Blend::Fillet { radius, round: Round::CurvatureContinuous } => {
            Blend::Profiled { radius, profile: Profile::CurvatureContinuous }
        }
        kernel::Blend::Chamfer(d) => Blend::Chamfer(d),
        kernel::Blend::ChamferOffset(d) => Blend::ChamferOffset(d),
        kernel::Blend::ChamferAngle { .. } => return None,
    })
}

/// Fillets or chamfers the named edges. Every run of an edge is blended, and with
/// `propagate` every crease running smoothly on from them too (tangent propagation).
/// An angle-distance chamfer's distance runs along one of each edge's two named faces:
/// the first, or the second when flipped.
pub fn blend(
    bodies: &[brep::Solid],
    edges: &[EdgeRef],
    treatment: kernel::Blend,
    propagate: bool,
    feature: u32,
) -> Result<Vec<brep::Solid>, String> {
    if edges.is_empty() {
        return Err("select at least one edge".into());
    }
    let mut bodies = bodies.to_vec();
    let mut runs = 0;
    for e in edges {
        let n = find_edges(&bodies, e).len();
        if n == 0 {
            return Err("an edge to blend no longer exists".into());
        }
        runs += n;
    }
    // Where every edge at a vertex is rounded, the ball closes the corner with a patch of
    // its sphere once the rounds are made. The corners are found on the bodies as they
    // are now, before blending renumbers them.
    let corners = match treatment {
        kernel::Blend::Fillet { radius, round: kernel::Round::Circular } => {
            ball_corners(&bodies, edges, radius, propagate)?
        }
        _ => Vec::new(),
    };
    // Blending renumbers a body, so the edges are found afresh each time. Edges that
    // run smoothly one into the next (a slot's outline) are blended together, in one
    // step, so their rounds join up.
    for _ in 0..runs {
        let found: Vec<(usize, brep::EdgeId, usize)> = edges
            .iter()
            .enumerate()
            .flat_map(|(k, e)| find_edges(&bodies, e).into_iter().map(move |(b, id)| (b, id, k)))
            .collect();
        let Some(&(b, first, k)) = found.first() else {
            return Ok(bodies);
        };
        let kind = match (plain_blend(treatment), treatment) {
            (Some(kind), _) => kind,
            (None, kernel::Blend::ChamferAngle { distance, angle, flip }) => {
                let name = if flip { edges[k].b } else { edges[k].a };
                let along = bodies[b]
                    .faces
                    .iter()
                    .position(|f| face_id(f) == name)
                    .ok_or("the face a chamfer's distance runs along no longer exists")?;
                brep::blend::Blend::ChamferAngle { distance, angle, along }
            }
            (None, _) => return Err("this blend has no exact form".into()),
        };
        let mut candidates: Vec<brep::EdgeId> = found.iter().filter(|f| f.0 == b).map(|f| f.1).collect();
        if propagate {
            candidates = brep::blend::tangent_edges(&bodies[b], &candidates);
        }
        let run = brep::blend::smooth_run(&bodies[b], first, &candidates);
        // An edge reached by propagation is named after the selected edge it runs on from.
        let tagged: Vec<(brep::EdgeId, u32)> = run
            .iter()
            .map(|id| {
                let k = found.iter().find(|f| f.0 == b && f.1 == *id).map_or(k, |f| f.2);
                (*id, 8 * k as u32)
            })
            .collect();
        let result =
            brep::blend::blend_chain(&bodies[b], &tagged, kind, feature).map_err(|err| err.to_string())?;
        bodies.splice(b..=b, result);
    }
    if edges.iter().any(|e| !find_edges(&bodies, e).is_empty()) {
        return Err("the edges to blend did not all blend".into());
    }
    for (corner, tag) in &corners {
        // The body holding the corner: around the ball's centre at a convex corner (the
        // ball lies in the part), at the vertex at a concave one (the rounds filled it).
        let probe = if corner.convex { corner.centre } else { corner.vertex };
        let Some(b) = bodies.iter().position(|s| brep::boolean::contains_point(s, probe).unwrap_or(false))
        else {
            return Err("a rounded corner is no longer in any body".into());
        };
        let result =
            brep::corner::close_corner(&bodies[b], corner, feature, *tag).map_err(|e| e.to_string())?;
        bodies.splice(b..=b, result);
    }
    Ok(bodies)
}

/// The corners that rounding `edges` with radius `r` closes with a ball (see
/// [`brep::corner`]), each with the tag its patch's face is named by. A corner is named
/// after the first selected edge at it, `k`: the patch at that edge's start vertex is face
/// `8 + 8k` and at its end vertex `9 + 8k` (indices the edge's own blend faces, `2 + 8k`
/// to `6 + 8k`, leave free). Should another corner already have the name (an edge that
/// stands for several, each with corners), the corner takes the first free index past
/// every edge's names, from `8 + 8n` for `n` edges.
fn ball_corners(
    bodies: &[brep::Solid],
    edges: &[EdgeRef],
    r: f64,
    propagate: bool,
) -> Result<Vec<(brep::corner::Corner, u32)>, String> {
    let mut out: Vec<(brep::corner::Corner, u32)> = Vec::new();
    for (b, solid) in bodies.iter().enumerate() {
        let found: Vec<(brep::EdgeId, usize)> = edges
            .iter()
            .enumerate()
            .flat_map(|(k, e)| find_edges(bodies, e).into_iter().filter(|f| f.0 == b).map(move |f| (f.1, k)))
            .collect();
        if found.is_empty() {
            continue;
        }
        let mut ids: Vec<brep::EdgeId> = found.iter().map(|f| f.0).collect();
        if propagate {
            ids = brep::blend::tangent_edges(solid, &ids);
        }
        for corner in brep::corner::ball_corners(solid, &ids, r).map_err(|e| e.to_string())? {
            let named = corner
                .edges
                .iter()
                .filter_map(|(id, _)| found.iter().find(|f| f.0 == *id).map(|f| (f.1, *id)))
                .min_by_key(|f| f.0);
            let (k, at_start) = match named {
                Some((k, id)) => {
                    let start = solid.edge(id).and_then(|e| solid.vertex(e.start)).map(|v| v.point);
                    (k, start.is_some_and(|p| (p - corner.vertex).length() < 1e-9))
                }
                None => (0, true),
            };
            let taken = |t: u32| out.iter().any(|(_, u)| *u == t);
            let mut tag = 8 * k as u32 + if at_start { 6 } else { 7 };
            if taken(tag) {
                // Past every edge's names.
                tag = 8 * edges.len() as u32 + 6;
                while taken(tag) {
                    tag += 1;
                }
            }
            out.push((corner, tag));
        }
    }
    Ok(out)
}

/// The faceted solid for exact bodies, with matching face names and surfaces.
pub fn faceted(bodies: &[brep::Solid]) -> Result<Solid, String> {
    let mut polys = Vec::new();
    for solid in bodies {
        let mesh =
            brep::tess::tessellate(solid, &brep::tess::TessParams::default()).map_err(|e| e.to_string())?;
        for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_face) {
            let Some(face) = solid.faces.get(*f as usize) else { continue };
            let tag = FaceTag { id: face_id(face), surface: surface_kind(face) };
            let verts = t.iter().map(|&i| v3(mesh.positions[i as usize])).collect();
            polys.extend(Polygon::new(verts, tag));
        }
    }
    Ok(Solid::new(polys))
}

/// [`anvil_brep`] as the rebuild's exact kernel.
#[derive(Clone, Copy, Debug, Default)]
pub struct Brep;

impl ExactKernel for Brep {
    type Body = brep::Solid;

    fn extrude(
        &self,
        profiles: &[Profile],
        f: &Frame,
        z0: f64,
        z1: f64,
        feature: u32,
    ) -> Result<Vec<brep::Solid>, String> {
        extrude(profiles, f, z0, z1, feature)
    }

    fn extrude_drafted(
        &self,
        profiles: &[Profile],
        f: &Frame,
        z0: f64,
        z1: f64,
        taper: f64,
        feature: u32,
    ) -> Result<Vec<brep::Solid>, String> {
        extrude_drafted(profiles, f, z0, z1, taper, feature)
    }

    fn revolve(
        &self,
        profiles: &[Profile],
        f: &Frame,
        axis_point: anvil_kernel::Vec2,
        axis_dir: anvil_kernel::Vec2,
        angle: f64,
        feature: u32,
    ) -> Result<Vec<brep::Solid>, String> {
        revolve(profiles, f, axis_point, axis_dir, angle, feature)
    }

    fn combine(
        &self,
        bodies: &[brep::Solid],
        tool: &brep::Solid,
        op: BodyOp,
    ) -> Result<Vec<brep::Solid>, String> {
        combine(bodies, tool, op)
    }

    fn intersect(&self, a: &brep::Solid, b: &brep::Solid) -> Result<Vec<brep::Solid>, String> {
        boolean::boolean(a, b, BoolOp::Intersect).map_err(|e| e.to_string())
    }

    fn blend(
        &self,
        bodies: &[brep::Solid],
        edges: &[EdgeRef],
        b: kernel::Blend,
        propagate: bool,
        feature: u32,
    ) -> Result<Vec<brep::Solid>, String> {
        blend(bodies, edges, b, propagate, feature)
    }

    fn shell(
        &self,
        bodies: &[brep::Solid],
        remove: &[FaceId],
        thickness: f64,
        feature: u32,
    ) -> Result<Vec<brep::Solid>, String> {
        shell(bodies, remove, thickness, feature)
    }

    fn faceted(&self, bodies: &[brep::Solid]) -> Result<Solid, String> {
        faceted(bodies)
    }

    fn face_names(&self, body: &brep::Solid) -> Vec<FaceId> {
        body.faces.iter().map(face_id).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn brep_passes_the_kernel_conformance_tests() {
        crate::kernel::conformance::run(&super::Brep);
    }
}
