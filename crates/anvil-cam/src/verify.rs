//! The authoritative G-code verifier.
//!
//! It reads a program as text, with no help from the post that wrote it, and checks it
//! against the post's declared rules:
//!
//! - every character, word and code is one it understands and the post allows;
//! - dangerous constructs (offset and parameter writes, coordinate shifts, macro
//!   variables and calls, subprogram and external program calls, file output, override
//!   locks, soft limits off, controller settings, block delete) are rejected unless the
//!   post declares them with a reason; even then the lines are reported, and a program
//!   that uses them cannot be certified by simulation because their effect cannot be
//!   followed;
//! - values are in range (feed, spindle speed, tool and offset numbers) and every point
//!   of every move is inside the machine's travel, given where program zero is;
//! - arcs are consistent, feeds are set before cutting, the spindle runs while cutting,
//!   the length offset matches the tool, and the program ends.
//!
//! It also rebuilds the tool path the program describes (expanding canned cycles), so
//! the simulator can check it for gouges and collisions. Nothing a post or an AI says
//! can override a rejection here.

use crate::post::{Construct, PostDefinition, Units};
use crate::toolpath::{Item, MoveKind, SpindleDir, Toolpath, arc_points};
use anvil_kernel::{Vec2, Vec3, v2, v3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Largest program accepted (bytes).
pub const MAX_PROGRAM_BYTES: usize = 64 * 1024 * 1024;
/// Coordinates beyond this (mm) are absurd whatever the machine.
pub const ABSURD: f64 = 100_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    /// 1-based line number.
    pub line: usize,
    pub severity: Severity,
    pub message: String,
    pub construct: Option<Construct>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerifyOptions {
    /// Machine coordinates of program zero (the work offset in use).
    pub work_offset: Vec3,
    /// Tool position (program coordinates) when the program starts, if known.
    pub start: Option<Vec3>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerifyReport {
    pub issues: Vec<Issue>,
    /// Declared constructs the program uses.
    pub declared_used: BTreeSet<Construct>,
    /// Lines whose effect the verifier could not follow (declared constructs).
    pub opaque_lines: usize,
    /// The motion the program describes, in program coordinates.
    pub toolpath: Toolpath,
}

impl VerifyReport {
    /// No errors.
    pub fn passed(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
}

/// Codes the verifier can follow. Anything else is rejected even if a post allows it.
const UNDERSTOOD_G: &[&str] = &[
    "G0", "G1", "G2", "G3", "G4", "G17", "G20", "G21", "G28", "G40", "G43", "G49", "G54", "G55", "G56",
    "G57", "G58", "G59", "G61", "G64", "G73", "G80", "G81", "G82", "G83", "G90", "G91", "G91.1", "G94",
    "G98", "G99",
];
const UNDERSTOOD_M: &[&str] = &["M0", "M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9", "M30"];

/// Multi-letter words that write files or ports on common controls.
const FILE_WORDS: &[&str] = &["DPRNT", "BPRNT", "POPEN", "PCLOS", "OPEN", "CLOSE", "PRINT", "WRITE", "READ"];
/// Multi-letter words of macro programming.
const MACRO_WORDS: &[&str] = &[
    "IF", "GOTO", "WHILE", "DO", "END", "THEN", "SETVN", "CALL", "SUB", "ENDSUB", "RETURN", "REPEAT",
    "ENDWHILE", "ELSE",
];
/// LinuxCNC comments that write files or the log.
const FILE_COMMENTS: &[&str] =
    &["PRINT,", "DEBUG,", "LOGOPEN,", "LOGAPPEND,", "LOGCLOSE", "LOG,", "PROBEOPEN", "PROBECLOSE"];

/// The construct a code belongs to, if it is dangerous.
fn dangerous_code(code: &str, p_word: Option<f64>) -> Option<Construct> {
    match code {
        "G10" => Some(Construct::OffsetWrite),
        "G92" | "G92.1" | "G92.2" | "G92.3" | "G52" => Some(Construct::CoordinateShift),
        "G28.1" | "G30.1" => Some(Construct::ParameterWrite),
        "G65" | "G66" | "G66.1" | "G67" => Some(Construct::MacroCall),
        "G23" => Some(Construct::SoftLimitsOff),
        "M98" | "M99" | "M198" => Some(Construct::SubprogramCall),
        "M49" => Some(Construct::OverrideLock),
        "M50" | "M51" | "M53" if p_word == Some(0.0) => Some(Construct::OverrideLock),
        _ => {
            if let Some(n) = code.strip_prefix('M').and_then(|n| n.parse::<u32>().ok())
                && (100..=199).contains(&n)
            {
                return Some(Construct::UserMCode);
            }
            None
        }
    }
}

/// Normalises "G00" to "G0", "G091.1" to "G91.1".
fn normalise(letter: char, v: &str) -> Option<String> {
    let (int, frac) = match v.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (v, None),
    };
    if int.is_empty() || !int.chars().all(|c| c.is_ascii_digit()) || int.len() > 4 {
        return None;
    }
    let n: u32 = int.parse().ok()?;
    match frac {
        None | Some("") => Some(format!("{letter}{n}")),
        Some(f) if f.chars().all(|c| c.is_ascii_digit()) && f.len() <= 2 => {
            let f = f.trim_end_matches('0');
            if f.is_empty() { Some(format!("{letter}{n}")) } else { Some(format!("{letter}{n}.{f}")) }
        }
        _ => None,
    }
}

struct State {
    pos: [Option<f64>; 3],
    absolute: bool,
    motion: Option<String>,
    feed: Option<f64>,
    spindle_on: bool,
    tool: Option<u32>,
    length_offset: Option<u32>,
    cycle_return_initial: bool,
    cycle: Option<CycleState>,
    ended: bool,
    first_block: bool,
    assumed_start: bool,
}

#[derive(Clone)]
struct CycleState {
    kind: String,
    z: f64,
    r: f64,
    q: Option<f64>,
    initial_z: f64,
}

struct Ctx<'a> {
    def: &'a PostDefinition,
    opts: &'a VerifyOptions,
    rep: VerifyReport,
    line: usize,
    scale: f64,
}

impl Ctx<'_> {
    fn err(&mut self, msg: impl Into<String>) {
        self.rep.issues.push(Issue {
            line: self.line,
            severity: Severity::Error,
            message: msg.into(),
            construct: None,
        });
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.rep.issues.push(Issue {
            line: self.line,
            severity: Severity::Warning,
            message: msg.into(),
            construct: None,
        });
    }

    /// A dangerous construct: an error unless declared (and, for codes, allowed); a
    /// warning and an opaque line otherwise. Returns true when the line may continue.
    fn construct(&mut self, c: Construct, what: &str, allowed: bool) -> bool {
        if self.def.declares(c) && allowed {
            self.rep.issues.push(Issue {
                line: self.line,
                severity: Severity::Warning,
                message: format!("{what}: declared {} (needs human review)", c.label()),
                construct: Some(c),
            });
            self.rep.declared_used.insert(c);
            self.rep.opaque_lines += 1;
            false
        } else {
            self.rep.issues.push(Issue {
                line: self.line,
                severity: Severity::Error,
                message: if self.def.declares(c) {
                    format!("{what}: {} is declared but the code is not in the allow list", c.label())
                } else {
                    format!("{what}: {} is not allowed (the post does not declare it)", c.label())
                },
                construct: Some(c),
            });
            false
        }
    }

    fn check_travel(&mut self, p: Vec3) {
        let m = &self.def.machine;
        let w = p + self.opts.work_offset;
        let tol = 1e-6;
        let out = |v: f64, r: [f64; 2]| v < r[0] - tol || v > r[1] + tol;
        if p.x.abs() > ABSURD || p.y.abs() > ABSURD || p.z.abs() > ABSURD {
            self.err(format!("absurd coordinate ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z));
        } else if out(w.x, m.x) || out(w.y, m.y) || out(w.z, m.z) {
            self.err(format!(
                "move to ({:.3}, {:.3}, {:.3}) is outside the machine's travel (machine ({:.3}, {:.3}, {:.3}))",
                p.x, p.y, p.z, w.x, w.y, w.z
            ));
        }
    }
}

/// One word of a block.
#[derive(Clone, Debug)]
struct Word {
    letter: char,
    text: String,
    value: f64,
}

/// Verifies `text` against `def`.
pub fn verify(text: &str, def: &PostDefinition, opts: &VerifyOptions) -> VerifyReport {
    let mut ctx = Ctx {
        def,
        opts,
        rep: VerifyReport::default(),
        line: 0,
        scale: if def.format.units == Units::Inch { 25.4 } else { 1.0 },
    };
    if text.len() > MAX_PROGRAM_BYTES {
        ctx.err("the program is too large to verify");
        return ctx.rep;
    }
    if let Err(e) = def.check() {
        ctx.err(format!("the post definition is not valid: {e}"));
        return ctx.rep;
    }
    let mut st = State {
        pos: match opts.start {
            Some(s) => [Some(s.x), Some(s.y), Some(s.z)],
            None => [None, None, None],
        },
        absolute: true,
        motion: None,
        feed: None,
        spindle_on: false,
        tool: None,
        length_offset: None,
        cycle_return_initial: true,
        cycle: None,
        ended: false,
        first_block: true,
        assumed_start: false,
    };
    ctx.rep.toolpath.start = opts.start;
    let allow_g: BTreeSet<&str> = def.allow.g.iter().map(String::as_str).collect();
    let allow_m: BTreeSet<&str> = def.allow.m.iter().map(String::as_str).collect();
    for (idx, raw) in text.lines().enumerate() {
        ctx.line = idx + 1;
        if st.ended {
            if !raw.trim().is_empty() && raw.trim() != "%" {
                ctx.err("text after the end of the program");
            }
            continue;
        }
        if raw.len() > def.format.max_line_length {
            ctx.err(format!("line longer than {} characters", def.format.max_line_length));
            continue;
        }
        if raw.chars().any(|c| !(c == '\t' || (' '..='~').contains(&c))) {
            ctx.err("line has characters other than printable ASCII");
            continue;
        }
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == "%" {
            continue;
        }
        // Comments.
        let mut code = String::new();
        let mut comments: Vec<String> = Vec::new();
        let mut in_comment: Option<String> = None;
        let mut bad = false;
        for c in line.chars() {
            match (&mut in_comment, c) {
                (Some(_), '(') => {
                    ctx.err("nested comment");
                    bad = true;
                    break;
                }
                (Some(cm), ')') => {
                    comments.push(std::mem::take(cm));
                    in_comment = None;
                }
                (Some(cm), c) => cm.push(c),
                (None, '(') => in_comment = Some(String::new()),
                (None, ')') => {
                    ctx.err("unmatched `)`");
                    bad = true;
                    break;
                }
                (None, ';') => {
                    in_comment = Some(String::new());
                    // A semicolon comment runs to the end of the line.
                    break;
                }
                (None, c) => code.push(c),
            }
        }
        if bad {
            continue;
        }
        if let Some(cm) = in_comment {
            if line.contains(';') {
                let after = line.split_once(';').map(|x| x.1).unwrap_or("");
                comments.push(after.to_string());
            } else {
                ctx.err("comment not closed");
                let _ = cm;
                continue;
            }
        }
        let mut opaque = false;
        for cm in &comments {
            let u = cm.trim().to_ascii_uppercase();
            if FILE_COMMENTS.iter().any(|k| u.starts_with(k)) {
                ctx.construct(Construct::FileOutput, &format!("comment `({cm})`"), true);
                opaque = true;
            }
        }
        let code = code.trim().to_ascii_uppercase();
        if code.is_empty() {
            continue;
        }
        if code.contains('$') {
            ctx.construct(Construct::ControllerSettings, &format!("`{code}`"), true);
            continue;
        }
        if code.contains('/') {
            ctx.construct(Construct::BlockDelete, "block delete `/`", true);
            continue;
        }
        if code.contains('#') {
            ctx.construct(Construct::MacroVariables, "`#` variable", true);
            continue;
        }
        if let Some(c) = code.chars().find(|c| !(c.is_ascii_alphanumeric() || " .+-\t".contains(*c))) {
            ctx.err(format!(
                "character `{c}` is not part of plain G-code (expressions and variables are not accepted)"
            ));
            continue;
        }
        if opaque {
            continue;
        }
        // Words.
        let mut words: Vec<Word> = Vec::new();
        let chars: Vec<char> = code.chars().filter(|c| !c.is_whitespace()).collect();
        let mut i = 0usize;
        let mut ok = true;
        while i < chars.len() {
            let letter = chars[i];
            if !letter.is_ascii_alphabetic() {
                ctx.err(format!("expected a letter, found `{letter}`"));
                ok = false;
                break;
            }
            // Multi-letter words.
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j - i > 1 {
                let w: String = chars[i..j].iter().collect();
                if FILE_WORDS.iter().any(|k| w.starts_with(k)) {
                    ctx.construct(Construct::FileOutput, &format!("`{w}`"), true);
                } else if MACRO_WORDS.contains(&w.as_str()) {
                    ctx.construct(Construct::MacroVariables, &format!("`{w}`"), true);
                } else {
                    ctx.err(format!("`{w}` is not a G-code word"));
                }
                ok = false;
                break;
            }
            let mut k = i + 1;
            while k < chars.len()
                && (chars[k].is_ascii_digit() || chars[k] == '.' || chars[k] == '-' || chars[k] == '+')
            {
                k += 1;
            }
            let num: String = chars[i + 1..k].iter().collect();
            let value: f64 = match num.parse() {
                Ok(v) if f64::is_finite(v) => v,
                _ => {
                    ctx.err(format!("`{letter}` has no valid number"));
                    ok = false;
                    break;
                }
            };
            if !def.allow.words.contains(letter) {
                ctx.err(format!("word `{letter}` is not allowed by this post"));
                ok = false;
            }
            words.push(Word { letter, text: num, value });
            i = k;
        }
        // A leading line number is only a label.
        if words.first().is_some_and(|w| w.letter == 'N') {
            let n = words.remove(0);
            if n.value < 0.0 || n.value.fract() != 0.0 {
                ctx.err("line number is not a whole number");
            }
        }
        if words.iter().any(|w| w.letter == 'N') {
            ctx.err("line number in the middle of a block");
            ok = false;
        }
        if !ok {
            // Still report any dangerous code on the line.
            for w in words.iter().filter(|w| w.letter == 'G' || w.letter == 'M') {
                if let Some(c) =
                    normalise(w.letter, &w.text).and_then(|c| dangerous_code(&c, None).map(|d| (c, d)))
                {
                    ctx.construct(c.1, &format!("`{}`", c.0), false);
                }
            }
            continue;
        }
        block(&mut ctx, &mut st, &words, &allow_g, &allow_m);
    }
    if !st.ended {
        ctx.line = text.lines().count();
        ctx.err("the program does not end with M2 or M30");
    }
    if st.assumed_start {
        ctx.line = 0;
        ctx.warn(
            "the start position was not given; unknown axes were taken as not moving on the first moves",
        );
    }
    ctx.rep
}

fn block(ctx: &mut Ctx, st: &mut State, words: &[Word], allow_g: &BTreeSet<&str>, allow_m: &BTreeSet<&str>) {
    let get = |l: char| words.iter().find(|w| w.letter == l);
    // Program number only on the first block.
    if let Some(o) = get('O') {
        if !st.first_block || words.len() > 1 {
            ctx.construct(Construct::SubprogramCall, &format!("`O{}`", o.text), true);
            return;
        }
        st.first_block = false;
        return;
    }
    st.first_block = false;
    for l in "XYZIJKFSTHDRPQ".chars() {
        if words.iter().filter(|w| w.letter == l).count() > 1 {
            ctx.err(format!("word `{l}` appears twice in one block"));
            return;
        }
    }
    let p_word = get('P').map(|w| w.value);
    let mut gs: Vec<String> = Vec::new();
    let mut ms: Vec<String> = Vec::new();
    for w in words {
        if w.letter == 'G' || w.letter == 'M' {
            let Some(c) = normalise(w.letter, &w.text) else {
                ctx.err(format!("`{}{}` is not a valid code", w.letter, w.text));
                return;
            };
            let allowed =
                if w.letter == 'G' { allow_g.contains(c.as_str()) } else { allow_m.contains(c.as_str()) };
            if let Some(con) = dangerous_code(&c, p_word) {
                ctx.construct(con, &format!("`{c}`"), allowed);
                return;
            }
            if !allowed {
                ctx.err(format!("`{c}` is not in this post's allow list"));
                return;
            }
            let understood = if w.letter == 'G' {
                UNDERSTOOD_G.contains(&c.as_str())
            } else {
                UNDERSTOOD_M.contains(&c.as_str())
            };
            if !understood {
                ctx.err(format!("`{c}` cannot be followed by the verifier, so it is not accepted"));
                return;
            }
            if w.letter == 'G' { gs.push(c) } else { ms.push(c) }
        }
    }
    let scale = ctx.scale;
    let m = ctx.def.machine.clone();
    // Values.
    if let Some(f) = get('F') {
        let f = f.value * scale;
        if !(f > 0.0 && f <= m.max_feed + 1e-9) {
            ctx.err(format!("feed {f:.1} mm/min is outside 0 to {}", m.max_feed));
        } else {
            st.feed = Some(f);
        }
    }
    let has = |c: &str| gs.iter().any(|g| g == c);
    let hasm = |c: &str| ms.iter().any(|g| g == c);
    if has("G20") && ctx.def.format.units == Units::Mm || has("G21") && ctx.def.format.units == Units::Inch {
        ctx.err("units code does not match the post's units");
    }
    if has("G90") {
        st.absolute = true;
    }
    if has("G91") {
        st.absolute = false;
    }
    if has("G98") {
        st.cycle_return_initial = true;
    }
    if has("G99") {
        st.cycle_return_initial = false;
    }
    if let Some(t) = get('T')
        && (t.value < 0.0 || t.value.fract() != 0.0 || t.value > m.max_tool as f64)
    {
        ctx.err(format!("tool number {} is not valid for this machine", t.text));
    }
    if let Some(s) = get('S')
        && !(s.value >= m.min_rpm && s.value <= m.max_rpm)
    {
        ctx.err(format!("spindle speed {} is outside {} to {}", s.text, m.min_rpm, m.max_rpm));
    }
    for l in ['H', 'D'] {
        if let Some(h) = get(l)
            && (h.value < 0.0 || h.value.fract() != 0.0 || h.value > m.max_tool as f64)
        {
            ctx.err(format!("offset number {l}{} is not valid", h.text));
        }
    }
    for l in ['P', 'Q', 'R', 'I', 'J', 'K'] {
        if let Some(w) = get(l)
            && w.value.abs() > ABSURD
        {
            ctx.err(format!("absurd value {l}{}", w.text));
            return;
        }
    }
    // M codes.
    if hasm("M6") {
        match get('T') {
            Some(t) => {
                let n = t.value as u32;
                st.tool = Some(n);
                st.length_offset = None;
                st.spindle_on = false;
                ctx.rep.toolpath.push(Item::ToolChange { tool: n, name: String::new() });
            }
            None => ctx.err("tool change without a tool number"),
        }
    }
    if hasm("M3") || hasm("M4") {
        match get('S').map(|s| s.value) {
            Some(s) if s > 0.0 => {
                st.spindle_on = true;
                let dir = if hasm("M3") { SpindleDir::Clockwise } else { SpindleDir::CounterClockwise };
                ctx.rep.toolpath.push(Item::Spindle { rpm: s, dir });
            }
            _ => ctx.err("spindle started without a speed in the same block"),
        }
    }
    if hasm("M0") || hasm("M1") {
        ctx.rep.toolpath.push(Item::OptionalStop);
    }
    if hasm("M5") {
        st.spindle_on = false;
        ctx.rep.toolpath.push(Item::SpindleStop);
    }
    if hasm("M2") || hasm("M30") {
        st.ended = true;
        st.spindle_on = false;
    }
    if has("G43") {
        match get('H') {
            Some(h) => {
                let n = h.value as u32;
                if st.tool.is_some_and(|t| t != n) {
                    ctx.err(format!(
                        "length offset H{n} does not match the tool in the spindle (T{})",
                        st.tool.unwrap_or(0)
                    ));
                }
                st.length_offset = Some(n);
            }
            None => ctx.err("G43 without an H word"),
        }
    }
    if has("G49") {
        st.length_offset = None;
    }
    if has("G4") {
        if p_word.is_none_or(|p| p < 0.0) {
            ctx.err("dwell without a valid P");
        }
        return;
    }
    if has("G28") {
        // Through the intermediate point to the machine's home (top of travel).
        let mut inter = current(st);
        for (a, l) in ['X', 'Y', 'Z'].iter().enumerate() {
            if let Some(w) = get(*l) {
                let v = w.value * scale;
                let base = inter[a].unwrap_or(0.0);
                inter[a] = Some(if st.absolute { v } else { base + v });
            }
        }
        let home = [m.x[1], m.y[1], m.z[1]];
        let off = ctx.opts.work_offset;
        let offs = [off.x, off.y, off.z];
        let mut home_pt = current(st);
        for (a, l) in ['X', 'Y', 'Z'].iter().enumerate() {
            if get(*l).is_some() {
                home_pt[a] = Some(home[a] - offs[a]);
            }
        }
        rapid_to(ctx, st, inter);
        rapid_to(ctx, st, home_pt);
        st.motion = Some("G0".into());
        return;
    }
    if has("G80") {
        st.cycle = None;
        st.motion = None;
    }
    // Motion.
    let motion_code = gs
        .iter()
        .find(|g| ["G0", "G1", "G2", "G3", "G73", "G81", "G82", "G83"].contains(&g.as_str()))
        .cloned();
    if let Some(mc) = &motion_code {
        st.motion = Some(mc.clone());
        if !["G73", "G81", "G82", "G83"].contains(&mc.as_str()) {
            st.cycle = None;
        }
    }
    let axes: Vec<(usize, f64)> = ['X', 'Y', 'Z']
        .iter()
        .enumerate()
        .filter_map(|(a, l)| get(*l).map(|w| (a, w.value * scale)))
        .collect();
    let Some(mode) = st.motion.clone() else {
        if !axes.is_empty() {
            ctx.err("coordinates without a motion mode");
        }
        return;
    };
    if axes.is_empty() && motion_code.is_none() {
        return;
    }
    let mut target = current(st);
    for &(a, v) in &axes {
        target[a] = Some(if st.absolute { v } else { target[a].unwrap_or(0.0) + v });
    }
    match mode.as_str() {
        "G0" => rapid_to(ctx, st, target),
        "G1" => {
            if feed_ready(ctx, st) {
                feed_to(ctx, st, target, None);
            }
        }
        "G2" | "G3" => {
            if get('R').is_some() {
                ctx.err("radius-format arcs are not accepted; use I and J");
                return;
            }
            if get('K').is_some() {
                ctx.err("K on an XY-plane arc is not accepted");
                return;
            }
            let (Some(i), Some(j)) = (get('I'), get('J')) else {
                ctx.err("arc without I and J");
                return;
            };
            if !feed_ready(ctx, st) {
                return;
            }
            let start = resolve(st, current(st));
            let center = v2(start.x + i.value * scale, start.y + j.value * scale);
            let end = resolve(st, target);
            let r0 = (v2(start.x, start.y) - center).length();
            let r1 = (v2(end.x, end.y) - center).length();
            if r0 < 1e-4 || (r0 - r1).abs() > 0.005 + 1e-3 * r0 {
                ctx.err(format!(
                    "arc end is not on the arc (radius {r0:.4} at the start, {r1:.4} at the end)"
                ));
                return;
            }
            feed_to(ctx, st, target, Some((center, mode == "G3")));
        }
        cycle => drill_cycle(ctx, st, cycle, words, motion_code.is_some(), target),
    }
}

fn current(st: &State) -> [Option<f64>; 3] {
    st.pos
}

/// Unknown axes are taken as not moving.
fn resolve(st: &State, p: [Option<f64>; 3]) -> Vec3 {
    let c = |a: usize| p[a].or(st.pos[a]).unwrap_or(0.0);
    v3(c(0), c(1), c(2))
}

fn mark_assumed(st: &mut State, target: [Option<f64>; 3]) -> [Option<f64>; 3] {
    let mut start = st.pos;
    for a in 0..3 {
        if start[a].is_none() {
            st.assumed_start = true;
            start[a] = target[a];
        }
    }
    start
}

fn rapid_to(ctx: &mut Ctx, st: &mut State, target: [Option<f64>; 3]) {
    let start = mark_assumed(st, target);
    st.pos = start;
    let to = resolve(st, target);
    ctx.check_travel(to);
    if ctx.rep.toolpath.start.is_none() && ctx.rep.toolpath.items.iter().all(|i| !matches!(i, Item::Move(_)))
    {
        ctx.rep.toolpath.start = Some(resolve(st, start));
    }
    ctx.rep.toolpath.rapid(to);
    st.pos = [target[0].or(start[0]), target[1].or(start[1]), target[2].or(start[2])];
}

fn feed_ready(ctx: &mut Ctx, st: &State) -> bool {
    if st.feed.is_none() {
        ctx.err("feed move before a feed rate is set");
        return false;
    }
    if !st.spindle_on {
        ctx.err("feed move with the spindle stopped");
        return false;
    }
    if ctx.def.capabilities.tool_changer && st.length_offset.is_none() {
        ctx.err("feed move without a tool length offset (G43 H)");
        return false;
    }
    true
}

fn feed_to(ctx: &mut Ctx, st: &mut State, target: [Option<f64>; 3], arc: Option<(Vec2, bool)>) {
    let start = mark_assumed(st, target);
    st.pos = start;
    let from = resolve(st, start);
    let to = resolve(st, target);
    let feed = st.feed.unwrap_or(1.0);
    match arc {
        None => {
            ctx.check_travel(to);
            ctx.rep.toolpath.line(to, MoveKind::Cut, feed);
        }
        Some((center, ccw)) => {
            for p in arc_points(from, to, center, ccw, 0.01) {
                ctx.check_travel(p);
            }
            ctx.rep.toolpath.arc(to, center, ccw, MoveKind::Cut, feed);
        }
    }
    st.pos = [Some(to.x), Some(to.y), Some(to.z)];
}

fn drill_cycle(
    ctx: &mut Ctx,
    st: &mut State,
    kind: &str,
    words: &[Word],
    new: bool,
    target: [Option<f64>; 3],
) {
    let scale = ctx.scale;
    let get = |l: char| words.iter().find(|w| w.letter == l).map(|w| w.value * scale);
    if !feed_ready(ctx, st) {
        return;
    }
    if new || st.cycle.is_none() {
        let (Some(z), Some(r)) = (get('Z'), get('R')) else {
            ctx.err("drilling cycle without Z and R");
            return;
        };
        if !st.absolute {
            ctx.err("drilling cycles are only accepted in absolute mode (G90)");
            return;
        }
        let q = get('Q');
        if matches!(kind, "G73" | "G83") && q.is_none_or(|q| q <= 0.0) {
            ctx.err("peck cycle without a positive Q");
            return;
        }
        if kind == "G82" && words.iter().find(|w| w.letter == 'P').is_none_or(|p| p.value < 0.0) {
            ctx.err("G82 without a dwell P");
            return;
        }
        let initial = st.pos[2].unwrap_or(r);
        if r <= z || initial < r {
            ctx.err("drilling cycle with R below Z or above the starting height");
            return;
        }
        st.cycle = Some(CycleState { kind: kind.to_string(), z, r, q, initial_z: initial });
    }
    let Some(c) = st.cycle.clone() else { return };
    let x = target[0].or(st.pos[0]);
    let y = target[1].or(st.pos[1]);
    let (Some(x), Some(y)) = (x, y) else {
        ctx.err("drilling cycle position unknown");
        return;
    };
    let feed = st.feed.unwrap_or(1.0);
    let here_z = st.pos[2].unwrap_or(c.initial_z);
    if ctx.rep.toolpath.start.is_none() {
        ctx.rep.toolpath.start = Some(v3(x, y, here_z));
    }
    let tp = &mut ctx.rep.toolpath;
    tp.rapid(v3(x, y, here_z));
    tp.rapid(v3(x, y, c.r));
    match (c.kind.as_str(), c.q) {
        ("G73", Some(q)) | ("G83", Some(q)) => {
            let full = c.kind == "G83";
            let mut depth = c.r;
            let mut first = true;
            for _ in 0..100_000 {
                let next = (depth - q).max(c.z);
                if !first && full {
                    tp.rapid(v3(x, y, depth + 0.5));
                }
                tp.line(v3(x, y, next), MoveKind::Entry, feed);
                first = false;
                depth = next;
                if depth <= c.z + 1e-9 {
                    break;
                }
                tp.rapid(v3(x, y, if full { c.r } else { depth + 0.5 }));
            }
        }
        _ => tp.line(v3(x, y, c.z), MoveKind::Entry, feed),
    }
    let back = if st.cycle_return_initial { c.initial_z } else { c.r };
    tp.rapid(v3(x, y, back));
    for z in [c.r, c.z, back] {
        ctx.check_travel(v3(x, y, z));
    }
    st.pos = [Some(x), Some(y), Some(back)];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post::{ProgramSettings, bundled, generate};
    use crate::toolpath::{Coolant, CycleKind, DrillCycle};

    fn fanuc() -> PostDefinition {
        PostDefinition::from_toml(bundled()[0].1).unwrap()
    }

    fn opts() -> VerifyOptions {
        VerifyOptions { work_offset: v3(-400.0, -225.0, -300.0), start: None }
    }

    /// A small program with every kind of item.
    pub(crate) fn sample_path() -> Toolpath {
        let mut tp = Toolpath::new();
        tp.push(Item::Comment("sample".into()));
        tp.push(Item::ToolChange { tool: 2, name: "8 mm end mill".into() });
        tp.push(Item::Spindle { rpm: 8000.0, dir: SpindleDir::Clockwise });
        tp.push(Item::Coolant(Coolant::Flood));
        tp.rapid(v3(10.0, 10.0, 25.0));
        tp.rapid(v3(10.0, 10.0, 2.0));
        tp.line(v3(10.0, 10.0, -2.0), MoveKind::Entry, 300.0);
        tp.line(v3(40.0, 10.0, -2.0), MoveKind::Cut, 1200.0);
        tp.arc(v3(50.0, 20.0, -2.0), v2(40.0, 20.0), true, MoveKind::Cut, 1200.0);
        tp.arc(v3(50.0, 20.0, -4.0), v2(45.0, 20.0), true, MoveKind::Entry, 400.0);
        tp.rapid(v3(50.0, 20.0, 25.0));
        tp.push(Item::ToolChange { tool: 3, name: "6 mm drill".into() });
        tp.push(Item::Spindle { rpm: 2500.0, dir: SpindleDir::Clockwise });
        tp.push(Item::Cycle(DrillCycle {
            kind: CycleKind::Peck { peck: 6.0 },
            points: vec![v2(20.0, 30.0), v2(30.0, 30.0)],
            top: 0.0,
            r_plane: 2.0,
            z: -15.0,
            clearance: 25.0,
            feed: 150.0,
        }));
        tp.push(Item::Coolant(Coolant::Off));
        tp.push(Item::SpindleStop);
        tp
    }

    #[test]
    fn generated_programs_pass_for_every_bundled_post() {
        let tp = sample_path();
        for (name, text) in bundled() {
            let def = PostDefinition::from_toml(text).unwrap();
            let prog = generate(&def, &tp, &ProgramSettings::default()).unwrap();
            // Program zero in the middle of each machine's travel.
            let m = &def.machine;
            let o = v3((m.x[0] + m.x[1]) * 0.5, (m.y[0] + m.y[1]) * 0.5, m.z[1] - 40.0);
            let rep = verify(&prog.text, &def, &VerifyOptions { work_offset: o, start: None });
            assert!(rep.passed(), "{name}:\n{}\n{:?}", prog.text, rep.errors().collect::<Vec<_>>());
            // The rebuilt path reaches the same points.
            let want: Vec<Vec3> = tp.expanded().moves().map(|m| m.to).collect();
            let got: Vec<Vec3> = rep.toolpath.moves().map(|m| m.to).collect();
            for w in &want {
                assert!(got.iter().any(|g| g.distance(*w) < 2e-3), "{name}: {w:?} missing");
            }
        }
    }

    fn program_with(extra_start: &str) -> String {
        let mut def = fanuc();
        def.blocks.program_start.push(extra_start.into());
        generate(&def, &sample_path(), &ProgramSettings::default()).unwrap().text
    }

    #[test]
    fn dangerous_lines_are_rejected() {
        let def = fanuc();
        for (line, c) in [
            ("G10 L2 P1 X0 Y0 Z0", Some(Construct::OffsetWrite)),
            ("#3000=1 (ALARM)", Some(Construct::MacroVariables)),
            ("#5221=100.", Some(Construct::MacroVariables)),
            ("G92 X0 Y0", Some(Construct::CoordinateShift)),
            ("M98 P9000", Some(Construct::SubprogramCall)),
            ("G65 P9001 A1.", Some(Construct::MacroCall)),
            ("POPEN", Some(Construct::FileOutput)),
            ("DPRNT[X#5021]", None),
            ("G23", Some(Construct::SoftLimitsOff)),
            ("$20=0", Some(Construct::ControllerSettings)),
            ("(LOGOPEN,/tmp/x.txt)", Some(Construct::FileOutput)),
            ("M120", Some(Construct::UserMCode)),
            ("G18", None),
            ("X1.0E5", None),
            ("G0 X100000000", None),
        ] {
            let text = program_with(line);
            let rep = verify(&text, &def, &opts());
            assert!(!rep.passed(), "accepted `{line}`");
            if let Some(c) = c {
                assert!(rep.errors().any(|i| i.construct == Some(c)), "`{line}`: {:?}", rep.issues);
            }
        }
    }

    #[test]
    fn declared_constructs_are_warnings_but_opaque() {
        let mut def = fanuc();
        def.allow.g.push("G10".into());
        def.allow.words.push('L');
        def.declared.push(crate::post::Declared {
            construct: Construct::OffsetWrite,
            reason: "sets the G54 offset for a fixture plate probed by the operator".into(),
        });
        def.blocks.program_start.push("G10 L2 P1 X-400. Y-225. Z-300.".into());
        let text = generate(&def, &sample_path(), &ProgramSettings::default()).unwrap().text;
        let rep = verify(&text, &def, &opts());
        assert!(rep.passed(), "{:?}", rep.issues);
        assert!(rep.declared_used.contains(&Construct::OffsetWrite));
        assert_eq!(rep.opaque_lines, 1);
        // Declared but not allowed is still an error.
        let mut def2 = def.clone();
        def2.allow.g.retain(|g| g != "G10");
        assert!(!verify(&text, &def2, &opts()).passed());
    }

    #[test]
    fn limits_and_consistency() {
        let def = fanuc();
        let base = generate(&def, &sample_path(), &ProgramSettings::default()).unwrap().text;
        assert!(verify(&base, &def, &opts()).passed());
        // Beyond travel: program zero placed so the part is off the table.
        let far = VerifyOptions { work_offset: v3(-5.0, -225.0, -300.0), start: None };
        assert!(verify(&base, &def, &far).errors().any(|i| i.message.contains("travel")));
        // Feed above the machine's maximum.
        let fast = base.replacen("F1200", "F99999", 1);
        assert!(!verify(&fast, &def, &opts()).passed());
        // Wrong length offset.
        let wrong_h = base.replacen("G43 H2", "G43 H7", 1);
        assert!(verify(&wrong_h, &def, &opts()).errors().any(|i| i.message.contains("length offset")));
        // An arc whose end is not on the circle.
        let bad_arc = base.replacen("X50.000 Y20.000 I0.000 J10.000", "X50.000 Y21.000 I0.000 J10.000", 1);
        assert_ne!(bad_arc, base);
        assert!(verify(&bad_arc, &def, &opts()).errors().any(|i| i.message.contains("arc")));
        // No end.
        let cut = base.replace("M30", "");
        assert!(verify(&cut, &def, &opts()).errors().any(|i| i.message.contains("end")));
    }

    #[test]
    fn garbage_never_panics() {
        // A small linear congruential generator keeps the test deterministic.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let alphabet: Vec<char> = "GMXYZIJKFSTHNOPQR0123456789.-+ ()#[]=;%$/\t\u{e9}\u{0}".chars().collect();
        let def = fanuc();
        for _ in 0..300 {
            let mut text = String::new();
            for _ in 0..(next() % 40) {
                for _ in 0..(next() % 30) {
                    text.push(alphabet[next() % alphabet.len()]);
                }
                text.push('\n');
            }
            let _ = verify(&text, &def, &opts());
            let _ = PostDefinition::from_toml(&text);
        }
        // Valid start, then nonsense numbers.
        for line in [
            "G1 X1e400",
            "G2 X1 Y1 I0 J0",
            "G83 X0 Y0 Z-5 R2 Q0",
            "G81 X0 Y0 Z5 R2",
            "T-1 M6",
            "S-100 M3",
            "N-5 G0",
        ] {
            let text = format!("O1000\nG90 G21 G54\n{line}\nM30\n");
            assert!(!verify(&text, &def, &opts()).passed(), "{line}");
        }
    }
}
