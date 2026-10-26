//! The report and the setup sheet, and their HTML and Markdown renderings.
//!
//! The setup sheet doubles as setup documentation and as the verification record: the
//! tool table, offsets, operations, additions and a checklist with sign-off lines.
//! Every entry says where it came from: "program" (read from the G-code), "check" (the
//! deterministic checker), "AI" (the AI review, advisory) or "standard" (Proveout's
//! fixed checklist). Values read from the program win over AI values.

use crate::ai::{AiReview, DrawingStatus};
use crate::check::{DeterministicResult, Finding, Level, Source};
use crate::machine::MachineProfile;
use crate::plan::ReviewLevel;
use crate::score::{Score, ScoreInputs, score};
use crate::{APP_NAME, DISCLOSURE_VERSION, NO_GUARANTEE_DISCLOSURE, PRIVACY_DISCLOSURE};
use anvil_cam::post::Units;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputsMeta {
    pub gcode_bytes: usize,
    /// SHA-256 of the program text, so a printed sheet can be matched to the file.
    pub gcode_sha256: String,
    pub drawing_file: Option<String>,
    pub drawing_notes: bool,
    pub anonymized: bool,
    pub post: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupTool {
    pub tool: u32,
    pub description: String,
    pub description_source: String,
    pub length_offsets: String,
    pub diameter_offsets: String,
    pub speeds: String,
    pub feeds: String,
    pub min_z: String,
    pub first_line: usize,
    pub holder_and_stickout: String,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupItem {
    pub text: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupOffset {
    pub name: String,
    pub note: String,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupSheet {
    pub program: String,
    pub part: String,
    pub material: String,
    pub stock: String,
    pub workholding: String,
    pub program_zero: String,
    pub units: String,
    pub cycle_time: String,
    pub extents: String,
    pub tools: Vec<SetupTool>,
    pub offsets: Vec<SetupOffset>,
    pub operations: Vec<SetupItem>,
    pub additions: Vec<SetupItem>,
    pub checklist: Vec<SetupItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub app: String,
    pub version: String,
    /// Set by whoever stores the report.
    pub id: String,
    /// ISO 8601, UTC.
    pub created: String,
    pub title: String,
    pub level: Option<ReviewLevel>,
    pub machine: MachineProfile,
    pub inputs: InputsMeta,
    pub deterministic: DeterministicResult,
    pub ai: Option<AiReview>,
    pub score: Score,
    pub setup: SetupSheet,
    pub disclosure_version: String,
    pub disclaimers: Vec<String>,
}

pub fn sha256_hex(text: &str) -> String {
    let d = Sha256::digest(text.as_bytes());
    d.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn range(r: Option<[f64; 2]>) -> String {
    match r {
        Some([a, b]) if a == b => num(a),
        Some([a, b]) => format!("{} - {}", num(a), num(b)),
        None => "-".into(),
    }
}

fn list(v: &[u32], prefix: &str) -> String {
    if v.is_empty() {
        "-".into()
    } else {
        v.iter().map(|n| format!("{prefix}{n}")).collect::<Vec<_>>().join(", ")
    }
}

const STANDARD_CHECKS: &[&str] = &[
    "The program, drawing and revision on the traveller are the same ones this report was made from (compare the SHA-256 above).",
    "Each tool in the tool table is in its pocket, matches its description, and has its length (H) and diameter (D) offsets measured and entered.",
    "Each work offset in the table is set by touching off or probing, and checked.",
    "The stock size and work holding match the setup notes; clamps, vise jaws and fixtures are clear of every tool path.",
    "The program has been run in the control's graphics or simulation mode.",
    "The first run is in single block with rapid and feed overrides turned down and a hand on feed hold.",
    "The first part is inspected against the drawing before more are run.",
];

/// Builds the setup sheet from what the program says and, when present, the AI review.
pub fn setup_sheet(det: &DeterministicResult, ai: Option<&AiReview>) -> SetupSheet {
    let s = &det.summary;
    let unknown = "unknown - confirm at the machine".to_string();
    let ai_setup = ai.map(|a| &a.setup);
    let pick = |f: fn(&crate::ai::AiSetup) -> &String| {
        ai_setup
            .map(f)
            .filter(|v| !v.is_empty())
            .map(|v| format!("{v} (AI)"))
            .unwrap_or_else(|| unknown.clone())
    };
    let units = match det.units {
        Some(Units::Inch) => "inch (G20)".to_string(),
        Some(Units::Mm) => "mm (G21)".to_string(),
        None => "not set in the program".to_string(),
    };
    let mut tools: Vec<SetupTool> = s
        .tools
        .iter()
        .map(|t| {
            let at = ai_setup.and_then(|a| a.tools.iter().find(|x| x.tool == t.tool));
            let (description, description_source) = match (&t.description, at) {
                (Some(d), _) => (d.clone(), "program".to_string()),
                (None, Some(a)) if !a.description.is_empty() => (a.description.clone(), "AI".to_string()),
                _ => (unknown.clone(), "-".to_string()),
            };
            SetupTool {
                tool: t.tool,
                description,
                description_source,
                length_offsets: list(&t.length_offsets, "H"),
                diameter_offsets: list(&t.diameter_offsets, "D"),
                speeds: if t.speeds.is_empty() {
                    "-".into()
                } else {
                    t.speeds.iter().map(|v| num(*v)).collect::<Vec<_>>().join(", ")
                },
                feeds: range(t.feed_range),
                min_z: t.min_z.map(num).unwrap_or_else(|| "-".into()),
                first_line: t.first_line,
                holder_and_stickout: at
                    .map(|a| a.holder_and_stickout.clone())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_default(),
                notes: at.map(|a| a.notes.clone()).unwrap_or_default(),
            }
        })
        .collect();
    // Tools the AI names that the scanner did not find are listed but marked.
    if let Some(a) = ai_setup {
        for at in &a.tools {
            if at.tool > 0 && !tools.iter().any(|t| t.tool == at.tool) {
                tools.push(SetupTool {
                    tool: at.tool,
                    description: at.description.clone(),
                    description_source: "AI only - not found in the program".into(),
                    length_offsets: "-".into(),
                    diameter_offsets: "-".into(),
                    speeds: "-".into(),
                    feeds: "-".into(),
                    min_z: "-".into(),
                    first_line: 0,
                    holder_and_stickout: at.holder_and_stickout.clone(),
                    notes: at.notes.clone(),
                });
            }
        }
    }
    let mut offsets: Vec<SetupOffset> = s
        .work_offsets
        .iter()
        .map(|w| SetupOffset {
            name: w.clone(),
            note: "X ______  Y ______  Z ______".into(),
            source: "program".into(),
        })
        .collect();
    for t in &s.tools {
        for h in &t.length_offsets {
            offsets.push(SetupOffset {
                name: format!("H{h} (T{})", t.tool),
                note: "length ______".into(),
                source: "program".into(),
            });
        }
        for d in &t.diameter_offsets {
            offsets.push(SetupOffset {
                name: format!("D{d} (T{})", t.tool),
                note: "diameter / wear ______".into(),
                source: "program".into(),
            });
        }
    }
    if let Some(a) = ai_setup {
        for o in &a.offsets {
            offsets.push(SetupOffset { name: o.name.clone(), note: o.note.clone(), source: "AI".into() });
        }
    }
    let operations = ai_setup
        .map(|a| {
            let mut ops = a.operations.clone();
            ops.sort_by_key(|o| o.order);
            ops.iter()
                .map(|o| SetupItem {
                    text: format!(
                        "{}. T{} {}{}",
                        o.order,
                        o.tool,
                        o.description,
                        if o.lines.is_empty() { String::new() } else { format!(" (lines {})", o.lines) }
                    ),
                    source: "AI".into(),
                })
                .collect()
        })
        .unwrap_or_default();
    let additions = ai
        .map(|a| {
            a.additions
                .iter()
                .map(|x| SetupItem {
                    text: format!("[{}] {}: {}", x.kind, x.title, x.detail),
                    source: "AI".into(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut checklist: Vec<SetupItem> =
        STANDARD_CHECKS.iter().map(|c| SetupItem { text: (*c).into(), source: "standard".into() }).collect();
    for f in det.findings.iter().filter(|f| matches!(f.level, Level::Error | Level::Review)) {
        checklist.push(SetupItem { text: format!("Resolve: {}", finding_line(f)), source: "check".into() });
    }
    if let Some(a) = ai_setup {
        for c in &a.checklist {
            checklist.push(SetupItem { text: c.clone(), source: "AI".into() });
        }
    }
    let ext = det.cut_extents.map(|e| {
        format!(
            "feed moves X {} to {}, Y {} to {}, Z {} to {} mm",
            num(e.min[0]),
            num(e.max[0]),
            num(e.min[1]),
            num(e.max[1]),
            num(e.min[2]),
            num(e.max[2])
        )
    });
    SetupSheet {
        program: s.program_number.clone().unwrap_or_else(|| "-".into()),
        part: pick(|a| &a.part),
        material: pick(|a| &a.material),
        stock: pick(|a| &a.stock),
        workholding: pick(|a| &a.workholding),
        program_zero: pick(|a| &a.program_zero),
        units,
        cycle_time: det
            .cycle_minutes
            .map(|m| {
                let secs = (m * 60.0).round() as u64;
                format!(
                    "about {}:{:02} (estimate; tool changes and dwells not included)",
                    secs / 60,
                    secs % 60
                )
            })
            .unwrap_or_else(|| "-".into()),
        extents: ext.unwrap_or_else(|| "-".into()),
        tools,
        offsets,
        operations,
        additions,
        checklist,
    }
}

fn finding_line(f: &Finding) -> String {
    let lines = match f.lines.len() {
        0 => String::new(),
        1 => format!("line {}: ", f.lines[0]),
        n if n <= 5 => {
            format!("lines {}: ", f.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "))
        }
        n => format!(
            "lines {} and {} more: ",
            f.lines[..3].iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "),
            n - 3
        ),
    };
    format!("{lines}{}", f.message)
}

/// Builds the report. `id` and `created` are left for the caller to fill in.
#[allow(clippy::too_many_arguments)]
pub fn build(
    title: &str,
    machine: &MachineProfile,
    gcode: &str,
    det: DeterministicResult,
    ai: Option<AiReview>,
    level: Option<ReviewLevel>,
    inputs: InputsMeta,
) -> Report {
    let mut inputs = inputs;
    inputs.gcode_bytes = gcode.len();
    inputs.gcode_sha256 = sha256_hex(gcode);
    let (mut m, mut mm, mut un) = (0, 0, 0);
    if let Some(a) = &ai {
        for d in &a.drawing_checks {
            match d.status {
                DrawingStatus::Match => m += 1,
                DrawingStatus::Mismatch | DrawingStatus::NotInProgram => mm += 1,
                DrawingStatus::Unclear => un += 1,
            }
        }
    }
    let ai_findings = ai.as_ref().map(|a| a.findings.as_slice()).unwrap_or(&[]);
    let sc = score(&ScoreInputs {
        rule_pass_rate: det.supported.then_some(det.rule_pass_rate),
        deterministic_errors: det.errors(),
        ai_errors: ai_findings.iter().filter(|f| f.level == Level::Error).count(),
        review_findings: det.findings.iter().chain(ai_findings).filter(|f| f.level == Level::Review).count(),
        drawing_matched: m,
        drawing_mismatched: mm,
        drawing_unclear: un,
        ai_confidence: ai.as_ref().map(|a| a.confidence),
    });
    let setup = setup_sheet(&det, ai.as_ref());
    let mut disclaimers = vec![NO_GUARANTEE_DISCLOSURE.to_string()];
    if ai.is_some() {
        disclaimers.push(PRIVACY_DISCLOSURE.to_string());
    }
    let title: String = title.chars().filter(|c| !c.is_control()).take(120).collect();
    Report {
        app: APP_NAME.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        id: String::new(),
        created: String::new(),
        title: if title.trim().is_empty() { "Untitled program".into() } else { title },
        level,
        machine: machine.clone(),
        inputs,
        deterministic: det,
        ai,
        score: sc,
        setup,
        disclosure_version: DISCLOSURE_VERSION.into(),
        disclaimers,
    }
}

/// HTML-escapes text.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Escapes text for a Markdown table cell or line.
fn md(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|").replace('\n', " ").replace('<', "&lt;")
}

fn all_findings(r: &Report) -> Vec<&Finding> {
    let mut v: Vec<&Finding> = r.deterministic.findings.iter().collect();
    if let Some(a) = &r.ai {
        v.extend(a.findings.iter());
    }
    v.sort_by_key(|f| (f.level, f.lines.first().copied().unwrap_or(0)));
    v
}

fn source_label(s: Source) -> &'static str {
    match s {
        Source::Deterministic => "check",
        Source::Ai => "AI",
    }
}

const CSS: &str = "body{font:14px/1.45 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;color:#111;margin:24px;max-width:1100px}\
h1{font-size:22px;margin:0 0 4px}h2{font-size:16px;margin:22px 0 6px;border-bottom:2px solid #111;padding-bottom:2px}\
table{border-collapse:collapse;width:100%;margin:6px 0}th,td{border:1px solid #999;padding:4px 6px;text-align:left;vertical-align:top}\
th{background:#eee}.meta td{border:none;padding:1px 8px 1px 0}.score{font-size:28px;font-weight:700}\
.src{color:#555;font-size:12px}.error{color:#a00;font-weight:600}.review{color:#a60;font-weight:600}\
.box{border:2px solid #111;padding:8px;margin:12px 0}.small{font-size:12px;color:#333}.sig td{height:28px}\
.chk{width:22px}@media print{body{margin:10mm}h2{break-after:avoid}tr{break-inside:avoid}}";

/// A self-contained, printable HTML page: the report and the setup sheet.
pub fn render_html(r: &Report) -> String {
    let mut h = String::new();
    let st = &r.setup;
    let _ = write!(
        h,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width,initial-scale=1\"><title>{} setup sheet - {}</title><style>{CSS}</style></head><body>",
        esc(&r.app),
        esc(&r.title)
    );
    let _ = write!(
        h,
        "<h1>{} - setup sheet and check report</h1><table class=\"meta\"><tr><td><b>Program</b></td><td>{}</td>\
         <td><b>Title</b></td><td>{}</td></tr><tr><td><b>Machine</b></td><td>{} ({})</td><td><b>Post</b></td><td>{}</td></tr>\
         <tr><td><b>Created</b></td><td>{}</td><td><b>Report</b></td><td>{}</td></tr>\
         <tr><td><b>Review</b></td><td>{}</td><td><b>Program SHA-256</b></td><td class=\"small\">{}</td></tr></table>",
        esc(&r.app),
        esc(&st.program),
        esc(&r.title),
        esc(&r.machine.name),
        esc(r.machine.controller.label()),
        esc(&r.inputs.post),
        esc(&r.created),
        esc(&r.id),
        esc(&match (&r.level, &r.ai) {
            (Some(l), Some(a)) =>
                format!("{} ({}{})", l.label(), a.model, if a.fell_back { ", fallback model" } else { "" }),
            _ => "deterministic check only".into(),
        }),
        esc(&r.inputs.gcode_sha256),
    );
    let _ = write!(
        h,
        "<div class=\"box\"><span class=\"score\">{}%</span> {} <span class=\"small\">(rules {}, drawing {}, AI {})</span>\
         <div class=\"small\">Agreement score, not a probability that the program is right.{}</div></div>",
        r.score.percent,
        esc(r.score.band.label()),
        r.score.rules.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        r.score.drawing.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        r.score.ai.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        if r.score.caps.is_empty() {
            String::new()
        } else {
            format!(" Limits: {}.", esc(&r.score.caps.join("; ")))
        },
    );
    if let Some(a) = &r.ai {
        let _ = write!(
            h,
            "<h2>Summary (AI)</h2><p>{}</p><p class=\"small\">Confidence {}%: {}</p>",
            esc(&a.summary),
            a.confidence.round(),
            esc(&a.confidence_reason)
        );
    }
    h.push_str("<h2>Setup</h2><table>");
    for (k, v) in [
        ("Part", &st.part),
        ("Material", &st.material),
        ("Stock", &st.stock),
        ("Work holding", &st.workholding),
        ("Program zero", &st.program_zero),
        ("Units", &st.units),
        ("Extents", &st.extents),
        ("Cycle time", &st.cycle_time),
    ] {
        let _ = write!(h, "<tr><th style=\"width:150px\">{k}</th><td>{}</td></tr>", esc(v));
    }
    h.push_str("</table><h2>Tools</h2><table><tr><th>T</th><th>Description</th><th>H</th><th>D</th><th>RPM</th>\
                <th>Feed</th><th>Min Z</th><th>Line</th><th>Holder / stick-out</th><th>Notes</th><th class=\"chk\">OK</th></tr>");
    for t in &st.tools {
        let _ = write!(
            h,
            "<tr><td>T{}</td><td>{} <span class=\"src\">({})</span></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>\
             <td>{}</td><td>{}</td><td>{}</td><td>{}</td><td></td></tr>",
            t.tool,
            esc(&t.description),
            esc(&t.description_source),
            esc(&t.length_offsets),
            esc(&t.diameter_offsets),
            esc(&t.speeds),
            esc(&t.feeds),
            esc(&t.min_z),
            if t.first_line > 0 { t.first_line.to_string() } else { "-".into() },
            esc(&t.holder_and_stickout),
            esc(&t.notes),
        );
    }
    h.push_str("</table><h2>Offsets</h2><table><tr><th>Offset</th><th>Value / note</th><th>Source</th><th class=\"chk\">OK</th></tr>");
    for o in &st.offsets {
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td class=\"src\">{}</td><td></td></tr>",
            esc(&o.name),
            esc(&o.note),
            esc(&o.source)
        );
    }
    h.push_str("</table>");
    if !st.operations.is_empty() {
        h.push_str("<h2>Operations</h2><ol style=\"list-style:none;padding:0\">");
        for o in &st.operations {
            let _ = write!(h, "<li>{} <span class=\"src\">({})</span></li>", esc(&o.text), esc(&o.source));
        }
        h.push_str("</ol>");
    }
    if let Some(a) = &r.ai
        && !a.drawing_checks.is_empty()
    {
        h.push_str(
            "<h2>Drawing comparison (AI)</h2><table><tr><th>Feature</th><th>Drawing</th><th>Program</th>\
                    <th>Status</th><th>Lines</th><th>Note</th></tr>",
        );
        for d in &a.drawing_checks {
            let status = match d.status {
                DrawingStatus::Match => "match",
                DrawingStatus::Mismatch => "<span class=\"error\">MISMATCH</span>",
                DrawingStatus::Unclear => "<span class=\"review\">unclear</span>",
                DrawingStatus::NotInProgram => "<span class=\"error\">not in program</span>",
            };
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{status}</td><td>{}</td><td>{}</td></tr>",
                esc(&d.feature),
                esc(&d.drawing_value),
                esc(&d.program_value),
                esc(&d.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")),
                esc(&d.note)
            );
        }
        h.push_str("</table>");
    }
    h.push_str("<h2>Findings</h2>");
    let fs = all_findings(r);
    if fs.is_empty() {
        h.push_str("<p>No findings.</p>");
    } else {
        h.push_str("<table><tr><th>Level</th><th>From</th><th>Category</th><th>Lines</th><th>Finding</th><th>Suggestion</th></tr>");
        for f in fs {
            let class = match f.level {
                Level::Error => "error",
                Level::Review => "review",
                _ => "",
            };
            let _ = write!(
                h,
                "<tr><td class=\"{class}\">{}</td><td class=\"src\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                f.level.label(),
                source_label(f.source),
                esc(&f.category),
                esc(&f.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")),
                esc(&f.message),
                esc(f.suggestion.as_deref().unwrap_or("")),
            );
        }
        h.push_str("</table>");
    }
    if !st.additions.is_empty() {
        h.push_str("<h2>Suggested additions and changes (AI)</h2><ul>");
        for a in &st.additions {
            let _ = write!(h, "<li>{}</li>", esc(&a.text));
        }
        h.push_str("</ul>");
    }
    if let Some(a) = &r.ai
        && !a.questions.is_empty()
    {
        h.push_str("<h2>Open questions (AI)</h2><ul>");
        for q in &a.questions {
            let _ = write!(h, "<li>{}</li>", esc(q));
        }
        h.push_str("</ul>");
    }
    h.push_str("<h2>Verification checklist</h2><table><tr><th class=\"chk\">OK</th><th>Check</th><th>Source</th><th>Initials</th></tr>");
    for c in &st.checklist {
        let _ = write!(
            h,
            "<tr><td>&#9744;</td><td>{}</td><td class=\"src\">{}</td><td style=\"width:70px\"></td></tr>",
            esc(&c.text),
            esc(&c.source)
        );
    }
    h.push_str("</table><h2>Sign-off</h2><table class=\"sig\"><tr><th>Role</th><th>Name</th><th>Date</th><th>Signature</th></tr>\
                <tr><td>Programmer</td><td></td><td></td><td></td></tr><tr><td>Setup</td><td></td><td></td><td></td></tr>\
                <tr><td>First article inspection</td><td></td><td></td><td></td></tr></table>");
    let _ = write!(
        h,
        "<h2>About this report</h2><p class=\"small\">Rules: {}. Blocks checked: {}, lines flagged: {}.</p>",
        esc(&r.deterministic.rules),
        r.deterministic.blocks,
        r.deterministic.lines_flagged
    );
    for d in &r.disclaimers {
        let _ = write!(h, "<p class=\"small\"><b>No guarantee.</b> {}</p>", esc(d));
    }
    let _ = write!(
        h,
        "<p class=\"small\">{} {} - disclosure {}</p></body></html>",
        esc(&r.app),
        esc(&r.version),
        esc(&r.disclosure_version)
    );
    h
}

/// The same report as Markdown.
pub fn render_markdown(r: &Report) -> String {
    let st = &r.setup;
    let mut m = String::new();
    let _ = writeln!(m, "# {} setup sheet: {}\n", md(&r.app), md(&r.title));
    let _ = writeln!(m, "- Program: {}", md(&st.program));
    let _ = writeln!(m, "- Machine: {} ({})", md(&r.machine.name), r.machine.controller.label());
    let _ = writeln!(m, "- Post: {}", md(&r.inputs.post));
    let _ = writeln!(m, "- Created: {}  Report: {}", md(&r.created), md(&r.id));
    let _ = writeln!(m, "- Program SHA-256: `{}`", r.inputs.gcode_sha256);
    let _ = writeln!(
        m,
        "- Agreement score: **{}%** ({}). Rules {}, drawing {}, AI {}. {}\n",
        r.score.percent,
        r.score.band.label(),
        r.score.rules.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        r.score.drawing.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        r.score.ai.map(|v| format!("{v}%")).unwrap_or_else(|| "n/a".into()),
        md(&r.score.caps.join("; "))
    );
    if let Some(a) = &r.ai {
        let _ = writeln!(
            m,
            "## Summary (AI, {})\n\n{}\n\nConfidence {}%: {}\n",
            md(&a.model),
            md(&a.summary),
            a.confidence.round(),
            md(&a.confidence_reason)
        );
    }
    m.push_str("## Setup\n\n| | |\n|---|---|\n");
    for (k, v) in [
        ("Part", &st.part),
        ("Material", &st.material),
        ("Stock", &st.stock),
        ("Work holding", &st.workholding),
        ("Program zero", &st.program_zero),
        ("Units", &st.units),
        ("Extents", &st.extents),
        ("Cycle time", &st.cycle_time),
    ] {
        let _ = writeln!(m, "| {k} | {} |", md(v));
    }
    m.push_str("\n## Tools\n\n| T | Description | H | D | RPM | Feed | Min Z | Line | Holder / stick-out | Notes |\n|---|---|---|---|---|---|---|---|---|---|\n");
    for t in &st.tools {
        let _ = writeln!(
            m,
            "| T{} | {} ({}) | {} | {} | {} | {} | {} | {} | {} | {} |",
            t.tool,
            md(&t.description),
            md(&t.description_source),
            md(&t.length_offsets),
            md(&t.diameter_offsets),
            md(&t.speeds),
            md(&t.feeds),
            md(&t.min_z),
            t.first_line,
            md(&t.holder_and_stickout),
            md(&t.notes)
        );
    }
    m.push_str("\n## Offsets\n\n| Offset | Value / note | Source |\n|---|---|---|\n");
    for o in &st.offsets {
        let _ = writeln!(m, "| {} | {} | {} |", md(&o.name), md(&o.note), md(&o.source));
    }
    if !st.operations.is_empty() {
        m.push_str("\n## Operations\n\n");
        for o in &st.operations {
            let _ = writeln!(m, "- {} ({})", md(&o.text), o.source);
        }
    }
    if let Some(a) = &r.ai
        && !a.drawing_checks.is_empty()
    {
        m.push_str("\n## Drawing comparison (AI)\n\n| Feature | Drawing | Program | Status | Lines | Note |\n|---|---|---|---|---|---|\n");
        for d in &a.drawing_checks {
            let _ = writeln!(
                m,
                "| {} | {} | {} | {:?} | {} | {} |",
                md(&d.feature),
                md(&d.drawing_value),
                md(&d.program_value),
                d.status,
                d.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", "),
                md(&d.note)
            );
        }
    }
    m.push_str("\n## Findings\n\n");
    let fs = all_findings(r);
    if fs.is_empty() {
        m.push_str("No findings.\n");
    }
    for f in fs {
        let _ = writeln!(
            m,
            "- **{}** ({}, {}) {}{}",
            f.level.label(),
            source_label(f.source),
            md(&f.category),
            md(&finding_line(f)),
            f.suggestion.as_deref().map(|s| format!(" Suggestion: {}", md(s))).unwrap_or_default()
        );
    }
    if !st.additions.is_empty() {
        m.push_str("\n## Suggested additions and changes (AI)\n\n");
        for a in &st.additions {
            let _ = writeln!(m, "- {}", md(&a.text));
        }
    }
    m.push_str("\n## Verification checklist\n\n");
    for c in &st.checklist {
        let _ = writeln!(m, "- [ ] {} ({})", md(&c.text), c.source);
    }
    m.push_str("\n## Sign-off\n\n| Role | Name | Date | Signature |\n|---|---|---|---|\n| Programmer | | | |\n| Setup | | | |\n| First article inspection | | | |\n\n");
    for d in &r.disclaimers {
        let _ = writeln!(m, "> **No guarantee.** {}\n", md(d));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiSetup, AiTool, DrawingCheck};
    use crate::check::{PostInput, check};
    use crate::machine::preset;

    const PROG: &str = "O1001 (BRACKET)\n(T1 D=10 FLAT END MILL)\nG21 G90 G54\nT1 M6\nS8000 M3\nG0 X0 Y0\n\
                        G43 H1 Z10\nG1 Z-3 F250\nG1 X50 F1000\nG0 Z10\nM30\n";

    fn ai() -> AiReview {
        AiReview {
            model: "claude-opus-5-5".into(),
            confidence: 80.0,
            summary: "Faces <b>and</b> cuts".into(),
            drawing_checks: vec![
                DrawingCheck {
                    feature: "Depth".into(),
                    drawing_value: "3".into(),
                    program_value: "3".into(),
                    status: DrawingStatus::Match,
                    lines: vec![8],
                    note: String::new(),
                },
                DrawingCheck {
                    feature: "Slot length".into(),
                    drawing_value: "60".into(),
                    program_value: "50".into(),
                    status: DrawingStatus::Mismatch,
                    lines: vec![9],
                    note: String::new(),
                },
            ],
            setup: AiSetup {
                material: "6061-T6".into(),
                tools: vec![
                    AiTool {
                        tool: 1,
                        description: "AI says 10mm".into(),
                        holder_and_stickout: "ER32, 30 mm".into(),
                        notes: String::new(),
                    },
                    AiTool { tool: 7, description: "ghost".into(), ..AiTool::default() },
                ],
                checklist: vec!["Check the slot length".into()],
                ..AiSetup::default()
            },
            ..AiReview::default()
        }
    }

    #[test]
    fn setup_sheet_prefers_the_program_and_marks_sources() {
        let m = preset("vmc-fanuc").unwrap();
        let det = check(PROG, &m, &PostInput::None).unwrap();
        let r = build("Bracket", &m, PROG, det, Some(ai()), Some(ReviewLevel::High), InputsMeta::default());
        let st = &r.setup;
        assert_eq!(st.program, "O1001");
        assert_eq!(st.material, "6061-T6 (AI)");
        assert_eq!(st.stock, "unknown - confirm at the machine");
        assert_eq!(st.units, "mm (G21)");
        assert_eq!(st.tools.len(), 2);
        assert_eq!(st.tools[0].description, "T1 D=10 FLAT END MILL");
        assert_eq!(st.tools[0].description_source, "program");
        assert_eq!(st.tools[0].holder_and_stickout, "ER32, 30 mm");
        assert_eq!(st.tools[0].speeds, "8000");
        assert_eq!(st.tools[0].feeds, "250 - 1000");
        assert_eq!(st.tools[0].min_z, "-3");
        assert_eq!(st.tools[1].description_source, "AI only - not found in the program");
        assert_eq!(st.offsets[0].name, "G54");
        assert_eq!(st.offsets[1].name, "H1 (T1)");
        assert_eq!(st.extents, "feed moves X 0 to 50, Y 0 to 0, Z -3 to 10 mm");
        assert_eq!(st.checklist.len(), STANDARD_CHECKS.len() + 1);
        // One mismatch caps the score at 70.
        assert_eq!(r.score.percent, 70);
        assert_eq!(r.inputs.gcode_sha256.len(), 64);
        assert_eq!(r.disclaimers.len(), 2);
    }

    #[test]
    fn renderings_escape_user_text() {
        let m = preset("vmc-fanuc").unwrap();
        let det = check(PROG, &m, &PostInput::None).unwrap();
        let r = build(
            "<script>x</script>|",
            &m,
            PROG,
            det,
            Some(ai()),
            Some(ReviewLevel::High),
            InputsMeta::default(),
        );
        let html = render_html(&r);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("Faces &lt;b&gt;and&lt;/b&gt; cuts"));
        assert!(html.contains("MISMATCH"));
        assert!(html.contains("No guarantee."));
        let mdx = render_markdown(&r);
        assert!(mdx.contains("&lt;script>x&lt;/script>\\|"));
        assert!(mdx.contains("- [ ] Check the slot length (AI)"));
    }

    #[test]
    fn deterministic_only_report() {
        let m = preset("vmc-fanuc").unwrap();
        let det = check(PROG, &m, &PostInput::None).unwrap();
        let r = build("", &m, PROG, det, None, None, InputsMeta::default());
        assert_eq!(r.title, "Untitled program");
        assert_eq!(r.score.percent, 84);
        assert_eq!(r.disclaimers.len(), 1);
        assert!(render_html(&r).contains("deterministic check only"));
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
