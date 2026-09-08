//! Common error type for the foundation crate.

use thiserror::Error;

/// Errors shared across crates.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A computation received degenerate input (zero-length vector,
    /// collapsed triangle, …).
    #[error("degenerate geometry: {0}")]
    Degenerate(String),
    /// A value was outside its valid domain.
    #[error("invalid value: {0}")]
    Invalid(String),
    /// An operation is known but not implemented yet (see the roadmap in the
    /// SRS document).
    #[error("not implemented yet: {0}")]
    NotImplemented(String),
}

/// Format a `f64` with a fixed precision for error messages / UI hints.
pub fn fmt_f64(v: f64, digits: usize) -> String {
    format!("{:.*}", digits, v)
}
