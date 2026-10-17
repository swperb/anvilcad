//! Jobs, setups and operation lists.
//!
//! A job holds the tool library, the material and one or more setups. A setup places
//! program zero on the part (and names the work offset that holds it on the machine),
//! describes the stock, and lists operations in order. Building a setup turns its
//! operations into one tool path with tool changes, spindle and coolant, in program
//! coordinates (part coordinates less the setup's origin).
//!
//! [`plan_operations`] proposes an operation list from the part itself, the way
//! feature-based CAM systems do: face the stock if it is taller than the part, clear each
//! recognised pocket adaptively with the largest end mill that fits and finish its walls,
//! and drill each hole with a matching drill. The rules are deliberately few and are
//! listed in `docs/cam/DESIGN.md`.

use crate::adaptive::{AdaptiveParams, AdaptiveReport, adaptive_clear};
use crate::drill::{DrillParams, drill_holes};
use crate::error::{CamError, Result};
use crate::facing::{FacingParams, facing};
use crate::features::{HoleFeature, Recognized, recognize};
use crate::geom::Region;
use crate::profile::{ProfileParams, Side, profile};
use crate::tool::{ToolLibrary, cutting_data};
use crate::toolpath::{Coolant, Item, SpindleDir, Toolpath};
use anvil_kernel::{Solid, Vec2, Vec3, v2, v3};
use anvil_model::material::Material;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    Facing(FacingParams),
    Adaptive { region: Region, stock: Option<Vec<Region>>, params: AdaptiveParams },
    Profile { region: Region, params: ProfileParams },
    Drill { holes: Vec<HoleFeature>, params: DrillParams },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationEntry {
    pub name: String,
    pub tool: u32,
    pub rpm: f64,
    pub coolant: Coolant,
    pub operation: Operation,
}

/// Stock as a block in part coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stock {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub name: String,
    /// 1 = G54 ... 6 = G59.
    pub work_offset: u32,
    /// Program zero, in part coordinates.
    pub origin: Vec3,
    pub stock: Stock,
    /// Height (program coordinates) for moves between operations.
    pub safe_z: f64,
    pub operations: Vec<OperationEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub tools: ToolLibrary,
    pub material: Material,
    pub max_rpm: f64,
    pub setups: Vec<Setup>,
}

/// What building a setup produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupOutput {
    pub toolpath: Toolpath,
    pub adaptive_reports: Vec<(String, AdaptiveReport)>,
    /// Tools in the order they are used.
    pub tool_sequence: Vec<u32>,
}

fn shift_region(r: &Region, d: Vec2) -> Result<Region> {
    Region::new(
        r.outer.iter().map(|p| *p - d).collect(),
        r.islands.iter().map(|l| l.iter().map(|p| *p - d).collect()).collect(),
    )
}

/// Builds the tool path for one setup. Operation geometry is in part coordinates.
pub fn build_setup(job: &Job, setup: &Setup) -> Result<SetupOutput> {
    let o = setup.origin;
    let d = v2(o.x, o.y);
    let mut out = SetupOutput::default();
    let mut tp = Toolpath::new();
    tp.start = Some(v3(0.0, 0.0, setup.safe_z));
    let mut current: Option<u32> = None;
    for op in &setup.operations {
        let tool = job.tools.get(op.tool)?;
        if current != Some(op.tool) {
            if current.is_some() {
                tp.push(Item::Coolant(Coolant::Off));
                tp.push(Item::SpindleStop);
            }
            tp.push(Item::ToolChange { tool: op.tool, name: tool.name.clone() });
            tp.push(Item::Spindle { rpm: op.rpm, dir: SpindleDir::Clockwise });
            if op.coolant != Coolant::Off {
                tp.push(Item::Coolant(op.coolant));
            }
            out.tool_sequence.push(op.tool);
            current = Some(op.tool);
        }
        tp.push(Item::Comment(op.name.clone()));
        let z = |v: f64| v - o.z;
        let part = match &op.operation {
            Operation::Facing(p) => {
                let mut q = p.clone();
                q.min = p.min - d;
                q.max = p.max - d;
                q.stock_top = z(p.stock_top);
                q.face_z = z(p.face_z);
                q.safe_z = setup.safe_z;
                q.tool_diameter = tool.diameter;
                facing(&q)?
            }
            Operation::Adaptive { region, stock, params } => {
                let region = shift_region(region, d)?;
                let stock: Option<Vec<Region>> = stock
                    .as_ref()
                    .map(|s| s.iter().map(|r| shift_region(r, d)).collect::<Result<Vec<_>>>())
                    .transpose()?;
                let mut q = params.clone();
                q.top = z(params.top);
                q.bottom = z(params.bottom);
                q.safe_z = setup.safe_z;
                q.tool_diameter = tool.diameter;
                let res = adaptive_clear(&region, stock.as_deref(), &q)?;
                out.adaptive_reports.push((op.name.clone(), res.report));
                res.toolpath
            }
            Operation::Profile { region, params } => {
                let region = shift_region(region, d)?;
                let mut q = params.clone();
                q.top = z(params.top);
                q.bottom = z(params.bottom);
                q.safe_z = setup.safe_z;
                q.tool_diameter = tool.diameter;
                profile(&region, &q)?
            }
            Operation::Drill { holes, params } => {
                let holes: Vec<HoleFeature> = holes
                    .iter()
                    .map(|h| HoleFeature {
                        center: h.center - d,
                        top: z(h.top),
                        bottom: z(h.bottom),
                        ..h.clone()
                    })
                    .collect();
                let mut q = params.clone();
                q.safe_z = setup.safe_z;
                let mut t = Toolpath::new();
                for c in drill_holes(&holes, tool, &q, z(setup.stock.min.z))? {
                    t.push(Item::Cycle(c));
                }
                t
            }
        };
        tp.append(part);
    }
    if current.is_some() {
        tp.push(Item::Coolant(Coolant::Off));
        tp.push(Item::SpindleStop);
    }
    out.toolpath = tp;
    Ok(out)
}

/// Proposes operations for `part` from `stock`, using `job`'s tools and material.
pub fn plan_operations(
    job: &Job,
    part: &Solid,
    stock: &Stock,
    safe_z: f64,
) -> Result<(Vec<OperationEntry>, Recognized)> {
    let rec = recognize(part, 0.0)?;
    let mut ops = Vec::new();
    let cut = |t: u32| -> Result<crate::tool::CuttingData> {
        cutting_data(job.tools.get(t)?, &job.material, job.max_rpm)
    };
    // The end mill for clearing: the largest that leaves room in every pocket.
    let narrowest = rec
        .pockets
        .iter()
        .filter(|p| !p.open)
        .map(|p| narrowest_gap(&p.region))
        .fold(f64::INFINITY, f64::min);
    let mill =
        job.tools.largest_end_mill(if narrowest.is_finite() { 0.6 * narrowest } else { f64::INFINITY });
    if stock.max.z > rec.top + 1e-6 {
        let Some(t) = job.tools.largest_end_mill(f64::INFINITY) else {
            return Err(CamError::NotFound("no end mill in the library for facing".into()));
        };
        let c = cut(t.number)?;
        let mut p = FacingParams::new(
            t.diameter,
            v2(stock.min.x, stock.min.y),
            v2(stock.max.x, stock.max.y),
            stock.max.z,
            rec.top,
            c.feed,
        );
        p.step_down = (0.5 * t.diameter).min(stock.max.z - rec.top).max(0.1);
        p.safe_z = safe_z;
        ops.push(OperationEntry {
            name: "face".into(),
            tool: t.number,
            rpm: c.rpm,
            coolant: Coolant::Flood,
            operation: Operation::Facing(p),
        });
    }
    for (k, pk) in rec.pockets.iter().enumerate() {
        if pk.open || !pk.vertical_walls {
            continue;
        }
        let Some(t) = mill else {
            return Err(CamError::NotFound("no end mill small enough for the pockets".into()));
        };
        let c = cut(t.number)?;
        let mut a = AdaptiveParams::new(t.diameter, pk.top, pk.floor, c.feed);
        a.step_down = (2.0 * t.diameter).min(t.flute_length * 0.9);
        a.stock_to_leave = 0.2;
        a.plunge_feed = c.plunge_feed;
        a.link_feed = c.feed;
        a.safe_z = safe_z;
        ops.push(OperationEntry {
            name: format!("pocket {} clearing", k + 1),
            tool: t.number,
            rpm: c.rpm,
            coolant: Coolant::Flood,
            operation: Operation::Adaptive { region: pk.region.clone(), stock: None, params: a },
        });
        let mut f = ProfileParams::new(t.diameter, Side::Inside, pk.top, pk.floor, c.feed * 0.8);
        f.step_down = t.flute_length * 0.9;
        f.safe_z = safe_z;
        ops.push(OperationEntry {
            name: format!("pocket {} walls", k + 1),
            tool: t.number,
            rpm: c.rpm,
            coolant: Coolant::Flood,
            operation: Operation::Profile { region: pk.region.clone(), params: f },
        });
    }
    // Drill holes, grouped by diameter.
    let mut sizes: Vec<f64> = rec.holes.iter().map(|h| h.diameter).collect();
    sizes.sort_by(f64::total_cmp);
    sizes.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    for dia in sizes {
        let holes: Vec<HoleFeature> =
            rec.holes.iter().filter(|h| (h.diameter - dia).abs() < 1e-6).cloned().collect();
        let Some(t) = job.tools.drill_for(dia, 0.05) else { continue };
        let c = cut(t.number)?;
        let mut p = DrillParams::new(c.feed, safe_z);
        p.r_clearance = 2.0;
        ops.push(OperationEntry {
            name: format!("drill {dia:.2} mm"),
            tool: t.number,
            rpm: c.rpm,
            coolant: Coolant::Flood,
            operation: Operation::Drill { holes, params: p },
        });
    }
    Ok((ops, rec))
}

/// Twice the largest distance from any point of the region to its boundary, estimated
/// on a grid: the widest tool that could enter.
fn narrowest_gap(region: &Region) -> f64 {
    let (lo, hi) = region.bounds();
    let step = ((hi - lo).x.max((hi - lo).y) / 200.0).max(1e-3);
    let mut best = 0.0f64;
    let mut y = lo.y;
    while y <= hi.y {
        let mut x = lo.x;
        while x <= hi.x {
            let p = v2(x, y);
            if region.contains(p) {
                best = best.max(region.boundary_distance(p));
            }
            x += step;
        }
        y += step;
    }
    // The tool must also pass between islands and walls: take the narrowest neck along
    // the medial line approximately as the smaller of the largest inscribed circle and
    // the smallest gap between an island and the outer wall.
    let gap = region
        .islands
        .iter()
        .map(|isl| {
            isl.iter()
                .map(|p| {
                    crate::geom::Region::new(region.outer.clone(), vec![])
                        .map_or(f64::INFINITY, |r| r.boundary_distance(*p))
                })
                .fold(f64::INFINITY, f64::min)
        })
        .fold(f64::INFINITY, f64::min);
    (2.0 * best).min(gap)
}
