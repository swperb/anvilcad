//! Part Check: what a part needs to be machined, read from its STEP file.
//!
//! [`check_step`] takes a STEP file, a material and a quantity and returns a
//! [`PartCheckResult`]: a fingerprint of the file, the bounding box and stock, the
//! recognised machining features, design-for-manufacturing flags, and an estimated
//! machine time per operation type. The result is the contract with Kerfa (the shop
//! side), so its shape is versioned: [`SCHEMA_ID`] and [`SCHEMA_VERSION`] are written into
//! every result, and `schema/part-check.v1.schema.json` is generated from these types and
//! checked by a test so it cannot drift.
//!
//! Versioning: a new optional field is a minor version (consumers must ignore fields they
//! do not know); renaming, removing or changing the meaning of a field is a new major
//! version with its own schema file.
//!
//! The machine time is an estimate from a planned 2.5D operation list with a stock tool
//! library, not from the shop's own machines and tools, so it is always marked
//! [`TimeStatus::Unvalidated`] until a benchmark on synthetic parts shows it is close
//! enough to quote from. Consumers show it as a suggestion.
//!
//! Privacy: the check runs locally on the bytes it is given. It makes no network calls,
//! writes nothing to disk and logs nothing; the result holds a hash of the file and
//! derived numbers, never the geometry itself.

use anvil_cam::features::Recognized;
use anvil_cam::job::{Job, Operation, Setup, Stock, build_setup, plan_operations};
use anvil_cam::tool::{Tool, ToolLibrary};
use anvil_kernel::{Solid, v3};
use anvil_model::dfm::{self, Process, Severity};
use anvil_model::material::Material;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Names the document type; consumers check it before reading anything else.
pub const SCHEMA_ID: &str = "anvilcad.part-check";
/// Semantic version of the result's shape.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Rapid traverse rate assumed for the time estimate, mm/min.
pub const RAPID_MM_PER_MIN: f64 = 10_000.0;
/// Time assumed for one automatic tool change, minutes.
pub const TOOL_CHANGE_MIN: f64 = 0.1;
/// Stock added around the part on each side in X and Y, mm.
pub const STOCK_MARGIN_XY: f64 = 3.0;
/// Stock added above the part, faced off, mm.
pub const STOCK_MARGIN_TOP: f64 = 2.0;
/// Spindle speed limit assumed for cutting data, rpm.
pub const MAX_RPM: f64 = 12_000.0;
/// Flat end mill diameters in the stock tool library, mm.
pub const END_MILLS: [f64; 9] = [20.0, 16.0, 12.0, 10.0, 8.0, 6.0, 4.0, 3.0, 2.0];

#[derive(Debug, thiserror::Error)]
pub enum PartCheckError {
    #[error("the file is not valid STEP text: {0}")]
    Step(String),
    #[error("the STEP file holds no solid bodies")]
    NoBodies,
    #[error("quantity must be at least 1")]
    Quantity,
}

/// What to check.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Material name or part of one, matched case-insensitively against the material
    /// library ("6061" finds "6061-T6 Aluminum").
    pub material: String,
    pub quantity: u32,
    /// Shown back in the result; never read from disk.
    pub file_name: Option<String>,
}

/// The versioned Part Check result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "AnvilCAD Part Check result", extend("$id" = "https://anvilcad.dev/schema/part-check.v1.schema.json"))]
pub struct PartCheckResult {
    /// Always "anvilcad.part-check".
    pub schema: String,
    /// Semantic version of this document's shape, e.g. "1.0.0".
    pub schema_version: String,
    pub engine: Engine,
    pub part: PartInfo,
    pub request: RequestEcho,
    /// Lengths are millimetres throughout.
    pub bounding_box: BoundingBox,
    pub stock: StockInfo,
    pub volume_mm3: f64,
    /// Present when the material was found in the library.
    pub mass_g: Option<f64>,
    pub features: Vec<Feature>,
    pub dfm_flags: Vec<DfmFlag>,
    /// Absent when no estimate could be made (unknown material, or no operations
    /// could be planned); `warnings` says why.
    pub machine_time: Option<MachineTime>,
    /// Things the estimator should know that are not DFM findings.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Engine {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartInfo {
    /// Identifies the exact file, so a result can be matched to a quote line.
    pub fingerprint: Fingerprint,
    pub file_name: Option<String>,
    /// The unit the STEP file was written in; values here are converted to mm.
    pub source_length_unit: String,
    pub bodies: u32,
    /// Bodies imported as exact B-rep (the others were read as meshes).
    pub exact_bodies: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fingerprint {
    /// Always "sha256".
    pub algorithm: String,
    /// Lower-case hex digest of the file's bytes.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RequestEcho {
    pub material: MaterialEcho,
    pub quantity: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MaterialEcho {
    /// What was asked for.
    pub requested: String,
    /// The library material used, or null when none matched.
    pub matched: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub size: [f64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StockInfo {
    /// Always "block" in v1.
    pub kind: String,
    pub size: [f64; 3],
    pub margin_xy_mm: f64,
    pub margin_top_mm: f64,
}

/// A group of identical recognised features.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Feature {
    /// Round holes along the tool axis (+Z), grouped by diameter, depth and through.
    Hole { count: u32, diameter_mm: f64, depth_mm: f64, through: bool },
    /// A closed pocket or an open floor (step).
    Pocket {
        count: u32,
        depth_mm: f64,
        /// Area the tool clears, mm^2.
        area_mm2: f64,
        /// True for an open floor reachable from the side.
        open: bool,
        /// False when the walls are not vertical, so a 2.5D path cannot follow them.
        vertical_walls: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlagSeverity {
    /// Cannot be made as drawn, or will likely fail.
    Error,
    /// Can be made, at extra cost or risk.
    Warning,
    /// Worth knowing.
    Note,
}

/// A design-for-manufacturing finding. Consumers show it to the estimator and never
/// change a price from it on their own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DfmFlag {
    pub severity: FlagSeverity,
    /// Short, stable rule name, e.g. "Thin wall".
    pub rule: String,
    pub message: String,
    /// How many places on the part this applies to.
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimeStatus {
    /// Not yet benchmarked: show as a suggestion, do not quote from it automatically.
    Unvalidated,
    /// Benchmarked within the agreed accuracy bar.
    Validated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Facing,
    PocketClearing,
    WallFinishing,
    Drilling,
    ToolChanges,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MachineTime {
    pub status: TimeStatus,
    /// Setups (part orientations) planned; v1 plans one, from +Z.
    pub setups: u32,
    pub operations: Vec<OperationTime>,
    /// Cycle time for one part, minutes.
    pub per_part_min: f64,
    /// Cycle time for the requested quantity, minutes (no setup or load time).
    pub total_min: f64,
    pub assumptions: TimeAssumptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OperationTime {
    #[serde(rename = "type")]
    pub kind: OperationType,
    /// Operations of this type in the plan.
    pub count: u32,
    /// Minutes per part.
    pub minutes: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimeAssumptions {
    pub rapid_mm_per_min: f64,
    pub tool_change_min: f64,
    pub max_rpm: f64,
    /// Always "stock" in v1: a generic flat end mill set plus a drill for every hole.
    pub tool_library: String,
    /// What the estimate leaves out.
    pub excludes: Vec<String>,
}

/// Runs Part Check on a STEP file's bytes.
pub fn check_step(bytes: &[u8], req: &Request) -> Result<PartCheckResult, PartCheckError> {
    if req.quantity == 0 {
        return Err(PartCheckError::Quantity);
    }
    let text = std::str::from_utf8(bytes).map_err(|e| PartCheckError::Step(e.to_string()))?;
    let name = req.file_name.clone().unwrap_or_else(|| "part.step".into());
    let (mut doc, report) =
        anvil_model::step::document_from_step(text, &name).map_err(PartCheckError::Step)?;
    if report.bodies == 0 {
        return Err(PartCheckError::NoBodies);
    }
    let st = doc.rebuild();
    let mut warnings: Vec<String> = st.errors.values().map(|e| format!("rebuild: {e}")).collect();
    warnings.extend(report.meshed.iter().map(|m| format!("read as a mesh: {m}")));
    if report.bodies > 1 {
        warnings.push(format!("the file holds {} bodies; they are checked as one part", report.bodies));
    }
    let part = PartInfo {
        fingerprint: Fingerprint { algorithm: "sha256".into(), value: sha256_hex(bytes) },
        file_name: req.file_name.clone(),
        source_length_unit: report.length_unit.clone(),
        bodies: count(report.bodies),
        exact_bodies: count(report.exact),
    };
    Ok(check_solid(&st.body, part, req, warnings))
}

/// Runs Part Check on a body that is already built. `part` describes where it came from.
pub fn check_solid(
    body: &Solid,
    part: PartInfo,
    req: &Request,
    mut warnings: Vec<String>,
) -> PartCheckResult {
    let material = find_material(&req.material);
    if material.is_none() {
        warnings.push(format!("no library material matches \"{}\"", req.material));
    }
    let props = body.mass_properties();
    let rec = match anvil_cam::features::recognize(body, 0.0) {
        Ok(r) => Some(r),
        Err(e) => {
            warnings.push(format!("feature recognition failed: {e}"));
            None
        }
    };
    let (min, max) = match &rec {
        Some(r) => ([r.min.x, r.min.y, r.min.z], [r.max.x, r.max.y, r.max.z]),
        None => bounds(body),
    };
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let stock = Stock {
        min: v3(min[0] - STOCK_MARGIN_XY, min[1] - STOCK_MARGIN_XY, min[2]),
        max: v3(max[0] + STOCK_MARGIN_XY, max[1] + STOCK_MARGIN_XY, max[2] + STOCK_MARGIN_TOP),
    };
    rec.iter().flat_map(|r| r.notes.iter()).for_each(|n| warnings.push(n.clone()));

    let dfm = dfm::check(body, material.as_ref(), Process::CncMachining);
    let dfm_flags = dfm
        .findings
        .iter()
        .map(|f| DfmFlag {
            severity: match f.severity {
                Severity::Error => FlagSeverity::Error,
                Severity::Warning => FlagSeverity::Warning,
                Severity::Note => FlagSeverity::Note,
            },
            rule: f.rule.to_string(),
            message: f.message.clone(),
            count: count(f.count),
        })
        .collect();

    let machine_time = match (&material, &rec) {
        (Some(m), Some(r)) => match estimate(body, r, m, &stock, req.quantity) {
            Ok(t) => Some(t),
            Err(e) => {
                warnings.push(format!("no machine time estimate: {e}"));
                None
            }
        },
        _ => None,
    };

    PartCheckResult {
        schema: SCHEMA_ID.into(),
        schema_version: SCHEMA_VERSION.into(),
        engine: Engine { name: "anvilcad-partcheck".into(), version: env!("CARGO_PKG_VERSION").into() },
        part,
        request: RequestEcho {
            material: MaterialEcho {
                requested: req.material.clone(),
                matched: material.as_ref().map(|m| m.name.clone()),
            },
            quantity: req.quantity,
        },
        bounding_box: BoundingBox { min: r3(min), max: r3(max), size: r3(size) },
        stock: StockInfo {
            kind: "block".into(),
            size: r3([
                size[0] + 2.0 * STOCK_MARGIN_XY,
                size[1] + 2.0 * STOCK_MARGIN_XY,
                size[2] + STOCK_MARGIN_TOP,
            ]),
            margin_xy_mm: STOCK_MARGIN_XY,
            margin_top_mm: STOCK_MARGIN_TOP,
        },
        volume_mm3: round(props.volume),
        // density g/cm^3, volume mm^3: 1 cm^3 = 1000 mm^3.
        mass_g: material.as_ref().map(|m| round(m.density * props.volume / 1000.0)),
        features: rec.as_ref().map(features).unwrap_or_default(),
        dfm_flags,
        machine_time,
        warnings,
    }
}

/// The JSON Schema for [`PartCheckResult`], as committed in `schema/`.
pub fn json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(PartCheckResult)).unwrap_or_default()
}

fn find_material(query: &str) -> Option<Material> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let lib = Material::library();
    lib.iter()
        .find(|m| m.name.to_lowercase() == q)
        .or_else(|| lib.iter().find(|m| m.name.to_lowercase().contains(&q)))
        .cloned()
}

fn features(rec: &Recognized) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::new();
    for h in &rec.holes {
        let (d, depth) = (round(h.diameter), round(h.depth()));
        match out.iter_mut().find(|f| {
            matches!(f, Feature::Hole { diameter_mm, depth_mm, through, .. }
                if *diameter_mm == d && *depth_mm == depth && *through == h.through)
        }) {
            Some(Feature::Hole { count, .. }) => *count += 1,
            _ => out.push(Feature::Hole { count: 1, diameter_mm: d, depth_mm: depth, through: h.through }),
        }
    }
    for p in &rec.pockets {
        let (depth, area) = (round(p.top - p.floor), round(p.region.area()));
        match out.iter_mut().find(|f| {
            matches!(f, Feature::Pocket { depth_mm, area_mm2, open, vertical_walls, .. }
                if *depth_mm == depth && *area_mm2 == area && *open == p.open && *vertical_walls == p.vertical_walls)
        }) {
            Some(Feature::Pocket { count, .. }) => *count += 1,
            _ => out.push(Feature::Pocket {
                count: 1,
                depth_mm: depth,
                area_mm2: area,
                open: p.open,
                vertical_walls: p.vertical_walls,
            }),
        }
    }
    out
}

/// The stock tool library: flat end mills, plus a drill for every recognised hole size.
fn tool_library(rec: &Recognized) -> Result<ToolLibrary, anvil_cam::error::CamError> {
    let mut tools = ToolLibrary::default();
    for (i, d) in END_MILLS.iter().enumerate() {
        tools.add(Tool::flat_end_mill(i as u32 + 1, *d, 3))?;
    }
    let mut sizes: Vec<f64> = rec.holes.iter().map(|h| h.diameter).collect();
    sizes.sort_by(f64::total_cmp);
    sizes.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    for (i, d) in sizes.iter().enumerate() {
        tools.add(Tool::drill(100 + i as u32, *d))?;
    }
    Ok(tools)
}

fn estimate(
    body: &Solid,
    rec: &Recognized,
    material: &Material,
    stock: &Stock,
    quantity: u32,
) -> Result<MachineTime, anvil_cam::error::CamError> {
    let job =
        Job { tools: tool_library(rec)?, material: material.clone(), max_rpm: MAX_RPM, setups: Vec::new() };
    let safe_z = stock.max.z + 10.0;
    let (ops, _) = plan_operations(&job, body, stock, safe_z)?;
    let mut by_kind: Vec<OperationTime> = Vec::new();
    let mut add = |kind: OperationType, minutes: f64| match by_kind.iter_mut().find(|o| o.kind == kind) {
        Some(o) => {
            o.count += 1;
            o.minutes += minutes;
        }
        None => by_kind.push(OperationTime { kind, count: 1, minutes }),
    };
    let mut changes = 0u32;
    let mut last_tool = None;
    for op in &ops {
        let setup = Setup {
            name: op.name.clone(),
            work_offset: 1,
            origin: v3(0.0, 0.0, 0.0),
            stock: *stock,
            safe_z,
            operations: vec![op.clone()],
        };
        let s = build_setup(&job, &setup)?.toolpath.stats();
        let minutes = s.feed_minutes + s.rapid_length / RAPID_MM_PER_MIN;
        let kind = match op.operation {
            Operation::Facing(_) => OperationType::Facing,
            Operation::Adaptive { .. } => OperationType::PocketClearing,
            Operation::Profile { .. } => OperationType::WallFinishing,
            Operation::Drill { .. } => OperationType::Drilling,
        };
        add(kind, minutes);
        if last_tool != Some(op.tool) {
            changes += 1;
            last_tool = Some(op.tool);
        }
    }
    if changes > 0 {
        by_kind.push(OperationTime {
            kind: OperationType::ToolChanges,
            count: changes,
            minutes: f64::from(changes) * TOOL_CHANGE_MIN,
        });
    }
    let per_part: f64 = by_kind.iter().map(|o| o.minutes).sum();
    for o in &mut by_kind {
        o.minutes = round(o.minutes);
    }
    Ok(MachineTime {
        status: TimeStatus::Unvalidated,
        setups: 1,
        operations: by_kind,
        per_part_min: round(per_part),
        total_min: round(per_part * f64::from(quantity)),
        assumptions: TimeAssumptions {
            rapid_mm_per_min: RAPID_MM_PER_MIN,
            tool_change_min: TOOL_CHANGE_MIN,
            max_rpm: MAX_RPM,
            tool_library: "stock".into(),
            excludes: vec![
                "machine setup and fixturing".into(),
                "part loading and unloading".into(),
                "acceleration and deceleration".into(),
                "features not reachable from +Z".into(),
                "deburring, inspection and secondary operations".into(),
            ],
        },
    })
}

fn bounds(body: &Solid) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &body.polygons {
        for v in &p.verts {
            for (i, c) in [v.x, v.y, v.z].into_iter().enumerate() {
                lo[i] = lo[i].min(c);
                hi[i] = hi[i].max(c);
            }
        }
    }
    if lo[0].is_finite() { (lo, hi) } else { ([0.0; 3], [0.0; 3]) }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Rounds to 0.001 so results are stable across platforms and easy to compare.
fn round(v: f64) -> f64 {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

fn r3(v: [f64; 3]) -> [f64; 3] {
    v.map(round)
}
