//! The published report of a corpus run (issue #89): a Markdown page anyone can read to
//! check the kernel's reliability claim, written by `anvil-evals --publish FILE`.
//!
//! The page must come out byte for byte the same from the same commit, so it holds only
//! what the run decides (results, reference volumes, errors), with floating-point values
//! rounded well above their noise, and no live timings: timings vary from run to run and
//! machine to machine. Instead it shows the timings of a committed reference run (a
//! baseline), and says which one.

use crate::{Case, CaseResult, Expect, Status};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A committed timing run: where it came from, and its report.
pub struct Baseline<'a> {
    /// The baseline file, as the page should name it.
    pub path: &'a str,
    /// What it was recorded on, in a few words.
    pub machine: &'a str,
    /// The report, as `--save-baseline` writes it.
    pub json: &'a str,
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "pass",
        Status::Graceful => "refused",
        Status::Fail => "**FAIL**",
        Status::Panic => "**PANIC**",
    }
}

fn expect_word(e: Expect) -> &'static str {
    match e {
        Expect::Pass => "pass",
        Expect::Graceful => "may refuse",
    }
}

/// Text safe inside a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// A number to `sig` significant figures, without float noise in the last places.
fn sig(x: f64, sig: usize) -> String {
    if x == 0.0 || !x.is_finite() {
        return format!("{x}");
    }
    let digits = (sig as i32 - 1 - x.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{x:.digits$}");
    // 15, not 15.0000: the trailing zeros say nothing.
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Timings per case name from a baseline report: (build ms, tessellation ms).
fn timings(json: &str) -> Result<BTreeMap<String, (f64, f64)>, String> {
    let v: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("the baseline is not a report: {e}"))?;
    Ok(v.iter()
        .filter_map(|r| {
            Some((
                r.get("name")?.as_str()?.to_string(),
                (r.get("build_ms")?.as_f64()?, r.get("tess_ms")?.as_f64()?),
            ))
        })
        .collect())
}

/// The report for `results`, which were run from `cases` (matched by name for each case's
/// note and whether its reference is closed-form).
pub fn report(cases: &[Case], results: &[CaseResult], baseline: Option<&Baseline>) -> Result<String, String> {
    let by_name: BTreeMap<&str, &Case> = cases.iter().map(|c| (c.name.as_str(), c)).collect();
    let times = match baseline {
        Some(b) => Some(timings(b.json)?),
        None => None,
    };
    let count = |s: Status| results.iter().filter(|r| r.status == s).count();
    let broke = results.iter().filter(|r| r.regressed()).count();
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(w, "# Kernel evaluation results\n");
    let _ = writeln!(
        w,
        "**{} cases: {} pass, {} refused, {} fail, {} panic. {}**\n",
        results.len(),
        count(Status::Pass),
        count(Status::Graceful),
        count(Status::Fail),
        count(Status::Panic),
        if broke == 0 {
            "Every case met its expectation.".to_string()
        } else {
            format!("{broke} broke their expectation.")
        }
    );
    let _ = writeln!(
        w,
        "Each case is a solid built by AnvilCAD's exact kernel and checked against an independent \
         reference: its topology and mesh are audited, its volume compared, 300 points tested for \
         inside and outside, its STEP export read back, and its images compared pixel by pixel. \
         **pass** means every check agreed; **refused** means the kernel declined with an error, \
         so the user sees a message and loses nothing; a **fail** would be a wrong or invalid \
         body, which is never accepted. A case marked *may refuse* is a known limit. How each \
         check works is in [EVALS.md](EVALS.md).\n"
    );
    let _ = writeln!(
        w,
        "This page is generated: `cargo run --release -p anvil-evals -- --publish docs/kernel/RESULTS.md`. \
         It comes out the same, byte for byte, from the same commit.\n"
    );
    if let Some(b) = baseline {
        let _ = writeln!(
            w,
            "Timings are from the committed reference run `{}` ({}), not from the run that made \
             this page, since they vary from machine to machine. A dash means the case is newer \
             than that run.\n",
            b.path, b.machine
        );
    }

    let mut categories: Vec<&str> = results.iter().map(|r| r.category.as_str()).collect();
    categories.dedup();
    let mut seen = std::collections::BTreeSet::new();
    categories.retain(|c| seen.insert(*c));
    for cat in categories {
        let _ = writeln!(w, "## {cat}\n");
        let head = if times.is_some() { " Build ms | Mesh ms |" } else { "" };
        let rule = if times.is_some() { "---:|---:|" } else { "" };
        let _ = writeln!(w, "| Case | Note | Reference volume (mm³) | Result | Expected |{head}");
        let _ = writeln!(w, "|---|---|---:|---|---|{rule}");
        let mut notes = Vec::new();
        for r in results.iter().filter(|r| r.category == cat) {
            let case = by_name.get(r.name.as_str());
            let why = case.map(|c| c.note).unwrap_or("");
            let reference = if case.is_some_and(|c| c.volume.is_some()) || r.reference_error == 0.0 {
                format!("{} (exact)", sig(r.reference_volume, 6))
            } else {
                format!("{} ± {} (sampled)", sig(r.reference_volume, 6), sig(r.reference_error, 1))
            };
            let time = match &times {
                Some(t) => match t.get(&r.name) {
                    Some((b, m)) => format!(" {b:.2} | {m:.2} |"),
                    None => " – | – |".to_string(),
                },
                None => String::new(),
            };
            let _ = writeln!(
                w,
                "| `{}` | {} | {} | {} | {} |{time}",
                cell(&r.name),
                cell(why),
                reference,
                status_word(r.status),
                expect_word(r.expect)
            );
            if r.status != Status::Pass && !r.reasons.is_empty() {
                notes.push(format!("- `{}`: {}", r.name, cell(&r.reasons.join("; "))));
            }
        }
        if !notes.is_empty() {
            let _ = writeln!(w, "\nWhy cases did not pass:\n");
            for n in notes {
                let _ = writeln!(w, "{n}");
            }
        }
        let _ = writeln!(w);
    }
    Ok(out.trim_end().to_string() + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_to_significant_figures() {
        assert_eq!(sig(15.0, 6), "15");
        assert_eq!(sig(4.992, 6), "4.992");
        assert_eq!(sig(1234.56789, 6), "1234.57");
        assert_eq!(sig(0.000123456, 2), "0.00012");
        assert_eq!(sig(0.0, 6), "0");
    }

    #[test]
    fn a_bad_baseline_is_reported() {
        let b = Baseline { path: "x.json", machine: "m", json: "{" };
        assert!(report(&[], &[], Some(&b)).is_err_and(|e| e.starts_with("the baseline is not a report")));
    }
}
