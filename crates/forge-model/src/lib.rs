//! # forge-model
//!
//! The parametric document model: feature tree (DAG), evaluation with
//! dirty propagation, and the undo/redo command stack.
//!
//! Architecture notes:
//! - The [`Document`] holds *parametric data only* (tree, sketches,
//!   parameters) – no meshes. This keeps snapshots cheap, which is what
//!   makes the unbounded undo/redo history (FR-UI-04) practical.
//! - Mesh results live in a separate [`Evaluator`] cache and are rebuilt
//!   from dirty features only (FR-SM-04: history-based DAG re-evaluation).
//! - Feature evaluation is wrapped in [`std::panic::catch_unwind`]
//!   (NFR-RES-03): a failing solid operation degrades to an error message
//!   on that feature, never a crash.

pub mod command;
pub mod document;
pub mod error;
pub mod evaluate;
pub mod feature;
pub mod selection;
pub mod tree;

pub use command::{Command, CommandStack};
pub use document::{Document, Param};
pub use error::ModelError;
pub use evaluate::{EvalBody, Evaluation, Evaluator};
pub use feature::{
    BooleanFeature, ExtrudeOp, ExtrudeParams, Feature, LoftParams, PrimitiveKind,
    PrimitiveParams, RevolveParams, SweepParams,
};
pub use selection::{Selection, SelectionItem};
pub use tree::{FeatureNode, FeatureTree};

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ModelError>;
