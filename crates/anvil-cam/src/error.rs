//! Errors returned by the CAM crate. Public functions return these instead of panicking.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CamError {
    /// A parameter is out of its valid range (negative diameter, zero step-down...).
    #[error("invalid parameter `{name}`: {reason}")]
    InvalidParameter { name: &'static str, reason: String },
    /// The geometry handed in cannot be machined as given (empty region, open loop...).
    #[error("invalid geometry: {0}")]
    InvalidGeometry(String),
    /// The tool cannot reach the region (too large, too short...).
    #[error("the tool cannot machine this region: {0}")]
    Unreachable(String),
    /// A computation would exceed its memory or time budget.
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    /// A post processor definition is malformed or breaks a rule.
    #[error("post processor: {0}")]
    Post(String),
    /// A tool or other referenced item does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A network or service error from the optional AI client.
    #[error("AI service: {0}")]
    Service(String),
}

pub type Result<T> = std::result::Result<T, CamError>;

/// Checks that a parameter is finite and strictly positive.
pub(crate) fn positive(name: &'static str, v: f64) -> Result<f64> {
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(CamError::InvalidParameter { name, reason: format!("must be a positive number, got {v}") })
    }
}

/// Checks that a parameter is finite and not negative.
pub(crate) fn non_negative(name: &'static str, v: f64) -> Result<f64> {
    if v.is_finite() && v >= 0.0 {
        Ok(v)
    } else {
        Err(CamError::InvalidParameter { name, reason: format!("must be zero or more, got {v}") })
    }
}

/// Checks that a parameter is finite.
pub(crate) fn finite(name: &'static str, v: f64) -> Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(CamError::InvalidParameter { name, reason: format!("must be a finite number, got {v}") })
    }
}
