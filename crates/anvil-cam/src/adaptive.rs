//! Adaptive (engagement-limited) 2.5D clearing.
//!
//! The planner keeps an explicit raster model of the uncut material in the layer
//! ([`crate::raster::Grid`]) and clears it in rounds:
//!
//! 1. Trace. At every place the tool centre may go, it measures how much of the tool's
//!    circumference lies over material, and traces the curves where that equals twice
//!    the target engagement (along a straight edge the tool's own sweep clears the half
//!    behind it, so a moving tool then engages the target). These curves are
//!    constant-engagement offsets of the cut boundary, in the sense of Stori and Wright
//!    (2000); at inside corners of the material they pull back by themselves, because the
//!    engagement rises there.
//! 2. Cut. Each curve is a guide for one pass, with the material on the tool's right
//!    (climb milling with a clockwise spindle). The pass starts clear of the material and
//!    blends onto the curve over 1.5 tool diameters. It is then simulated against the
//!    material model point by point: a point whose engagement would exceed a guard set
//!    under the hard limit is moved out to the free side by as little as fixes it (and
//!    eases back afterwards), and where no such point exists the pass stops. Nothing the
//!    planner emits has exceeded the guard in its own model.
//! 3. Link. The tool moves to the next pass through cleared space, along straight feeds
//!    or a path found on a grid of clear positions; it retracts only when there is none.
//! 4. Enter. When no curve can be started from cleared space (a closed pocket at first,
//!    or a pocket of its own) the tool enters by a helix at the point farthest from the
//!    walls. Where the region extends past the stock, passes start in the air instead.
//!
//! A ring along the boundary of where the tool may go picks up thin material at the
//! walls. Tool positions are always at least the tool radius plus the stock to leave from
//! every wall and island, tested exactly against the boundary, so the path never gouges.
//!
//! Engagement is the angle of the tool's circumference over uncut material, which for a
//! straight cut at radial depth `ae` is `acos(1 - ae / R)`. The finished path is checked
//! independently by [`measure_engagement`], which replays it into a fresh, finer model.
//! Background, sources and the patents checked are in `docs/cam/ADAPTIVE.md`.

use crate::error::{CamError, Result, finite, non_negative, positive};
use crate::geom::{Region, SegIndex};
use crate::raster::{Grid, distance_transform};
use crate::tool::chip_thinning_factor;
use crate::toolpath::{MoveKind, Toolpath};
use anvil_kernel::{Vec2, Vec3, v2, v3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// Parameters for adaptive clearing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveParams {
    pub tool_diameter: f64,
    /// Hard limit on the radial engagement angle, degrees (0 to 180, exclusive).
    pub max_engagement_deg: f64,
    /// Depth of each layer.
    pub step_down: f64,
    /// Top of the material (Z).
    pub top: f64,
    /// Floor of the cut (Z).
    pub bottom: f64,
    /// Material left on the walls and islands for finishing (radial).
    pub stock_to_leave: f64,
    /// Height for rapids between cuts (absolute Z, above `top`).
    pub safe_z: f64,
    /// Cutting feed at full engagement, mm/min.
    pub feed: f64,
    /// Feed for helical entries and plunges into cleared space.
    pub plunge_feed: f64,
    /// Feed for links through cleared space.
    pub link_feed: f64,
    /// Helix ramp angle, degrees.
    pub helix_ramp_deg: f64,
    /// Preferred helix radius (tool-centre path); `None` for 0.3 x diameter.
    pub helix_radius: Option<f64>,
    /// Raise the feed where engagement is low so the chip thickness stays as programmed
    /// (radial chip thinning), by at most this factor. `None` keeps the feed constant.
    pub chip_thinning_max: Option<f64>,
    /// Raster cell size, at most 1/60 of the tool diameter; `None` for 1/80.
    pub resolution: Option<f64>,
    /// Link between passes at depth through cleared space where that is safe.
    pub keep_tool_down: bool,
}

impl AdaptiveParams {
    /// Typical parameters: 40 degree engagement limit, one-diameter step-down.
    pub fn new(tool_diameter: f64, top: f64, bottom: f64, feed: f64) -> AdaptiveParams {
        AdaptiveParams {
            tool_diameter,
            max_engagement_deg: 40.0,
            step_down: tool_diameter,
            top,
            bottom,
            stock_to_leave: 0.0,
            safe_z: top + 5.0,
            feed,
            plunge_feed: feed * 0.3,
            link_feed: feed,
            helix_ramp_deg: 2.0,
            helix_radius: None,
            chip_thinning_max: None,
            resolution: None,
            keep_tool_down: true,
        }
    }

    /// The engagement angle (degrees) of a straight cut with radial stepover `ae`.
    pub fn engagement_for_stepover(tool_diameter: f64, ae: f64) -> f64 {
        let r = tool_diameter * 0.5;
        (1.0 - (ae / r).clamp(0.0, 2.0)).acos().to_degrees()
    }

    fn validate(&self) -> Result<()> {
        positive("tool diameter", self.tool_diameter)?;
        let e = finite("max engagement", self.max_engagement_deg)?;
        if !(1.0..180.0).contains(&e) {
            return Err(CamError::InvalidParameter {
                name: "max engagement",
                reason: "must be at least 1 and less than 180 degrees (180 is a full slot)".into(),
            });
        }
        positive("step-down", self.step_down)?;
        finite("top", self.top)?;
        finite("bottom", self.bottom)?;
        if self.bottom >= self.top {
            return Err(CamError::InvalidParameter {
                name: "bottom",
                reason: "must be below the top".into(),
            });
        }
        non_negative("stock to leave", self.stock_to_leave)?;
        finite("safe Z", self.safe_z)?;
        if self.safe_z <= self.top {
            return Err(CamError::InvalidParameter {
                name: "safe Z",
                reason: "must be above the top".into(),
            });
        }
        positive("feed", self.feed)?;
        positive("plunge feed", self.plunge_feed)?;
        positive("link feed", self.link_feed)?;
        let ramp = positive("helix ramp angle", self.helix_ramp_deg)?;
        if ramp > 45.0 {
            return Err(CamError::InvalidParameter {
                name: "helix ramp angle",
                reason: "must be 45 degrees or less".into(),
            });
        }
        if let Some(r) = self.helix_radius {
            positive("helix radius", r)?;
        }
        if let Some(f) = self.chip_thinning_max {
            let f = positive("chip thinning limit", f)?;
            if f < 1.0 {
                return Err(CamError::InvalidParameter {
                    name: "chip thinning limit",
                    reason: "must be 1 or more".into(),
                });
            }
        }
        if let Some(c) = self.resolution {
            positive("resolution", c)?;
            if c > self.tool_diameter / 60.0 + 1e-12 {
                return Err(CamError::InvalidParameter {
                    name: "resolution",
                    reason:
                        "must be at most 1/60 of the tool diameter (coarser rasters cannot hold the limit)"
                            .into(),
                });
            }
        }
        Ok(())
    }
}

/// What a 2D plan is made of, before it is placed at each layer's depth.
#[derive(Clone, Debug, PartialEq)]
enum Chunk {
    /// Helical entry about `center` with tool-centre radius `radius`, ending at the angle 0
    /// point, travelling counter-clockwise.
    Helix { center: Vec2, radius: f64 },
    /// Retract, rapid and feed down into cleared space at `at`.
    Plunge { at: Vec2 },
    /// Feed through cleared space to `to`.
    Link { to: Vec2 },
    /// A cutting pass: points with the engagement measured when each was planned.
    Cut { pts: Vec<(Vec2, f64)> },
}

/// Summary of a plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveReport {
    pub layers: usize,
    pub passes: usize,
    pub helix_entries: usize,
    /// Retract and re-enter moves between passes (per layer).
    pub retracts: usize,
    /// Links made at depth through cleared space (per layer).
    pub links: usize,
    /// Largest engagement the planner accepted, degrees.
    pub planned_max_engagement_deg: f64,
    /// Area removed per layer, mm^2.
    pub removed_area: f64,
    /// Material the tool could reach but the plan left (per layer), mm^2.
    pub remaining_reachable_area: f64,
    /// Material the tool cannot reach (corners tighter than the tool, narrow gaps), mm^2.
    pub unreachable_area: f64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveResult {
    pub toolpath: Toolpath,
    pub report: AdaptiveReport,
}

struct Planner<'a> {
    region: &'a Region,
    walls: SegIndex,
    r: f64,
    /// Radius at which the guard measures engagement: a quarter cell beyond the tool, so the
    /// raster's error at the rim of a cut counts against the cut, not for it.
    r_guard: f64,
    keep: f64,
    cell: f64,
    /// Engagement the passes are traced at (radians).
    target: f64,
    /// Engagement no cut may exceed in the planner's own simulation (radians).
    guard: f64,
    samples: usize,
    nx: usize,
    ny: usize,
    /// Uncut material in the layer.
    m: Grid,
    /// Tool centre allowed at each cell centre.
    allowed: Vec<bool>,
    /// Material the tool can reach.
    reach: Grid,
    /// Distance from each cell centre to the nearest cell outside the region (mm).
    wall_dist: Vec<f64>,
    helix_pref: f64,
    keep_down: bool,
    /// A cut that removes fewer cells than this is not worth its moves; what it would
    /// have cut is reported as left over.
    min_cut_cells: usize,
    chunks: Vec<Chunk>,
    planned_max: f64,
    work: usize,
    warnings: Vec<String>,
    /// Where the tool is at depth, if it is down.
    pos: Option<Vec2>,
    /// Pieces of material given up on (cells marked so the helix search skips them).
    abandoned: Grid,
    /// The engagement field from the last round, and the cells cut since it was computed
    /// (only cells within reach of those need computing again).
    last_field: Option<Vec<f32>>,
    dirty: Vec<bool>,
    all_dirty: bool,
}

/// Work units (engagement evaluations and simulated samples) allowed per layer.
const MAX_WORK: usize = 2_000_000_000;
/// Rounds of tracing allowed per layer.
const MAX_ROUNDS: usize = 10_000;
/// Passes are traced where the engagement equals this fraction of the guard.
const TARGET_FRACTION: f64 = 0.85;
/// Least margin between the guard (which no planned cut exceeds in the planner's own
/// model) and the hard limit, as a fraction of the limit.
const GUARD_MARGIN: f64 = 0.05;

/// One traced pass before it is cut: points along it, closed or open.
struct Piece {
    pts: Vec<Vec2>,
    closed: bool,
}

impl<'a> Planner<'a> {
    fn new(region: &'a Region, stock: Option<&[Region]>, p: &AdaptiveParams) -> Result<Planner<'a>> {
        let r = p.tool_diameter * 0.5;
        let cell = p.resolution.unwrap_or(p.tool_diameter / 80.0);
        let keep = r + p.stock_to_leave;
        let (lo, hi) = region.bounds();
        let mut inside = Grid::new(lo, hi, cell, 2.0 * cell)?;
        inside.fill_region(region, true);
        let mut m = inside.clone();
        if let Some(stock) = stock {
            let mut s = Grid::new(lo, hi, cell, 2.0 * cell)?;
            for st in stock {
                s.fill_region(st, true);
            }
            m.intersect(&s);
        }
        let walls = SegIndex::new(region.segments(), keep.max(cell * 4.0));
        let (nx, ny) = (inside.nx, inside.ny);
        let outside: Vec<bool> = (0..nx * ny).map(|k| !inside.get(k % nx, k / nx)).collect();
        let wall_dist: Vec<f64> =
            distance_transform(&outside, nx, ny).into_iter().map(|d| d * cell).collect();
        let mut allowed = vec![false; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let k = j * nx + i;
                // Cheap rejection by the raster distance, exact test near the limit.
                if !inside.get(i, j) || wall_dist[k] + cell < keep {
                    continue;
                }
                let q = inside.center(i, j);
                allowed[k] = !walls.any_within(q, keep) && region.contains(q);
            }
        }
        let d = distance_transform(&allowed, nx, ny);
        let mut reach = Grid::new(lo, hi, cell, 2.0 * cell)?;
        for j in 0..ny {
            for i in 0..nx {
                if m.get(i, j) && d[j * nx + i] * cell <= r - 0.5 * cell {
                    reach.set(i, j, true);
                }
            }
        }
        let limit = p.max_engagement_deg.to_radians();
        // The raster places a cut's rim to within about a cell, which moves the engagement
        // of a straight cut by roughly cell / R radians; the guard keeps two of those clear
        // of the limit (and never less than 5 %), and passes are traced below the guard.
        let guard = (limit - 2.0 * cell / r).min(limit * (1.0 - GUARD_MARGIN)).max(0.5 * limit);
        Ok(Planner {
            region,
            walls,
            r,
            r_guard: r + 0.25 * cell,
            keep,
            cell,
            target: guard * TARGET_FRACTION,
            guard,
            samples: ((4.0 * PI * r / cell).ceil() as usize).clamp(64, 2048),
            nx,
            ny,
            abandoned: Grid::new(lo, hi, cell, 2.0 * cell)?,
            m,
            allowed,
            reach,
            wall_dist,
            helix_pref: p.helix_radius.unwrap_or(0.3 * p.tool_diameter),
            keep_down: p.keep_tool_down,
            min_cut_cells: 4,
            chunks: Vec::new(),
            planned_max: 0.0,
            work: 0,
            warnings: Vec::new(),
            pos: None,
            last_field: None,
            dirty: vec![false; nx * ny],
            all_dirty: true,
        })
    }

    /// The tool centre may be at `q`: inside the region and at least `keep` from every
    /// wall and island (exact test against the boundary).
    fn allowed_at(&self, q: Vec2) -> bool {
        match self.m.cell_of(q) {
            Some((i, j)) => {
                let k = j * self.nx + i;
                self.wall_dist[k] + 2.0 * self.cell >= self.keep
                    && !self.walls.any_within(q, self.keep)
                    && self.region.contains(q)
            }
            None => false,
        }
    }

    /// The tool at `q` touches no material.
    fn free(&self, q: Vec2) -> bool {
        self.m.count_disk(q, self.r) == 0
    }

    /// Distance (cells) from every cell centre to the nearest material cell.
    fn material_distance(&self) -> Vec<f64> {
        let mat: Vec<bool> = (0..self.nx * self.ny).map(|k| self.m.get(k % self.nx, k / self.nx)).collect();
        distance_transform(&mat, self.nx, self.ny)
    }

    /// The engagement field the passes are traced on: at each allowed cell centre, the
    /// angle of the tool's whole circumference over material with the tool standing still.
    /// Along a straight edge this is twice the engagement of a tool travelling along it
    /// (the half behind is cut by the tool itself), so passes are traced at twice the
    /// target. Far from material it is 0; deep in material a full turn; where the tool may
    /// not go it is NaN.
    fn field(&mut self, dist: &[f64]) -> Vec<f32> {
        let (nx, ny) = (self.nx, self.ny);
        let c = self.cell;
        let bite = self.r * (1.0 - self.target.cos());
        let lo = self.r - bite - 3.0 * c;
        let hi = self.r + 1.5 * c;
        // Only cells whose tool circle can reach a cell cut since last time can change.
        let recompute: Option<Vec<bool>> = match (&self.last_field, self.all_dirty) {
            (Some(_), false) => {
                let d = distance_transform(&self.dirty, nx, ny);
                let reach = (self.r + 2.0 * c) / c;
                Some(d.into_iter().map(|x| x <= reach).collect())
            }
            _ => None,
        };
        let mut f = match (&recompute, self.last_field.take()) {
            (Some(_), Some(old)) => old,
            _ => vec![f32::NAN; nx * ny],
        };
        self.dirty.iter_mut().for_each(|d| *d = false);
        self.all_dirty = false;
        for j in 1..ny.saturating_sub(1) {
            for i in 1..nx.saturating_sub(1) {
                let k = j * nx + i;
                if !self.allowed[k] || recompute.as_ref().is_some_and(|rc| !rc[k]) {
                    continue;
                }
                let d = dist[k] * c;
                if d > hi {
                    f[k] = 0.0;
                    continue;
                }
                if d < lo {
                    f[k] = TAU as f32;
                    continue;
                }
                let q = self.m.center(i, j);
                self.work += self.samples;
                let e = self.m.engagement(q, self.r, self.samples);
                f[k] = e as f32;
            }
        }
        self.last_field = Some(f.clone());
        f
    }

    /// Curves where the field crosses `level`, by marching squares over cell centres.
    /// Squares with a NaN corner are skipped, so curves stop where the tool may not go.
    fn contours(&self, f: &[f32], level: f32) -> Vec<Piece> {
        let (nx, ny) = (self.nx, self.ny);
        // Edge ids: horizontal edge (i,j)-(i+1,j) is 2*(j*nx+i), vertical (i,j)-(i,j+1) is +1.
        let h_id = |i: usize, j: usize| 2 * (j * nx + i);
        let v_id = |i: usize, j: usize| 2 * (j * nx + i) + 1;
        let point = |id: usize| -> Vec2 {
            let k = id / 2;
            let (i, j) = (k % nx, k / nx);
            let (a, b, k2) =
                if id.is_multiple_of(2) { (k, k + 1, (i + 1, j)) } else { (k, k + nx, (i, j + 1)) };
            let (fa, fb) = (f[a] as f64, f[b] as f64);
            let t =
                if (fb - fa).abs() < 1e-12 { 0.5 } else { ((level as f64 - fa) / (fb - fa)).clamp(0.0, 1.0) };
            self.m.center(i, j).lerp(self.m.center(k2.0, k2.1), t)
        };
        // Segment list as pairs of edge ids, oriented so that the higher field (material)
        // is on the right of a -> b.
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for j in 0..ny.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                let k = j * nx + i;
                let v = [f[k], f[k + 1], f[k + 1 + nx], f[k + nx]];
                if v.iter().any(|x| x.is_nan()) {
                    continue;
                }
                let above = |x: f32| x > level;
                let code = (above(v[0]) as u8)
                    | (above(v[1]) as u8) << 1
                    | (above(v[2]) as u8) << 2
                    | (above(v[3]) as u8) << 3;
                // Edges of the square: bottom, right, top, left.
                let (eb, er, et, el) = (h_id(i, j), v_id(i + 1, j), h_id(i, j + 1), v_id(i, j));
                // The table pairs edges; each segment is then turned so the corner it cuts
                // off lies on the correct side (higher field on the right).
                let corners = [
                    self.m.center(i, j),
                    self.m.center(i + 1, j),
                    self.m.center(i + 1, j + 1),
                    self.m.center(i, j + 1),
                ];
                let mut push = |a: usize, b: usize| {
                    let (pa, pb) = (point(a), point(b));
                    let mid = pa.lerp(pb, 0.5);
                    let (ci, _) = corners
                        .iter()
                        .enumerate()
                        .map(|(ci, c)| (ci, c.distance(mid)))
                        .fold((0, f64::INFINITY), |acc, x| if x.1 < acc.1 { x } else { acc });
                    let left = (pb - pa).cross(corners[ci] - pa) > 0.0;
                    let high = above(v[ci]);
                    if left == high {
                        segs.push((b, a));
                    } else {
                        segs.push((a, b));
                    }
                };
                match code {
                    0 | 15 => {}
                    1 => push(eb, el),
                    2 => push(er, eb),
                    3 => push(er, el),
                    4 => push(et, er),
                    6 => push(et, eb),
                    7 => push(et, el),
                    8 => push(el, et),
                    9 => push(eb, et),
                    11 => push(er, et),
                    12 => push(el, er),
                    13 => push(eb, er),
                    14 => push(el, eb),
                    5 | 10 => {
                        let centre = (v[0] + v[1] + v[2] + v[3]) * 0.25;
                        let joined = above(centre);
                        match (code, joined) {
                            (5, true) => {
                                push(eb, el);
                                push(et, er);
                            }
                            (5, false) => {
                                push(eb, er);
                                push(et, el);
                            }
                            (10, true) => {
                                push(er, eb);
                                push(el, et);
                            }
                            _ => {
                                push(er, et);
                                push(el, eb);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // Chain: each edge id starts at most one segment and ends at most one.
        let mut next: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::with_capacity(segs.len());
        let mut has_prev: std::collections::HashSet<usize> =
            std::collections::HashSet::with_capacity(segs.len());
        for &(a, b) in &segs {
            next.insert(a, b);
            has_prev.insert(b);
        }
        let mut used: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut out = Vec::new();
        // Open chains first (starting where nothing leads in), then closed loops.
        let mut starts: Vec<usize> = segs.iter().map(|s| s.0).filter(|a| !has_prev.contains(a)).collect();
        starts.extend(segs.iter().map(|s| s.0));
        for s in starts {
            if used.contains(&s) || !next.contains_key(&s) {
                continue;
            }
            let mut ids = vec![s];
            used.insert(s);
            let mut cur = s;
            let mut closed = false;
            while let Some(&n) = next.get(&cur) {
                if n == s {
                    closed = true;
                    break;
                }
                if used.contains(&n) {
                    break;
                }
                used.insert(n);
                ids.push(n);
                cur = n;
            }
            let mut pts: Vec<Vec2> = ids.iter().map(|&id| point(id)).collect();
            pts.dedup_by(|a, b| a.distance(*b) < 1e-9);
            if pts.len() >= 2 {
                out.push(Piece { pts, closed });
            }
        }
        out
    }

    /// Simulates cutting along the guide curve `pts` from its start, which must be clear
    /// of material. Each sample point may be moved out to the free (left) side of the
    /// guide, by as little as keeps its engagement within the guard and by at most a
    /// quarter cell less than the previous sample (so the path returns to the guide
    /// smoothly); this is the offset correction of Stori and Wright's constant-engagement
    /// method. The cut stops where no such point exists. Returns the points cut with the
    /// engagement at each, the cells removed, and the log needed to undo it.
    fn simulate(&mut self, pts: &[Vec2]) -> (Vec<(Vec2, f64)>, usize, Vec<usize>) {
        let mut log = Vec::new();
        let mut out = vec![(pts[0], 0.0)];
        let mut removed = 0usize;
        let h = 0.5 * self.cell;
        let step = 0.25 * self.cell;
        let max_shift = self.r * (1.0 - self.target.cos()) + 4.0 * self.cell;
        let mut prev = pts[0];
        let mut shift = 0.0f64;
        'outer: for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            if d.length() < 1e-12 {
                continue;
            }
            let left = d.normalized().perp();
            let n = ((a.distance(b) / h).ceil() as usize).max(1);
            for k in 1..=n {
                let g = a.lerp(b, k as f64 / n as f64);
                let mut found = None;
                // Ease back towards the guide only when that is comfortably within the
                // target; otherwise hold the shift, and move out only as far as needed.
                if shift > 0.0 {
                    let q = g + left * (shift - step).max(0.0);
                    if self.allowed_at(q) {
                        let e = self.m.engagement(q, self.r_guard, self.samples);
                        self.work += self.samples;
                        if e <= self.target {
                            found = Some((q, e, (shift - step).max(0.0)));
                        }
                    }
                }
                let mut sh = shift;
                while found.is_none() && sh <= max_shift + 1e-12 {
                    let q = g + left * sh;
                    if self.allowed_at(q) {
                        let e = self.m.engagement(q, self.r_guard, self.samples);
                        self.work += self.samples;
                        if e <= self.guard {
                            found = Some((q, e, sh));
                            break;
                        }
                    }
                    sh += step;
                }
                let Some((q, e, sh)) = found else { break 'outer };
                // A shift that jumps sideways by more than a sample spacing is a gap, not a path.
                if q.distance(prev) > 2.0 * h + max_shift {
                    break 'outer;
                }
                shift = sh;
                removed += self.m.clear_disk_after_logged(prev, q, self.r, &mut log);
                out.push((q, e));
                prev = q;
            }
        }
        (out, removed, log)
    }

    fn undo(&mut self, log: &[usize]) {
        for &k in log {
            self.m.set(k % self.nx, k / self.nx, true);
        }
    }

    /// The start of a pass bent out to the free (left) side so the cut begins clear of the
    /// material and joins the traced curve gradually over 1.5 tool diameters: the bite
    /// grows from nothing, and what lies behind the tool is what it has just cut. Joining
    /// a curve head-on would leave uncut material behind the tool as well as ahead of it.
    /// Returns `None` when no clear starting point is found.
    fn blend_in(&self, pts: &[Vec2]) -> Option<Vec<Vec2>> {
        let total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        let len = (3.0 * self.r).min(0.5 * total);
        if len <= self.cell {
            return None;
        }
        let bite = self.r * (1.0 - self.target.cos());
        for out in [bite + 2.0 * self.cell, 2.0 * bite + 3.0 * self.cell, 0.5 * self.r + bite] {
            let mut res = Vec::with_capacity(pts.len());
            let mut s = 0.0;
            for (k, &p) in pts.iter().enumerate() {
                if k > 0 {
                    s += pts[k - 1].distance(p);
                }
                if s >= len {
                    res.extend_from_slice(&pts[k..]);
                    break;
                }
                let a = if k + 1 < pts.len() { pts[k + 1] } else { p };
                let b = if k > 0 { pts[k - 1] } else { p };
                let t = (a - b).normalized();
                // Cosine blend: steepest at half bite, tangent where it joins the curve.
                res.push(p + t.perp() * out * 0.5 * (1.0 + (PI * s / len).cos()));
            }
            if self.allowed_at(res[0]) && self.free(res[0]) {
                return Some(res);
            }
        }
        None
    }

    /// Straight feed from `a` to `b` touches no material and stays where the tool may go.
    fn link_clear(&self, a: Vec2, b: Vec2) -> bool {
        let n = (a.distance(b) / self.cell).ceil().max(1.0) as usize;
        if n > 200_000 {
            return false;
        }
        // A cell's width less than the tool, as the verifier checks links: the cells on the
        // rim of a cut are judged at their centres and may lie just outside it.
        let rr = self.r - self.cell;
        (0..=n).all(|k| {
            let q = a.lerp(b, k as f64 / n as f64);
            self.allowed_at(q) && self.m.engagement(q, rr, self.samples) == 0.0
        }) && self.m.count_disk(b, rr) == 0
    }

    /// A path of straight feeds through cleared space from `a` to `b`, found on a coarse
    /// grid of clear tool positions and shortened where a straight line is clear.
    fn link_path(&mut self, a: Vec2, b: Vec2) -> Option<Vec<Vec2>> {
        if self.link_clear(a, b) {
            return Some(vec![b]);
        }
        let k = ((self.r / 16.0) / self.cell).round().max(1.0) as usize;
        let (gx, gy) = (self.nx.div_ceil(k), self.ny.div_ceil(k));
        let dist = self.material_distance();
        // Node centres need only be clear; every straight piece is checked exactly below.
        let margin = (self.r - self.cell) / self.cell;
        let node_free = |gi: usize, gj: usize| -> bool {
            let (i, j) = ((gi * k + k / 2).min(self.nx - 1), (gj * k + k / 2).min(self.ny - 1));
            let idx = j * self.nx + i;
            self.allowed[idx] && dist[idx] > margin
        };
        let node_of = |p: Vec2| -> Option<(usize, usize)> { self.m.cell_of(p).map(|(i, j)| (i / k, j / k)) };
        let (sa, sb) = (node_of(a)?, node_of(b)?);
        let centre = |gi: usize, gj: usize| {
            self.m.center((gi * k + k / 2).min(self.nx - 1), (gj * k + k / 2).min(self.ny - 1))
        };
        // A* with 8 neighbours.
        let n = gx * gy;
        let mut g = vec![f64::INFINITY; n];
        let mut from = vec![usize::MAX; n];
        let mut open = std::collections::BinaryHeap::new();
        let id = |gi: usize, gj: usize| gj * gx + gi;
        let goal = centre(sb.0, sb.1);
        g[id(sa.0, sa.1)] = 0.0;
        open.push(Node { f: -centre(sa.0, sa.1).distance(goal), id: id(sa.0, sa.1) });
        let mut found = false;
        let mut expanded = 0usize;
        while let Some(Node { id: cur, .. }) = open.pop() {
            if cur == id(sb.0, sb.1) {
                found = true;
                break;
            }
            expanded += 1;
            if expanded > n {
                break;
            }
            let (ci, cj) = (cur % gx, cur / gx);
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (ni, nj) = (ci as i64 + di, cj as i64 + dj);
                if ni < 0 || nj < 0 || ni >= gx as i64 || nj >= gy as i64 {
                    continue;
                }
                let (ni, nj) = (ni as usize, nj as usize);
                let nid = id(ni, nj);
                if nid != id(sb.0, sb.1) && !node_free(ni, nj) {
                    continue;
                }
                let step = centre(ci, cj).distance(centre(ni, nj));
                let cand = g[cur] + step;
                if cand < g[nid] {
                    g[nid] = cand;
                    from[nid] = cur;
                    open.push(Node { f: -(cand + centre(ni, nj).distance(goal)), id: nid });
                }
            }
        }
        self.work += expanded * 8;
        if !found {
            return None;
        }
        let mut nodes = vec![b];
        let mut cur = from[id(sb.0, sb.1)];
        while cur != usize::MAX && cur != id(sa.0, sa.1) {
            nodes.push(centre(cur % gx, cur / gx));
            cur = from[cur];
        }
        nodes.reverse();
        // Shorten: from each point jump to the farthest point that is in straight view.
        let mut path = Vec::new();
        let mut here = a;
        let mut idx = 0usize;
        while idx < nodes.len() {
            let mut best = None;
            for j in (idx..nodes.len()).rev() {
                if self.link_clear(here, nodes[j]) {
                    best = Some(j);
                    break;
                }
            }
            let j = best?;
            path.push(nodes[j]);
            here = nodes[j];
            idx = j + 1;
        }
        Some(path)
    }

    /// Moves the tool (at depth or from above) to the clear point `q`.
    fn go_to(&mut self, q: Vec2) {
        match self.pos {
            Some(p) if self.keep_down => match self.link_path(p, q) {
                Some(path) => {
                    for to in path {
                        self.chunks.push(Chunk::Link { to });
                    }
                }
                None => self.chunks.push(Chunk::Plunge { at: q }),
            },
            _ => self.chunks.push(Chunk::Plunge { at: q }),
        }
        self.pos = Some(q);
    }

    /// Tries to cut `piece` (or, for a closed loop, the loop from one of a few starting
    /// points). Returns true when something worth cutting was cut.
    fn cut_piece(&mut self, piece: &Piece) -> bool {
        let n = piece.pts.len();
        let starts: Vec<usize> = if piece.closed {
            // Prefer the point nearest the tool, then a few others round the loop.
            let near = self.pos.map_or(0, |p| {
                (0..n)
                    .min_by(|&a, &b| piece.pts[a].distance(p).total_cmp(&piece.pts[b].distance(p)))
                    .unwrap_or(0)
            });
            (0..4).map(|k| (near + k * n / 4) % n).collect()
        } else {
            vec![0]
        };
        for s in starts {
            let pts: Vec<Vec2> = if piece.closed {
                let mut v: Vec<Vec2> = piece.pts[s..].to_vec();
                v.extend_from_slice(&piece.pts[..=s]);
                v
            } else {
                piece.pts.clone()
            };
            let Some(pts) = self.blend_in(&pts) else { continue };
            let lead = pts[0];
            let (cut, removed, log) = self.simulate(&pts);
            // Worth cutting: some material for each cell of length, and a few cells at least.
            let length: f64 = cut.windows(2).map(|w| w[0].0.distance(w[1].0)).sum();
            let wanted = ((0.05 * length / self.cell) as usize).max(self.min_cut_cells);
            if removed < wanted || cut.len() < 2 {
                self.undo(&log);
                continue;
            }
            // Plan the move to the start against the material as it was before this cut.
            self.undo(&log);
            self.go_to(lead);
            for &k in &log {
                self.m.set(k % self.nx, k / self.nx, false);
                self.dirty[k] = true;
            }
            for (_, e) in &cut {
                self.planned_max = self.planned_max.max(*e);
            }
            self.pos = cut.last().map(|c| c.0);
            self.chunks.push(Chunk::Cut { pts: cut });
            return true;
        }
        false
    }

    /// Picks a helix in the remaining reachable material: the point farthest from the
    /// walls, with the preferred radius or less.
    fn find_helix(&mut self) -> Option<(Vec2, f64)> {
        let min_radius = 0.1 * 2.0 * self.r;
        for _attempt in 0..64 {
            let mut best: Option<(f64, usize, usize)> = None;
            for j in 0..self.ny {
                for i in 0..self.nx {
                    let k = j * self.nx + i;
                    if self.m.get(i, j)
                        && self.reach.get(i, j)
                        && !self.abandoned.get(i, j)
                        && self.allowed[k]
                    {
                        let d = self.wall_dist[k];
                        if best.is_none_or(|b| d > b.0) {
                            best = Some((d, i, j));
                        }
                    }
                }
            }
            let (d, i, j) = best?;
            let c = self.m.center(i, j);
            let mut radius = self.helix_pref.min(d - self.keep - 2.0 * self.cell);
            while radius >= min_radius {
                let ok =
                    (0..36).all(|k| self.allowed_at(c + Vec2::from_angle(TAU * k as f64 / 36.0) * radius));
                if ok {
                    return Some((c, radius));
                }
                radius *= 0.8;
            }
            let n = self.flood_abandon(i, j);
            self.warnings.push(format!(
                "about {:.1} mm^2 of material near ({:.2}, {:.2}) has no room for a helical entry with this tool",
                n as f64 * self.m.cell_area(),
                c.x,
                c.y
            ));
        }
        None
    }

    /// Marks the connected piece of reachable material containing (i, j) as abandoned.
    fn flood_abandon(&mut self, i: usize, j: usize) -> usize {
        let mut stack = vec![(i, j)];
        let mut n = 0;
        while let Some((a, b)) = stack.pop() {
            if self.abandoned.get(a, b) || !self.m.get(a, b) || !self.reach.get(a, b) {
                continue;
            }
            self.abandoned.set(a, b, true);
            n += 1;
            if a > 0 {
                stack.push((a - 1, b));
            }
            if b > 0 {
                stack.push((a, b - 1));
            }
            if a + 1 < self.nx {
                stack.push((a + 1, b));
            }
            if b + 1 < self.ny {
                stack.push((a, b + 1));
            }
        }
        n
    }

    fn plan(&mut self) {
        let mut cleanup = false;
        for _round in 0..MAX_ROUNDS {
            if self.work > MAX_WORK {
                self.warnings.push("the planning budget ran out; some material was left".into());
                break;
            }
            let dist = self.material_distance();
            let f = self.field(&dist);
            let mut pieces = self.contours(&f, (2.0 * self.target) as f32);
            for p in pieces.iter_mut() {
                smooth(&mut p.pts, p.closed, self.cell);
            }
            // Near pieces first.
            let mut cut_any = false;
            while !pieces.is_empty() {
                let here = self.pos.unwrap_or(Vec2::ZERO);
                let (k, _) = pieces
                    .iter()
                    .enumerate()
                    .map(|(k, p)| {
                        let d = if p.closed {
                            p.pts.iter().map(|q| q.distance(here)).fold(f64::INFINITY, f64::min)
                        } else {
                            p.pts[0].distance(here)
                        };
                        (k, d)
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap_or((0, 0.0));
                let piece = pieces.swap_remove(k);
                if self.cut_piece(&piece) {
                    cut_any = true;
                }
            }
            if cut_any {
                cleanup = false;
                continue;
            }
            if !cleanup {
                // Thin material along the walls that no traced pass reaches: run along
                // the boundary of where the tool may go, cutting where the cap allows.
                cleanup = true;
                let ring: Vec<f32> =
                    (0..self.nx * self.ny).map(|k| if self.allowed[k] { 0.0 } else { 1.0 }).collect();
                let mut pieces = self.contours(&ring, 0.5);
                let mut any = false;
                for p in pieces.iter_mut() {
                    // Pull the ring a quarter cell into the allowed side.
                    for q in p.pts.iter_mut() {
                        *q = self.nudge_inside(*q);
                    }
                }
                for p in &pieces {
                    any |= self.cut_piece(p);
                }
                if any {
                    continue;
                }
            }
            if let Some((c, radius)) = self.find_helix() {
                self.m.clear_disk(c, radius + self.r);
                self.all_dirty = true;
                self.chunks.push(Chunk::Helix { center: c, radius });
                self.pos = Some(c + v2(radius, 0.0));
                cleanup = false;
                continue;
            }
            break;
        }
        // Drop trailing positioning moves with nothing after them.
        while matches!(self.chunks.last(), Some(Chunk::Link { .. } | Chunk::Plunge { .. })) {
            self.chunks.pop();
        }
    }

    /// Moves a point on the boundary of the allowed cells to the nearest allowed point.
    fn nudge_inside(&self, q: Vec2) -> Vec2 {
        if self.allowed_at(q) {
            return q;
        }
        for r in 1..=8 {
            let rr = r as f64 * 0.25 * self.cell;
            for k in 0..16 {
                let p = q + Vec2::from_angle(TAU * k as f64 / 16.0) * rr;
                if self.allowed_at(p) {
                    return p;
                }
            }
        }
        q
    }

    fn remaining(&self) -> (f64, f64) {
        let (mut reach, mut unreach) = (0usize, 0usize);
        for (i, j) in self.m.set_cells() {
            if self.reach.get(i, j) {
                reach += 1;
            } else {
                unreach += 1;
            }
        }
        (reach as f64 * self.m.cell_area(), unreach as f64 * self.m.cell_area())
    }
}

/// A* queue entry (max-heap on `f`, so `f` holds the negated cost).
#[derive(PartialEq)]
struct Node {
    f: f64,
    id: usize,
}

impl Eq for Node {}

impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.f.total_cmp(&o.f)
    }
}

/// Smooths a traced curve (three passes of neighbour averaging, ends held) and
/// resamples it at about one cell, removing the raster's stair steps.
fn smooth(pts: &mut Vec<Vec2>, closed: bool, cell: f64) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    for _ in 0..3 {
        let old = pts.clone();
        for k in 0..n {
            let (a, b) = if closed {
                (old[(k + n - 1) % n], old[(k + 1) % n])
            } else if k == 0 || k == n - 1 {
                continue;
            } else {
                (old[k - 1], old[k + 1])
            };
            pts[k] = old[k] * 0.5 + (a + b) * 0.25;
        }
    }
    // Resample: drop points closer than a cell to the last one kept.
    let mut out = vec![pts[0]];
    for &p in pts.iter().skip(1) {
        if out.last().is_some_and(|q| q.distance(p) >= cell) {
            out.push(p);
        }
    }
    if !closed
        && out.last() != pts.last()
        && let Some(&last) = pts.last()
    {
        out.push(last);
    }
    *pts = out;
}

/// Removes points that lie within `tol` of the line through their neighbours and share
/// the same feed, so long straight runs become single moves.
fn simplify(pts: &[(Vec2, f64)], tol: f64) -> Vec<(Vec2, f64)> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    let mut anchor = 0usize;
    for k in 1..pts.len() - 1 {
        let a = pts[anchor].0;
        let c = pts[k + 1].0;
        let ok = (anchor + 1..=k).all(|m| {
            crate::geom::seg_distance(pts[m].0, a, c) <= tol && (pts[m].1 - pts[k + 1].1).abs() < 1e-9
        });
        if !ok {
            out.push(pts[k]);
            anchor = k;
        }
    }
    if let Some(last) = pts.last() {
        out.push(*last);
    }
    out
}

/// Plans adaptive clearing of `region` (the tool stays inside it, clear of its islands).
/// `stock` is where material actually is; `None` means the whole region. Where the
/// region extends past the stock the tool enters from the side through air; elsewhere it
/// enters by a helix.
pub fn adaptive_clear(
    region: &Region,
    stock: Option<&[Region]>,
    params: &AdaptiveParams,
) -> Result<AdaptiveResult> {
    params.validate()?;
    let mut planner = Planner::new(region, stock, params)?;
    let initial = planner.m.count() as f64 * planner.m.cell_area();
    planner.plan();
    let (remaining_reachable, unreachable) = planner.remaining();
    let removed = initial - remaining_reachable - unreachable;
    if planner.chunks.is_empty() && initial > 0.0 {
        return Err(CamError::Unreachable(
            "no entry was found: the region is too narrow for this tool or holds no reachable material"
                .into(),
        ));
    }

    let depth = params.top - params.bottom;
    let layers = ((depth / params.step_down) - 1e-9).ceil().max(1.0) as usize;
    let levels: Vec<f64> = (1..=layers).map(|k| params.top - depth * k as f64 / layers as f64).collect();
    let cell = planner.cell;
    let mut tp = Toolpath::new();
    let mut report = AdaptiveReport {
        layers,
        planned_max_engagement_deg: planner.planned_max.to_degrees(),
        removed_area: removed.max(0.0),
        remaining_reachable_area: remaining_reachable,
        unreachable_area: unreachable,
        warnings: planner.warnings.clone(),
        ..AdaptiveReport::default()
    };
    for c in &planner.chunks {
        match c {
            Chunk::Cut { .. } => report.passes += 1,
            Chunk::Helix { .. } => report.helix_entries += 1,
            Chunk::Link { .. } => report.links += 1,
            Chunk::Plunge { .. } => report.retracts += 1,
        }
    }
    if remaining_reachable > 0.05 * initial.max(1e-9) {
        report
            .warnings
            .push(format!("{remaining_reachable:.1} mm^2 of reachable material per layer was left uncut"));
    }

    let feed_for = |e: f64| -> f64 {
        match params.chip_thinning_max {
            Some(max) => {
                // Quantized to 5 % steps so runs of similar engagement share a feed.
                let f = chip_thinning_factor(e, max);
                params.feed * (f * 20.0).round() / 20.0
            }
            None => params.feed,
        }
    };

    let mut up = true;
    for (k, &z) in levels.iter().enumerate() {
        let above = if k == 0 { params.top } else { levels[k - 1] };
        let approach = (above + 1.0).min(params.safe_z);
        tp.push(crate::toolpath::Item::Comment(format!("adaptive layer {} of {} at Z{z:.3}", k + 1, layers)));
        let mut here = Vec2::ZERO;
        for c in &planner.chunks {
            match c {
                Chunk::Helix { center, radius } => {
                    let start = *center + v2(*radius, 0.0);
                    if !up {
                        tp.rapid(v3(here.x, here.y, params.safe_z));
                    }
                    tp.rapid(v3(start.x, start.y, params.safe_z));
                    tp.rapid(v3(start.x, start.y, approach));
                    // Half turns, each dropping by the ramp, then a flat full turn.
                    let per_turn = TAU * radius * params.helix_ramp_deg.to_radians().tan();
                    let drop = approach - z;
                    let halves = ((2.0 * drop / per_turn.max(1e-6)).ceil() as usize).clamp(1, 100_000);
                    let other = *center - v2(*radius, 0.0);
                    for h in 1..=halves {
                        let zz = approach - drop * h as f64 / halves as f64;
                        let to = if h % 2 == 1 { other } else { start };
                        tp.arc(v3(to.x, to.y, zz), *center, true, MoveKind::Entry, params.plunge_feed);
                    }
                    let at = if halves % 2 == 1 { other } else { start };
                    let back = if halves % 2 == 1 { start } else { other };
                    tp.arc(v3(back.x, back.y, z), *center, true, MoveKind::Entry, params.plunge_feed);
                    tp.arc(v3(at.x, at.y, z), *center, true, MoveKind::Entry, params.plunge_feed);
                    if halves % 2 == 1 {
                        tp.arc(v3(start.x, start.y, z), *center, true, MoveKind::Entry, params.plunge_feed);
                    }
                    here = start;
                    up = false;
                }
                Chunk::Plunge { at } => {
                    if !up {
                        tp.rapid(v3(here.x, here.y, params.safe_z));
                    }
                    tp.rapid(v3(at.x, at.y, params.safe_z));
                    tp.rapid(v3(at.x, at.y, approach));
                    tp.line(v3(at.x, at.y, z), MoveKind::Entry, params.plunge_feed);
                    here = *at;
                    up = false;
                }
                Chunk::Link { to } => {
                    tp.line(v3(to.x, to.y, z), MoveKind::Link, params.link_feed);
                    here = *to;
                }
                Chunk::Cut { pts } => {
                    // Feed per point, then merge runs that share a feed.
                    let fed: Vec<(Vec2, f64)> = pts.iter().map(|(p, e)| (*p, feed_for(*e))).collect();
                    for (p, f) in simplify(&fed, 0.1 * cell).iter().skip(1) {
                        tp.line(v3(p.x, p.y, z), MoveKind::Cut, *f);
                        here = *p;
                    }
                }
            }
        }
        tp.rapid(v3(here.x, here.y, params.safe_z));
        up = true;
    }
    Ok(AdaptiveResult { toolpath: tp, report })
}

/// Engagement measured along a finished tool path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EngagementReport {
    /// Largest engagement on a cutting move, degrees.
    pub max_engagement_deg: f64,
    /// Where it happened.
    pub max_at: Option<Vec3>,
    /// Samples taken on cutting moves.
    pub samples: usize,
    /// Cutting samples over the limit, with their positions and engagement (degrees).
    pub over_limit: Vec<(Vec3, f64)>,
    /// Link or rapid samples that touched material.
    pub links_touching: Vec<Vec3>,
    /// Cutting samples at or near a full slot (over 170 degrees).
    pub slotting: Vec<Vec3>,
    /// Material left in the last layer replayed, mm^2.
    pub remaining_area: f64,
}

/// Replays `toolpath` into a fresh raster of `region` intersected with `stock` at cell
/// size `cell`, layer by layer, and measures the engagement of a tool of `tool_diameter`
/// at points no more than half a cell apart along every cutting move, counting contact
/// where the material at the tool's edge is at least two cells thick. Entry moves remove
/// material but are not held to the limit (they cut with the tool's end). Link and rapid
/// moves must clear the material by `link_tolerance` (use the planner's cell size: rims
/// of cuts are known only to a cell). This is the check that the planner kept its
/// promise; it shares no state with the planner.
/// What [`measure_engagement`] checks against.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngagementCheck {
    pub tool_diameter: f64,
    /// Hard limit, degrees.
    pub limit_deg: f64,
    /// Top of the material: moves above it are ignored.
    pub top: f64,
    /// Raster cell size of the replay.
    pub cell: f64,
    /// How far link and rapid moves may brush the rim of earlier cuts (use the planner's
    /// cell size).
    pub link_tolerance: f64,
}

pub fn measure_engagement(
    region: &Region,
    stock: Option<&[Region]>,
    toolpath: &Toolpath,
    check: &EngagementCheck,
) -> Result<EngagementReport> {
    let EngagementCheck { tool_diameter, limit_deg, top, cell, link_tolerance } = *check;
    positive("tool diameter", tool_diameter)?;
    positive("cell", cell)?;
    finite("top", top)?;
    non_negative("link tolerance", link_tolerance)?;
    let r = tool_diameter * 0.5;
    let (lo, hi) = region.bounds();
    let mut base = Grid::new(lo, hi, cell, 2.0 * cell + r)?;
    base.fill_region(region, true);
    if let Some(stock) = stock {
        let mut s = Grid::new(lo, hi, cell, 2.0 * cell + r)?;
        for st in stock {
            s.fill_region(st, true);
        }
        base.intersect(&s);
    }
    let samples = ((4.0 * PI * r / cell).ceil() as usize).clamp(64, 4096);
    let mut rep = EngagementReport::default();
    let mut layer: Option<(f64, Grid)> = None;
    let mut last_cleared: Option<Vec2> = None;
    let limit = limit_deg.to_radians();
    // Removes the tool disk at `q`, visiting only the new crescent when the previous
    // cleared disk overlaps it.
    let clear = |g: &mut Grid, prev: Option<Vec2>, q: Vec2| match prev {
        Some(pv) if pv.distance(q) < r => {
            g.clear_disk_after(pv, q, r);
        }
        _ => {
            g.clear_disk(q, r);
        }
    };
    for seg in toolpath.segments(cell * 0.25) {
        let (a, b) = (seg.a, seg.b);
        let low = a.z.min(b.z);
        if low >= top - 1e-9 {
            continue;
        }
        // A new layer starts when the tool goes lower than the current one.
        let new_layer = layer.as_ref().is_none_or(|(z, _)| low < *z - 1e-6);
        if new_layer {
            if let Some((_, g)) = &layer {
                rep.remaining_area = g.count() as f64 * g.cell_area();
            }
            layer = Some((low, base.clone()));
            last_cleared = None;
        }
        let Some((lz, g)) = layer.as_mut() else { continue };
        let at_layer = |z: f64| (z - *lz).abs() < 1e-6;
        let pa = v2(a.x, a.y);
        let pb = v2(b.x, b.y);
        let n = ((pa.distance(pb) / (0.5 * cell)).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let q = pa.lerp(pb, t);
            let z = a.z + (b.z - a.z) * t;
            if z > *lz + 1e-6 {
                // Above this layer: only rapids and entries pass here, through cleared space.
                continue;
            }
            match seg.kind {
                MoveKind::Cut | MoveKind::Lead => {
                    if !at_layer(z) {
                        continue;
                    }
                    // Engagement against the material left by everything before this point,
                    // counting contact only where the material is at least two cells thick:
                    // a rim thinner than the raster can show is not a cut.
                    let e = g.engagement_solid(q, r, samples);
                    rep.samples += 1;
                    let p3 = v3(q.x, q.y, z);
                    if e > rep.max_engagement_deg.to_radians() {
                        rep.max_engagement_deg = e.to_degrees();
                        rep.max_at = Some(p3);
                    }
                    if e > limit {
                        rep.over_limit.push((p3, e.to_degrees()));
                    }
                    if e > 170f64.to_radians() {
                        rep.slotting.push(p3);
                    }
                    clear(g, last_cleared, q);
                    last_cleared = Some(q);
                }
                MoveKind::Link | MoveKind::Rapid => {
                    let rr = (r - link_tolerance.max(cell)).max(0.0);
                    if g.engagement(q, rr, samples) > 0.0 || g.count_disk(q, rr) > 0 {
                        rep.links_touching.push(v3(q.x, q.y, z));
                    }
                    last_cleared = None;
                }
                MoveKind::Entry | MoveKind::Retract => {
                    clear(g, last_cleared, q);
                    last_cleared = Some(q);
                }
            }
        }
    }
    if let Some((_, g)) = &layer {
        rep.remaining_area = g.count() as f64 * g.cell_area();
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vec2> {
        vec![v2(x0, y0), v2(x1, y0), v2(x1, y1), v2(x0, y1)]
    }

    #[test]
    fn stepover_to_engagement() {
        let e = AdaptiveParams::engagement_for_stepover(10.0, 1.0);
        assert!((e - (0.8f64).acos().to_degrees()).abs() < 1e-12);
        assert!((AdaptiveParams::engagement_for_stepover(10.0, 5.0) - 90.0).abs() < 1e-12);
    }

    #[test]
    fn bad_parameters_are_errors() {
        let r = Region::rectangle(v2(0.0, 0.0), v2(30.0, 20.0)).unwrap();
        let mut p = AdaptiveParams::new(6.0, 0.0, -3.0, 1000.0);
        p.max_engagement_deg = 180.0;
        assert!(adaptive_clear(&r, None, &p).is_err());
        let mut p = AdaptiveParams::new(6.0, 0.0, -3.0, 1000.0);
        p.step_down = 0.0;
        assert!(adaptive_clear(&r, None, &p).is_err());
        let p = AdaptiveParams::new(f64::NAN, 0.0, -3.0, 1000.0);
        assert!(adaptive_clear(&r, None, &p).is_err());
        let p = AdaptiveParams::new(6.0, 0.0, 1.0, 1000.0);
        assert!(adaptive_clear(&r, None, &p).is_err());
        // A tool wider than the pocket cannot enter.
        let small = Region::rectangle(v2(0.0, 0.0), v2(5.0, 5.0)).unwrap();
        let p = AdaptiveParams::new(6.0, 0.0, -3.0, 1000.0);
        assert!(adaptive_clear(&small, None, &p).is_err());
    }

    #[test]
    fn closed_pocket_with_islands_stays_under_limit() {
        // Every channel between islands and walls is at least 11 mm (1.4 tool diameters)
        // wide. One layer: layers repeat the same plan (checked in the open-stock test),
        // so replaying one is enough here.
        let region = Region::new(
            rect(0.0, 0.0, 54.0, 34.0),
            vec![rect(12.0, 11.0, 20.0, 23.0), crate::geom::circle_points(v2(37.0, 17.0), 5.0, 48)],
        )
        .unwrap();
        let mut p = AdaptiveParams::new(8.0, 0.0, -6.0, 1200.0);
        p.step_down = 6.0;
        p.max_engagement_deg = 45.0;
        // The coarsest raster allowed keeps the test quick; the verifier runs finer.
        p.resolution = Some(0.125);
        let res = adaptive_clear(&region, None, &p).unwrap();
        let rep = &res.report;
        assert_eq!(rep.layers, 1);
        assert_eq!(rep.helix_entries, 1, "{rep:?}");
        assert!(rep.passes > 3);
        // Most moves between passes stay down in cleared space.
        assert!(rep.links > 5 * rep.retracts, "{rep:?}");
        assert!(rep.planned_max_engagement_deg <= 45.0);
        let total = region.area();
        assert!(rep.remaining_reachable_area < 0.01 * total, "{rep:?}");

        let m = measure_engagement(
            &region,
            None,
            &res.toolpath,
            &EngagementCheck {
                tool_diameter: 8.0,
                limit_deg: 45.0,
                top: 0.0,
                cell: 0.0625,
                link_tolerance: 0.25,
            },
        )
        .unwrap();
        assert!(m.samples > 1000);
        // The check is not vacuous: the passes run close to the limit.
        assert!(m.max_engagement_deg > 30.0, "max {}", m.max_engagement_deg);
        assert!(
            m.over_limit.is_empty(),
            "max {} at {:?}, {} over",
            m.max_engagement_deg,
            m.max_at,
            m.over_limit.len()
        );
        assert!(m.slotting.is_empty());
        assert!(m.links_touching.is_empty(), "{:?}", &m.links_touching[..m.links_touching.len().min(5)]);
        // What is left is only what the tool cannot reach (corners) plus raster error.
        assert!(
            m.remaining_area < rep.unreachable_area + 0.02 * total,
            "{} vs {}",
            m.remaining_area,
            rep.unreachable_area
        );
        // The tool never goes nearer a wall than its radius.
        for seg in res.toolpath.segments(0.01) {
            if seg.kind != MoveKind::Rapid && seg.b.z < 0.0 {
                let q = v2(seg.b.x, seg.b.y);
                assert!(region.contains(q));
                assert!(region.boundary_distance(q) >= 4.0 - 1e-6, "{q:?}");
            }
        }
    }

    #[test]
    fn open_stock_is_entered_from_the_side() {
        // Stock 40 x 30; the region extends past it so the tool can start in air.
        let stock = vec![Region::rectangle(v2(0.0, 0.0), v2(40.0, 30.0)).unwrap()];
        let region = Region::rectangle(v2(-10.0, -10.0), v2(50.0, 40.0)).unwrap();
        let mut p = AdaptiveParams::new(10.0, 0.0, -5.0, 1500.0);
        p.max_engagement_deg = 30.0;
        p.chip_thinning_max = Some(2.0);
        p.resolution = Some(10.0 / 64.0);
        // Two layers: the plan is repeated at each depth and checked layer by layer below.
        p.step_down = 2.5;
        let res = adaptive_clear(&region, Some(&stock), &p).unwrap();
        assert_eq!(res.report.helix_entries, 0);
        assert_eq!(res.report.layers, 2);
        let depths: std::collections::BTreeSet<i64> = res
            .toolpath
            .moves()
            .filter(|m| m.kind == MoveKind::Cut)
            .map(|m| (m.to.z * 1000.0).round() as i64)
            .collect();
        assert_eq!(depths.into_iter().collect::<Vec<_>>(), vec![-5000, -2500]);
        let m = measure_engagement(
            &region,
            Some(&stock),
            &res.toolpath,
            &EngagementCheck {
                tool_diameter: 10.0,
                limit_deg: 30.0,
                top: 0.0,
                cell: 0.078125,
                link_tolerance: 0.3125,
            },
        )
        .unwrap();
        assert!(m.over_limit.is_empty(), "max {} at {:?}", m.max_engagement_deg, m.max_at);
        assert!(m.max_engagement_deg > 20.0, "max {}", m.max_engagement_deg);
        assert!(m.remaining_area < 1.0, "{}", m.remaining_area);
        // Chip thinning raised the feed on light cuts, never above the limit.
        let feeds: Vec<f64> =
            res.toolpath.moves().filter(|m| m.kind == MoveKind::Cut).filter_map(|m| m.feed).collect();
        assert!(feeds.iter().any(|f| *f > 1500.0));
        assert!(feeds.iter().all(|f| *f >= 1500.0 - 1e-9 && *f <= 3000.0 + 1e-9));
    }
}
