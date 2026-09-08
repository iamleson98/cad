//! Document-level error type.

use thiserror::Error;

/// Errors of the document model.
#[derive(Debug, Error)]
pub enum ModelError {
    /// A feature referenced by another feature is missing.
    #[error("missing referenced feature {0}")]
    MissingFeature(String),
    /// The feature graph contains a cycle (it must be a DAG).
    #[error("the feature graph contains a cycle at {0}")]
    Cycle(String),
    /// An entity referenced by a constraint/feature is missing.
    #[error("missing referenced entity {0}")]
    MissingEntity(String),
    /// Invalid parameter value.
    #[error("invalid parameter: {0}")]
    Invalid(String),
    /// Geometry failure propagated from the kernel.
    #[error(transparent)]
    Geometry(#[from] forge_geometry::GeometryError),
    /// Sketch failure.
    #[error(transparent)]
    Sketch(#[from] forge_sketch::SketchError),
    /// A feature's evaluation panicked (caught by `catch_unwind`).
    #[error("feature evaluation panicked: {0}")]
    Panicked(String),
    /// Undo/redo stack is empty.
    #[error("nothing to {0}")]
    EmptyStack(String),
}
