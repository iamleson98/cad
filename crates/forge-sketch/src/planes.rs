//! Sketch planes: where a sketch lives in 3D space.
//!
//! FR-SK-03: sketches can be defined on the three standard datum planes
//! or on arbitrary planar faces of existing geometry.

use forge_core::{BodyId, FaceId, FeatureId, Plane, Point3, Vector3};
use serde::{Deserialize, Serialize};

/// The three standard datum planes through the origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatumPlane {
    /// The XY plane (normal +Z).
    XY,
    /// The YZ plane (normal +X).
    YZ,
    /// The XZ plane (normal +Y).
    XZ,
}

impl DatumPlane {
    /// The 3D plane of this datum.
    pub fn to_plane(self) -> Plane {
        let (origin, normal) = match self {
            DatumPlane::XY => (Point3::origin(), Vector3::z()),
            DatumPlane::YZ => (Point3::origin(), Vector3::x()),
            DatumPlane::XZ => (Point3::origin(), Vector3::y()),
        };
        Plane::new(origin, normal).expect("datum normals are unit length")
    }
}

/// Carrier of a sketch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SketchPlane {
    /// A standard datum plane.
    Datum {
        /// Which datum.
        datum: DatumPlane,
    },
    /// A planar face of an existing body (captured as a plane snapshot +
    /// the face reference for re-evaluation).
    Face {
        /// Snapshot of the face's plane (kept stable across re-evals as
        /// long as the referenced feature is unchanged).
        plane: Plane,
        /// The body the face belongs to.
        body: BodyId,
        /// The face reference.
        face: FaceId,
    },
    /// An arbitrary plane defined by a point and normal.
    Offset {
        /// The plane itself.
        plane: Plane,
    },
    /// A reference to a user datum feature (D-01). The plane is resolved
    /// by the document model at evaluation time (the sketch crate cannot
    /// see the feature tree); [`SketchPlane::to_plane`] falls back to the
    /// XY datum for this variant, so **always** resolve through the model.
    DatumRef {
        /// The datum feature carrying this sketch.
        feature: FeatureId,
    },
}

impl SketchPlane {
    /// The 3D plane of this sketch plane.
    ///
    /// For [`SketchPlane::DatumRef`] this returns the XY datum as a
    /// fallback — the real plane must be resolved against the feature tree
    /// by the caller (see `forge_model`'s plane resolution).
    pub fn to_plane(&self) -> Plane {
        match self {
            SketchPlane::Datum { datum } => datum.to_plane(),
            SketchPlane::Face { plane, .. } | SketchPlane::Offset { plane } => *plane,
            SketchPlane::DatumRef { .. } => DatumPlane::XY.to_plane(),
        }
    }

    /// The datum feature this carrier references, if any (D-01).
    pub fn datum_ref(&self) -> Option<FeatureId> {
        match self {
            SketchPlane::DatumRef { feature } => Some(*feature),
            _ => None,
        }
    }
}

impl Default for SketchPlane {
    fn default() -> Self {
        SketchPlane::Datum {
            datum: DatumPlane::XY,
        }
    }
}
