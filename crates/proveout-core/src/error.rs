//! Errors. Public functions return these instead of panicking.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum Error {
    /// An input is missing, malformed or out of range.
    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    /// An input is larger than the limit for the chosen level or plan.
    #[error("too large: {0}")]
    TooLarge(String),
    /// The AI service failed, declined or replied with something unusable.
    #[error("AI service: {0}")]
    Service(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Error {
    Error::Invalid { field, reason: reason.into() }
}
