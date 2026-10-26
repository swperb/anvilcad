//! Machine profiles: what the machine can do, in the terms the checks need.
//!
//! A profile is the user's own description of their machine (travel, spindle, feed,
//! tool changer, control). It is turned into an AnvilCAD post definition for the
//! deterministic verifier with [`MachineProfile::verifier_post`]. Program zero is not
//! known, so the travel check there only rejects moves further from zero than the
//! whole travel; [`crate::check`] separately compares the program's span with the
//! travel, which does not depend on where zero is.

use crate::error::{Result, invalid};
use anvil_cam::post::{PostDefinition, Units, bundled};
use serde::{Deserialize, Serialize};

/// The control family, which decides the G-code dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    Fanuc,
    Haas,
    Mazak,
    Okuma,
    LinuxCnc,
    Grbl,
    Siemens,
    Heidenhain,
    Other,
}

impl Controller {
    pub fn label(self) -> &'static str {
        match self {
            Controller::Fanuc => "Fanuc-style",
            Controller::Haas => "Haas",
            Controller::Mazak => "Mazak (EIA/ISO mode)",
            Controller::Okuma => "Okuma OSP (ISO G-code)",
            Controller::LinuxCnc => "LinuxCNC",
            Controller::Grbl => "GRBL",
            Controller::Siemens => "Siemens Sinumerik",
            Controller::Heidenhain => "Heidenhain",
            Controller::Other => "Other RS274 / ISO control",
        }
    }

    /// Whether the deterministic verifier reads this dialect. It reads word-address
    /// RS274/ISO G-code; Sinumerik and Heidenhain programs are usually written in their
    /// own languages, which it would misread, so for those only the AI review runs.
    pub fn deterministic(self) -> bool {
        !matches!(self, Controller::Siemens | Controller::Heidenhain)
    }

    /// File name of the bundled AnvilCAD post the verifier rules start from.
    fn base_post(self) -> &'static str {
        match self {
            Controller::LinuxCnc => "linuxcnc-mill.toml",
            Controller::Grbl => "grbl-1.1.toml",
            _ => "fanuc-generic-mill.toml",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineKind {
    VerticalMill,
    HorizontalMill,
    Router,
    Other,
}

/// A user's description of one machine. Lengths are millimetres, feeds mm/min.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProfile {
    pub name: String,
    pub controller: Controller,
    pub kind: MachineKind,
    /// Travel length along X, Y and Z.
    pub travel: [f64; 3],
    pub max_rpm: f64,
    #[serde(default)]
    pub min_rpm: f64,
    /// Highest programmable cutting feed.
    pub max_feed: f64,
    /// Rapid traverse rate, for the cycle-time estimate.
    pub rapid_rate: f64,
    /// Pockets in the tool changer; 0 for manual tool changes.
    pub tool_capacity: u32,
    /// Highest tool and offset number the control accepts.
    pub max_tool: u32,
    /// Units used when the program does not say (no G20 or G21).
    #[serde(default = "default_units")]
    pub default_units: Units,
    /// Anything else worth knowing: spindle taper, probe, through-spindle coolant.
    #[serde(default)]
    pub notes: String,
}

fn default_units() -> Units {
    Units::Mm
}

/// Longest name and notes accepted.
pub const MAX_NAME: usize = 80;
pub const MAX_NOTES: usize = 2000;

impl MachineProfile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.chars().count() > MAX_NAME {
            return Err(invalid("machine name", format!("needs 1 to {MAX_NAME} characters")));
        }
        if self.notes.chars().count() > MAX_NOTES {
            return Err(invalid("machine notes", format!("longer than {MAX_NOTES} characters")));
        }
        if self.name.chars().chain(self.notes.chars()).any(|c| c.is_control() && c != '\n') {
            return Err(invalid("machine profile", "has control characters"));
        }
        for (axis, t) in ["X", "Y", "Z"].iter().zip(self.travel) {
            if !(t.is_finite() && t > 0.0 && t <= 50_000.0) {
                return Err(invalid("travel", format!("{axis} travel must be between 0 and 50 000 mm")));
            }
        }
        let ok = |v: f64, hi: f64| v.is_finite() && v > 0.0 && v <= hi;
        if !(ok(self.max_rpm, 1e6) && self.min_rpm.is_finite() && self.min_rpm >= 0.0) {
            return Err(invalid("spindle speed", "limits must be positive"));
        }
        if self.min_rpm > self.max_rpm {
            return Err(invalid("spindle speed", "the minimum is above the maximum"));
        }
        if !ok(self.max_feed, 1e6) || !ok(self.rapid_rate, 1e6) {
            return Err(invalid("feed", "the cutting feed and rapid rate must be positive"));
        }
        if self.max_tool == 0 || self.max_tool > 9999 || self.tool_capacity > 9999 {
            return Err(invalid("tools", "the highest tool number must be 1 to 9999"));
        }
        Ok(())
    }

    /// The post definition the deterministic verifier uses for this machine, in the
    /// program's units. With `user_post` the user's own rules (allowed codes, declared
    /// constructs, format) are kept and only the machine limits come from the profile;
    /// without it the rules start from the bundled post for the control family and
    /// allow every code the verifier can follow, so it flags what it cannot follow
    /// rather than what one particular post would not write.
    pub fn verifier_post(&self, units: Units, user_post: Option<&PostDefinition>) -> Result<PostDefinition> {
        self.validate()?;
        let mut def = match user_post {
            Some(p) => p.clone(),
            None => {
                let name = self.controller.base_post();
                let text = bundled()
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, t)| *t)
                    .ok_or_else(|| invalid("controller", "no bundled post for this control"))?;
                let mut d =
                    PostDefinition::from_toml(text).map_err(|e| invalid("bundled post", e.to_string()))?;
                d.allow.g = FOLLOWED_G.iter().map(|s| s.to_string()).collect();
                d.allow.m = FOLLOWED_M.iter().map(|s| s.to_string()).collect();
                d.allow.words = "GMXYZIJKFSTHDPQRNO".into();
                d.capabilities.canned_cycles = true;
                d.format.max_line_length = 256;
                // The bundled posts are metric with three decimals. In inches that
                // rounds arc end points by up to 0.0005 in, more than the verifier's
                // arc tolerance, so the reference program would fail its own check.
                if units == Units::Inch {
                    d.format.decimals = d.format.decimals.max(4);
                }
                d
            }
        };
        def.format.units = units;
        def.capabilities.tool_changer = self.tool_capacity > 0;
        let [tx, ty, tz] = self.travel;
        def.machine.x = [-tx, tx];
        def.machine.y = [-ty, ty];
        def.machine.z = [-tz, tz];
        def.machine.max_feed = self.max_feed;
        def.machine.max_rpm = self.max_rpm;
        def.machine.min_rpm = self.min_rpm;
        def.machine.max_tool = self.max_tool;
        def.check().map_err(|e| invalid("post processor", e.to_string()))?;
        Ok(def)
    }
}

/// G codes the `anvil-cam` verifier follows (its `UNDERSTOOD_G`). Codes outside this
/// list are reported as ones the checker cannot follow.
pub const FOLLOWED_G: &[&str] = &[
    "G0", "G1", "G2", "G3", "G4", "G17", "G20", "G21", "G28", "G40", "G43", "G49", "G54", "G55", "G56",
    "G57", "G58", "G59", "G61", "G64", "G73", "G80", "G81", "G82", "G83", "G90", "G91", "G91.1", "G94",
    "G98", "G99",
];
/// M codes the verifier follows (its `UNDERSTOOD_M`).
pub const FOLLOWED_M: &[&str] = &["M0", "M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9", "M30"];

/// Starting points for a profile. They are generic: the user should replace every
/// number with their own machine's.
pub fn presets() -> Vec<(&'static str, MachineProfile)> {
    let p = |name: &str, controller, kind, travel, max_rpm, max_feed, rapid, cap, max_tool, units| {
        MachineProfile {
            name: name.into(),
            controller,
            kind,
            travel,
            max_rpm,
            min_rpm: 0.0,
            max_feed,
            rapid_rate: rapid,
            tool_capacity: cap,
            max_tool,
            default_units: units,
            notes: String::new(),
        }
    };
    use Controller as C;
    use MachineKind as K;
    vec![
        (
            "vmc-fanuc",
            p(
                "Generic VMC, Fanuc-style control",
                C::Fanuc,
                K::VerticalMill,
                [762.0, 406.0, 508.0],
                8100.0,
                12700.0,
                25400.0,
                20,
                99,
                Units::Mm,
            ),
        ),
        (
            "vmc-haas",
            p(
                "Generic VMC, Haas control",
                C::Haas,
                K::VerticalMill,
                [762.0, 406.0, 508.0],
                8100.0,
                16500.0,
                25400.0,
                20,
                200,
                Units::Inch,
            ),
        ),
        (
            "hmc-fanuc",
            p(
                "Generic HMC, Fanuc-style control",
                C::Fanuc,
                K::HorizontalMill,
                [560.0, 560.0, 625.0],
                12000.0,
                30000.0,
                50000.0,
                60,
                99,
                Units::Mm,
            ),
        ),
        (
            "mill-linuxcnc",
            p(
                "Generic knee mill, LinuxCNC",
                C::LinuxCnc,
                K::VerticalMill,
                [500.0, 250.0, 300.0],
                4000.0,
                2000.0,
                3000.0,
                0,
                99,
                Units::Mm,
            ),
        ),
        (
            "router-grbl",
            p(
                "Generic router, GRBL 1.1",
                C::Grbl,
                K::Router,
                [300.0, 300.0, 80.0],
                24000.0,
                5000.0,
                5000.0,
                0,
                99,
                Units::Mm,
            ),
        ),
        (
            "vmc-siemens",
            p(
                "Generic VMC, Siemens control (AI review only)",
                C::Siemens,
                K::VerticalMill,
                [800.0, 500.0, 500.0],
                12000.0,
                20000.0,
                30000.0,
                24,
                99,
                Units::Mm,
            ),
        ),
        (
            "vmc-heidenhain",
            p(
                "Generic VMC, Heidenhain control (AI review only)",
                C::Heidenhain,
                K::VerticalMill,
                [800.0, 500.0, 500.0],
                12000.0,
                20000.0,
                30000.0,
                24,
                99,
                Units::Mm,
            ),
        ),
    ]
}

/// A preset by its id.
pub fn preset(id: &str) -> Option<MachineProfile> {
    presets().into_iter().find(|(k, _)| *k == id).map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_build_verifier_posts() {
        for (id, p) in presets() {
            p.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
            let def = p.verifier_post(Units::Mm, None).unwrap();
            assert_eq!(def.machine.x, [-p.travel[0], p.travel[0]]);
            assert_eq!(def.machine.max_rpm, p.max_rpm);
            assert_eq!(def.capabilities.tool_changer, p.tool_capacity > 0);
        }
    }

    #[test]
    fn the_followed_lists_are_valid_post_codes() {
        let p = preset("vmc-fanuc").unwrap();
        let def = p.verifier_post(Units::Inch, None).unwrap();
        assert_eq!(def.allow.g.len(), FOLLOWED_G.len());
        assert_eq!(def.format.units, Units::Inch);
    }

    #[test]
    fn inch_reference_programs_pass_their_own_check() {
        for id in ["vmc-haas", "vmc-fanuc"] {
            let p = preset(id).unwrap();
            let def = p.verifier_post(Units::Inch, None).unwrap();
            assert_eq!(def.format.decimals, 4);
            let (text, rep) = anvil_cam::registry::reference_check(&def);
            assert!(text.contains("G20"));
            assert!(rep.passed(), "{id}: {:#?}", rep.issues);
        }
    }

    #[test]
    fn bad_profiles_are_rejected() {
        let mut p = preset("vmc-fanuc").unwrap();
        p.travel[1] = -1.0;
        assert!(p.validate().is_err());
        let mut p = preset("vmc-fanuc").unwrap();
        p.min_rpm = 9000.0;
        assert!(p.validate().is_err());
        let mut p = preset("vmc-fanuc").unwrap();
        p.name = "x".repeat(81);
        assert!(p.validate().is_err());
        let mut p = preset("vmc-fanuc").unwrap();
        p.max_feed = f64::NAN;
        assert!(p.validate().is_err());
    }

    #[test]
    fn a_user_post_keeps_its_rules_but_takes_the_machine_limits() {
        let text = bundled()[2].1; // GRBL: no M6 in its allow list
        let user = PostDefinition::from_toml(text).unwrap();
        let p = preset("vmc-fanuc").unwrap();
        let def = p.verifier_post(Units::Mm, Some(&user)).unwrap();
        assert_eq!(def.allow.m, user.allow.m);
        assert_eq!(def.machine.max_tool, 99);
        assert_eq!(def.machine.z, [-508.0, 508.0]);
    }
}
