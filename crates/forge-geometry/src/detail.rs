//! Detailing operations: fillet, chamfer, shell, offset.
//!
//! **Status (v0.1):** these operations require topological (B-Rep) face
//! and edge information, which the mesh kernel does not carry. The API
//! surface is defined here so the feature tree, UI and file format can
//! already reference them; every call returns
//! [`forge_core::CoreError::NotImplemented`] until the Phase-4 B-Rep
//! integration (see the roadmap chapter of the SRS document) lands.
//!
//! The Phase-4 plan, in order:
//! 1. edge classification on B-Rep faces (variable-radius profiles),
//! 2. rolling-ball surface blending + trim/stitch against adjacent faces,
//! 3. shell: offset face removal + wall offsetting with corner resolution,
//! 4. mesh tessellation of the new faces for the preview path.

use crate::error::Result;
use crate::mesh::TriMesh;
use forge_core::{EdgeId, FaceId};

/// Apply a constant-radius fillet to the given edges.
pub fn fillet(_mesh: &TriMesh, _edges: &[EdgeId], _radius: f64) -> Result<TriMesh> {
    not_implemented("variable-radius fillet")
}

/// Apply a chamfer of the given distance to the given edges.
pub fn chamfer(_mesh: &TriMesh, _edges: &[EdgeId], _distance: f64) -> Result<TriMesh> {
    not_implemented("chamfer")
}

/// Hollow the solid, keeping walls of `thickness` and opening the listed
/// faces.
pub fn shell(_mesh: &TriMesh, _thickness: f64, _open_faces: &[FaceId]) -> Result<TriMesh> {
    not_implemented("shell")
}

/// Offset a body by `distance` (positive grows outward).
pub fn offset(_mesh: &TriMesh, _distance: f64) -> Result<TriMesh> {
    not_implemented("offset body")
}

fn not_implemented(what: &str) -> Result<TriMesh> {
    Err(crate::error::GeometryError::Core(
        forge_core::CoreError::NotImplemented(format!(
            "{what} requires the B-Rep kernel (roadmap Phase 4)"
        )),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillet_reports_not_implemented() {
        let err = fillet(&TriMesh::default(), &[], 2.0).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("B-Rep"));
    }
}
