//! Profiling a machine or a post on its own, before any program is checked.
//!
//! The profile check answers "is my setup in Proveout right?": are the machine's
//! numbers plausible, does the post agree with the machine, what risky codes can the
//! post write, and does a program the user knows is good come back clean. That last
//! step is the calibration loop: a known-good program that fails means the profile is
//! wrong, not the program. None of this is a guarantee; the report says what to check
//! by hand.

use crate::check::{Level, PostInput, check};
use crate::machine::MachineProfile;
use anvil_cam::registry::reference_check;
use anvil_cam::verify::Severity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileCheck {
    pub name: String,
    pub state: State,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RiskyLine {
    pub line: usize,
    pub token: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileReport {
    pub subject: String,
    pub checks: Vec<ProfileCheck>,
    /// A short program written by the post (or the bundled rules for the control) for
    /// the control's graphics mode. It cuts metal if run on a part, and says so.
    pub reference_program: Option<String>,
    /// Lines of a foreign post that write risky codes.
    pub risky_lines: Vec<RiskyLine>,
    pub next_steps: Vec<String>,
}

/// Codes worth a person's attention when a post can write them.
const RISKY: &[(&str, &str)] = &[
    ("G10", "writes offsets or parameters"),
    ("G92", "shifts the coordinate system"),
    ("G52", "shifts the coordinate system"),
    ("G65", "calls a macro"),
    ("M98", "calls a subprogram"),
    ("M99", "returns from or loops a subprogram"),
    ("DPRNT", "writes to a port or file"),
    ("POPEN", "opens a port or file"),
    ("BPRNT", "writes to a port or file"),
    ("G23", "turns stored stroke limits off"),
    ("M49", "locks the overrides"),
    ("#", "uses macro or system variables"),
];

/// Quoted strings on a line of post source.
fn quoted(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for q in ['"', '\''] {
        let mut parts = line.split(q);
        parts.next();
        while let Some(inside) = parts.next() {
            out.push(inside);
            if parts.next().is_none() {
                break;
            }
        }
    }
    out
}

fn has_token(s: &str, token: &str) -> bool {
    if token == "#" {
        return s.contains('#');
    }
    let up = s.to_ascii_uppercase();
    up.match_indices(token).any(|(i, _)| {
        let after = up[i + token.len()..].chars().next();
        let before = up[..i].chars().last();
        before.is_none_or(|c| !c.is_ascii_alphanumeric())
            && after.is_none_or(|c| !c.is_ascii_digit() && c != '.')
    })
}

/// Lines of a post's source whose string literals contain a risky code.
pub fn scan_foreign_post(text: &str) -> Vec<RiskyLine> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
            continue;
        }
        for q in quoted(line) {
            if let Some((tok, _)) = RISKY.iter().find(|(tok, _)| has_token(q, tok)) {
                out.push(RiskyLine {
                    line: i + 1,
                    token: tok.to_string(),
                    text: line.trim().chars().take(160).collect(),
                });
                break;
            }
        }
        if out.len() >= 100 {
            break;
        }
    }
    out
}

fn add(checks: &mut Vec<ProfileCheck>, name: &str, state: State, detail: impl Into<String>) {
    checks.push(ProfileCheck { name: name.into(), state, detail: detail.into() });
}

/// Profiles a machine and post. `known_good` is a program the user has run without
/// trouble on this machine, if they have one.
pub fn profile(machine: &MachineProfile, post: &PostInput, known_good: Option<&str>) -> ProfileReport {
    let mut checks = Vec::new();
    let mut reference_program = None;
    let mut risky_lines = Vec::new();
    match machine.validate() {
        Ok(()) => add(&mut checks, "Profile values", State::Pass, "All values are in range."),
        Err(e) => {
            add(&mut checks, "Profile values", State::Fail, e.to_string());
            return ProfileReport {
                subject: machine.name.clone(),
                checks,
                reference_program,
                risky_lines,
                next_steps: next_steps(),
            };
        }
    }
    if machine.rapid_rate < machine.max_feed {
        add(
            &mut checks,
            "Rapid rate",
            State::Warn,
            format!(
                "The rapid rate ({:.0} mm/min) is below the highest cutting feed ({:.0} mm/min); one of them is \
                 probably wrong.",
                machine.rapid_rate, machine.max_feed
            ),
        );
    } else {
        add(&mut checks, "Rapid rate", State::Pass, format!("{:.0} mm/min.", machine.rapid_rate));
    }
    if machine.tool_capacity > machine.max_tool {
        add(
            &mut checks,
            "Tool numbers",
            State::Warn,
            format!(
                "The changer has {} pockets but tool numbers stop at {}.",
                machine.tool_capacity, machine.max_tool
            ),
        );
    } else {
        add(
            &mut checks,
            "Tool numbers",
            State::Pass,
            if machine.tool_capacity == 0 {
                format!("Manual tool changes, tool numbers up to {}.", machine.max_tool)
            } else {
                format!("{} pockets, tool numbers up to {}.", machine.tool_capacity, machine.max_tool)
            },
        );
    }
    if machine.controller.deterministic() {
        add(
            &mut checks,
            "Control dialect",
            State::Pass,
            format!("{} programs are read by the deterministic checker.", machine.controller.label()),
        );
    } else {
        add(
            &mut checks,
            "Control dialect",
            State::Info,
            format!(
                "{} programs are not read by the deterministic checker; only the AI review applies.",
                machine.controller.label()
            ),
        );
    }

    match post {
        PostInput::None => add(
            &mut checks,
            "Post processor",
            State::Info,
            "No post given. Programs are checked against the codes the checker can follow and this machine's limits.",
        ),
        PostInput::Foreign { file_name, text } => {
            add(
                &mut checks,
                "Post processor",
                State::Info,
                format!(
                    "{file_name} is not an AnvilCAD post, so it was read but not run. The AI review sees it as context."
                ),
            );
            risky_lines = scan_foreign_post(text);
            if risky_lines.is_empty() {
                add(
                    &mut checks,
                    "Risky output",
                    State::Pass,
                    "No risky codes found in the post's output strings.",
                );
            } else {
                add(
                    &mut checks,
                    "Risky output",
                    State::Warn,
                    format!(
                        "{} line(s) of the post can write codes that change offsets, call programs or bypass \
                         limits. Make sure each is intended.",
                        risky_lines.len()
                    ),
                );
            }
        }
        PostInput::Anvil { .. } => match post.anvil() {
            None => {
                add(&mut checks, "Post processor", State::Fail, "The AnvilCAD post definition does not load.")
            }
            Some(def) => {
                add(&mut checks, "Post processor", State::Pass, post.label());
                let m = &def.machine;
                let mut conflicts = Vec::new();
                if m.max_rpm > machine.max_rpm {
                    conflicts.push(format!("spindle {:.0} > {:.0} rpm", m.max_rpm, machine.max_rpm));
                }
                if m.max_feed > machine.max_feed {
                    conflicts.push(format!("feed {:.0} > {:.0} mm/min", m.max_feed, machine.max_feed));
                }
                for (i, (axis, r)) in [("X", m.x), ("Y", m.y), ("Z", m.z)].iter().enumerate() {
                    if r[1] - r[0] > machine.travel[i] + 1e-6 {
                        conflicts.push(format!(
                            "{axis} travel {:.0} > {:.0} mm",
                            r[1] - r[0],
                            machine.travel[i]
                        ));
                    }
                }
                if m.max_tool > machine.max_tool {
                    conflicts.push(format!("tools up to {} > {}", m.max_tool, machine.max_tool));
                }
                if conflicts.is_empty() {
                    add(
                        &mut checks,
                        "Post against machine",
                        State::Pass,
                        "The post's limits fit the machine.",
                    );
                } else {
                    add(
                        &mut checks,
                        "Post against machine",
                        State::Warn,
                        format!(
                            "The post allows more than the machine: {}. Programs are checked with the machine's \
                             limits.",
                            conflicts.join("; ")
                        ),
                    );
                }
                if !def.declared.is_empty() {
                    let list: Vec<String> = def
                        .declared
                        .iter()
                        .map(|d| format!("{} ({})", d.construct.label(), d.reason))
                        .collect();
                    add(&mut checks, "Declared constructs", State::Warn, list.join("; "));
                }
            }
        },
    }

    if machine.controller.deterministic() {
        let user = post.anvil();
        let units = user.as_ref().map(|d| d.format.units).unwrap_or(machine.default_units);
        match machine.verifier_post(units, user.as_ref()) {
            Ok(def) => {
                let (text, rep) = reference_check(&def);
                let errors: Vec<String> = rep
                    .issues
                    .iter()
                    .filter(|i| i.severity == Severity::Error)
                    .map(|i| format!("line {}: {}", i.line, i.message))
                    .take(10)
                    .collect();
                if errors.is_empty() {
                    add(
                        &mut checks,
                        "Reference program",
                        State::Pass,
                        "The rules write a 60 x 40 x 25 mm test program that passes the checker.",
                    );
                } else {
                    add(
                        &mut checks,
                        "Reference program",
                        State::Fail,
                        format!(
                            "The test program (two tools, 6000 and 2000 rpm, a peck cycle) fails: {}",
                            errors.join("; ")
                        ),
                    );
                }
                if !text.is_empty() {
                    reference_program = Some(format!(
                        "(PROVEOUT REFERENCE PROGRAM - FOR THE CONTROL'S GRAPHICS OR SIMULATION ONLY)\n\
                         (IT CUTS 4 MM DEEP AND DRILLS 15 MM - DO NOT RUN ON A PART)\n{text}"
                    ));
                }
            }
            Err(e) => add(&mut checks, "Reference program", State::Fail, e.to_string()),
        }
    }

    if let Some(prog) = known_good.filter(|p| !p.trim().is_empty()) {
        match check(prog, machine, post) {
            Ok(r) if !r.supported => add(
                &mut checks,
                "Known-good program",
                State::Info,
                "Not checked: the dialect is not supported.",
            ),
            Ok(r) => {
                let errors: Vec<String> = r
                    .findings
                    .iter()
                    .filter(|f| f.level == Level::Error)
                    .map(|f| match f.lines.first() {
                        Some(l) => format!("line {l}: {}", f.message),
                        None => f.message.clone(),
                    })
                    .take(10)
                    .collect();
                let review = r.findings.iter().filter(|f| f.level == Level::Review).count();
                if errors.is_empty() {
                    add(
                        &mut checks,
                        "Known-good program",
                        State::Pass,
                        format!(
                            "No errors in {} blocks ({review} finding(s) to review, usually codes the checker cannot \
                             follow). The profile agrees with a program you trust.",
                            r.blocks
                        ),
                    );
                } else {
                    add(
                        &mut checks,
                        "Known-good program",
                        State::Fail,
                        format!(
                            "A program you trust fails, so the profile is probably wrong: {}",
                            errors.join("; ")
                        ),
                    );
                }
            }
            Err(e) => add(&mut checks, "Known-good program", State::Fail, e.to_string()),
        }
    } else {
        add(
            &mut checks,
            "Known-good program",
            State::Info,
            "Add a program that has run well on this machine to calibrate the profile.",
        );
    }

    ProfileReport {
        subject: machine.name.clone(),
        checks,
        reference_program,
        risky_lines,
        next_steps: next_steps(),
    }
}

fn next_steps() -> Vec<String> {
    vec![
        "Compare every number in the profile with the machine's manual or the builder's specification, not memory."
            .into(),
        "Check a program you know is good: it should come back with no errors. If it does not, fix the profile first."
            .into(),
        "Run the reference program in the control's graphics or simulation mode only, never on a part.".into(),
        "These checks carry no guarantee. They only know what the profile tells them.".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::preset;

    #[test]
    fn a_preset_profiles_cleanly_and_writes_a_reference_program() {
        let m = preset("vmc-fanuc").unwrap();
        let r = profile(&m, &PostInput::None, None);
        let fails: Vec<_> = r.checks.iter().filter(|c| c.state == State::Fail).collect();
        assert!(fails.is_empty(), "{fails:#?}");
        let prog = r.reference_program.unwrap();
        assert!(prog.starts_with("(PROVEOUT REFERENCE PROGRAM"));
        assert!(prog.contains("M30"));
    }

    #[test]
    fn known_good_program_calibrates_the_profile() {
        let mut m = preset("vmc-fanuc").unwrap();
        let good = "O1\nG21 G90\nT1 M6\nS7000 M3\nG0 X0 Y0\nG43 H1 Z10\nG1 Z-1 F100\nG0 Z10\nM30\n";
        let r = profile(&m, &PostInput::None, Some(good));
        let kg = r.checks.iter().find(|c| c.name == "Known-good program").unwrap();
        assert_eq!(kg.state, State::Pass, "{}", kg.detail);
        // A profile with the spindle limit set too low fails the same program.
        m.max_rpm = 6000.0;
        let r = profile(&m, &PostInput::None, Some(good));
        let kg = r.checks.iter().find(|c| c.name == "Known-good program").unwrap();
        assert_eq!(kg.state, State::Fail);
        assert!(kg.detail.contains("line 4"), "{}", kg.detail);
    }

    #[test]
    fn foreign_posts_are_scanned_not_run() {
        let cps = "function onOpen() {\n  writeBlock(\"G10 L2 P1\");\n  // writeBlock(\"G92\");\n  \
                   writeBlock('G90 G54');\n  writeBlock(\"#100=1\");\n  writeBlock(\"G100\");\n}\n";
        let post = PostInput::Foreign { file_name: "x.cps".into(), text: cps.into() };
        let r = profile(&preset("vmc-fanuc").unwrap(), &post, None);
        let lines: Vec<(usize, &str)> = r.risky_lines.iter().map(|l| (l.line, l.token.as_str())).collect();
        assert_eq!(lines, vec![(2, "G10"), (5, "#")]);
    }

    #[test]
    fn post_limits_above_the_machine_are_warned() {
        let text = anvil_cam::post::bundled()[2].1; // GRBL, 24000 rpm
        let post = PostInput::Anvil { file_name: "grbl.toml".into(), text: text.into() };
        let r = profile(&preset("vmc-fanuc").unwrap(), &post, None);
        let c = r.checks.iter().find(|c| c.name == "Post against machine").unwrap();
        assert_eq!(c.state, State::Warn);
        assert!(c.detail.contains("spindle 24000 > 8100 rpm"), "{}", c.detail);
    }
}
