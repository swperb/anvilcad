//! Contour (profile) finishing with the cutter radius compensated in CAM: the tool
//! centre runs on the boundary offset by the tool radius plus any stock to leave, so the
//! program holds tool-centre coordinates and needs no G41/G42 at the machine. Arcs in
//! the offset stay arcs.
//!
//! The tool climbs (material on its right with a clockwise spindle). Each pass starts
//! with a tangent quarter-circle lead-in from the free side and ends with a matching
//! lead-out. Profiling expects the space beside the wall to be clear already (by
//! adaptive clearing or because it is outside the stock): it is a finishing pass.

use crate::error::{CamError, Result, finite, non_negative, positive};
use crate::geom::{ArcLoop, Region, bulge_arc, offset_loops, offset_region};
use crate::toolpath::{Item, MoveKind, Toolpath};
use anvil_kernel::{Vec2, v2, v3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The tool stays inside the region: pocket walls and the outside of islands.
    Inside,
    /// The tool stays outside the region's outer loop: a part's outline.
    Outside,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileParams {
    pub tool_diameter: f64,
    pub side: Side,
    /// Material left on the wall (radial).
    pub stock_to_leave: f64,
    pub top: f64,
    pub bottom: f64,
    pub step_down: f64,
    /// Number of radial passes (1 = one finishing pass).
    pub passes: usize,
    /// Distance between radial passes.
    pub pass_spacing: f64,
    /// Lead-in and lead-out arc radius (tool centre); 0 for none.
    pub lead_radius: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub safe_z: f64,
}

impl ProfileParams {
    pub fn new(tool_diameter: f64, side: Side, top: f64, bottom: f64, feed: f64) -> ProfileParams {
        ProfileParams {
            tool_diameter,
            side,
            stock_to_leave: 0.0,
            top,
            bottom,
            step_down: (top - bottom).max(0.1),
            passes: 1,
            pass_spacing: 0.25 * tool_diameter,
            lead_radius: 0.25 * tool_diameter,
            feed,
            plunge_feed: feed * 0.3,
            safe_z: top + 5.0,
        }
    }
}

/// Tool-centre loops for a profile at radial offset `extra` beyond the tool radius,
/// oriented so the material is on the right.
pub fn compensated_loops(region: &Region, side: Side, offset: f64) -> Result<Vec<ArcLoop>> {
    match side {
        // Free space is the region: its offset boundary, outer CCW and holes CW, already
        // has the material on the right.
        Side::Inside => Ok(offset_region(region, offset)?.loops),
        Side::Outside => {
            let outer = ArcLoop::from_polygon(&region.outer);
            let grown = offset_loops(&[outer], -offset)?;
            // Free space is outside: run each loop clockwise.
            Ok(grown.loops.into_iter().filter(|l| l.signed_area() > 0.0).map(|l| l.reversed()).collect())
        }
    }
}

/// Profiles the walls of `region`.
pub fn profile(region: &Region, p: &ProfileParams) -> Result<Toolpath> {
    let d = positive("tool diameter", p.tool_diameter)?;
    let leave = non_negative("stock to leave", p.stock_to_leave)?;
    finite("top", p.top)?;
    finite("bottom", p.bottom)?;
    if p.bottom >= p.top {
        return Err(CamError::InvalidParameter { name: "bottom", reason: "must be below the top".into() });
    }
    positive("step-down", p.step_down)?;
    if p.passes == 0 || p.passes > 100 {
        return Err(CamError::InvalidParameter { name: "passes", reason: "must be 1 to 100".into() });
    }
    let spacing = if p.passes > 1 { positive("pass spacing", p.pass_spacing)? } else { 0.0 };
    let lead = non_negative("lead radius", p.lead_radius)?;
    positive("feed", p.feed)?;
    positive("plunge feed", p.plunge_feed)?;
    if p.safe_z <= p.top {
        return Err(CamError::InvalidParameter { name: "safe Z", reason: "must be above the top".into() });
    }
    let r = d * 0.5;
    let depth = p.top - p.bottom;
    let layers = ((depth / p.step_down) - 1e-9).ceil().max(1.0) as usize;
    // Outermost radial pass first, finishing pass last.
    let offsets: Vec<f64> = (0..p.passes).rev().map(|k| r + leave + spacing * k as f64).collect();
    let mut passes: Vec<Vec<ArcLoop>> = Vec::new();
    for &off in &offsets {
        let loops = compensated_loops(region, p.side, off)?;
        passes.push(loops);
    }
    if passes.iter().all(|l| l.is_empty()) {
        return Err(CamError::Unreachable("the tool does not fit: the offset boundary is empty".into()));
    }
    // Where lead arcs may go: at least the finishing offset from every wall, on the free side.
    let finish = r + leave;
    let free = |q: Vec2| -> bool {
        let inside = region.contains(q);
        let ok_side = match p.side {
            Side::Inside => inside,
            Side::Outside => !anvil_kernel::profile::point_in_polygon(q, &region.outer),
        };
        ok_side && region.boundary_distance(q) >= finish - 1e-6
    };
    let mut tp = Toolpath::new();
    tp.push(Item::Comment(format!("profile, {} pass(es), {} layer(s)", p.passes, layers)));
    for k in 1..=layers {
        let z = p.top - depth * k as f64 / layers as f64;
        for loops in &passes {
            for l in loops {
                emit_loop(&mut tp, l, z, p, lead, &free);
            }
        }
    }
    Ok(tp)
}

/// Writes one closed loop at height `z` with lead-in and lead-out arcs.
fn emit_loop(
    tp: &mut Toolpath,
    l: &ArcLoop,
    z: f64,
    p: &ProfileParams,
    lead: f64,
    free: &dyn Fn(Vec2) -> bool,
) {
    let n = l.pts.len();
    if n < 2 {
        return;
    }
    // Start in the middle of the longest straight segment, so the lead arc meets a line.
    let mut best: Option<(usize, f64)> = None;
    for i in 0..n {
        if l.bulges[i].abs() < 1e-12 {
            let len = l.pts[i].distance(l.pts[(i + 1) % n]);
            if best.is_none_or(|b| len > b.1) {
                best = Some((i, len));
            }
        }
    }
    let mut lp = l.clone();
    if let Some((i, len)) = best.filter(|b| b.1 > 1e-6) {
        let (a, b) = (l.pts[i], l.pts[(i + 1) % n]);
        let mid = a.lerp(b, 0.5);
        let _ = len;
        lp.pts.insert(i + 1, mid);
        lp.bulges.insert(i + 1, 0.0);
        lp = lp.rotated(i + 1);
    }
    let start = lp.pts[0];
    let dir = start_direction(&lp);
    let left = dir.perp();
    // Lead arc: a quarter circle on the free (left) side ending tangent at the start.
    let mut lead_r = lead;
    let lead_ok = |rad: f64| -> bool {
        if rad <= 0.0 {
            return false;
        }
        let c = start + left * rad;
        (0..=8).all(|k| {
            let t = std::f64::consts::PI + std::f64::consts::FRAC_PI_2 * k as f64 / 8.0;
            // Angles relative to `dir`: from behind (pi) round to the right (3 pi / 2).
            let q = c + rotate(dir, t) * rad;
            free(q)
        })
    };
    while lead_r > 0.05 && !lead_ok(lead_r) {
        lead_r *= 0.5;
    }
    let use_lead = lead_ok(lead_r);
    let entry = if use_lead { start + left * lead_r - dir * lead_r } else { start };
    tp.rapid(v3(entry.x, entry.y, p.safe_z));
    tp.rapid(v3(entry.x, entry.y, p.top + 1.0));
    tp.line(v3(entry.x, entry.y, z), MoveKind::Entry, p.plunge_feed);
    if use_lead {
        let c = start + left * lead_r;
        tp.arc(v3(start.x, start.y, z), c, true, MoveKind::Lead, p.feed);
    }
    let m = lp.pts.len();
    for i in 0..m {
        let (a, b) = (lp.pts[i], lp.pts[(i + 1) % m]);
        match bulge_arc(a, b, lp.bulges[i]) {
            Some(g) => tp.arc(v3(b.x, b.y, z), g.center, g.sweep > 0.0, MoveKind::Cut, p.feed),
            None => {
                if a.distance(b) > 1e-9 {
                    tp.line(v3(b.x, b.y, z), MoveKind::Cut, p.feed)
                }
            }
        }
    }
    let mut end = start;
    if use_lead {
        let c = start + left * lead_r;
        end = c + dir * lead_r;
        tp.arc(v3(end.x, end.y, z), c, true, MoveKind::Lead, p.feed);
    }
    tp.rapid(v3(end.x, end.y, p.safe_z));
}

/// Unit direction of travel at the loop's first vertex.
fn start_direction(l: &ArcLoop) -> Vec2 {
    let (a, b) = (l.pts[0], l.pts[1 % l.pts.len()]);
    match bulge_arc(a, b, l.bulges[0]) {
        Some(g) => {
            let radial = (a - g.center).normalized();
            if g.sweep > 0.0 { radial.perp() } else { -radial.perp() }
        }
        None => (b - a).normalized(),
    }
}

fn rotate(v: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    v2(v.x * c - v.y * s, v.x * s + v.y * c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolpath::Motion;

    #[test]
    fn outside_profile_of_a_square() {
        let part = Region::rectangle(v2(0.0, 0.0), v2(40.0, 30.0)).unwrap();
        let mut p = ProfileParams::new(10.0, Side::Outside, 0.0, -12.0, 600.0);
        p.step_down = 6.0;
        let tp = profile(&part, &p).unwrap();
        let cuts: Vec<_> = tp.moves().filter(|m| m.kind == MoveKind::Cut).collect();
        // Two layers; each loop has 4 lines, 4 corner arcs of radius 5, plus the split.
        let arcs = cuts.iter().filter(|m| matches!(m.motion, Motion::Arc { .. })).count();
        assert_eq!(arcs, 8);
        // Every cutting point is exactly 5 mm from the part.
        for seg in tp.segments(0.001) {
            if seg.kind == MoveKind::Cut {
                let q = v2(seg.b.x, seg.b.y);
                assert!((part.boundary_distance(q) - 5.0).abs() < 1e-6, "{q:?}");
                assert!(!part.contains(q));
            }
        }
        // Climb milling round the outside runs clockwise.
        let corner_arcs: Vec<bool> = cuts
            .iter()
            .filter_map(|m| if let Motion::Arc { ccw, .. } = m.motion { Some(ccw) } else { None })
            .collect();
        assert!(corner_arcs.iter().all(|c| !c));
        // Cut length per layer: perimeter of the offset rectangle with round corners.
        let per_layer = 2.0 * (40.0 + 30.0) + 2.0 * std::f64::consts::PI * 5.0;
        let stats = tp.stats();
        assert!((stats.cut_length - 2.0 * per_layer).abs() < 1e-6, "{}", stats.cut_length);
    }

    #[test]
    fn inside_profile_of_a_pocket_with_island() {
        let pocket = Region::new(
            vec![v2(0.0, 0.0), v2(60.0, 0.0), v2(60.0, 40.0), v2(0.0, 40.0)],
            vec![crate::geom::circle_points(v2(30.0, 20.0), 5.0, 64)],
        )
        .unwrap();
        let mut p = ProfileParams::new(6.0, Side::Inside, 0.0, -5.0, 500.0);
        p.stock_to_leave = 0.2;
        p.passes = 2;
        p.pass_spacing = 0.5;
        let tp = profile(&pocket, &p).unwrap();
        let mut dists: Vec<f64> = tp
            .segments(0.001)
            .iter()
            .filter(|s| s.kind == MoveKind::Cut)
            .map(|s| pocket.boundary_distance(v2(s.b.x, s.b.y)))
            .collect();
        dists.sort_by(f64::total_cmp);
        // Two radial passes: 3.7 then the finishing 3.2 mm from the walls.
        assert!((dists[0] - 3.2).abs() < 1e-6, "{}", dists[0]);
        assert!(dists.iter().all(|d| (d - 3.2).abs() < 1e-6 || (d - 3.7).abs() < 1e-6));
        // Lead arcs stay clear of the walls.
        for s in tp.segments(0.001).iter().filter(|s| s.kind == MoveKind::Lead) {
            assert!(pocket.boundary_distance(v2(s.b.x, s.b.y)) >= 3.2 - 1e-6);
        }
    }

    #[test]
    fn tool_too_big_is_an_error() {
        let pocket = Region::rectangle(v2(0.0, 0.0), v2(8.0, 8.0)).unwrap();
        let p = ProfileParams::new(10.0, Side::Inside, 0.0, -5.0, 500.0);
        assert!(profile(&pocket, &p).is_err());
    }
}
