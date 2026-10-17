//! Post processors as data.
//!
//! A post is a TOML document (see `crates/anvil-cam/posts/` and `docs/cam/POSTS.md`):
//! number formats, what the machine supports, its limits, the G and M codes and address
//! words it may emit, any dangerous construct it needs (declared with a reason), and a
//! few text templates for the blocks around the motion (program start and end, tool
//! change, spindle, coolant). It holds no code. The template language has one feature:
//! `{name}` is replaced by a value the generator provides (`{{` and `}}` are literal
//! braces). There are no conditions, loops, file, network or process access, and every
//! expansion is bounded in size, so loading or running a post cannot run anything on the
//! user's machine.
//!
//! The generator writes the motion itself (modal G codes, changed axes only, arcs with
//! incremental centres, canned drilling cycles when the post allows them). Whatever a
//! post produces is then checked by [`crate::verify`], which does not trust the post.

use crate::error::{CamError, Result};
use crate::toolpath::{
    Coolant, CycleKind, DrillCycle, Item, Motion, Move, SpindleDir, Toolpath, arc_points, arc_sweep,
};
use anvil_kernel::{Vec3, v2, v3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Largest post definition accepted (bytes).
pub const MAX_POST_BYTES: usize = 64 * 1024;
/// Largest expansion of one template line (bytes).
pub const MAX_TEMPLATE_LINE: usize = 256;
/// Most lines in one template block.
pub const MAX_TEMPLATE_LINES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    Mm,
    Inch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentStyle {
    /// `(text)`
    Parens,
    /// `; text`
    Semicolon,
    /// Comments are dropped.
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// SPDX licence expression of the post definition itself.
    pub license: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    pub units: Units,
    /// Decimal places for coordinates.
    pub decimals: u32,
    /// Decimal places for feeds.
    #[serde(default)]
    pub feed_decimals: u32,
    /// Number blocks with N words, starting here, in steps of `line_step` (0 = off).
    #[serde(default)]
    pub line_start: u32,
    #[serde(default)]
    pub line_step: u32,
    pub comments: CommentStyle,
    /// Longest line the controller accepts.
    #[serde(default = "default_line_len")]
    pub max_line_length: usize,
    /// Wrap the program in `%` lines.
    #[serde(default)]
    pub percent: bool,
    /// Write `G0` as `G00` and so on.
    #[serde(default)]
    pub two_digit_codes: bool,
}

fn default_line_len() -> usize {
    128
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub arcs: bool,
    /// Arcs that also move in Z.
    pub helical_arcs: bool,
    /// G81, G82, G83, G73 with G98 and G80.
    pub canned_cycles: bool,
    /// Automatic tool changer (M6); otherwise the program stops for a manual change.
    pub tool_changer: bool,
    /// Largest arc sweep in one block, degrees (arcs are split above this).
    #[serde(default = "default_sweep")]
    pub max_arc_sweep_deg: f64,
}

fn default_sweep() -> f64 {
    180.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Travel limits in machine coordinates, [min, max] per axis.
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
    pub max_feed: f64,
    pub max_rpm: f64,
    #[serde(default)]
    pub min_rpm: f64,
    /// Highest tool number.
    #[serde(default = "default_tools")]
    pub max_tool: u32,
}

fn default_tools() -> u32 {
    99
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    /// G codes the post may emit, for example "G0" or "G91.1".
    pub g: Vec<String>,
    pub m: Vec<String>,
    /// Address letters the post may emit.
    pub words: String,
}

/// A dangerous construct the post needs, with the reason. Reviewers see these first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    pub construct: Construct,
    pub reason: String,
}

/// Constructs the verifier rejects unless the post declares them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Construct {
    /// G10: writes work offsets, tool offsets or parameters.
    OffsetWrite,
    /// G92 and its variants: shifts the coordinate system.
    CoordinateShift,
    /// G28.1, G30.1 and similar: stores positions into parameters.
    ParameterWrite,
    /// `#` macro or system variables.
    MacroVariables,
    /// G65, G66, G67: macro calls.
    MacroCall,
    /// M98, M99, M198, O-word calls: subprograms and external programs.
    SubprogramCall,
    /// M100 to M199 on LinuxCNC: user M codes that run programs on the controller's host.
    UserMCode,
    /// DPRNT, POPEN, PCLOS, BPRNT and LinuxCNC LOG/PRINT/PROBE comments: file or port output.
    FileOutput,
    /// Commands that disable feed or spindle overrides (M48/M49/M50/M51 on LinuxCNC).
    OverrideLock,
    /// Stored stroke limits off (G23) or other soft-limit changes.
    SoftLimitsOff,
    /// Controller settings lines (GRBL `$`).
    ControllerSettings,
    /// Block delete (`/`).
    BlockDelete,
}

impl Construct {
    pub fn label(self) -> &'static str {
        match self {
            Construct::OffsetWrite => "offset or parameter write (G10)",
            Construct::CoordinateShift => "coordinate system shift (G92)",
            Construct::ParameterWrite => "position stored into parameters",
            Construct::MacroVariables => "macro or system variables (#)",
            Construct::MacroCall => "macro call",
            Construct::SubprogramCall => "subprogram or external program call",
            Construct::UserMCode => "user M code that runs a program",
            Construct::FileOutput => "file or port output",
            Construct::OverrideLock => "override lock",
            Construct::SoftLimitsOff => "soft limits disabled",
            Construct::ControllerSettings => "controller settings change",
            Construct::BlockDelete => "block delete",
        }
    }
}

/// Text templates for the blocks around the motion. Each is a list of lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blocks {
    pub program_start: Vec<String>,
    pub program_end: Vec<String>,
    pub tool_change: Vec<String>,
    pub spindle_on: Vec<String>,
    pub spindle_off: Vec<String>,
    #[serde(default)]
    pub coolant_flood: Vec<String>,
    #[serde(default)]
    pub coolant_mist: Vec<String>,
    #[serde(default)]
    pub coolant_off: Vec<String>,
    #[serde(default)]
    pub optional_stop: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostDefinition {
    pub post: PostInfo,
    pub format: Format,
    pub capabilities: Capabilities,
    pub machine: Machine,
    pub allow: Allow,
    #[serde(default)]
    pub declared: Vec<Declared>,
    pub blocks: Blocks,
}

/// Variables a template may use, per block. Values are already formatted text.
fn template_vars(block: &str) -> &'static [&'static str] {
    const COMMON: &[&str] = &["program_number", "program_name", "units_code", "work_offset", "safe_z"];
    const TOOL: &[&str] = &[
        "program_number",
        "program_name",
        "units_code",
        "work_offset",
        "safe_z",
        "tool",
        "tool_name",
        "length_offset",
    ];
    const SPINDLE: &[&str] =
        &["program_number", "program_name", "units_code", "work_offset", "safe_z", "rpm", "spindle_code"];
    match block {
        "tool_change" => TOOL,
        "spindle_on" => SPINDLE,
        _ => COMMON,
    }
}

/// Parts of a template line.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    Text(String),
    Var(String),
}

/// Parses a template line; unknown variables are an error.
fn parse_template(line: &str, block: &str) -> Result<Vec<Part>> {
    if line.len() > MAX_TEMPLATE_LINE {
        return Err(CamError::Post(format!(
            "a `{block}` template line is longer than {MAX_TEMPLATE_LINE} bytes"
        )));
    }
    if line.chars().any(|c| c.is_control()) {
        return Err(CamError::Post(format!("a `{block}` template line has a control character")));
    }
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c.is_ascii_alphanumeric() || c == '_' => name.push(c),
                        _ => {
                            return Err(CamError::Post(format!(
                                "bad placeholder in `{block}` template: {line}"
                            )));
                        }
                    }
                }
                if !template_vars(block).contains(&name.as_str()) {
                    return Err(CamError::Post(format!("`{block}` templates cannot use `{{{name}}}`")));
                }
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::Var(name));
            }
            '}' => return Err(CamError::Post(format!("unmatched `}}` in `{block}` template: {line}"))),
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(parts)
}

fn expand(parts: &[Part], vars: &BTreeMap<&str, String>) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            Part::Text(t) => out.push_str(t),
            Part::Var(v) => out.push_str(vars.get(v.as_str()).map(String::as_str).unwrap_or("")),
        }
        if out.len() > MAX_TEMPLATE_LINE {
            out.truncate(MAX_TEMPLATE_LINE);
            break;
        }
    }
    out
}

fn valid_code(code: &str, letter: char) -> bool {
    let mut it = code.chars();
    if it.next() != Some(letter) {
        return false;
    }
    let rest: String = it.collect();
    let mut sp = rest.splitn(2, '.');
    let int = sp.next().unwrap_or("");
    let frac = sp.next();
    !int.is_empty()
        && int.len() <= 3
        && int.chars().all(|c| c.is_ascii_digit())
        && frac.is_none_or(|f| f.len() == 1 && f.chars().all(|c| c.is_ascii_digit()))
}

impl PostDefinition {
    /// Parses and checks a post definition.
    pub fn from_toml(text: &str) -> Result<PostDefinition> {
        if text.len() > MAX_POST_BYTES {
            return Err(CamError::Post(format!("the definition is larger than {MAX_POST_BYTES} bytes")));
        }
        let def: PostDefinition =
            toml::from_str(text).map_err(|e| CamError::Post(format!("not a valid post definition: {e}")))?;
        def.check()?;
        Ok(def)
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string(self).map_err(|e| CamError::Post(format!("cannot write the definition: {e}")))
    }

    /// Structural checks: names, numbers, codes and templates.
    pub fn check(&self) -> Result<()> {
        let p = &self.post;
        if p.name.trim().is_empty() || p.name.len() > 80 {
            return Err(CamError::Post("the post needs a name of 1 to 80 characters".into()));
        }
        let ver: Vec<&str> = p.version.split('.').collect();
        if ver.len() != 3 || ver.iter().any(|v| v.is_empty() || !v.chars().all(|c| c.is_ascii_digit())) {
            return Err(CamError::Post("the version must look like 1.2.3".into()));
        }
        if p.license.trim().is_empty() {
            return Err(CamError::Post("the post must state its licence".into()));
        }
        let f = &self.format;
        if f.decimals > 6 || f.feed_decimals > 4 {
            return Err(CamError::Post("too many decimal places".into()));
        }
        if !(32..=1024).contains(&f.max_line_length) {
            return Err(CamError::Post("max_line_length must be 32 to 1024".into()));
        }
        if !(1.0..=360.0).contains(&self.capabilities.max_arc_sweep_deg) {
            return Err(CamError::Post("max_arc_sweep_deg must be 1 to 360".into()));
        }
        let m = &self.machine;
        for (a, r) in [("x", m.x), ("y", m.y), ("z", m.z)] {
            if !(r[0].is_finite() && r[1].is_finite() && r[0] < r[1]) || r[0].abs() > 1e5 || r[1].abs() > 1e5
            {
                return Err(CamError::Post(format!("the {a} travel limits are not valid")));
            }
        }
        if !(m.max_feed > 0.0 && m.max_feed <= 1e6 && m.max_rpm > 0.0 && m.max_rpm <= 1e6 && m.min_rpm >= 0.0)
            || m.min_rpm > m.max_rpm
        {
            return Err(CamError::Post("feed and speed limits are not valid".into()));
        }
        for g in &self.allow.g {
            if !valid_code(g, 'G') {
                return Err(CamError::Post(format!("`{g}` is not a G code")));
            }
        }
        for c in &self.allow.m {
            if !valid_code(c, 'M') {
                return Err(CamError::Post(format!("`{c}` is not an M code")));
            }
        }
        if !self.allow.words.chars().all(|c| c.is_ascii_uppercase()) || self.allow.words.is_empty() {
            return Err(CamError::Post("allowed words must be upper-case letters".into()));
        }
        for d in &self.declared {
            if d.reason.trim().len() < 20 {
                return Err(CamError::Post(format!(
                    "the declaration of {} needs a reason of at least 20 characters",
                    d.construct.label()
                )));
            }
        }
        for (name, lines) in self.block_list() {
            if lines.len() > MAX_TEMPLATE_LINES {
                return Err(CamError::Post(format!(
                    "the `{name}` block has more than {MAX_TEMPLATE_LINES} lines"
                )));
            }
            for l in lines {
                parse_template(l, name)?;
            }
        }
        Ok(())
    }

    fn block_list(&self) -> [(&'static str, &Vec<String>); 9] {
        let b = &self.blocks;
        [
            ("program_start", &b.program_start),
            ("program_end", &b.program_end),
            ("tool_change", &b.tool_change),
            ("spindle_on", &b.spindle_on),
            ("spindle_off", &b.spindle_off),
            ("coolant_flood", &b.coolant_flood),
            ("coolant_mist", &b.coolant_mist),
            ("coolant_off", &b.coolant_off),
            ("optional_stop", &b.optional_stop),
        ]
    }

    pub fn declares(&self, c: Construct) -> bool {
        self.declared.iter().any(|d| d.construct == c)
    }
}

/// The three example posts shipped with the crate: (file name, text).
pub fn bundled() -> [(&'static str, &'static str); 3] {
    [
        ("fanuc-generic-mill.toml", include_str!("../posts/fanuc-generic-mill.toml")),
        ("linuxcnc-mill.toml", include_str!("../posts/linuxcnc-mill.toml")),
        ("grbl-1.1.toml", include_str!("../posts/grbl-1.1.toml")),
    ]
}

/// Program-level settings for generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgramSettings {
    pub program_number: u32,
    pub program_name: String,
    /// 1 = G54 ... 6 = G59.
    pub work_offset: u32,
    pub safe_z: f64,
}

impl Default for ProgramSettings {
    fn default() -> Self {
        ProgramSettings {
            program_number: 1000,
            program_name: "ANVILCAD".into(),
            work_offset: 1,
            safe_z: 25.0,
        }
    }
}

/// A generated program.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub text: String,
}

struct Writer<'a> {
    def: &'a PostDefinition,
    out: String,
    line_no: u32,
    pos: Option<Vec3>,
    motion: Option<&'static str>,
    feed: Option<f64>,
    lines: usize,
}

/// Programs longer than this many lines are refused.
pub const MAX_PROGRAM_LINES: usize = 5_000_000;

impl Writer<'_> {
    fn code(&self, letter: char, n: u32) -> String {
        if self.def.format.two_digit_codes { format!("{letter}{n:02}") } else { format!("{letter}{n}") }
    }

    fn num(&self, v: f64) -> String {
        let v = if self.def.format.units == Units::Inch { v / 25.4 } else { v };
        let d = self.def.format.decimals as usize;
        let s = format!("{v:.d$}");
        // Avoid "-0.000".
        if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
            format!("{:.d$}", 0.0)
        } else {
            s
        }
    }

    fn feed_text(&self, f: f64) -> String {
        let f = if self.def.format.units == Units::Inch { f / 25.4 } else { f };
        let d = self.def.format.feed_decimals as usize;
        format!("F{f:.d$}")
    }

    fn line(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.lines += 1;
        if self.lines > MAX_PROGRAM_LINES {
            return Err(CamError::LimitExceeded("the program is too long".into()));
        }
        let numbered = self.def.format.line_step > 0
            && !text.starts_with('%')
            && !text.starts_with('(')
            && !text.starts_with(';')
            && !text.starts_with('O');
        if numbered {
            let _ = write!(self.out, "N{} ", self.line_no);
            self.line_no = self.line_no.saturating_add(self.def.format.line_step);
        }
        self.out.push_str(text);
        self.out.push('\n');
        Ok(())
    }

    fn comment(&mut self, text: &str) -> Result<()> {
        // Only plain characters, and nothing a controller could read as a command.
        let clean: String = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || " .-_".contains(*c))
            .take(self.def.format.max_line_length.saturating_sub(4))
            .collect::<String>()
            .to_ascii_uppercase();
        match self.def.format.comments {
            CommentStyle::Parens => self.line(&format!("({clean})")),
            CommentStyle::Semicolon => self.line(&format!("; {clean}")),
            CommentStyle::None => Ok(()),
        }
    }

    fn block(&mut self, name: &str, lines: &[String], vars: &BTreeMap<&str, String>) -> Result<()> {
        for l in lines {
            let parts = parse_template(l, name)?;
            let text = expand(&parts, vars);
            self.line(&text)?;
        }
        Ok(())
    }

    fn axes(&self, to: Vec3) -> String {
        let mut s = String::new();
        for (letter, v, old) in [
            ('X', to.x, self.pos.map(|p| p.x)),
            ('Y', to.y, self.pos.map(|p| p.y)),
            ('Z', to.z, self.pos.map(|p| p.z)),
        ] {
            let t = self.num(v);
            if old.is_none_or(|o| self.num(o) != t) {
                let _ = write!(s, " {letter}{t}");
            }
        }
        s
    }

    fn motion_word(&mut self, g: &'static str, n: u32) -> String {
        if self.motion == Some(g) {
            String::new()
        } else {
            self.motion = Some(g);
            self.code('G', n)
        }
    }

    fn feed_word(&mut self, f: f64) -> String {
        let t = self.feed_text(f);
        if self.feed.is_some_and(|old| self.feed_text(old) == t) {
            String::new()
        } else {
            self.feed = Some(f);
            format!(" {t}")
        }
    }

    fn rapid(&mut self, to: Vec3) -> Result<()> {
        let axes = self.axes(to);
        if axes.is_empty() {
            return Ok(());
        }
        let g = self.motion_word("G0", 0);
        self.line(format!("{g}{axes}").trim())?;
        self.pos = Some(to);
        Ok(())
    }

    fn linear(&mut self, to: Vec3, feed: f64) -> Result<()> {
        let axes = self.axes(to);
        if axes.is_empty() {
            return Ok(());
        }
        let g = self.motion_word("G1", 1);
        let f = self.feed_word(feed);
        self.line(format!("{g}{axes}{f}").trim())?;
        self.pos = Some(to);
        Ok(())
    }

    fn arc(&mut self, m: &Move, feed: f64) -> Result<()> {
        let Motion::Arc { center, ccw } = m.motion else { return self.linear(m.to, feed) };
        let Some(from) = self.pos else { return self.linear(m.to, feed) };
        let helical = (m.to.z - from.z).abs() > 1e-9;
        if !self.def.capabilities.arcs || (helical && !self.def.capabilities.helical_arcs) {
            for p in arc_points(from, m.to, center, ccw, 0.005).into_iter().skip(1) {
                self.linear(p, feed)?;
            }
            return Ok(());
        }
        let sweep = arc_sweep(from, m.to, center, ccw);
        let max = self.def.capabilities.max_arc_sweep_deg.to_radians();
        let pieces = ((sweep.abs() / max).ceil() as usize).clamp(1, 1000);
        let r0 = (v2(from.x, from.y) - center).length();
        let r1 = (v2(m.to.x, m.to.y) - center).length();
        let a0 = (v2(from.x, from.y) - center).angle();
        for k in 1..=pieces {
            let t = k as f64 / pieces as f64;
            let to = if k == pieces {
                m.to
            } else {
                let p = center + anvil_kernel::Vec2::from_angle(a0 + sweep * t) * (r0 + (r1 - r0) * t);
                v3(p.x, p.y, from.z + (m.to.z - from.z) * t)
            };
            let start = self.pos.unwrap_or(from);
            let (gname, n) = if ccw { ("G3", 3) } else { ("G2", 2) };
            let g = self.motion_word(gname, n);
            // Always state the end point of an arc in full.
            let mut axes = format!(" X{} Y{}", self.num(to.x), self.num(to.y));
            if self.pos.is_none_or(|p| self.num(p.z) != self.num(to.z)) {
                let _ = write!(axes, " Z{}", self.num(to.z));
            }
            let i = self.num(center.x - start.x);
            let j = self.num(center.y - start.y);
            let f = self.feed_word(feed);
            self.line(format!("{g}{axes} I{i} J{j}{f}").trim())?;
            self.pos = Some(to);
        }
        Ok(())
    }

    fn cycle(&mut self, c: &DrillCycle) -> Result<()> {
        if !self.def.capabilities.canned_cycles {
            let mut tp = Toolpath::new();
            tp.push(Item::Cycle(c.clone()));
            for it in tp.expanded().items {
                self.item(&it)?;
            }
            return Ok(());
        }
        let Some(first) = c.points.first() else { return Ok(()) };
        self.rapid(v3(first.x, first.y, c.clearance))?;
        let (g, extra) = match c.kind {
            CycleKind::Drill => (81, String::new()),
            CycleKind::SpotDwell { dwell_s } => {
                (82, format!(" P{}", (dwell_s * 1000.0).round().max(0.0) as u64))
            }
            CycleKind::Peck { peck } => (83, format!(" Q{}", self.num(peck))),
            CycleKind::ChipBreak { peck } => (73, format!(" Q{}", self.num(peck))),
        };
        let g98 = self.code('G', 98);
        let gc = self.code('G', g);
        let f = self.feed_text(c.feed);
        let text = format!(
            "{g98} {gc} X{} Y{} Z{} R{}{extra} {f}",
            self.num(first.x),
            self.num(first.y),
            self.num(c.z),
            self.num(c.r_plane)
        );
        self.line(&text)?;
        self.feed = Some(c.feed);
        for p in c.points.iter().skip(1) {
            self.line(&format!("X{} Y{}", self.num(p.x), self.num(p.y)))?;
        }
        let g80 = self.code('G', 80);
        self.line(&g80)?;
        self.motion = None;
        let last = c.points.last().copied().unwrap_or(*first);
        self.pos = Some(v3(last.x, last.y, c.clearance));
        Ok(())
    }

    fn item(&mut self, it: &Item) -> Result<()> {
        match it {
            Item::Comment(t) => self.comment(t),
            Item::Move(m) => match (m.motion, m.feed) {
                (_, None) => self.rapid(m.to),
                (Motion::Line, Some(f)) => self.linear(m.to, f),
                (Motion::Arc { .. }, Some(f)) => self.arc(m, f),
            },
            Item::Cycle(c) => self.cycle(c),
            Item::Dwell { seconds } => {
                let g4 = self.code('G', 4);
                self.line(&format!("{g4} P{:.3}", seconds.max(0.0)))
            }
            Item::OptionalStop => {
                let b = self.def.blocks.optional_stop.clone();
                self.block("optional_stop", &b, &BTreeMap::new())
            }
            // Handled by `generate`, which knows the program settings.
            Item::ToolChange { .. } | Item::Spindle { .. } | Item::SpindleStop | Item::Coolant(_) => Ok(()),
        }
    }
}

/// Writes `toolpath` as a program for the machine `def` describes.
pub fn generate(def: &PostDefinition, toolpath: &Toolpath, settings: &ProgramSettings) -> Result<Program> {
    def.check()?;
    if !(1..=6).contains(&settings.work_offset) {
        return Err(CamError::InvalidParameter {
            name: "work offset",
            reason: "must be 1 (G54) to 6 (G59)".into(),
        });
    }
    crate::error::finite("safe Z", settings.safe_z)?;
    let mut w = Writer {
        def,
        out: String::new(),
        line_no: def.format.line_start,
        pos: None,
        motion: None,
        feed: None,
        lines: 0,
    };
    let units_code = match def.format.units {
        Units::Mm => w.code('G', 21),
        Units::Inch => w.code('G', 20),
    };
    let name: String = settings
        .program_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(24)
        .collect();
    let mut vars: BTreeMap<&str, String> = BTreeMap::new();
    vars.insert("program_number", format!("{}", settings.program_number.min(9999)));
    vars.insert("program_name", name.to_ascii_uppercase());
    vars.insert("units_code", units_code);
    vars.insert("work_offset", w.code('G', 53 + settings.work_offset));
    vars.insert("safe_z", w.num(settings.safe_z));
    if def.format.percent {
        w.line("%")?;
    }
    w.block("program_start", &def.blocks.program_start, &vars)?;
    for it in &toolpath.items {
        match it {
            Item::ToolChange { tool, name } => {
                if *tool == 0 || *tool > def.machine.max_tool {
                    return Err(CamError::Post(format!("tool {tool} is outside this machine's range")));
                }
                let mut v = vars.clone();
                v.insert("tool", format!("{tool}"));
                v.insert("length_offset", format!("{tool}"));
                let clean: String = name
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || " .-_".contains(*c))
                    .take(40)
                    .collect();
                v.insert("tool_name", clean.to_ascii_uppercase());
                w.block("tool_change", &def.blocks.tool_change, &v)?;
                // The block may move the machine; positions and modes are unknown after it.
                w.pos = None;
                w.motion = None;
                w.feed = None;
            }
            Item::Spindle { rpm, dir } => {
                let rpm = rpm.round();
                if !(rpm >= def.machine.min_rpm && rpm <= def.machine.max_rpm) {
                    return Err(CamError::Post(format!(
                        "spindle speed {rpm} is outside this machine's range"
                    )));
                }
                let mut v = vars.clone();
                v.insert("rpm", format!("{rpm:.0}"));
                v.insert("spindle_code", w.code('M', if *dir == SpindleDir::Clockwise { 3 } else { 4 }));
                w.block("spindle_on", &def.blocks.spindle_on, &v)?;
            }
            Item::SpindleStop => w.block("spindle_off", &def.blocks.spindle_off, &vars)?,
            Item::Coolant(c) => {
                let (name, lines) = match c {
                    Coolant::Flood => ("coolant_flood", &def.blocks.coolant_flood),
                    Coolant::Mist => ("coolant_mist", &def.blocks.coolant_mist),
                    Coolant::Off => ("coolant_off", &def.blocks.coolant_off),
                };
                w.block(name, lines, &vars)?;
            }
            Item::Move(m) => {
                if let Some(f) = m.feed
                    && !(f > 0.0 && f <= def.machine.max_feed)
                {
                    return Err(CamError::Post(format!("feed {f:.1} is outside this machine's range")));
                }
                if !(m.to.x.is_finite() && m.to.y.is_finite() && m.to.z.is_finite()) {
                    return Err(CamError::InvalidGeometry(
                        "a move has a coordinate that is not finite".into(),
                    ));
                }
                w.item(it)?;
            }
            other => w.item(other)?,
        }
    }
    w.block("program_end", &def.blocks.program_end, &vars)?;
    if def.format.percent {
        w.line("%")?;
    }
    Ok(Program { text: w.out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_posts_load() {
        for (name, text) in bundled() {
            let def = PostDefinition::from_toml(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!def.post.name.is_empty());
            // Round trip through TOML keeps the definition.
            let again = PostDefinition::from_toml(&def.to_toml().unwrap()).unwrap();
            assert_eq!(again, def);
        }
    }

    #[test]
    fn templates_reject_unknown_variables_and_bad_braces() {
        assert!(parse_template("T{tool} M6", "tool_change").is_ok());
        assert!(parse_template("T{tool} M6", "program_start").is_err());
        assert!(parse_template("G0 {x", "program_start").is_err());
        assert!(parse_template("{{literal}}", "program_start").is_ok());
        assert!(parse_template("{../../etc}", "program_start").is_err());
    }

    #[test]
    fn oversized_or_unknown_fields_are_rejected() {
        let (_, text) = bundled()[0];
        let extra = format!("{text}\n[script]\nrun = \"rm -rf /\"\n");
        assert!(PostDefinition::from_toml(&extra).is_err());
        let big = "#".repeat(MAX_POST_BYTES + 1);
        assert!(PostDefinition::from_toml(&big).is_err());
    }
}
