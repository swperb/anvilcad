//! Machining feature recognition from a part body, for 2.5D milling from +Z.
//!
//! It reads the faceted body (`anvil_kernel::Solid`), whose polygons carry the name of
//! their face and its exact surface:
//!
//! - Holes: concave cylindrical faces whose axis is parallel to Z and which go all the
//!   way round. A hole is through when a ray down its axis from below the face leaves the
//!   part; otherwise it is blind and its depth is the bottom of the cylinder. Coaxial
//!   holes of different sizes are reported separately (the larger ones are counterbores).
//! - Pockets: upward-facing planar faces below the top of the part are floors. The part is
//!   sliced just above each floor height; a floor inside a loop of the slice that bounds
//!   empty space is a closed pocket (that loop is its boundary and the loops inside it
//!   are islands); a floor outside every loop is an open floor (a step), cleared from
//!   the stock outline around the part's section. Loops that are recognised holes are
//!   left to drilling. Walls that are not vertical are reported, since a 2.5D path cannot
//!   follow them.
//!
//! The rules are simple and stated here so they can be checked and changed; they do not
//! yet cover slots open on one side, chamfers, fillets between floor and wall, or tool
//! axes other than +Z.

use crate::error::{CamError, Result};
use crate::geom::{Region, point_in_loop, signed_area};
use anvil_kernel::{FaceId, Solid, SurfaceKind, Vec2, Vec3, v2, v3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::TAU;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleFeature {
    pub center: Vec2,
    pub diameter: f64,
    /// Top of the cylindrical wall.
    pub top: f64,
    /// Bottom of the cylindrical wall.
    pub bottom: f64,
    pub through: bool,
    pub face: FaceId,
}

impl HoleFeature {
    pub fn depth(&self) -> f64 {
        self.top - self.bottom
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PocketFeature {
    /// Where the tool may cut: the pocket outline with its islands, or for an open floor
    /// the stock outline less the part.
    pub region: Region,
    /// Height of the opening (top of the walls).
    pub top: f64,
    pub floor: f64,
    /// True for an open floor (a step) that the tool can reach from the side.
    pub open: bool,
    pub floor_faces: Vec<FaceId>,
    pub wall_faces: Vec<FaceId>,
    /// Walls are vertical (otherwise a 2.5D path cannot follow them).
    pub vertical_walls: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recognized {
    pub holes: Vec<HoleFeature>,
    pub pockets: Vec<PocketFeature>,
    /// Highest point of the part.
    pub top: f64,
    /// Bounding box of the part.
    pub min: Vec3,
    pub max: Vec3,
    pub notes: Vec<String>,
}

/// One closed loop of a slice, counter-clockwise when material is inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceLoop {
    pub pts: Vec<Vec2>,
    /// Faces the loop runs across.
    pub faces: Vec<FaceId>,
}

/// Bit-exact key for a point (crossings computed the same way from the same edge give
/// identical values).
fn key(p: Vec2) -> (u64, u64) {
    (p.x.to_bits(), p.y.to_bits())
}

/// Crossing of the edge a-b with the plane z, computed from the endpoints in a fixed
/// order so both polygons that share the edge get the same bits.
fn crossing(a: Vec3, b: Vec3, z: f64) -> Vec2 {
    let (a, b) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) { (a, b) } else { (b, a) };
    let t = (z - a.z) / (b.z - a.z);
    v2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Slices the body with the plane at height `z` (which should not contain any vertex)
/// and returns the closed loops, material on the left of each.
pub fn slice(body: &Solid, z: f64) -> Result<Vec<SliceLoop>> {
    let mut segs: Vec<(Vec2, Vec2, FaceId)> = Vec::new();
    for p in &body.polygons {
        if p.normal.z.abs() > 1.0 - 1e-12 {
            continue;
        }
        let k = p.verts.len();
        let mut pts = Vec::with_capacity(2);
        for i in 0..k {
            let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
            // A vertex exactly on the plane counts as above it.
            if (a.z >= z) != (b.z >= z) {
                pts.push(crossing(a, b, z));
            }
        }
        if pts.len() != 2 {
            continue;
        }
        let t = v2(-p.normal.y, p.normal.x);
        let (a, b) = if (pts[1] - pts[0]).dot(t) >= 0.0 { (pts[0], pts[1]) } else { (pts[1], pts[0]) };
        if a.distance(b) > 1e-12 {
            segs.push((a, b, p.tag.id));
        }
    }
    let mut by_start: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        by_start.entry(key(s.0)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    let mut open = 0usize;
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut pts = vec![segs[start].0];
        let mut faces = vec![segs[start].2];
        let mut cur = segs[start].1;
        let first = segs[start].0;
        let mut closed = false;
        for _ in 0..segs.len() {
            if cur.distance(first) < 1e-9 {
                closed = true;
                break;
            }
            pts.push(cur);
            let next =
                by_start.get(&key(cur)).and_then(|v| v.iter().copied().find(|&j| !used[j])).or_else(|| {
                    // Fall back to the nearest unused start within a micron.
                    (0..segs.len()).filter(|&j| !used[j]).find(|&j| segs[j].0.distance(cur) < 1e-6)
                });
            let Some(j) = next else { break };
            used[j] = true;
            if faces.last() != Some(&segs[j].2) {
                faces.push(segs[j].2);
            }
            cur = segs[j].1;
        }
        if closed && pts.len() >= 3 && signed_area(&pts).abs() > 1e-9 {
            faces.sort();
            faces.dedup();
            loops.push(SliceLoop { pts, faces });
        } else {
            open += 1;
        }
    }
    if open > 0 && loops.is_empty() {
        return Err(CamError::InvalidGeometry(format!(
            "the slice at Z{z} did not close ({open} open chains)"
        )));
    }
    Ok(loops)
}

/// Recognises holes and 2.5D pockets in `body`, machined from +Z. `stock_margin` is how
/// far the stock outline extends past the part (for open floors).
pub fn recognize(body: &Solid, stock_margin: f64) -> Result<Recognized> {
    if body.polygons.is_empty() {
        return Err(CamError::InvalidGeometry("the part body is empty".into()));
    }
    let bounds = body.bounds();
    let mut rec = Recognized { top: bounds.max.z, min: bounds.min, max: bounds.max, ..Recognized::default() };
    let top = bounds.max.z;

    // Holes.
    let mut by_face: BTreeMap<FaceId, Vec<usize>> = BTreeMap::new();
    for (i, p) in body.polygons.iter().enumerate() {
        by_face.entry(p.tag.id).or_default().push(i);
    }
    for (&id, polys) in &by_face {
        let SurfaceKind::Cylinder { axis_origin, axis_dir, radius } = body.polygons[polys[0]].tag.surface
        else {
            continue;
        };
        let axis = axis_dir.normalized();
        if axis.z.abs() < 1.0 - 1e-9 || radius <= 0.0 {
            continue;
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut angles = Vec::new();
        let mut votes = 0.0;
        for &pi in polys {
            let p = &body.polygons[pi];
            for v in &p.verts {
                lo = lo.min(v.z);
                hi = hi.max(v.z);
                angles.push((v.y - axis_origin.y).atan2(v.x - axis_origin.x).rem_euclid(TAU));
            }
            let c = p.verts.iter().fold(Vec3::ZERO, |a, b| a + *b) / p.verts.len().max(1) as f64;
            let radial = v3(c.x - axis_origin.x, c.y - axis_origin.y, 0.0).normalized();
            votes += p.normal.dot(radial) * p.area();
        }
        angles.sort_by(f64::total_cmp);
        let n = angles.len();
        let gap = (0..n)
            .map(|i| if i + 1 < n { angles[i + 1] - angles[i] } else { angles[0] + TAU - angles[i] })
            .fold(0.0, f64::max);
        // Material outside the cylinder (normals point at the axis) and all the way round.
        if votes >= 0.0 || TAU - gap < TAU - 0.2 {
            continue;
        }
        let center = v2(axis_origin.x, axis_origin.y);
        let below = v3(center.x, center.y, lo - 1e-4);
        let through = body.raycast(below, v3(0.0, 0.0, -1.0)).is_none();
        rec.holes.push(HoleFeature {
            center,
            diameter: 2.0 * radius,
            top: hi,
            bottom: lo,
            through,
            face: id,
        });
    }

    // Pockets: floors below the top.
    let mut floors: BTreeMap<i64, Vec<(FaceId, Vec2)>> = BTreeMap::new();
    for p in &body.polygons {
        if p.normal.z > 1.0 - 1e-9 {
            let z = p.w / p.normal.z;
            if z < top - 1e-6 {
                let c = p.verts.iter().fold(Vec3::ZERO, |a, b| a + *b) / p.verts.len() as f64;
                floors.entry((z * 1e6).round() as i64).or_default().push((p.tag.id, v2(c.x, c.y)));
            }
        }
    }
    for (zk, fl) in floors {
        let z = zk as f64 * 1e-6;
        // Just above the floor, below anything else close to it.
        let loops = slice(body, z + 1e-4)?;
        let depth = nesting(&loops);
        let mut done: Vec<Option<usize>> = Vec::new();
        for (face, at) in &fl {
            // The innermost loop containing the floor point.
            let owner =
                (0..loops.len()).filter(|&i| point_in_loop(*at, &loops[i].pts)).max_by_key(|&i| depth[i]);
            if done.contains(&owner) {
                if let Some(pk) = rec.pockets.iter_mut().rev().find(|pk| (pk.floor - z).abs() < 1e-6)
                    && !pk.floor_faces.contains(face)
                {
                    pk.floor_faces.push(*face);
                }
                continue;
            }
            done.push(owner);
            match owner {
                Some(h) if depth[h] % 2 == 1 => {
                    // Empty space enclosed by material: a closed pocket (or a hole).
                    if is_hole_loop(&loops[h], &rec.holes) {
                        continue;
                    }
                    let islands: Vec<Vec<Vec2>> = (0..loops.len())
                        .filter(|&i| {
                            depth[i] == depth[h] + 1 && point_in_loop(loops[i].pts[0], &loops[h].pts)
                        })
                        .map(|i| loops[i].pts.clone())
                        .collect();
                    let region = Region::new(loops[h].pts.clone(), islands)?;
                    let walls: Vec<FaceId> = loops[h].faces.clone();
                    let (wall_top, vertical) = wall_info(body, &walls);
                    rec.pockets.push(PocketFeature {
                        region,
                        top: wall_top,
                        floor: z,
                        open: false,
                        floor_faces: vec![*face],
                        wall_faces: walls,
                        vertical_walls: vertical,
                    });
                }
                Some(_) => {
                    // Inside material at this height: not a floor we can reach from above.
                    rec.notes.push(format!("floor face {face:?} at Z{z:.3} is covered by material"));
                }
                None => {
                    // Outside the part's section: an open floor, cleared from the stock.
                    let m = stock_margin.max(0.0);
                    let outer = vec![
                        v2(bounds.min.x - m, bounds.min.y - m),
                        v2(bounds.max.x + m, bounds.min.y - m),
                        v2(bounds.max.x + m, bounds.max.y + m),
                        v2(bounds.min.x - m, bounds.max.y + m),
                    ];
                    let islands: Vec<Vec<Vec2>> =
                        (0..loops.len()).filter(|&i| depth[i] == 0).map(|i| loops[i].pts.clone()).collect();
                    let walls: Vec<FaceId> = (0..loops.len())
                        .filter(|&i| depth[i] == 0)
                        .flat_map(|i| loops[i].faces.clone())
                        .collect();
                    let (_, vertical) = wall_info(body, &walls);
                    let region = Region::new(outer, islands)?;
                    rec.pockets.push(PocketFeature {
                        region,
                        top,
                        floor: z,
                        open: true,
                        floor_faces: vec![*face],
                        wall_faces: walls,
                        vertical_walls: vertical,
                    });
                }
            }
        }
    }
    for pk in &rec.pockets {
        if !pk.vertical_walls {
            rec.notes.push(format!(
                "the {} at Z{:.3} has walls that are not vertical; 2.5D clearing would leave or cut material on them",
                if pk.open { "open floor" } else { "pocket" },
                pk.floor
            ));
        }
    }
    Ok(rec)
}

/// Nesting depth of each loop (0 = not inside any other).
fn nesting(loops: &[SliceLoop]) -> Vec<usize> {
    (0..loops.len())
        .map(|i| {
            let p = loops[i].pts[0];
            (0..loops.len()).filter(|&j| j != i && point_in_loop(p, &loops[j].pts)).count()
        })
        .collect()
}

/// Top of the walls and whether they are all vertical.
fn wall_info(body: &Solid, walls: &[FaceId]) -> (f64, bool) {
    let mut top = f64::NEG_INFINITY;
    let mut vertical = true;
    for p in body.polygons.iter().filter(|p| walls.contains(&p.tag.id)) {
        for v in &p.verts {
            top = top.max(v.z);
        }
        if p.normal.z.abs() > 1e-6 {
            vertical = false;
        }
    }
    (top, vertical)
}

/// A loop that is the outline of a recognised hole (a circle round its axis).
fn is_hole_loop(l: &SliceLoop, holes: &[HoleFeature]) -> bool {
    holes.iter().any(|h| {
        let r = h.diameter * 0.5;
        l.pts.iter().all(|p| (p.distance(h.center) - r).abs() < 0.02 * r + 1e-3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::build::{block, cylinder};

    /// 80 x 60 x 20 block with a 40 x 30 pocket 10 deep holding a 10 x 10 island, a
    /// through hole of 6 mm and a blind hole of 5 mm, 8 deep.
    pub(crate) fn sample_part() -> Solid {
        let base = block(v3(0.0, 0.0, 0.0), v3(80.0, 60.0, 20.0), 1);
        let pocket = block(v3(20.0, 15.0, 10.0), v3(60.0, 45.0, 25.0), 2);
        let island = block(v3(35.0, 25.0, 10.0), v3(45.0, 35.0, 20.0), 3);
        let through = cylinder(v3(10.0, 10.0, -5.0), 3.0, 30.0, 4);
        let blind = cylinder(v3(70.0, 50.0, 12.0), 2.5, 10.0, 5);
        base.subtract(&pocket).union(&island).subtract(&through).subtract(&blind)
    }

    #[test]
    fn slice_of_a_block_is_one_ccw_loop() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 20.0, 5.0), 1);
        let loops = slice(&b, 2.5).unwrap();
        assert_eq!(loops.len(), 1);
        assert!((signed_area(&loops[0].pts) - 200.0).abs() < 1e-9);
    }

    #[test]
    fn recognizes_pocket_island_and_holes() {
        let part = sample_part();
        let rec = recognize(&part, 5.0).unwrap();
        assert_eq!(rec.holes.len(), 2, "{:?}", rec.holes);
        let through = rec.holes.iter().find(|h| h.through).unwrap();
        assert!((through.diameter - 6.0).abs() < 1e-9);
        assert!(through.center.distance(v2(10.0, 10.0)) < 1e-9);
        assert!((through.top - 20.0).abs() < 1e-9 && through.bottom.abs() < 1e-9);
        let blind = rec.holes.iter().find(|h| !h.through).unwrap();
        assert!((blind.diameter - 5.0).abs() < 1e-9);
        assert!((blind.depth() - 8.0).abs() < 1e-9, "{}", blind.depth());

        let closed: Vec<_> = rec.pockets.iter().filter(|p| !p.open).collect();
        assert_eq!(closed.len(), 1, "{:?}", rec.pockets);
        let pk = closed[0];
        assert!((pk.floor - 10.0).abs() < 1e-9);
        assert!((pk.top - 20.0).abs() < 1e-9);
        assert!(pk.vertical_walls);
        assert_eq!(pk.region.islands.len(), 1);
        assert!((pk.region.area() - (40.0 * 30.0 - 100.0)).abs() < 1e-6, "{}", pk.region.area());
        // The blind hole's flat bottom is not mistaken for a pocket.
        assert!(rec.pockets.iter().all(|p| (p.floor - 12.0).abs() > 1e-6));
    }

    #[test]
    fn empty_body_is_an_error() {
        assert!(recognize(&Solid::default(), 0.0).is_err());
    }
}
