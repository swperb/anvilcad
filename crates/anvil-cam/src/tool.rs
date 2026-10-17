//! Cutting tools, holders, and a starting-point feeds and speeds table keyed to the
//! material library in `anvil_model::material`.
//!
//! The cutting data are AnvilCAD's own conservative starting values for general-purpose
//! carbide (and high-speed steel where a tool says so), chosen inside the ranges tool
//! makers publish for each material group; they are not copied from any catalogue. The
//! formulas are the standard ones: spindle speed from surface speed and diameter, feed
//! from chip load, teeth and speed. A tool maker's recommendation for the actual tool
//! governs. See `docs/cam/DESIGN.md`.

use crate::error::{CamError, Result, non_negative, positive};
use anvil_model::material::{Material, MaterialClass};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToolShape {
    /// Flat end mill.
    Flat,
    /// Ball end mill (corner radius = half the diameter).
    Ball,
    /// Bull nose (corner radius) end mill.
    BullNose { corner_radius: f64 },
    /// Twist drill with the given point angle (degrees, usually 118 or 135).
    Drill { point_angle: f64 },
    /// Spot or centre drill with the given point angle (degrees, usually 90).
    Spot { point_angle: f64 },
}

/// A tool holder as a stack of cylinders measured up from the end of the holder that
/// grips the tool (the "gauge" end). Each is (diameter, length).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Holder {
    pub name: String,
    pub sections: Vec<(f64, f64)>,
}

impl Holder {
    /// A plain collet holder: a 40 mm long nose and a wider body.
    pub fn collet(nose_diameter: f64) -> Holder {
        Holder { name: "collet".into(), sections: vec![(nose_diameter, 40.0), (nose_diameter + 20.0, 30.0)] }
    }

    /// Radius of the holder at height `h` above its gauge end (None above the holder).
    pub fn radius_at(&self, h: f64) -> Option<f64> {
        let mut z = 0.0;
        for (d, l) in &self.sections {
            if h >= z && h <= z + l {
                return Some(d * 0.5);
            }
            z += l;
        }
        None
    }

    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.1).sum()
    }

    pub fn max_radius(&self) -> f64 {
        self.sections.iter().map(|s| s.0 * 0.5).fold(0.0, f64::max)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// Tool number in the machine's magazine.
    pub number: u32,
    pub name: String,
    pub shape: ToolShape,
    pub diameter: f64,
    pub flutes: u32,
    /// Length of cutting edge.
    pub flute_length: f64,
    pub shank_diameter: f64,
    /// Length the tool sticks out of the holder (tip to holder gauge end).
    pub stickout: f64,
    pub holder: Holder,
    /// True for carbide; false for high-speed steel.
    pub carbide: bool,
}

impl Tool {
    /// A carbide flat end mill with common proportions (flute length 3 x D, stick-out
    /// 4 x D, shank = D).
    pub fn flat_end_mill(number: u32, diameter: f64, flutes: u32) -> Tool {
        Tool {
            number,
            name: format!("{diameter} mm flat end mill, {flutes} flute"),
            shape: ToolShape::Flat,
            diameter,
            flutes,
            flute_length: 3.0 * diameter,
            shank_diameter: diameter,
            stickout: 4.0 * diameter,
            holder: Holder::collet(diameter + 12.0),
            carbide: true,
        }
    }

    pub fn ball_end_mill(number: u32, diameter: f64, flutes: u32) -> Tool {
        Tool {
            shape: ToolShape::Ball,
            name: format!("{diameter} mm ball end mill"),
            ..Tool::flat_end_mill(number, diameter, flutes)
        }
    }

    pub fn bull_nose(number: u32, diameter: f64, corner_radius: f64, flutes: u32) -> Tool {
        Tool {
            shape: ToolShape::BullNose { corner_radius },
            name: format!("{diameter} mm R{corner_radius} bull nose"),
            ..Tool::flat_end_mill(number, diameter, flutes)
        }
    }

    /// A carbide jobber-length drill with a 135 degree point.
    pub fn drill(number: u32, diameter: f64) -> Tool {
        Tool {
            number,
            name: format!("{diameter} mm drill"),
            shape: ToolShape::Drill { point_angle: 135.0 },
            diameter,
            flutes: 2,
            flute_length: 8.0 * diameter,
            shank_diameter: diameter,
            stickout: 10.0 * diameter,
            holder: Holder::collet(diameter + 12.0),
            carbide: true,
        }
    }

    pub fn radius(&self) -> f64 {
        self.diameter * 0.5
    }

    /// Height of the drill point (tip to full diameter); zero for end mills.
    pub fn point_length(&self) -> f64 {
        match self.shape {
            ToolShape::Drill { point_angle } | ToolShape::Spot { point_angle } => {
                self.radius() / (point_angle.to_radians() * 0.5).tan()
            }
            _ => 0.0,
        }
    }

    pub fn corner_radius(&self) -> f64 {
        match self.shape {
            ToolShape::Ball => self.radius(),
            ToolShape::BullNose { corner_radius } => corner_radius,
            _ => 0.0,
        }
    }

    pub fn is_drill(&self) -> bool {
        matches!(self.shape, ToolShape::Drill { .. } | ToolShape::Spot { .. })
    }

    /// Height above the tip of the tool's lower envelope at radial distance `d` from the
    /// axis, or `None` when `d` is outside the tool.
    pub fn profile_height(&self, d: f64) -> Option<f64> {
        let r = self.radius();
        if d > r + 1e-12 {
            return None;
        }
        let d = d.min(r);
        Some(match self.shape {
            ToolShape::Flat => 0.0,
            ToolShape::Ball => r - (r * r - d * d).max(0.0).sqrt(),
            ToolShape::BullNose { corner_radius } => {
                let rc = corner_radius.clamp(0.0, r);
                let flat = r - rc;
                if d <= flat {
                    0.0
                } else {
                    let x = d - flat;
                    rc - (rc * rc - x * x).max(0.0).sqrt()
                }
            }
            ToolShape::Drill { point_angle } | ToolShape::Spot { point_angle } => {
                d / (point_angle.to_radians() * 0.5).tan()
            }
        })
    }

    /// Checks the tool's numbers make sense.
    pub fn validate(&self) -> Result<()> {
        positive("tool diameter", self.diameter)?;
        positive("flute length", self.flute_length)?;
        positive("shank diameter", self.shank_diameter)?;
        positive("stick-out", self.stickout)?;
        if self.flutes == 0 || self.flutes > 32 {
            return Err(CamError::InvalidParameter { name: "flutes", reason: "must be 1 to 32".into() });
        }
        if self.stickout < self.flute_length {
            return Err(CamError::InvalidParameter {
                name: "stick-out",
                reason: "must be at least the flute length".into(),
            });
        }
        match self.shape {
            ToolShape::BullNose { corner_radius } => {
                non_negative("corner radius", corner_radius)?;
                if corner_radius > self.radius() {
                    return Err(CamError::InvalidParameter {
                        name: "corner radius",
                        reason: "cannot exceed half the diameter".into(),
                    });
                }
            }
            ToolShape::Drill { point_angle } | ToolShape::Spot { point_angle }
                if !(30.0..=180.0).contains(&point_angle) =>
            {
                return Err(CamError::InvalidParameter {
                    name: "point angle",
                    reason: "must be between 30 and 180 degrees".into(),
                });
            }
            _ => {}
        }
        for (d, l) in &self.holder.sections {
            positive("holder diameter", *d)?;
            positive("holder length", *l)?;
        }
        Ok(())
    }
}

/// Machinability groups used to pick cutting data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGroup {
    Aluminum,
    CopperAlloy,
    LowCarbonSteel,
    AlloySteel,
    Stainless,
    Titanium,
    SoftPlastic,
    HardPlastic,
}

impl MaterialGroup {
    /// The group for a library material, by name and class. Unknown metals fall back to
    /// alloy steel and unknown plastics to hard plastic, the more conservative choices.
    pub fn of(m: &Material) -> MaterialGroup {
        let n = m.name.to_ascii_lowercase();
        match m.class {
            MaterialClass::Plastic => {
                if ["uhmw", "hdpe", "ptfe", "teflon"].iter().any(|k| n.contains(k)) {
                    MaterialGroup::SoftPlastic
                } else {
                    MaterialGroup::HardPlastic
                }
            }
            MaterialClass::Metal => {
                if n.contains("alumin")
                    || n.contains("1060")
                    || n.contains("6061")
                    || n.contains("7075")
                    || n.contains("5052")
                {
                    MaterialGroup::Aluminum
                } else if n.contains("brass") || n.contains("copper") || n.contains("bronze") {
                    MaterialGroup::CopperAlloy
                } else if n.contains("titanium") || n.contains("ti-6") {
                    MaterialGroup::Titanium
                } else if n.contains("stainless")
                    || n.contains("304")
                    || n.contains("316")
                    || n.contains("17-4")
                {
                    MaterialGroup::Stainless
                } else if n.contains("1018") || n.contains("a36") || n.contains("plain carbon") {
                    MaterialGroup::LowCarbonSteel
                } else {
                    MaterialGroup::AlloySteel
                }
            }
        }
    }

    /// (carbide surface speed m/min, HSS surface speed m/min, chip load per tooth as a
    /// fraction of the diameter for end mills, the same for drills (feed per rev / D)).
    fn data(self) -> (f64, f64, f64, f64) {
        match self {
            MaterialGroup::Aluminum => (300.0, 90.0, 0.010, 0.020),
            MaterialGroup::CopperAlloy => (180.0, 50.0, 0.008, 0.015),
            MaterialGroup::LowCarbonSteel => (140.0, 30.0, 0.006, 0.014),
            MaterialGroup::AlloySteel => (110.0, 22.0, 0.005, 0.012),
            MaterialGroup::Stainless => (80.0, 15.0, 0.004, 0.010),
            MaterialGroup::Titanium => (50.0, 10.0, 0.004, 0.008),
            MaterialGroup::SoftPlastic => (250.0, 120.0, 0.010, 0.020),
            MaterialGroup::HardPlastic => (200.0, 90.0, 0.008, 0.016),
        }
    }
}

/// Cutting data for one tool in one material.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CuttingData {
    pub rpm: f64,
    /// Feed per tooth at full engagement (for drills, feed per revolution).
    pub chip_load: f64,
    /// Feed, mm/min.
    pub feed: f64,
    /// Plunge or ramp feed, mm/min.
    pub plunge_feed: f64,
    /// Surface speed used, m/min.
    pub surface_speed: f64,
}

/// Starting cutting data for `tool` in `material`, limited to `max_rpm`.
pub fn cutting_data(tool: &Tool, material: &Material, max_rpm: f64) -> Result<CuttingData> {
    tool.validate()?;
    positive("max rpm", max_rpm)?;
    let (vc_carbide, vc_hss, fz_ratio, f_rev_ratio) = MaterialGroup::of(material).data();
    let vc = if tool.carbide { vc_carbide } else { vc_hss };
    let ideal = vc * 1000.0 / (PI * tool.diameter);
    let rpm = ideal.min(max_rpm).max(1.0);
    let surface_speed = rpm * PI * tool.diameter / 1000.0;
    if tool.is_drill() {
        let f_rev = (f_rev_ratio * tool.diameter).clamp(0.01, 0.6);
        let feed = f_rev * rpm;
        return Ok(CuttingData { rpm, chip_load: f_rev, feed, plunge_feed: feed, surface_speed });
    }
    // Small tools are fragile: never below 0.005 mm per tooth, never above 0.25.
    let fz = (fz_ratio * tool.diameter).clamp(0.005, 0.25);
    let feed = fz * tool.flutes as f64 * rpm;
    Ok(CuttingData { rpm, chip_load: fz, feed, plunge_feed: feed * 0.3, surface_speed })
}

/// Feed multiplier that keeps the maximum chip thickness at the programmed chip load
/// when the radial engagement angle is `engagement` (radians), for a straight-fluted
/// cutter entering at the uncut edge: h_max = f_z sin(engagement) for engagements up to
/// 90 degrees. Limited to `max_factor`. Source: the radial chip thinning relation in
/// tool makers' milling formulas (see `docs/cam/ADAPTIVE.md`).
pub fn chip_thinning_factor(engagement: f64, max_factor: f64) -> f64 {
    let max_factor = if max_factor.is_finite() { max_factor.max(1.0) } else { 1.0 };
    if !engagement.is_finite() || engagement <= 0.0 {
        return max_factor;
    }
    if engagement >= PI / 2.0 {
        return 1.0;
    }
    (1.0 / engagement.sin()).min(max_factor)
}

/// A set of tools by number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolLibrary {
    pub tools: Vec<Tool>,
}

impl ToolLibrary {
    pub fn get(&self, number: u32) -> Result<&Tool> {
        self.tools
            .iter()
            .find(|t| t.number == number)
            .ok_or_else(|| CamError::NotFound(format!("tool {number} is not in the library")))
    }

    pub fn add(&mut self, tool: Tool) -> Result<()> {
        tool.validate()?;
        if self.tools.iter().any(|t| t.number == tool.number) {
            return Err(CamError::InvalidParameter {
                name: "tool number",
                reason: format!("{} is already used", tool.number),
            });
        }
        self.tools.push(tool);
        Ok(())
    }

    /// The largest flat end mill no wider than `max_diameter`.
    pub fn largest_end_mill(&self, max_diameter: f64) -> Option<&Tool> {
        self.tools
            .iter()
            .filter(|t| {
                matches!(t.shape, ToolShape::Flat | ToolShape::BullNose { .. }) && t.diameter <= max_diameter
            })
            .max_by(|a, b| a.diameter.total_cmp(&b.diameter))
    }

    /// A drill within `tol` of `diameter`.
    pub fn drill_for(&self, diameter: f64, tol: f64) -> Option<&Tool> {
        self.tools
            .iter()
            .filter(|t| matches!(t.shape, ToolShape::Drill { .. }) && (t.diameter - diameter).abs() <= tol)
            .min_by(|a, b| (a.diameter - diameter).abs().total_cmp(&(b.diameter - diameter).abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib_material(name: &str) -> Material {
        Material::library().into_iter().find(|m| m.name.contains(name)).unwrap()
    }

    #[test]
    fn groups_for_library() {
        assert_eq!(MaterialGroup::of(&lib_material("6061")), MaterialGroup::Aluminum);
        assert_eq!(MaterialGroup::of(&lib_material("316")), MaterialGroup::Stainless);
        assert_eq!(MaterialGroup::of(&lib_material("Ti-6Al")), MaterialGroup::Titanium);
        assert_eq!(MaterialGroup::of(&lib_material("4140")), MaterialGroup::AlloySteel);
        assert_eq!(MaterialGroup::of(&lib_material("1018")), MaterialGroup::LowCarbonSteel);
        assert_eq!(MaterialGroup::of(&lib_material("Delrin")), MaterialGroup::HardPlastic);
        assert_eq!(MaterialGroup::of(&lib_material("PTFE")), MaterialGroup::SoftPlastic);
        for m in Material::library() {
            let _ = MaterialGroup::of(&m);
        }
    }

    #[test]
    fn speeds_and_feeds() {
        let t = Tool::flat_end_mill(1, 10.0, 3);
        let d = cutting_data(&t, &lib_material("6061"), 12_000.0).unwrap();
        // 300 m/min on 10 mm is 9549 rpm; 0.1 mm/tooth, 3 teeth.
        assert!((d.rpm - 9549.3).abs() < 1.0);
        assert!((d.feed - 0.1 * 3.0 * d.rpm).abs() < 1e-6);
        let capped = cutting_data(&t, &lib_material("6061"), 6000.0).unwrap();
        assert_eq!(capped.rpm, 6000.0);
        let steel = cutting_data(&t, &lib_material("316"), 12_000.0).unwrap();
        assert!(steel.rpm < d.rpm && steel.feed < d.feed);
        let bad = Tool { diameter: -1.0, ..t };
        assert!(cutting_data(&bad, &lib_material("6061"), 1000.0).is_err());
    }

    #[test]
    fn thinning_factor() {
        assert!((chip_thinning_factor(PI / 2.0, 3.0) - 1.0).abs() < 1e-12);
        assert!((chip_thinning_factor(PI / 6.0, 3.0) - 2.0).abs() < 1e-12);
        assert_eq!(chip_thinning_factor(0.01, 2.5), 2.5);
        assert_eq!(chip_thinning_factor(f64::NAN, 2.0), 2.0);
    }

    #[test]
    fn tool_profiles() {
        let b = Tool::ball_end_mill(1, 10.0, 2);
        assert!((b.profile_height(5.0).unwrap() - 5.0).abs() < 1e-12);
        assert_eq!(b.profile_height(0.0), Some(0.0));
        assert_eq!(b.profile_height(5.1), None);
        let d = Tool::drill(2, 10.0);
        assert!((d.point_length() - 5.0 / (67.5f64.to_radians()).tan()).abs() < 1e-12);
    }
}
