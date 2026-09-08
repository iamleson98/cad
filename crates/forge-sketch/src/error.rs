//! Sketch crate error type.

use thiserror::Error;

/// Errors of the sketcher.
#[derive(Debug, Error)]
pub enum SketchError {
    /// The constraint system could not be solved.
    #[error("constraint system did not converge (residual {0:.3e})")]
    NotConverged(f64),
    /// A constraint references a missing entity.
    #[error("constraint references a missing entity")]
    BrokenReference,
    /// Invalid input.
    #[error("invalid sketch input: {0}")]
    Invalid(String),
}
