//! # forge-sketch
//!
//! 2D parametric sketcher: entities, geometric and dimensional constraints,
//! and a Levenberg–Marquardt constraint solver.
//!
//! The solver follows the approach validated by modern sketchers (e.g.
//! KittyCAD's constraint playground): all constraints are expressed as
//! scalar residual functions `r(x)` of the entity parameter vector `x`;
//! the solver minimizes `||r(x)||²` with damped Gauss–Newton steps. Fully
//! constrained systems converge to machine precision; under-constrained
//! systems settle into a least-squares solution that moves the geometry as
//! little as possible.
//!
//! Entities live on a [`SketchPlane`] (datum plane or a planar face of an
//! existing body) and are solved in plane-local 2D coordinates.

pub mod constraint;
pub mod entity;
pub mod error;
pub mod nurbs;
pub mod planes;
pub mod sketch;
pub mod solver;

pub use constraint::{Constraint, PointRole, TangentKind};
pub use entity::SketchEntity;
pub use error::SketchError;
pub use planes::{DatumPlane, SketchPlane};
pub use sketch::{Sketch, SolveReport};
pub use solver::SolveStatus;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, SketchError>;
