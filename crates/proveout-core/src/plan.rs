//! Review levels (which model and how hard it thinks) and account plans (how many
//! reviews and how much storage). Prices and allowances here are starting values for
//! the operator of the site to change; they are not tied to anything else.

use serde::{Deserialize, Serialize};

/// How thorough the AI review is. Better levels use a more capable model and cost
/// more credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLevel {
    Standard,
    High,
    Maximum,
}

impl ReviewLevel {
    pub const ALL: [ReviewLevel; 3] = [ReviewLevel::Standard, ReviewLevel::High, ReviewLevel::Maximum];

    pub fn model(self) -> &'static str {
        match self {
            ReviewLevel::Standard => "claude-sonnet-5-5",
            ReviewLevel::High => "claude-opus-5-5",
            ReviewLevel::Maximum => "claude-fable-5-1",
        }
    }

    pub fn effort(self) -> &'static str {
        match self {
            ReviewLevel::Standard => "medium",
            ReviewLevel::High | ReviewLevel::Maximum => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ReviewLevel::Standard => "Standard",
            ReviewLevel::High => "High accuracy",
            ReviewLevel::Maximum => "Maximum accuracy",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ReviewLevel::Standard => "Claude Sonnet 5.5. Good for routine programs.",
            ReviewLevel::High => "Claude Opus 5.5 thinking harder. Better at long programs and drawings.",
            ReviewLevel::Maximum => {
                "Claude Fable 5.1, Anthropic's most capable model. Slowest and most thorough."
            }
        }
    }

    /// Credits one review costs.
    pub fn credits(self) -> u32 {
        match self {
            ReviewLevel::Standard => 1,
            ReviewLevel::High => 3,
            ReviewLevel::Maximum => 8,
        }
    }

    /// Largest program sent to the model at this level. Programs are never cut short
    /// silently: a larger one is refused with a message, and the deterministic check
    /// still runs on all of it.
    pub fn max_program_bytes(self) -> usize {
        match self {
            ReviewLevel::Standard => 300 * 1024,
            ReviewLevel::High => 1024 * 1024,
            ReviewLevel::Maximum => 2 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    Free,
    Pro,
    Shop,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlanInfo {
    pub plan: Plan,
    pub name: &'static str,
    /// US dollars a month; 0 for free.
    pub price_usd: u32,
    pub credits_per_month: u32,
    pub levels: Vec<ReviewLevel>,
    pub storage_bytes: u64,
    pub summary: &'static str,
}

impl Plan {
    pub const ALL: [Plan; 3] = [Plan::Free, Plan::Pro, Plan::Shop];

    pub fn info(self) -> PlanInfo {
        match self {
            Plan::Free => PlanInfo {
                plan: self,
                name: "Free",
                price_usd: 0,
                credits_per_month: 3,
                levels: vec![ReviewLevel::Standard],
                storage_bytes: 5 * 1024 * 1024,
                summary: "Unlimited deterministic checks, 3 Standard AI reviews a month, 5 MB of saved reports.",
            },
            Plan::Pro => PlanInfo {
                plan: self,
                name: "Pro",
                price_usd: 29,
                credits_per_month: 60,
                levels: vec![ReviewLevel::Standard, ReviewLevel::High],
                storage_bytes: 1024 * 1024 * 1024,
                summary: "60 credits a month, High accuracy reviews, 1 GB of saved reports and projects.",
            },
            Plan::Shop => PlanInfo {
                plan: self,
                name: "Shop",
                price_usd: 99,
                credits_per_month: 250,
                levels: ReviewLevel::ALL.to_vec(),
                storage_bytes: 10 * 1024 * 1024 * 1024,
                summary: "250 credits a month, every level including Maximum, 10 GB of storage.",
            },
        }
    }

    pub fn allows(self, level: ReviewLevel) -> bool {
        self.info().levels.contains(&level)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Plan::Free => "free",
            Plan::Pro => "pro",
            Plan::Shop => "shop",
        }
    }

    pub fn parse(s: &str) -> Option<Plan> {
        Plan::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// Why a review cannot start, if it cannot.
pub fn can_review(plan: Plan, level: ReviewLevel, credits_used: u32) -> Result<(), String> {
    let info = plan.info();
    if !plan.allows(level) {
        return Err(format!("{} reviews need a paid plan that includes them", level.label()));
    }
    if credits_used.saturating_add(level.credits()) > info.credits_per_month {
        return Err(format!(
            "this review needs {} credit(s) and {} of {} are left this month",
            level.credits(),
            info.credits_per_month.saturating_sub(credits_used),
            info.credits_per_month
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_plan_gets_three_standard_reviews() {
        assert!(can_review(Plan::Free, ReviewLevel::Standard, 0).is_ok());
        assert!(can_review(Plan::Free, ReviewLevel::Standard, 2).is_ok());
        assert!(can_review(Plan::Free, ReviewLevel::Standard, 3).is_err());
        assert!(can_review(Plan::Free, ReviewLevel::High, 0).is_err());
        assert!(can_review(Plan::Pro, ReviewLevel::Maximum, 0).is_err());
        assert!(can_review(Plan::Shop, ReviewLevel::Maximum, 242).is_ok());
        assert!(can_review(Plan::Shop, ReviewLevel::Maximum, 243).is_err());
        assert_eq!(Plan::parse("pro"), Some(Plan::Pro));
        assert_eq!(Plan::parse("gold"), None);
    }
}
