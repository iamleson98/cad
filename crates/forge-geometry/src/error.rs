//! Error type of the geometry engine.

use forge_core::CoreError;
use thiserror::Error;

/// Errors produced by the geometry kernel.
#[derive(Debug, Error)]
pub enum GeometryError {
    /// A profile (closed 2D contour) was empty or degenerate.
    #[error("empty or degenerate profile")]
    EmptyProfile,
    /// A contour self-intersects, which makes triangulation unreliable.
    #[error("contour self-intersection detected near {0:?}")]
    SelfIntersecting(String),
    /// The requested operation produced no output geometry.
    #[error("empty result: {0}")]
    EmptyResult(String),
    /// Input was geometrically invalid (degenerate normal, zero area, …).
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, GeometryError>;
