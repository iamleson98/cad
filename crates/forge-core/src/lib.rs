//! # forge-core
//!
//! Shared foundation crate for **ForgeCAD**.
//!
//! This crate is the dependency-free (apart from math/serde) base that every
//! other crate in the workspace builds on. It deliberately contains *no*
//! geometry algorithms and *no* rendering code so it can be depended on by
//! both the CPU geometry engine ([`forge_geometry`]) and the GPU renderer
//! ([`forge_render`]) without dragging in the other side.
//!
//! Contents:
//! - [`ids`] – strongly typed identifiers for entities, features, bodies, …
//! - [`math`] – `f64` CAD math on top of `nalgebra` (points, vectors, planes,
//!   bounding boxes, transforms)
//! - [`units`] – length/angle units with conversion helpers (model unit: mm)
//! - [`error`] – common error type
//! - [`tessellation`] – tessellation quality settings shared between
//!   evaluation and rendering

pub mod error;
pub mod ids;
pub mod math;
pub mod tessellation;
pub mod units;

pub use error::CoreError;
pub use ids::{
    BodyId, EdgeId, EntityId, FaceId, FeatureId, ParamId, SketchId, VertexId, IdAllocator,
};
pub use math::{
    lerp, ray_triangle, BBox3, Plane, Point2, Point3, Ray3, Transform, UnitVector3, Vector2,
    Vector3,
};
pub use tessellation::TessellationConfig;
pub use units::{AngleUnit, LengthUnit};

/// Result type used across the non-UI crates.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Semantic version of the native file format written by [`forge_io`].
/// Bump whenever the serialized schema changes in a backwards-incompatible
/// way. Older versions must be importable for at least one major release.
pub const NATIVE_FORMAT_VERSION: u32 = 1;
