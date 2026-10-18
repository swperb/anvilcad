//! Bridge to the exact kernel ([`anvil_brep`]).
//!
//! While every feature so far is one the exact kernel can build, the rebuild keeps
//! exact bodies alongside the faceted body and regenerates the faceted body from them
//! after each feature. Face names match the faceted kernel's: start cap 0, end cap 1,
//! side faces `2 + sketch entity`, and blend faces `2 + 8k + 2` for the k-th edge.

use crate::document::BodyOp;
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

fn exact_loop(l: &Loop2) -> brep::Loop2 {
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

/// Fillets or chamfers the named edges. Every run of an edge is blended, and with
/// `propagate` every crease running smoothly on from them too (tangent propagation).
/// A blend as a feature gives it. An angle-distance chamfer's distance runs along one of
/// each edge's two named faces: the first, or the second when flipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Treatment {
    Plain(brep::blend::Blend),
    AngleChamfer { distance: f64, angle: f64, flip: bool },
}

pub fn blend(
    bodies: &[brep::Solid],
    edges: &[EdgeRef],
    treatment: Treatment,
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
        let kind = match treatment {
            Treatment::Plain(kind) => kind,
            Treatment::AngleChamfer { distance, angle, flip } => {
                let name = if flip { edges[k].b } else { edges[k].a };
                let along = bodies[b]
                    .faces
                    .iter()
                    .position(|f| face_id(f) == name)
                    .ok_or("the face a chamfer's distance runs along no longer exists")?;
                brep::blend::Blend::ChamferAngle { distance, angle, along }
            }
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
    Ok(bodies)
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
