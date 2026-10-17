//! Facing: flat parallel passes across the top of the stock down to a finished face.

use crate::error::{CamError, Result, finite, positive};
use crate::toolpath::{Item, MoveKind, Toolpath};
use anvil_kernel::{Vec2, v3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FacingParams {
    pub tool_diameter: f64,
    /// Stock outline to face (min and max corners, XY).
    pub min: Vec2,
    pub max: Vec2,
    /// Top of the stock before facing.
    pub stock_top: f64,
    /// Finished face height.
    pub face_z: f64,
    pub step_down: f64,
    /// Distance between passes as a fraction of the tool diameter (0 to 1].
    pub stepover: f64,
    /// How far past the stock the tool travels before turning, mm.
    pub overrun: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub safe_z: f64,
    /// Alternate direction on each pass (zig-zag) instead of returning at safe height.
    pub zigzag: bool,
}

impl FacingParams {
    pub fn new(
        tool_diameter: f64,
        min: Vec2,
        max: Vec2,
        stock_top: f64,
        face_z: f64,
        feed: f64,
    ) -> FacingParams {
        FacingParams {
            tool_diameter,
            min,
            max,
            stock_top,
            face_z,
            step_down: (stock_top - face_z).max(0.1),
            stepover: 0.7,
            overrun: 2.0,
            feed,
            plunge_feed: feed * 0.3,
            safe_z: stock_top + 5.0,
            zigzag: true,
        }
    }
}

/// Faces the stock rectangle with passes along X.
pub fn facing(p: &FacingParams) -> Result<Toolpath> {
    let d = positive("tool diameter", p.tool_diameter)?;
    for v in [p.min.x, p.min.y, p.max.x, p.max.y] {
        finite("stock corner", v)?;
    }
    if p.max.x <= p.min.x || p.max.y <= p.min.y {
        return Err(CamError::InvalidGeometry("the stock rectangle is empty".into()));
    }
    finite("stock top", p.stock_top)?;
    finite("face Z", p.face_z)?;
    if p.face_z > p.stock_top {
        return Err(CamError::InvalidParameter {
            name: "face Z",
            reason: "must not be above the stock top".into(),
        });
    }
    positive("step-down", p.step_down)?;
    let so = positive("stepover", p.stepover)?;
    if so > 1.0 {
        return Err(CamError::InvalidParameter {
            name: "stepover",
            reason: "must be at most 1 (one diameter)".into(),
        });
    }
    positive("feed", p.feed)?;
    positive("plunge feed", p.plunge_feed)?;
    let overrun = crate::error::non_negative("overrun", p.overrun)?;
    if p.safe_z <= p.stock_top {
        return Err(CamError::InvalidParameter {
            name: "safe Z",
            reason: "must be above the stock top".into(),
        });
    }
    let r = d * 0.5;
    let ae = so * d;
    // The tool overhangs each side edge by a twentieth of its diameter.
    let lap = 0.05 * d;
    let y0 = p.min.y + r - lap;
    let y1 = p.max.y - r + lap;
    let ys: Vec<f64> = if y1 <= y0 {
        vec![0.5 * (p.min.y + p.max.y)]
    } else {
        let n = ((y1 - y0) / ae).ceil().max(1.0) as usize;
        if n > 100_000 {
            return Err(CamError::LimitExceeded("too many facing passes".into()));
        }
        (0..=n).map(|k| y0 + (y1 - y0) * k as f64 / n as f64).collect()
    };
    // Start and end clear of the stock so every pass enters from air.
    let xa = p.min.x - r - overrun;
    let xb = p.max.x + r + overrun;
    let depth = p.stock_top - p.face_z;
    let layers = if depth <= 1e-9 { 1 } else { ((depth / p.step_down) - 1e-9).ceil().max(1.0) as usize };
    let mut tp = Toolpath::new();
    tp.push(Item::Comment(format!("facing to Z{:.3}", p.face_z)));
    for k in 1..=layers {
        let z = p.stock_top - depth * k as f64 / layers as f64;
        tp.rapid(v3(xa, ys[0], p.safe_z));
        tp.rapid(v3(xa, ys[0], p.stock_top + 1.0));
        tp.line(v3(xa, ys[0], z), MoveKind::Entry, p.plunge_feed);
        let mut at_a = true;
        for (i, &y) in ys.iter().enumerate() {
            if i > 0 {
                if p.zigzag {
                    // Step over beside the stock, in air.
                    let x = if at_a { xa } else { xb };
                    tp.line(v3(x, y, z), MoveKind::Link, p.feed);
                } else {
                    tp.rapid(v3(xb, ys[i - 1], p.safe_z));
                    tp.rapid(v3(xa, y, p.safe_z));
                    tp.rapid(v3(xa, y, p.stock_top + 1.0));
                    tp.line(v3(xa, y, z), MoveKind::Entry, p.plunge_feed);
                    at_a = true;
                }
            }
            let x = if at_a { xb } else { xa };
            tp.line(v3(x, y, z), MoveKind::Cut, p.feed);
            if p.zigzag {
                at_a = !at_a;
            }
        }
        let end_x = if p.zigzag && at_a { xa } else { xb };
        tp.rapid(v3(end_x, ys[ys.len() - 1], p.safe_z));
    }
    Ok(tp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Grid;
    use anvil_kernel::v2;

    #[test]
    fn facing_covers_the_stock_at_depth() {
        let mut p = FacingParams::new(20.0, v2(0.0, 0.0), v2(100.0, 63.0), 2.0, 0.0, 800.0);
        p.step_down = 1.0;
        let tp = facing(&p).unwrap();
        let cuts: Vec<_> = tp.segments(0.01).into_iter().filter(|s| s.kind == MoveKind::Cut).collect();
        // Two layers, at Z1 and Z0.
        let mut zs: Vec<f64> = cuts.iter().map(|s| s.b.z).collect();
        zs.dedup();
        assert_eq!(zs, vec![1.0, 0.0]);
        // Every point of the stock is swept by the final layer.
        let mut g = Grid::new(v2(0.0, 0.0), v2(100.0, 63.0), 0.25, 0.0).unwrap();
        g.fill_region(&crate::geom::Region::rectangle(v2(0.0, 0.0), v2(100.0, 63.0)).unwrap(), true);
        for s in cuts.iter().filter(|s| s.b.z == 0.0) {
            g.clear_capsule(v2(s.a.x, s.a.y), v2(s.b.x, s.b.y), 10.0);
        }
        assert_eq!(g.count(), 0);
        // Passes are no further apart than the stepover.
        let mut ys: Vec<f64> = cuts.iter().filter(|s| s.b.z == 0.0).map(|s| s.b.y).collect();
        ys.sort_by(f64::total_cmp);
        for w in ys.windows(2) {
            assert!(w[1] - w[0] <= 14.0 + 1e-9);
        }
        // Each cut starts and ends clear of the stock.
        for s in &cuts {
            assert!(s.a.x.min(s.b.x) < -10.0 && s.a.x.max(s.b.x) > 110.0);
        }
    }

    #[test]
    fn facing_rejects_bad_input() {
        let p = FacingParams::new(20.0, v2(0.0, 0.0), v2(100.0, 63.0), 0.0, 2.0, 800.0);
        assert!(facing(&p).is_err());
        let mut p = FacingParams::new(20.0, v2(0.0, 0.0), v2(100.0, 63.0), 2.0, 0.0, 800.0);
        p.stepover = 1.5;
        assert!(facing(&p).is_err());
        let p = FacingParams::new(20.0, v2(10.0, 0.0), v2(0.0, 63.0), 2.0, 0.0, 800.0);
        assert!(facing(&p).is_err());
    }
}
