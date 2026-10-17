//! Support for a community registry of post processors (see `docs/cam/POSTS.md`):
//! content hashes for pinning, and the deterministic review of an edit to a post.
//!
//! The review of an edit works in this order, and the order is the policy:
//!
//! 1. The deterministic checks: the new definition must load, and a reference program
//!    written with it must pass the verifier. Failing here rejects the edit, and nothing
//!    later can change that.
//! 2. Edit flags: anything that widens what the post may do (a new declared construct, a
//!    new G or M code or address word, raised limits, changed templates, an unchanged
//!    version) marks the edit for a person's attention.
//! 3. An optional AI review (see the `ai` feature) may add concerns or block the edit.
//!    It cannot approve anything: [`decide`] never turns a rejection or a flag into a
//!    pass because of it.
//!
//! Even when every automated check passes, merging into the registry needs a human
//! maintainer's review; [`Decision::AutomatedChecksPassed`] says only that the machines
//! found nothing.

use crate::error::{CamError, Result};
use crate::post::{Construct, PostDefinition, ProgramSettings, generate};
use crate::toolpath::{Coolant, CycleKind, DrillCycle, Item, MoveKind, SpindleDir, Toolpath};
use crate::verify::{VerifyOptions, verify};
use anvil_kernel::{v2, v3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// SHA-256 of the definition text with line endings normalised to `\n`, as lowercase hex.
pub fn content_hash(text: &str) -> String {
    let norm = text.replace("\r\n", "\n");
    let digest = Sha256::digest(norm.as_bytes());
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// A post pinned by name, version and content hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub name: String,
    pub version: String,
    pub sha256: String,
}

/// Loads `text` only if it is exactly the pinned post.
pub fn load_pinned(text: &str, pin: &Pin) -> Result<PostDefinition> {
    let h = content_hash(text);
    if h != pin.sha256 {
        return Err(CamError::Post(format!("content hash {h} does not match the pinned {}", pin.sha256)));
    }
    let def = PostDefinition::from_toml(text)?;
    if def.post.name != pin.name || def.post.version != pin.version {
        return Err(CamError::Post("name or version does not match the pin".into()));
    }
    Ok(def)
}

/// What an edit widens or changes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditFlags {
    pub added_declarations: Vec<Construct>,
    pub added_g: Vec<String>,
    pub added_m: Vec<String>,
    pub added_words: Vec<char>,
    pub raised_limits: Vec<String>,
    pub changed_blocks: Vec<String>,
    pub version_unchanged: bool,
}

impl EditFlags {
    pub fn reasons(&self) -> Vec<String> {
        let mut r = Vec::new();
        for c in &self.added_declarations {
            r.push(format!("adds a declared dangerous construct: {}", c.label()));
        }
        if !self.added_g.is_empty() {
            r.push(format!("allows new G codes: {}", self.added_g.join(", ")));
        }
        if !self.added_m.is_empty() {
            r.push(format!("allows new M codes: {}", self.added_m.join(", ")));
        }
        if !self.added_words.is_empty() {
            r.push(format!("allows new address words: {}", self.added_words.iter().collect::<String>()));
        }
        for l in &self.raised_limits {
            r.push(format!("raises a machine limit: {l}"));
        }
        for b in &self.changed_blocks {
            r.push(format!("changes the `{b}` template"));
        }
        if self.version_unchanged {
            r.push("the version number is unchanged".into());
        }
        r
    }

    pub fn is_empty(&self) -> bool {
        self.reasons().is_empty()
    }
}

/// Compares two definitions.
pub fn compare(old: &PostDefinition, new: &PostDefinition) -> EditFlags {
    let mut f = EditFlags {
        added_declarations: new.declared.iter().map(|d| d.construct).filter(|c| !old.declares(*c)).collect(),
        added_g: new.allow.g.iter().filter(|g| !old.allow.g.contains(g)).cloned().collect(),
        added_m: new.allow.m.iter().filter(|m| !old.allow.m.contains(m)).cloned().collect(),
        added_words: new.allow.words.chars().filter(|c| !old.allow.words.contains(*c)).collect(),
        version_unchanged: old.post.version == new.post.version,
        ..EditFlags::default()
    };
    let (o, n) = (&old.machine, &new.machine);
    for (name, a, b) in [
        ("x min", -o.x[0], -n.x[0]),
        ("x max", o.x[1], n.x[1]),
        ("y min", -o.y[0], -n.y[0]),
        ("y max", o.y[1], n.y[1]),
        ("z min", -o.z[0], -n.z[0]),
        ("z max", o.z[1], n.z[1]),
        ("max feed", o.max_feed, n.max_feed),
        ("max rpm", o.max_rpm, n.max_rpm),
        ("max tool", o.max_tool as f64, n.max_tool as f64),
    ] {
        if b > a {
            f.raised_limits.push(name.into());
        }
    }
    let (ob, nb) = (&old.blocks, &new.blocks);
    for (name, a, b) in [
        ("program_start", &ob.program_start, &nb.program_start),
        ("program_end", &ob.program_end, &nb.program_end),
        ("tool_change", &ob.tool_change, &nb.tool_change),
        ("spindle_on", &ob.spindle_on, &nb.spindle_on),
        ("spindle_off", &ob.spindle_off, &nb.spindle_off),
        ("coolant_flood", &ob.coolant_flood, &nb.coolant_flood),
        ("coolant_mist", &ob.coolant_mist, &nb.coolant_mist),
        ("coolant_off", &ob.coolant_off, &nb.coolant_off),
        ("optional_stop", &ob.optional_stop, &nb.optional_stop),
    ] {
        if a != b {
            f.changed_blocks.push(name.into());
        }
    }
    f
}

/// A tool path that exercises every kind of item, used to test a post. It spans 60 x 40
/// x 25 mm around program zero.
pub fn reference_toolpath() -> Toolpath {
    let mut tp = Toolpath::new();
    tp.push(Item::Comment("reference program".into()));
    tp.push(Item::ToolChange { tool: 1, name: "10 mm end mill".into() });
    tp.push(Item::Spindle { rpm: 6000.0, dir: SpindleDir::Clockwise });
    tp.push(Item::Coolant(Coolant::Flood));
    tp.rapid(v3(0.0, 0.0, 20.0));
    tp.rapid(v3(0.0, 0.0, 2.0));
    tp.line(v3(0.0, 0.0, -2.0), MoveKind::Entry, 200.0);
    tp.line(v3(30.0, 0.0, -2.0), MoveKind::Cut, 800.0);
    tp.arc(v3(30.0, 20.0, -2.0), v2(30.0, 10.0), true, MoveKind::Cut, 800.0);
    tp.arc(v3(30.0, 20.0, -4.0), v2(30.0, 15.0), false, MoveKind::Entry, 300.0);
    tp.line(v3(-30.0, 20.0, -4.0), MoveKind::Cut, 800.0);
    tp.rapid(v3(-30.0, 20.0, 20.0));
    tp.push(Item::OptionalStop);
    tp.push(Item::ToolChange { tool: 2, name: "6 mm drill".into() });
    tp.push(Item::Spindle { rpm: 2000.0, dir: SpindleDir::Clockwise });
    tp.push(Item::Cycle(DrillCycle {
        kind: CycleKind::Peck { peck: 5.0 },
        points: vec![v2(-20.0, -10.0), v2(20.0, -10.0)],
        top: 0.0,
        r_plane: 2.0,
        z: -15.0,
        clearance: 20.0,
        feed: 120.0,
    }));
    tp.push(Item::Coolant(Coolant::Off));
    tp.push(Item::SpindleStop);
    tp
}

/// Writes the reference program with `def` and verifies it, with program zero placed
/// 40 mm below the top of Z travel in the middle of X and Y.
pub fn reference_check(def: &PostDefinition) -> (String, crate::verify::VerifyReport) {
    let m = &def.machine;
    let opts = VerifyOptions {
        work_offset: v3((m.x[0] + m.x[1]) * 0.5, (m.y[0] + m.y[1]) * 0.5, m.z[1] - 40.0),
        start: None,
    };
    match generate(def, &reference_toolpath(), &ProgramSettings::default()) {
        Ok(p) => {
            let rep = verify(&p.text, def, &opts);
            (p.text, rep)
        }
        Err(e) => {
            let mut rep = crate::verify::VerifyReport::default();
            rep.issues.push(crate::verify::Issue {
                line: 0,
                severity: crate::verify::Severity::Error,
                message: format!("the post could not write the reference program: {e}"),
                construct: None,
            });
            (String::new(), rep)
        }
    }
}

/// Result of the deterministic checks on an edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Verdict {
    /// Fails a check; cannot be accepted.
    Reject(Vec<String>),
    /// Passes the checks but widens what the post may do.
    NeedsAttention(Vec<String>),
    Pass,
}

/// What an AI reviewer said about an edit. It can only add concerns or block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AiVerdict {
    NoConcerns,
    Concerns,
    Block,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiAssessment {
    pub verdict: AiVerdict,
    pub concerns: Vec<String>,
}

/// The combined outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Decision {
    Rejected(Vec<String>),
    /// A person must look at these points before the edit is used or merged.
    NeedsReview(Vec<String>),
    /// No automated check found a problem. Registry merges still need a maintainer.
    AutomatedChecksPassed,
}

/// Combines the deterministic verdict with an optional AI assessment. The AI can make
/// the outcome stricter, never looser.
pub fn decide(det: &Verdict, ai: Option<&AiAssessment>) -> Decision {
    let ai_notes: Vec<String> =
        ai.map(|a| a.concerns.iter().map(|c| format!("AI review: {c}")).collect()).unwrap_or_default();
    match det {
        Verdict::Reject(r) => {
            let mut all = r.clone();
            all.extend(ai_notes);
            Decision::Rejected(all)
        }
        _ if ai.is_some_and(|a| a.verdict == AiVerdict::Block) => {
            let mut all = match det {
                Verdict::NeedsAttention(r) => r.clone(),
                _ => Vec::new(),
            };
            all.push("the AI review blocked this edit".into());
            all.extend(ai_notes);
            Decision::Rejected(all)
        }
        Verdict::NeedsAttention(r) => {
            let mut all = r.clone();
            all.extend(ai_notes);
            Decision::NeedsReview(all)
        }
        Verdict::Pass => {
            if ai.is_some_and(|a| a.verdict == AiVerdict::Concerns) || !ai_notes.is_empty() {
                Decision::NeedsReview(ai_notes)
            } else {
                Decision::AutomatedChecksPassed
            }
        }
    }
}

/// The deterministic review of replacing `old_text` with `new_text`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditReview {
    pub verdict: Verdict,
    pub flags: EditFlags,
    pub diff: String,
}

pub fn review_edit(old_text: &str, new_text: &str) -> EditReview {
    let diff = line_diff(old_text, new_text);
    let new = match PostDefinition::from_toml(new_text) {
        Ok(d) => d,
        Err(e) => {
            return EditReview {
                verdict: Verdict::Reject(vec![e.to_string()]),
                flags: EditFlags::default(),
                diff,
            };
        }
    };
    let flags = match PostDefinition::from_toml(old_text) {
        Ok(old) => compare(&old, &new),
        Err(_) => EditFlags::default(),
    };
    let (_, rep) = reference_check(&new);
    let errors: Vec<String> = rep.errors().map(|i| format!("line {}: {}", i.line, i.message)).collect();
    if !errors.is_empty() {
        return EditReview { verdict: Verdict::Reject(errors), flags, diff };
    }
    let mut attention = flags.reasons();
    for c in &rep.declared_used {
        attention.push(format!("the reference program uses {}", c.label()));
    }
    let verdict = if attention.is_empty() { Verdict::Pass } else { Verdict::NeedsAttention(attention) };
    EditReview { verdict, flags, diff }
}

/// A line diff: unchanged lines start with two spaces, removed with "- ", added with
/// "+ ". Inputs over 4000 lines each are compared as wholes.
pub fn line_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    if a.len() > 4000 || b.len() > 4000 {
        for l in &a {
            let _ = writeln!(out, "- {l}");
        }
        for l in &b {
            let _ = writeln!(out, "+ {l}");
        }
        return out;
    }
    // Longest common subsequence table, from the ends.
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * (m + 1) + j] = if a[i] == b[j] {
                t[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            let _ = writeln!(out, "  {}", a[i]);
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[i * (m + 1) + j + 1] > t[(i + 1) * (m + 1) + j]) {
            let _ = writeln!(out, "+ {}", b[j]);
            j += 1;
        } else {
            let _ = writeln!(out, "- {}", a[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post::bundled;

    fn fanuc_text() -> &'static str {
        bundled()[0].1
    }

    #[test]
    fn hash_and_pin() {
        let text = fanuc_text();
        let h = content_hash(text);
        assert_eq!(h.len(), 64);
        assert_eq!(h, content_hash(&text.replace('\n', "\r\n")));
        let pin = Pin { name: "Generic Fanuc-style mill".into(), version: "1.0.0".into(), sha256: h };
        assert!(load_pinned(text, &pin).is_ok());
        let tampered = text.replace("max_feed = 10000.0", "max_feed = 90000.0");
        assert!(load_pinned(&tampered, &pin).is_err());
    }

    #[test]
    fn every_bundled_post_passes_its_reference_check() {
        for (name, text) in bundled() {
            let def = PostDefinition::from_toml(text).unwrap();
            let (prog, rep) = reference_check(&def);
            assert!(rep.passed(), "{name}\n{prog}\n{:?}", rep.errors().collect::<Vec<_>>());
            assert!(rep.declared_used.is_empty());
        }
    }

    #[test]
    fn malicious_edits_are_rejected() {
        let old = fanuc_text();
        // A parameter write slipped into the start block.
        let g10 = old.replace(
            "program_start = [\"O{program_number} ({program_name})\",",
            "program_start = [\"O{program_number} ({program_name})\", \"G10 L50 N1234 R1\",",
        );
        assert_ne!(g10, old);
        let r = review_edit(old, &g10);
        assert!(matches!(r.verdict, Verdict::Reject(_)), "{:?}", r.verdict);
        assert!(r.diff.contains("+ program_start"));
        // A macro system variable write.
        let macro_write = old.replace("program_end = [\"M5\",", "program_end = [\"#3004=7\", \"M5\",");
        assert!(matches!(review_edit(old, &macro_write).verdict, Verdict::Reject(_)));
        // G10 added to the allow list without a declaration.
        let allow_only = old
            .replace("\"G4\", \"G17\"", "\"G4\", \"G10\", \"G17\"")
            .replace("words = \"GMXYZIJKFSTHPQRNO\"", "words = \"GMXYZIJKFSTHPQRNOL\"")
            .replace("tool_change = [", "tool_change = [\"G10 L2 P1 X0 Y0 Z0\", ");
        assert!(matches!(review_edit(old, &allow_only).verdict, Verdict::Reject(_)));
        // Declared and allowed: passes the verifier, but is flagged for people.
        let declared = format!(
            "{}\n[[declared]]\nconstruct = \"offset_write\"\nreason = \"sets G54 from a probed fixture position\"\n",
            allow_only.replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        );
        let r = review_edit(old, &declared);
        match &r.verdict {
            Verdict::NeedsAttention(reasons) => {
                assert!(reasons.iter().any(|s| s.contains("declared dangerous construct")));
            }
            other => panic!("{other:?}"),
        }
        // An AI "approval" cannot lift a rejection, and an AI block rejects a clean edit.
        let ok = AiAssessment { verdict: AiVerdict::NoConcerns, concerns: vec![] };
        assert!(matches!(decide(&review_edit(old, &g10).verdict, Some(&ok)), Decision::Rejected(_)));
        let block = AiAssessment {
            verdict: AiVerdict::Block,
            concerns: vec!["looks like it disables limits".into()],
        };
        assert!(matches!(decide(&Verdict::Pass, Some(&block)), Decision::Rejected(_)));
        assert!(matches!(decide(&r.verdict, Some(&ok)), Decision::NeedsReview(_)));
        assert_eq!(decide(&Verdict::Pass, None), Decision::AutomatedChecksPassed);
    }

    #[test]
    fn harmless_edit_passes_automated_checks() {
        let old = fanuc_text();
        let new =
            old.replace("decimals = 3", "decimals = 4").replace("version = \"1.0.0\"", "version = \"1.0.1\"");
        let r = review_edit(old, &new);
        assert_eq!(r.verdict, Verdict::Pass, "{:?}", r.verdict);
    }

    #[test]
    fn diff_marks_changes() {
        let d = line_diff("a\nb\nc\n", "a\nx\nc\n");
        assert_eq!(d, "  a\n- b\n+ x\n  c\n");
    }
}
