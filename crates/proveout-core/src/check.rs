//! The deterministic check: the `anvil-cam` verifier run against the user's machine
//! profile (and post definition, when it is an AnvilCAD one), plus checks that do not
//! depend on where program zero is.
//!
//! The verifier is strict by design. Applied to programs from other CAM systems it
//! rejects codes it cannot follow, and the findings here say so in plain terms
//! ("the checker cannot follow G187") instead of calling the program wrong.

use crate::error::{Result, invalid};
use crate::machine::MachineProfile;
use crate::program::{ProgramSummary, summarize};
use anvil_cam::post::{MAX_POST_BYTES, PostDefinition, Units};
use anvil_cam::toolpath::MoveKind;
use anvil_cam::verify::{MAX_PROGRAM_BYTES, Severity, VerifyOptions, verify};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Largest foreign post file accepted (it is only read, never run).
pub const MAX_FOREIGN_POST_BYTES: usize = 512 * 1024;
/// Most line numbers kept on one finding.
pub const MAX_LINES_PER_FINDING: usize = 50;

/// Where a finding came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The rule-based checker. Same input, same answer.
    Deterministic,
    /// The AI review. Advisory.
    Ai,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Breaks a rule of the machine or post, or will not run.
    Error,
    /// A person must look at it: dangerous constructs, codes the checker cannot follow.
    Review,
    Warning,
    Info,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "Error",
            Level::Review => "Review",
            Level::Warning => "Warning",
            Level::Info => "Info",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub source: Source,
    pub level: Level,
    pub category: String,
    /// 1-based program lines; empty for the whole program.
    pub lines: Vec<usize>,
    pub message: String,
    #[serde(default)]
    pub suggestion: Option<String>,
}

/// The post processor the user gave, if any.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PostInput {
    None,
    /// An AnvilCAD post definition: its rules are applied by the verifier.
    Anvil {
        file_name: String,
        text: String,
    },
    /// Any other post (Fusion `.cps`, Mastercam `.pst`, ...). Never run; read as text by
    /// the AI review and scanned for risky output in [`crate::profile`].
    Foreign {
        file_name: String,
        text: String,
    },
}

impl PostInput {
    /// Sorts an uploaded file into an AnvilCAD post (if it parses as one) or a foreign
    /// post.
    pub fn from_upload(file_name: &str, text: &str) -> Result<PostInput> {
        let file_name: String = file_name.chars().filter(|c| !c.is_control()).take(120).collect();
        if text.trim().is_empty() {
            return Ok(PostInput::None);
        }
        if text.len() > MAX_FOREIGN_POST_BYTES {
            return Err(invalid(
                "post processor",
                format!("larger than {} KB", MAX_FOREIGN_POST_BYTES / 1024),
            ));
        }
        if text.len() <= MAX_POST_BYTES && PostDefinition::from_toml(text).is_ok() {
            return Ok(PostInput::Anvil { file_name, text: text.to_string() });
        }
        if file_name.to_ascii_lowercase().ends_with(".toml") {
            // Say why rather than quietly treating it as foreign.
            if let Err(e) = PostDefinition::from_toml(text) {
                return Err(invalid("AnvilCAD post definition", e.to_string()));
            }
        }
        Ok(PostInput::Foreign { file_name, text: text.to_string() })
    }

    pub fn label(&self) -> String {
        match self {
            PostInput::None => "none".into(),
            PostInput::Anvil { file_name, text } => match PostDefinition::from_toml(text) {
                Ok(d) => format!("{} {} (AnvilCAD post, {file_name})", d.post.name, d.post.version),
                Err(_) => format!("{file_name} (AnvilCAD post)"),
            },
            PostInput::Foreign { file_name, .. } => format!("{file_name} (read only, not run)"),
        }
    }

    pub fn anvil(&self) -> Option<PostDefinition> {
        match self {
            PostInput::Anvil { text, .. } => PostDefinition::from_toml(text).ok(),
            _ => None,
        }
    }
}

/// Program extents in millimetres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Extents {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Extents {
    pub fn span(&self) -> [f64; 3] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeterministicResult {
    /// False when the control's dialect is one the checker does not read.
    pub supported: bool,
    pub rules: String,
    pub units: Option<Units>,
    pub findings: Vec<Finding>,
    pub blocks: usize,
    /// Distinct lines with an error or review finding.
    pub lines_flagged: usize,
    /// Lines whose effect the checker could not follow (declared constructs).
    pub opaque_lines: usize,
    /// Share of blocks with no error or review finding, 0 to 1.
    pub rule_pass_rate: f64,
    /// Extents of every move the checker followed, in mm.
    pub extents: Option<Extents>,
    /// Extents of feed moves only, in mm.
    pub cut_extents: Option<Extents>,
    /// Feed time plus rapid time at the profile's rapid rate, minutes. A lower bound
    /// when the checker could not follow every line.
    pub cycle_minutes: Option<f64>,
    pub summary: ProgramSummary,
}

impl DeterministicResult {
    pub fn errors(&self) -> usize {
        self.findings.iter().filter(|f| f.level == Level::Error).count()
    }
}

fn classify(message: &str, construct: bool, user_rules: bool) -> (Level, &'static str, Option<String>) {
    if construct {
        return (
            Level::Review,
            "dangerous construct",
            Some(
                "Make sure this is intended and that its effect on offsets or the controller is understood."
                    .into(),
            ),
        );
    }
    if message.contains("cannot be followed by the verifier")
        || (!user_rules && message.contains("is not in this post's allow list"))
    {
        return (
            Level::Review,
            "not followed",
            Some("Check this code against the machine's manual; motion on this line was not checked.".into()),
        );
    }
    if message.contains("outside the machine's travel") || message.contains("absurd") {
        return (Level::Error, "travel", None);
    }
    if message.contains("spindle speed") || message.contains("feed") && message.contains("outside") {
        return (Level::Error, "limits", None);
    }
    (Level::Error, "rule", None)
}

/// Runs the deterministic check.
pub fn check(gcode: &str, machine: &MachineProfile, post: &PostInput) -> Result<DeterministicResult> {
    if gcode.trim().is_empty() {
        return Err(invalid("program", "is empty"));
    }
    if gcode.len() > MAX_PROGRAM_BYTES {
        return Err(invalid("program", "is too large to check"));
    }
    machine.validate()?;
    let summary = summarize(gcode);
    let mut out = DeterministicResult { units: summary.units, blocks: summary.blocks, ..Default::default() };
    if !machine.controller.deterministic() {
        out.rules = format!("none: the checker does not read {} programs", machine.controller.label());
        out.findings.push(Finding {
            source: Source::Deterministic,
            level: Level::Info,
            category: "dialect".into(),
            lines: vec![],
            message: format!(
                "The deterministic checker reads RS274/ISO word-address G-code and not the {} language, so only \
                 the AI review applies to this program.",
                machine.controller.label()
            ),
            suggestion: None,
        });
        out.summary = summary;
        return Ok(out);
    }
    out.supported = true;
    let units = summary.units.unwrap_or(machine.default_units);
    let user = post.anvil();
    let def = machine.verifier_post(units, user.as_ref())?;
    out.rules = match &user {
        Some(d) => format!("{} {} with the limits of {}", d.post.name, d.post.version, machine.name),
        None => format!("codes the checker can follow, with the limits of {}", machine.name),
    };
    let rep = verify(gcode, &def, &VerifyOptions::default());
    out.opaque_lines = rep.opaque_lines;

    // Merge identical messages so a code used on 400 lines is one finding.
    type Key = (Level, &'static str, String);
    let mut merged: BTreeMap<Key, (Vec<usize>, Option<String>)> = BTreeMap::new();
    let mut flagged = BTreeSet::new();
    for issue in &rep.issues {
        let (level, cat, sugg) = if issue.severity == Severity::Warning {
            // Every program without a known start position gets this note.
            if issue.line == 0 && issue.message.contains("start position") {
                (Level::Info, "rule", None)
            } else {
                (Level::Warning, "rule", None)
            }
        } else {
            classify(&issue.message, issue.construct.is_some(), user.is_some())
        };
        let message = match issue.message.strip_suffix(" is not in this post's allow list") {
            Some(code) if user.is_none() => format!("{code} is not a code the checker can follow"),
            _ => issue.message.clone(),
        };
        if matches!(level, Level::Error | Level::Review) && issue.line > 0 {
            flagged.insert(issue.line);
        }
        let e = merged.entry((level, cat, message)).or_insert((Vec::new(), sugg));
        if issue.line > 0 && e.0.len() < MAX_LINES_PER_FINDING {
            e.0.push(issue.line);
        }
    }
    for ((level, cat, message), (lines, suggestion)) in merged {
        out.findings.push(Finding {
            source: Source::Deterministic,
            level,
            category: cat.into(),
            lines,
            message,
            suggestion,
        });
    }

    // Extents and time from the motion the verifier followed.
    let mut all: Option<Extents> = None;
    let mut cut: Option<Extents> = None;
    let grow = |e: &mut Option<Extents>, p: anvil_kernel::Vec3| {
        let p = [p.x, p.y, p.z];
        let x = e.get_or_insert(Extents { min: p, max: p });
        for (i, v) in p.iter().enumerate() {
            x.min[i] = x.min[i].min(*v);
            x.max[i] = x.max[i].max(*v);
        }
    };
    for s in rep.toolpath.segments(0.05) {
        grow(&mut all, s.a);
        grow(&mut all, s.b);
        if s.kind != MoveKind::Rapid {
            grow(&mut cut, s.a);
            grow(&mut cut, s.b);
        }
    }
    let stats = rep.toolpath.stats();
    if stats.moves > 0 {
        out.cycle_minutes = Some(stats.feed_minutes + stats.rapid_length / machine.rapid_rate);
    }
    // The span of the feed moves: rapids are left out because the verifier sends G28
    // to the top of the assumed travel, which would inflate the span.
    if let Some(e) = cut {
        let span = e.span();
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            if span[i] > machine.travel[i] + 1e-6 {
                out.findings.push(Finding {
                    source: Source::Deterministic,
                    level: Level::Error,
                    category: "travel".into(),
                    lines: vec![],
                    message: format!(
                        "the program feeds over {:.1} mm along {axis} but the machine travels {:.1} mm, so it \
                         cannot run wherever program zero is",
                        span[i], machine.travel[i]
                    ),
                    suggestion: None,
                });
            }
        }
    }
    out.extents = all;
    out.cut_extents = cut;

    // Tool numbers against the changer.
    if machine.tool_capacity > 0 {
        let over: Vec<&crate::program::ToolUse> =
            summary.tools.iter().filter(|t| t.tool > machine.tool_capacity).collect();
        for t in over {
            out.findings.push(Finding {
                source: Source::Deterministic,
                level: Level::Warning,
                category: "tooling".into(),
                lines: vec![t.first_line],
                message: format!(
                    "T{} is above the {}-pocket tool changer; check the pocket numbering",
                    t.tool, machine.tool_capacity
                ),
                suggestion: None,
            });
        }
    } else if !summary.tools.is_empty() && summary.codes.contains_key("M6") {
        out.findings.push(Finding {
            source: Source::Deterministic,
            level: Level::Warning,
            category: "tooling".into(),
            lines: summary.tools.iter().map(|t| t.first_line).take(MAX_LINES_PER_FINDING).collect(),
            message: "the program uses M6 tool changes but the profile has no tool changer".into(),
            suggestion: Some(
                "Check how this control handles M6 (stop for a manual change, or ignore it).".into(),
            ),
        });
    }
    if summary.units.is_none() {
        out.findings.push(Finding {
            source: Source::Deterministic,
            level: Level::Warning,
            category: "units".into(),
            lines: vec![],
            message: format!(
                "the program does not set units with G20 or G21; it was checked in {}",
                if units == Units::Inch { "inches" } else { "millimetres" }
            ),
            suggestion: Some("State the units at the start of the program.".into()),
        });
    }
    for t in &summary.tools {
        if t.length_offsets.is_empty() && machine.tool_capacity > 0 {
            out.findings.push(Finding {
                source: Source::Deterministic,
                level: Level::Warning,
                category: "offsets".into(),
                lines: vec![t.first_line],
                message: format!("T{} is used without a G43 H length offset", t.tool),
                suggestion: None,
            });
        }
    }

    out.lines_flagged = flagged.len();
    out.rule_pass_rate =
        if out.blocks == 0 { 0.0 } else { 1.0 - (flagged.len() as f64 / out.blocks as f64).min(1.0) };
    out.findings.sort_by_key(|f| (f.level, f.lines.first().copied().unwrap_or(0)));
    out.summary = summary;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::preset;

    const GOOD: &str = "%
O1001 (BRACKET)
G90 G17 G21 G40 G49 G80
G54
T1 M6
S8000 M3
G0 X0. Y0.
G43 H1 Z50. M8
G0 Z2.
G1 Z-5. F300.
G1 X100. F1200.
G0 Z50.
M5
M9
G91 G28 Z0.
G90
M30
%";

    #[test]
    fn a_clean_program_passes_with_exact_extents_and_time() {
        let m = preset("vmc-fanuc").unwrap();
        let r = check(GOOD, &m, &PostInput::None).unwrap();
        assert!(r.supported);
        assert_eq!(r.errors(), 0, "{:#?}", r.findings);
        assert_eq!(r.blocks, 16);
        assert_eq!(r.lines_flagged, 0);
        assert_eq!(r.rule_pass_rate, 1.0);
        let e = r.cut_extents.unwrap();
        assert_eq!(e.min, [0.0, 0.0, -5.0]);
        assert_eq!(e.max, [100.0, 0.0, 2.0]);
        // Feed: 7 mm at 300 + 100 mm at 1200 = 0.10667 min.
        let feed = 7.0 / 300.0 + 100.0 / 1200.0;
        let t = r.cycle_minutes.unwrap();
        assert!(t > feed, "{t}");
    }

    #[test]
    fn unknown_codes_are_review_not_error() {
        let m = preset("vmc-haas").unwrap();
        let prog = GOOD.replace("G54\n", "G54\nG187 P1\n");
        let r = check(&prog, &m, &PostInput::None).unwrap();
        let f = r.findings.iter().find(|f| f.category == "not followed").unwrap();
        assert_eq!(f.level, Level::Review);
        assert_eq!(f.lines, vec![5]);
        assert_eq!(r.lines_flagged, 1);
        assert_eq!(r.blocks, 17);
        assert!((r.rule_pass_rate - 16.0 / 17.0).abs() < 1e-12);
    }

    #[test]
    fn over_speed_and_over_travel_are_errors() {
        let m = preset("vmc-fanuc").unwrap();
        let prog = GOOD.replace("S8000", "S9000").replace("X100.", "X800.");
        let r = check(&prog, &m, &PostInput::None).unwrap();
        assert!(
            r.findings
                .iter()
                .any(|f| f.level == Level::Error && f.category == "limits" && f.lines == vec![6])
        );
        let t = r.findings.iter().find(|f| f.category == "travel").unwrap();
        assert!(t.message.contains("800.0 mm along X"), "{}", t.message);
    }

    #[test]
    fn g10_is_flagged_for_review() {
        let m = preset("vmc-fanuc").unwrap();
        let prog = GOOD.replace("G54\n", "G54\nG10 L2 P1 X0 Y0 Z0\n");
        let r = check(&prog, &m, &PostInput::None).unwrap();
        assert!(r.findings.iter().any(|f| f.category == "dangerous construct" && f.level == Level::Review));
    }

    #[test]
    fn heidenhain_is_not_misread() {
        let m = preset("vmc-heidenhain").unwrap();
        let r = check("0 BEGIN PGM X MM\nL X+10 R0 FMAX\n", &m, &PostInput::None).unwrap();
        assert!(!r.supported);
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].level, Level::Info);
    }

    #[test]
    fn uploads_are_sorted() {
        let anvil = anvil_cam::post::bundled()[0].1;
        assert!(matches!(PostInput::from_upload("a.toml", anvil).unwrap(), PostInput::Anvil { .. }));
        assert!(matches!(
            PostInput::from_upload("fanuc.cps", "function onOpen() {}").unwrap(),
            PostInput::Foreign { .. }
        ));
        assert!(PostInput::from_upload("bad.toml", "[post]\nname = 1").is_err());
        assert_eq!(PostInput::from_upload("x.cps", "  ").unwrap(), PostInput::None);
    }

    #[test]
    fn manual_machine_with_m6_is_warned() {
        let m = preset("router-grbl").unwrap();
        let r = check(GOOD, &m, &PostInput::None).unwrap();
        assert!(r.findings.iter().any(|f| f.message.contains("no tool changer")));
    }
}
