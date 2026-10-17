//! Stock removal simulation and collision checks for 2.5D and 3-axis paths (tool along
//! +Z), on height fields.
//!
//! The stock and the finished part are each a grid of column heights: material from
//! below up to the height stored in the cell. The tool is swept along the path in steps
//! of at most half a cell, lowering the stock under its cutting envelope. At every step
//! it checks:
//!
//! - gouge: the cutting envelope below the finished part's surface;
//! - rapid in material: a rapid move whose tool would touch stock;
//! - link in material: a link feed that cuts (links are planned through cleared space);
//! - shank in material: stock above the flutes, within the shank;
//! - holder collision: the holder touching stock or part.
//!
//! Height fields cannot hold undercuts, which 2.5D and 3-axis machining from +Z cannot
//! make anyway. Their resolution limits what is seen: features narrower than a cell can
//! be missed, so the cell should be well under the tool radius.

use crate::error::{CamError, Result, positive};
use crate::geom::Region;
use crate::tool::Tool;
use crate::toolpath::{MoveKind, Toolpath};
use anvil_kernel::{Solid, Vec2, Vec3, v2};
use serde::{Deserialize, Serialize};

/// Below this, a cell is empty.
const EMPTY: f64 = f64::NEG_INFINITY;

#[derive(Clone, Debug, PartialEq)]
pub struct HeightField {
    pub origin: Vec2,
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub z: Vec<f64>,
}

impl HeightField {
    /// An empty field covering `lo`..`hi`.
    pub fn new(lo: Vec2, hi: Vec2, cell: f64) -> Result<HeightField> {
        positive("cell", cell)?;
        if !(hi.x > lo.x
            && hi.y > lo.y
            && lo.x.is_finite()
            && lo.y.is_finite()
            && hi.x.is_finite()
            && hi.y.is_finite())
        {
            return Err(CamError::InvalidGeometry("height field bounds are not valid".into()));
        }
        let nx = ((hi.x - lo.x) / cell).ceil() as usize + 1;
        let ny = ((hi.y - lo.y) / cell).ceil() as usize + 1;
        let n = nx.saturating_mul(ny);
        if n > crate::raster::MAX_CELLS {
            return Err(CamError::LimitExceeded(format!("the height field would need {n} cells")));
        }
        Ok(HeightField { origin: lo, cell, nx, ny, z: vec![EMPTY; n] })
    }

    /// A block of stock from `lo` to `hi` (corners) with its top at `top`.
    pub fn block(lo: Vec2, hi: Vec2, top: f64, cell: f64, margin: f64) -> Result<HeightField> {
        let mut f = HeightField::new(lo - v2(margin, margin), hi + v2(margin, margin), cell)?;
        for j in 0..f.ny {
            for i in 0..f.nx {
                let c = f.center(i, j);
                if c.x >= lo.x && c.x <= hi.x && c.y >= lo.y && c.y <= hi.y {
                    f.z[j * f.nx + i] = top;
                }
            }
        }
        Ok(f)
    }

    /// The same layout, every cell empty.
    pub fn empty_like(&self) -> HeightField {
        HeightField { z: vec![EMPTY; self.z.len()], ..self.clone() }
    }

    pub fn center(&self, i: usize, j: usize) -> Vec2 {
        self.origin + v2((i as f64 + 0.5) * self.cell, (j as f64 + 0.5) * self.cell)
    }

    pub fn at(&self, p: Vec2) -> f64 {
        let fx = (p.x - self.origin.x) / self.cell;
        let fy = (p.y - self.origin.y) / self.cell;
        if fx < 0.0 || fy < 0.0 {
            return EMPTY;
        }
        let (i, j) = (fx as usize, fy as usize);
        if i >= self.nx || j >= self.ny {
            return EMPTY;
        }
        self.z[j * self.nx + i]
    }

    /// The top surface of `body`: at each cell centre, the highest point of the body
    /// above it.
    pub fn from_solid(body: &Solid, like: &HeightField) -> HeightField {
        let mut f = like.empty_like();
        for p in &body.polygons {
            if p.normal.z.abs() < 1e-9 || p.verts.len() < 3 {
                continue;
            }
            let (mut lo, mut hi) =
                (v2(f64::INFINITY, f64::INFINITY), v2(f64::NEG_INFINITY, f64::NEG_INFINITY));
            for v in &p.verts {
                lo = v2(lo.x.min(v.x), lo.y.min(v.y));
                hi = v2(hi.x.max(v.x), hi.y.max(v.y));
            }
            let i0 = (((lo.x - f.origin.x) / f.cell) - 0.5).ceil().max(0.0) as usize;
            let j0 = (((lo.y - f.origin.y) / f.cell) - 0.5).ceil().max(0.0) as usize;
            let i1f = ((hi.x - f.origin.x) / f.cell) - 0.5;
            let j1f = ((hi.y - f.origin.y) / f.cell) - 0.5;
            if i1f < 0.0 || j1f < 0.0 {
                continue;
            }
            let i1 = (i1f.floor() as usize).min(f.nx - 1);
            let j1 = (j1f.floor() as usize).min(f.ny - 1);
            let pts: Vec<Vec2> = p.verts.iter().map(|v| v2(v.x, v.y)).collect();
            let up = p.normal.z > 0.0;
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let c = f.center(i, j);
                    // Convex polygon: inside every edge (orientation from the normal).
                    let k = pts.len();
                    let inside = (0..k).all(|e| {
                        let (a, b) = (pts[e], pts[(e + 1) % k]);
                        let cr = (b - a).cross(c - a);
                        if up { cr >= -1e-12 } else { cr <= 1e-12 }
                    });
                    if inside {
                        let z = (p.w - p.normal.x * c.x - p.normal.y * c.y) / p.normal.z;
                        let cell = &mut f.z[j * f.nx + i];
                        if z > *cell {
                            *cell = z;
                        }
                    }
                }
            }
        }
        f
    }

    /// Lowers the field to `z` inside `region` (for building stock with pockets).
    pub fn lower_region(&mut self, region: &Region, z: f64) {
        for j in 0..self.ny {
            for i in 0..self.nx {
                let c = self.center(i, j);
                if region.contains(c) {
                    let v = &mut self.z[j * self.nx + i];
                    *v = v.min(z);
                }
            }
        }
    }

    pub fn max(&self) -> f64 {
        self.z.iter().copied().fold(EMPTY, f64::max)
    }

    fn cells_within(&self, c: Vec2, r: f64) -> (usize, usize, usize, usize) {
        let i0 = (((c.x - r - self.origin.x) / self.cell) - 0.5).ceil().max(0.0) as usize;
        let j0 = (((c.y - r - self.origin.y) / self.cell) - 0.5).ceil().max(0.0) as usize;
        let i1 = ((((c.x + r - self.origin.x) / self.cell) - 0.5).floor().max(-1.0) + 1.0) as usize;
        let j1 = ((((c.y + r - self.origin.y) / self.cell) - 0.5).floor().max(-1.0) + 1.0) as usize;
        (i0, j0, i1.min(self.nx), j1.min(self.ny))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SimFindingKind {
    Gouge,
    RapidInMaterial,
    LinkInMaterial,
    ShankInMaterial,
    HolderHitsStock,
    HolderHitsPart,
}

impl SimFindingKind {
    pub fn label(self) -> &'static str {
        match self {
            SimFindingKind::Gouge => "gouge into the part",
            SimFindingKind::RapidInMaterial => "rapid move through material",
            SimFindingKind::LinkInMaterial => "link move cutting material",
            SimFindingKind::ShankInMaterial => "material above the flutes (shank rubbing)",
            SimFindingKind::HolderHitsStock => "holder collides with stock",
            SimFindingKind::HolderHitsPart => "holder collides with the part",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimFinding {
    pub kind: SimFindingKind,
    /// Tool tip position.
    pub at: Vec3,
    /// Index of the move in the expanded tool path.
    pub item: usize,
    /// How far into the material (mm).
    pub depth: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimReport {
    /// The worst finding of each kind on each move, at most [`MAX_FINDINGS`].
    pub findings: Vec<SimFinding>,
    pub removed_volume: f64,
    /// Largest height of stock left above the part, where the part is known.
    pub max_stock_left: f64,
}

impl SimReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn count(&self, kind: SimFindingKind) -> usize {
        self.findings.iter().filter(|f| f.kind == kind).count()
    }
}

/// Most findings kept.
pub const MAX_FINDINGS: usize = 1000;

/// Runs `toolpath` (expanded) with `tool` through `stock`, which is cut as it goes.
/// `part`, if given, is the finished part's top surface for gouge and holder checks.
/// Depths less than `tol` are ignored.
pub fn simulate(
    toolpath: &Toolpath,
    tool: &Tool,
    stock: &mut HeightField,
    part: Option<&HeightField>,
    tol: f64,
) -> Result<SimReport> {
    tool.validate()?;
    crate::error::non_negative("tolerance", tol)?;
    if let Some(p) = part
        && (p.nx != stock.nx || p.ny != stock.ny || p.cell != stock.cell || p.origin != stock.origin)
    {
        return Err(CamError::InvalidGeometry("the part and stock height fields differ in layout".into()));
    }
    let r = tool.radius();
    let holder_r = tool.holder.max_radius().max(tool.shank_diameter * 0.5);
    let mut rep = SimReport::default();
    let mut worst: std::collections::HashMap<(usize, SimFindingKind), usize> =
        std::collections::HashMap::new();
    let part_max = part.map_or(EMPTY, |p| p.max());
    // Cutting only lowers the stock, so its starting maximum bounds it throughout.
    let stock_max = stock.max();
    let mut record =
        |rep: &mut SimReport, kind: SimFindingKind, at: Vec3, item: usize, depth: f64| match worst
            .get(&(item, kind))
        {
            Some(&k) => {
                if depth > rep.findings[k].depth {
                    rep.findings[k].depth = depth;
                    rep.findings[k].at = at;
                }
            }
            None => {
                if rep.findings.len() < MAX_FINDINGS {
                    worst.insert((item, kind), rep.findings.len());
                    rep.findings.push(SimFinding { kind, at, item, depth });
                }
            }
        };
    let area = stock.cell * stock.cell;
    for seg in toolpath.segments(0.25 * stock.cell) {
        let len = seg.a.distance(seg.b);
        let n = ((len / (0.5 * stock.cell)).ceil() as usize).clamp(1, 10_000_000);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let tip = seg.a.lerp(seg.b, t);
            if !tip.x.is_finite() || !tip.y.is_finite() || !tip.z.is_finite() {
                return Err(CamError::InvalidGeometry(
                    "the tool path has a coordinate that is not finite".into(),
                ));
            }
            let c = v2(tip.x, tip.y);
            // Cutting envelope.
            if tip.z < stock_max.max(part_max) + tol {
                let (i0, j0, i1, j1) = stock.cells_within(c, r);
                for j in j0..j1 {
                    for i in i0..i1 {
                        let cc = stock.center(i, j);
                        let d = cc.distance(c);
                        let Some(h) = tool.profile_height(d) else { continue };
                        let bottom = tip.z + h;
                        let idx = j * stock.nx + i;
                        if let Some(p) = part {
                            let pz = p.z[idx];
                            if pz > bottom + tol {
                                record(&mut rep, SimFindingKind::Gouge, tip, seg.item, pz - bottom);
                            }
                        }
                        let sz = stock.z[idx];
                        if sz > bottom + tol {
                            match seg.kind {
                                MoveKind::Rapid => record(
                                    &mut rep,
                                    SimFindingKind::RapidInMaterial,
                                    tip,
                                    seg.item,
                                    sz - bottom,
                                ),
                                MoveKind::Link => record(
                                    &mut rep,
                                    SimFindingKind::LinkInMaterial,
                                    tip,
                                    seg.item,
                                    sz - bottom,
                                ),
                                _ => {}
                            }
                            if sz > tip.z + tool.flute_length + tol {
                                record(
                                    &mut rep,
                                    SimFindingKind::ShankInMaterial,
                                    tip,
                                    seg.item,
                                    sz - tip.z - tool.flute_length,
                                );
                            }
                            rep.removed_volume += (sz - bottom) * area;
                            stock.z[idx] = bottom;
                        }
                    }
                }
            }
            // Shank (above the flutes) and holder.
            let shank_bottom = tip.z + tool.flute_length;
            let holder_bottom = tip.z + tool.stickout;
            if shank_bottom < stock_max.max(part_max) + tol {
                let (i0, j0, i1, j1) = stock.cells_within(c, holder_r);
                for j in j0..j1 {
                    for i in i0..i1 {
                        let idx = j * stock.nx + i;
                        let d = stock.center(i, j).distance(c);
                        // Radius of the tool assembly at the height of this cell's material.
                        for (field, is_part) in [(Some(&*stock), false), (part, true)] {
                            let Some(f) = field else { continue };
                            let mz = f.z[idx];
                            if mz <= shank_bottom + tol {
                                continue;
                            }
                            // Shank between flutes and holder.
                            if d <= tool.shank_diameter * 0.5 && mz > shank_bottom + tol && d > r && !is_part
                            {
                                record(
                                    &mut rep,
                                    SimFindingKind::ShankInMaterial,
                                    tip,
                                    seg.item,
                                    mz - shank_bottom,
                                );
                            }
                            if mz > holder_bottom + tol {
                                if let Some(hr) = tool.holder.radius_at(mz - holder_bottom) {
                                    if d <= hr {
                                        let kind = if is_part {
                                            SimFindingKind::HolderHitsPart
                                        } else {
                                            SimFindingKind::HolderHitsStock
                                        };
                                        record(&mut rep, kind, tip, seg.item, mz - holder_bottom);
                                    }
                                } else if d <= tool.holder.sections.last().map_or(0.0, |s| s.0 * 0.5) {
                                    // Above the modelled holder: the spindle nose.
                                    let kind = if is_part {
                                        SimFindingKind::HolderHitsPart
                                    } else {
                                        SimFindingKind::HolderHitsStock
                                    };
                                    record(&mut rep, kind, tip, seg.item, mz - holder_bottom);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(p) = part {
        rep.max_stock_left = stock
            .z
            .iter()
            .zip(&p.z)
            .filter(|(s, q)| s.is_finite() && q.is_finite())
            .map(|(s, q)| s - q)
            .fold(0.0, f64::max);
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolpath::Toolpath;
    use anvil_kernel::v3;

    #[test]
    fn straight_slot_removes_the_right_volume() {
        let mut stock = HeightField::block(v2(0.0, 0.0), v2(50.0, 20.0), 10.0, 0.1, 10.0).unwrap();
        let tool = Tool::flat_end_mill(1, 6.0, 3);
        let mut tp = Toolpath::new();
        tp.start = Some(v3(-10.0, 10.0, 20.0));
        tp.rapid(v3(-10.0, 10.0, 8.0));
        tp.line(v3(60.0, 10.0, 8.0), MoveKind::Cut, 500.0);
        tp.rapid(v3(60.0, 10.0, 20.0));
        let rep = simulate(&tp, &tool, &mut stock, None, 1e-3).unwrap();
        assert!(rep.is_clean(), "{:?}", rep.findings);
        // 50 long, 6 wide, 2 deep.
        assert!((rep.removed_volume - 600.0).abs() < 600.0 * 0.03, "{}", rep.removed_volume);
        assert_eq!(stock.at(v2(25.0, 10.0)), 8.0);
        assert_eq!(stock.at(v2(25.0, 14.0)), 10.0);
    }

    #[test]
    fn rapid_through_stock_and_gouge_are_found() {
        let mut stock = HeightField::block(v2(0.0, 0.0), v2(50.0, 20.0), 10.0, 0.2, 10.0).unwrap();
        let mut part = stock.empty_like();
        for z in part.z.iter_mut() {
            *z = 5.0;
        }
        let tool = Tool::flat_end_mill(1, 6.0, 3);
        let mut tp = Toolpath::new();
        tp.start = Some(v3(-10.0, 10.0, 20.0));
        tp.rapid(v3(-10.0, 10.0, 8.0));
        tp.rapid(v3(25.0, 10.0, 8.0));
        tp.line(v3(25.0, 10.0, 4.0), MoveKind::Entry, 100.0);
        let rep = simulate(&tp, &tool, &mut stock, Some(&part), 1e-3).unwrap();
        assert_eq!(rep.count(SimFindingKind::RapidInMaterial), 1);
        let g = rep.findings.iter().find(|f| f.kind == SimFindingKind::Gouge).unwrap();
        assert!((g.depth - 1.0).abs() < 1e-9);
        assert!((g.at.z - 4.0).abs() < 1e-9);
    }

    #[test]
    fn holder_and_shank_collisions_are_found() {
        let mut stock = HeightField::block(v2(0.0, 0.0), v2(50.0, 50.0), 50.0, 0.5, 15.0).unwrap();
        // A 6 mm tool (18 mm of flute, 24 mm out of the holder) plunged 30 mm: the hole is
        // the tool's own size, so only the holder touches.
        let tool = Tool::flat_end_mill(1, 6.0, 3);
        let mut tp = Toolpath::new();
        tp.start = Some(v3(25.0, 25.0, 60.0));
        tp.line(v3(25.0, 25.0, 20.0), MoveKind::Entry, 100.0);
        let rep = simulate(&tp, &tool, &mut stock, None, 1e-3).unwrap();
        assert_eq!(rep.count(SimFindingKind::HolderHitsStock), 1, "{:?}", rep.findings);
        assert_eq!(rep.count(SimFindingKind::ShankInMaterial), 0);
        // Cutting 30 mm deep from the side puts the shank into the wall above the flutes.
        let mut stock = HeightField::block(v2(0.0, 0.0), v2(50.0, 50.0), 50.0, 0.5, 15.0).unwrap();
        let mut tp = Toolpath::new();
        tp.start = Some(v3(-10.0, 25.0, 20.0));
        tp.line(v3(10.0, 25.0, 20.0), MoveKind::Cut, 100.0);
        let rep = simulate(&tp, &tool, &mut stock, None, 1e-3).unwrap();
        let shank = rep.findings.iter().find(|f| f.kind == SimFindingKind::ShankInMaterial).unwrap();
        assert!((shank.depth - 12.0).abs() < 1e-9);
        assert_eq!(rep.count(SimFindingKind::HolderHitsStock), 1);
    }

    #[test]
    fn part_surface_from_solid() {
        let body = anvil_kernel::build::block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 5.0), 1);
        let like = HeightField::new(v2(-2.0, -2.0), v2(12.0, 12.0), 0.5).unwrap();
        let f = HeightField::from_solid(&body, &like);
        assert_eq!(f.at(v2(5.0, 5.0)), 5.0);
        assert_eq!(f.at(v2(11.0, 5.0)), EMPTY);
    }
}
