//! Drilling cycles from recognised holes.
//!
//! Holes with the same diameter, top and depth share one cycle, visited in
//! nearest-neighbour order. The cycle follows common shop practice: straight drilling
//! to three diameters deep, chip-breaking pecks to six, full-retract pecks beyond, with
//! one diameter per peck. Through holes are drilled past the bottom by the drill point
//! plus 1 mm so the full diameter breaks through; blind holes are drilled to full
//! diameter at the recognised depth, so the point goes deeper by its own length (an
//! error is returned if that would break out of the part, and the simulator reports it
//! as a gouge if the model has a flat-bottomed hole).

use crate::error::{CamError, Result, finite, positive};
use crate::features::HoleFeature;
use crate::tool::Tool;
use crate::toolpath::{CycleKind, DrillCycle};
use anvil_kernel::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrillParams {
    /// Feed, mm/min.
    pub feed: f64,
    /// Height of the retract plane above each hole's top.
    pub r_clearance: f64,
    /// Height for moves between holes.
    pub safe_z: f64,
    /// Extra depth past the bottom of through holes, besides the drill point.
    pub breakthrough: f64,
}

impl DrillParams {
    pub fn new(feed: f64, safe_z: f64) -> DrillParams {
        DrillParams { feed, r_clearance: 2.0, safe_z, breakthrough: 1.0 }
    }
}

/// A drilling cycle per group of like holes. Fails if the tool is not a drill or its
/// diameter differs from the holes' by more than 0.05 mm, or if `part_bottom` (the
/// lowest point of the part) would be broken through by a blind hole's drill point.
pub fn drill_holes(
    holes: &[HoleFeature],
    tool: &Tool,
    p: &DrillParams,
    part_bottom: f64,
) -> Result<Vec<DrillCycle>> {
    tool.validate()?;
    if !tool.is_drill() {
        return Err(CamError::InvalidParameter { name: "tool", reason: "drilling needs a drill".into() });
    }
    positive("feed", p.feed)?;
    positive("retract clearance", p.r_clearance)?;
    finite("safe Z", p.safe_z)?;
    crate::error::non_negative("breakthrough", p.breakthrough)?;
    let mut groups: Vec<(f64, f64, bool, Vec<Vec2>)> = Vec::new();
    for h in holes {
        if (h.diameter - tool.diameter).abs() > 0.05 {
            return Err(CamError::InvalidParameter {
                name: "tool",
                reason: format!("a {:.2} mm drill cannot make a {:.2} mm hole", tool.diameter, h.diameter),
            });
        }
        if h.top + p.r_clearance > p.safe_z {
            return Err(CamError::InvalidParameter {
                name: "safe Z",
                reason: "must be above the retract plane".into(),
            });
        }
        // The cycle's Z is the tip: full diameter reaches the bottom of the cylinder.
        let z = if h.through {
            h.bottom - tool.point_length() - p.breakthrough
        } else {
            h.bottom - tool.point_length()
        };
        if !h.through && z <= part_bottom + 0.2 {
            return Err(CamError::InvalidGeometry(format!(
                "the drill point would break through the part under the blind hole at ({:.2}, {:.2})",
                h.center.x, h.center.y
            )));
        }
        if (h.top - z) > tool.flute_length {
            return Err(CamError::Unreachable(format!(
                "the hole at ({:.2}, {:.2}) is deeper than the drill's flutes",
                h.center.x, h.center.y
            )));
        }
        match groups
            .iter_mut()
            .find(|g| (g.0 - h.top).abs() < 1e-6 && (g.1 - z).abs() < 1e-6 && g.2 == h.through)
        {
            Some(g) => g.3.push(h.center),
            None => groups.push((h.top, z, h.through, vec![h.center])),
        }
    }
    let d = tool.diameter;
    let mut out = Vec::new();
    for (top, z, _, pts) in groups {
        let depth = top - z;
        let kind = if depth <= 3.0 * d {
            CycleKind::Drill
        } else if depth <= 6.0 * d {
            CycleKind::ChipBreak { peck: d }
        } else {
            CycleKind::Peck { peck: d }
        };
        out.push(DrillCycle {
            kind,
            points: nearest_order(pts),
            top,
            r_plane: top + p.r_clearance,
            z,
            clearance: p.safe_z,
            feed: p.feed,
        });
    }
    Ok(out)
}

/// Visits points in nearest-neighbour order from the first.
fn nearest_order(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(pts.len());
    if pts.is_empty() {
        return out;
    }
    let mut cur = pts.remove(0);
    out.push(cur);
    while !pts.is_empty() {
        let (k, _) = pts
            .iter()
            .enumerate()
            .map(|(k, p)| (k, p.distance(cur)))
            .fold((0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a });
        cur = pts.remove(k);
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolpath::{Item, MoveKind, Toolpath};
    use anvil_kernel::{FaceId, v2};

    fn hole(x: f64, y: f64, d: f64, top: f64, bottom: f64, through: bool) -> HoleFeature {
        HoleFeature {
            center: v2(x, y),
            diameter: d,
            top,
            bottom,
            through,
            face: FaceId { feature: 1, index: 2 },
        }
    }

    #[test]
    fn through_and_blind_cycles() {
        let drill = Tool::drill(3, 6.0);
        let holes = vec![
            hole(10.0, 10.0, 6.0, 20.0, 0.0, true),
            hole(50.0, 10.0, 6.0, 20.0, 0.0, true),
            hole(30.0, 40.0, 6.0, 20.0, 12.0, false),
        ];
        let cycles = drill_holes(&holes, &drill, &DrillParams::new(150.0, 30.0), 0.0).unwrap();
        assert_eq!(cycles.len(), 2);
        let through = cycles.iter().find(|c| c.points.len() == 2).unwrap();
        // 20 mm through at 6 mm: chip-breaking pecks; depth past the bottom by point + 1.
        assert_eq!(through.kind, CycleKind::ChipBreak { peck: 6.0 });
        assert!((through.z - (-drill.point_length() - 1.0)).abs() < 1e-12);
        let blind = cycles.iter().find(|c| c.points.len() == 1).unwrap();
        assert_eq!(blind.kind, CycleKind::Drill);
        assert!((blind.z - (12.0 - drill.point_length())).abs() < 1e-12);
        // Expanded, every hole is fed to its depth and the tool ends at clearance.
        let mut tp = Toolpath::new();
        for c in cycles {
            tp.push(Item::Cycle(c));
        }
        let ex = tp.expanded();
        let deepest: Vec<f64> = ex.moves().filter(|m| m.kind == MoveKind::Entry).map(|m| m.to.z).collect();
        assert!(deepest.iter().any(|z| (*z - (12.0 - drill.point_length())).abs() < 1e-12));
        assert_eq!(ex.end().unwrap().z, 30.0);
    }

    #[test]
    fn wrong_drill_or_breakout_is_an_error() {
        let holes = vec![hole(10.0, 10.0, 6.0, 20.0, 1.0, false)];
        assert!(drill_holes(&holes, &Tool::drill(3, 8.0), &DrillParams::new(150.0, 30.0), 0.0).is_err());
        // A blind hole 1 mm above the bottom of the part: the point would break out.
        assert!(drill_holes(&holes, &Tool::drill(3, 6.0), &DrillParams::new(150.0, 30.0), 0.0).is_err());
        assert!(
            drill_holes(&holes, &Tool::flat_end_mill(1, 6.0, 3), &DrillParams::new(150.0, 30.0), 0.0)
                .is_err()
        );
    }
}
