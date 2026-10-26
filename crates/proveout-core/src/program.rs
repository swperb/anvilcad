//! A plain reading of the program for the setup sheet: tools and what they are called,
//! offsets, speeds, feeds, work offsets, cycles and depths, each with the line it came
//! from.
//!
//! This scanner is deliberately tolerant. It does not judge the program (that is
//! [`crate::check`]'s job); it collects what is written, so it still reads programs the
//! strict verifier gives up on. Values are in the program's own units.

use anvil_cam::post::Units;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What one tool does in the program.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolUse {
    pub tool: u32,
    /// Line of the first change to this tool.
    pub first_line: usize,
    /// How many times the program changes to this tool.
    pub changes: usize,
    /// From a header comment such as `(T1 D=10. CR=0. - FLAT END MILL)`, or the comment
    /// on the tool-change line.
    pub description: Option<String>,
    pub length_offsets: Vec<u32>,
    pub diameter_offsets: Vec<u32>,
    pub speeds: Vec<f64>,
    /// Lowest and highest programmed feed while this tool is in the spindle.
    pub feed_range: Option<[f64; 2]>,
    /// Lowest absolute Z programmed while this tool is in the spindle.
    pub min_z: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgramSummary {
    pub lines: usize,
    /// Lines with something other than whitespace, `%` or comments.
    pub blocks: usize,
    /// From the first G20 or G21; `None` when the program does not say.
    pub units: Option<Units>,
    pub program_number: Option<String>,
    pub tools: Vec<ToolUse>,
    pub work_offsets: Vec<String>,
    pub canned_cycles: Vec<String>,
    /// Coolant codes seen (M7, M8, M9 and builder codes such as M88).
    pub coolant: Vec<String>,
    pub feed_range: Option<[f64; 2]>,
    pub speed_range: Option<[f64; 2]>,
    /// Lines with `#` variables, macro calls or subprogram calls.
    pub macro_lines: Vec<usize>,
    /// Every G and M code used, with its count.
    pub codes: BTreeMap<String, usize>,
}

/// Splits a line into code text and comments. `(…)` and `;` comments are understood.
pub fn split_comments(line: &str) -> (String, Vec<String>) {
    let mut code = String::new();
    let mut comments = Vec::new();
    let mut cur: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (&mut cur, c) {
            (Some(cm), ')') => {
                comments.push(cm.trim().to_string());
                cur = None;
            }
            (Some(cm), c) => cm.push(c),
            (None, '(') => cur = Some(String::new()),
            (None, ';') => {
                comments.push(chars.by_ref().collect::<String>().trim().to_string());
                break;
            }
            (None, c) => code.push(c),
        }
    }
    if let Some(cm) = cur {
        comments.push(cm.trim().to_string());
    }
    (code, comments)
}

/// Letter and number pairs of a block. Text that is not a word (variables,
/// expressions, keywords) is skipped.
pub fn words(code: &str) -> Vec<(char, String)> {
    let chars: Vec<char> =
        code.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_uppercase()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    let number = |c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+');
    while i < chars.len() {
        if !chars[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut run = i;
        while run < chars.len() && chars[run].is_ascii_alphabetic() {
            run += 1;
        }
        let mut j = run;
        while j < chars.len() && number(chars[j]) {
            j += 1;
        }
        // A single letter followed by a number is a word; keywords such as GOTO are not.
        if run == i + 1 && j > run {
            out.push((chars[i], chars[run..j].iter().collect()));
        }
        i = j;
    }
    out
}

/// "G00" to "G0", "G54.1" stays.
fn code_name(letter: char, num: &str) -> Option<String> {
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    let n: u32 = int.parse().ok()?;
    let frac = frac.trim_end_matches('0');
    Some(if frac.is_empty() { format!("{letter}{n}") } else { format!("{letter}{n}.{frac}") })
}

fn widen(r: &mut Option<[f64; 2]>, v: f64) {
    *r = Some(match *r {
        Some([lo, hi]) => [lo.min(v), hi.max(v)],
        None => [v, v],
    });
}

/// A header comment that names a tool: `T1 D=10 ...`, `T01 - 1/2 END MILL`, `TOOL 3 ...`.
fn header_tool(comment: &str) -> Option<(u32, String)> {
    let u = comment.trim();
    let rest = u
        .strip_prefix("TOOL")
        .or_else(|| u.strip_prefix("Tool"))
        .or_else(|| u.strip_prefix("tool"))
        .map(str::trim_start)
        .or_else(|| u.strip_prefix('T'))?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let after = &rest[digits.len()..];
    if after.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() && c != 'D' && c != 'd') {
        return None;
    }
    Some((digits.parse().ok()?, u.to_string()))
}

const MOTION_FREE: &[&str] = &["G28", "G30", "G53", "G10", "G92", "G52", "G65"];

/// Reads the program.
pub fn summarize(text: &str) -> ProgramSummary {
    let mut s = ProgramSummary::default();
    let mut tools: BTreeMap<u32, ToolUse> = BTreeMap::new();
    let mut header: BTreeMap<u32, String> = BTreeMap::new();
    let mut offsets = BTreeSet::new();
    let mut cycles = BTreeSet::new();
    let mut coolant = BTreeSet::new();
    let mut current: Option<u32> = None;
    let mut pending: Option<u32> = None;
    let mut absolute = true;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        s.lines = line_no;
        let (code, comments) = split_comments(raw);
        if current.is_none() {
            for c in &comments {
                if let Some((n, d)) = header_tool(c) {
                    header.entry(n).or_insert(d);
                }
            }
        }
        let code = code.trim();
        if code.is_empty() || code == "%" {
            continue;
        }
        s.blocks += 1;
        if code.contains('#') || code.to_ascii_uppercase().contains("IF") {
            s.macro_lines.push(line_no);
        }
        let ws = words(code);
        let mut gs = Vec::new();
        let mut ms = Vec::new();
        for (l, n) in &ws {
            if (*l == 'G' || *l == 'M')
                && let Some(c) = code_name(*l, n)
            {
                *s.codes.entry(c.clone()).or_insert(0) += 1;
                if *l == 'G' { gs.push(c) } else { ms.push(c) }
            }
        }
        let has = |c: &str| gs.iter().any(|g| g == c);
        let hasm = |c: &str| ms.iter().any(|g| g == c);
        let num = |l: char| ws.iter().find(|(w, _)| *w == l).and_then(|(_, n)| n.parse::<f64>().ok());
        if s.units.is_none() {
            if has("G20") {
                s.units = Some(Units::Inch);
            } else if has("G21") {
                s.units = Some(Units::Mm);
            }
        }
        if s.program_number.is_none()
            && s.blocks == 1
            && let Some((_, n)) = ws.iter().find(|(l, _)| *l == 'O')
        {
            s.program_number = Some(format!("O{n}"));
        }
        if has("G90") {
            absolute = true;
        }
        if has("G91") {
            absolute = false;
        }
        for g in &gs {
            let n: f64 = g[1..].parse().unwrap_or(0.0);
            if (54.0..=59.0).contains(&n) || (110.0..=129.0).contains(&n) || g == "G54.1" {
                let label = if g == "G54.1" {
                    format!("G54.1 P{}", num('P').map(|p| p as i64).unwrap_or(0))
                } else {
                    g.clone()
                };
                offsets.insert(label);
            }
            if matches!(
                g.as_str(),
                "G73" | "G74" | "G76" | "G81" | "G82" | "G83" | "G84" | "G85" | "G86" | "G87" | "G88" | "G89"
            ) {
                cycles.insert(g.clone());
            }
        }
        if ms.iter().any(|m| matches!(m.as_str(), "M98" | "M97" | "M99" | "M198")) || has("G65") || has("G66")
        {
            s.macro_lines.push(line_no);
        }
        for m in &ms {
            if matches!(m.as_str(), "M7" | "M8" | "M9" | "M88" | "M89" | "M50" | "M51") {
                coolant.insert(m.clone());
            }
        }
        if let Some(t) = num('T').filter(|t| *t >= 0.0 && t.fract() == 0.0 && *t < 1e5) {
            pending = Some(t as u32);
        }
        if hasm("M6")
            && let Some(t) = pending
        {
            current = Some(t);
            let e = tools.entry(t).or_insert_with(|| ToolUse {
                tool: t,
                first_line: line_no,
                ..ToolUse::default()
            });
            e.changes += 1;
            if e.description.is_none() {
                e.description = comments.iter().find(|c| !c.is_empty()).cloned();
            }
        }
        if let Some(f) = num('F').filter(|f| *f > 0.0) {
            widen(&mut s.feed_range, f);
            if let Some(t) = current.and_then(|t| tools.get_mut(&t)) {
                widen(&mut t.feed_range, f);
            }
        }
        if let Some(sp) = num('S').filter(|v| *v > 0.0) {
            widen(&mut s.speed_range, sp);
            if let Some(t) = current.and_then(|t| tools.get_mut(&t))
                && !t.speeds.contains(&sp)
            {
                t.speeds.push(sp);
            }
        }
        if let Some(t) = current.and_then(|t| tools.get_mut(&t)) {
            if let Some(h) = num('H').filter(|h| *h >= 0.0 && h.fract() == 0.0) {
                let h = h as u32;
                if !t.length_offsets.contains(&h) {
                    t.length_offsets.push(h);
                }
            }
            if let Some(d) = num('D').filter(|d| *d >= 0.0 && d.fract() == 0.0)
                && (has("G41") || has("G42"))
            {
                let d = d as u32;
                if !t.diameter_offsets.contains(&d) {
                    t.diameter_offsets.push(d);
                }
            }
            if absolute
                && !gs.iter().any(|g| MOTION_FREE.contains(&g.as_str()))
                && let Some(z) = num('Z')
            {
                t.min_z = Some(t.min_z.map_or(z, |m: f64| m.min(z)));
            }
        }
    }
    for (n, d) in header {
        if let Some(t) = tools.get_mut(&n) {
            t.description = Some(d);
        }
    }
    s.tools = tools.into_values().collect();
    s.tools.sort_by_key(|t| t.first_line);
    s.work_offsets = offsets.into_iter().collect();
    s.canned_cycles = cycles.into_iter().collect();
    s.coolant = coolant.into_iter().collect();
    s.macro_lines.dedup();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROG: &str = "%
O1001 (BRACKET)
(T1 D=10. CR=0. - ZMIN=-5. - FLAT END MILL)
(T2 D=6.8 - DRILL)
G90 G17 G21 G40 G49 G80
G54
T1 M6
S8000 M3
G43 H1 Z50. M8
G0 X-5. Y-5.
G1 Z-5. F300.
G1 X40. F1200.
G41 D1 X40. Y20.
G0 Z50.
T2 M06 (SECOND COMMENT)
S2500 M3
G43 H2 Z50.
G98 G83 X10. Y10. Z-12. R2. Q3. F150.
G80
G91 G28 Z0.
G90
M30
%";

    #[test]
    fn reads_tools_offsets_and_ranges() {
        let s = summarize(PROG);
        assert_eq!(s.lines, 23);
        assert_eq!(s.blocks, 19);
        assert_eq!(s.units, Some(Units::Mm));
        assert_eq!(s.program_number.as_deref(), Some("O1001"));
        assert_eq!(s.work_offsets, vec!["G54".to_string()]);
        assert_eq!(s.canned_cycles, vec!["G83".to_string()]);
        assert_eq!(s.coolant, vec!["M8".to_string()]);
        assert_eq!(s.feed_range, Some([150.0, 1200.0]));
        assert_eq!(s.speed_range, Some([2500.0, 8000.0]));
        assert_eq!(s.tools.len(), 2);
        let t1 = &s.tools[0];
        assert_eq!(t1.tool, 1);
        assert_eq!(t1.first_line, 7);
        assert_eq!(t1.description.as_deref(), Some("T1 D=10. CR=0. - ZMIN=-5. - FLAT END MILL"));
        assert_eq!(t1.length_offsets, vec![1]);
        assert_eq!(t1.diameter_offsets, vec![1]);
        assert_eq!(t1.speeds, vec![8000.0]);
        assert_eq!(t1.feed_range, Some([300.0, 1200.0]));
        assert_eq!(t1.min_z, Some(-5.0));
        let t2 = &s.tools[1];
        assert_eq!(t2.first_line, 15);
        assert_eq!(t2.description.as_deref(), Some("T2 D=6.8 - DRILL"));
        // The G28 Z0 in incremental mode is not a depth.
        assert_eq!(t2.min_z, Some(-12.0));
        assert_eq!(s.codes.get("M6"), Some(&2));
        assert_eq!(s.codes.get("G0"), Some(&2));
    }

    #[test]
    fn comment_splitting_and_words() {
        let (code, cm) = split_comments("G1 X1 (a) Y2 ; rest (x)");
        assert_eq!(code.trim(), "G1 X1  Y2");
        assert_eq!(cm, vec!["a".to_string(), "rest (x)".to_string()]);
        assert_eq!(words("g1x-.5 y2."), vec![('G', "1".into()), ('X', "-.5".into()), ('Y', "2.".into())]);
        // Keywords and variables are skipped, not misread.
        assert!(words("IF[#1GT2]GOTO5").is_empty());
    }

    #[test]
    fn tool_staged_before_the_change() {
        let s = summarize("T3\nM6\nS100 M3\nT4\nG1 Z-1 F10\nM30");
        assert_eq!(s.tools.len(), 1);
        assert_eq!(s.tools[0].tool, 3);
        assert_eq!(s.tools[0].min_z, Some(-1.0));
    }
}
