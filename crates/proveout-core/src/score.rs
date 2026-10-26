//! The agreement score shown as a percentage on every report.
//!
//! It is not the probability that the program is right. It says how much of the
//! program the checks could confirm and how well the program agreed with the drawing,
//! and every report shows the parts it was built from. The formula:
//!
//! - **Rules** (weight 0.4): the deterministic rule pass rate, the share of blocks with
//!   no error or review finding. Left out when the checker does not read the dialect.
//! - **Drawing** (weight 0.3): matched features over all features checked against the
//!   drawing. Left out when nothing was checked.
//! - **AI confidence** (weight 0.3): the model's stated confidence, 0 to 100. Left out
//!   when there was no AI review.
//!
//! The parts present are averaged by their weights, then capped: at 60 with any
//! deterministic error, at 70 with any error the AI reports or any drawing mismatch, at
//! 75 when the deterministic checker could not read the program, and at 84 when a
//! finding needs a person's review or nothing was compared with a drawing. So "Checks
//! agree" (85 and above) needs a drawing comparison and no open findings. The result
//! is rounded to a whole percent.

use serde::{Deserialize, Serialize};

pub const W_RULES: f64 = 0.4;
pub const W_DRAWING: f64 = 0.3;
pub const W_AI: f64 = 0.3;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreInputs {
    /// Deterministic rule pass rate, 0 to 1; `None` when the dialect is unsupported.
    pub rule_pass_rate: Option<f64>,
    pub deterministic_errors: usize,
    /// Errors the AI review reports.
    pub ai_errors: usize,
    /// Findings of either source that need a person's review.
    pub review_findings: usize,
    pub drawing_matched: usize,
    pub drawing_mismatched: usize,
    pub drawing_unclear: usize,
    /// The model's confidence, 0 to 100; `None` without an AI review.
    pub ai_confidence: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// 0 to 100.
    pub percent: u32,
    pub band: Band,
    /// Each part as a percentage, `None` when left out.
    pub rules: Option<f64>,
    pub drawing: Option<f64>,
    pub ai: Option<f64>,
    /// The caps that applied, in words.
    pub caps: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// 85 and above: the checks agree. Still prove it out.
    Consistent,
    /// 60 to 84.
    Review,
    /// Below 60.
    Problems,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Band::Consistent => "Checks agree",
            Band::Review => "Needs review",
            Band::Problems => "Problems found",
        }
    }
}

fn clamp01(v: f64) -> f64 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

pub fn score(i: &ScoreInputs) -> Score {
    let rules = i.rule_pass_rate.map(clamp01);
    let checked = i.drawing_matched + i.drawing_mismatched + i.drawing_unclear;
    let drawing = (checked > 0).then(|| i.drawing_matched as f64 / checked as f64);
    let ai = i.ai_confidence.map(|c| clamp01(c / 100.0));
    let parts = [(rules, W_RULES), (drawing, W_DRAWING), (ai, W_AI)];
    let (sum, weight) = parts.iter().fold((0.0, 0.0), |(s, w), (v, wt)| match v {
        Some(v) => (s + v * wt, w + wt),
        None => (s, w),
    });
    let mut value = if weight > 0.0 { sum / weight } else { 0.0 };
    let mut caps = Vec::new();
    let mut cap = |limit: f64, why: &str| {
        if value > limit {
            value = limit;
        }
        caps.push(format!("at most {:.0}%: {why}", limit * 100.0));
    };
    if i.deterministic_errors > 0 {
        cap(0.60, "the deterministic check found errors");
    }
    if i.ai_errors > 0 {
        cap(0.70, "the AI review reported errors");
    }
    if i.drawing_mismatched > 0 {
        cap(0.70, "the program disagrees with the drawing");
    }
    if rules.is_none() {
        cap(0.75, "the deterministic checker could not read this program");
    }
    if i.review_findings > 0 {
        cap(0.84, "findings need a person's review");
    }
    if checked == 0 {
        cap(0.84, "nothing was compared with a drawing");
    }
    let percent = (value * 100.0).round() as u32;
    let band = match percent {
        85.. => Band::Consistent,
        60..=84 => Band::Review,
        _ => Band::Problems,
    };
    let pct = |v: Option<f64>| v.map(|v| (v * 1000.0).round() / 10.0);
    Score { percent, band, rules: pct(rules), drawing: pct(drawing), ai: pct(ai), caps }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_average_of_all_parts() {
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(1.0),
            drawing_matched: 9,
            drawing_mismatched: 0,
            drawing_unclear: 1,
            ai_confidence: Some(80.0),
            ..Default::default()
        });
        // 0.4 * 1.0 + 0.3 * 0.9 + 0.3 * 0.8 = 0.91
        assert_eq!(s.percent, 91);
        assert_eq!(s.band, Band::Consistent);
        assert_eq!(s.drawing, Some(90.0));
        assert!(s.caps.is_empty());
    }

    #[test]
    fn missing_parts_are_renormalised_and_capped() {
        // Rules only, no drawing, no AI: 1.0 capped at 0.84.
        let s = score(&ScoreInputs { rule_pass_rate: Some(1.0), ..Default::default() });
        assert_eq!(s.percent, 84);
        assert_eq!(s.band, Band::Review);
        assert_eq!(s.caps.len(), 1);
        // Rules 0.95 and AI 0.5: (0.38 + 0.15) / 0.7 = 0.757 -> 76.
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(0.95),
            ai_confidence: Some(50.0),
            ..Default::default()
        });
        assert_eq!(s.percent, 76);
        assert_eq!(s.band, Band::Review);
    }

    #[test]
    fn errors_and_mismatches_cap_the_score() {
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(0.99),
            deterministic_errors: 1,
            drawing_matched: 10,
            ai_confidence: Some(95.0),
            ..Default::default()
        });
        assert_eq!(s.percent, 60);
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(1.0),
            drawing_matched: 9,
            drawing_mismatched: 1,
            ai_confidence: Some(90.0),
            ..Default::default()
        });
        assert_eq!(s.percent, 70);
        // Unsupported dialect, AI only at 90: capped at 75.
        let s = score(&ScoreInputs { ai_confidence: Some(90.0), drawing_matched: 3, ..Default::default() });
        assert_eq!(s.percent, 75);
        // A review finding keeps an otherwise perfect result out of "Checks agree".
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(1.0),
            drawing_matched: 4,
            ai_confidence: Some(100.0),
            review_findings: 1,
            ..Default::default()
        });
        assert_eq!(s.percent, 84);
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(1.0),
            drawing_matched: 4,
            ai_errors: 2,
            ai_confidence: Some(90.0),
            ..Default::default()
        });
        assert_eq!(s.percent, 70);
        let s = score(&ScoreInputs { rule_pass_rate: Some(0.2), ..Default::default() });
        assert_eq!(s.percent, 20);
        assert_eq!(s.band, Band::Problems);
    }

    #[test]
    fn nonsense_inputs_do_not_escape_the_range() {
        let s = score(&ScoreInputs {
            rule_pass_rate: Some(f64::NAN),
            ai_confidence: Some(500.0),
            ..Default::default()
        });
        assert!(s.percent <= 100);
        assert_eq!(score(&ScoreInputs::default()).percent, 0);
    }
}
