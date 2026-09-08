//! STEP interface (FR-IO-01/02, roadmap Phase 6).
//!
//! Full STEP AP203/AP214 support requires a B-Rep kernel. The integration
//! path (per the architecture document):
//!
//! 1. **Import**: `opencascade-rs` (Rust bindings to OCCT) reads STEP,
//!    tessellates the shape at a configurable deviation, and yields a
//!    [`TriMesh`] (plus, in Phase 4+, topological face data for
//!    feature-tree reference). OCCT is a C++ dependency, so it lives
//!    behind an optional cargo feature to keep default builds pure-Rust.
//! 2. **Export**: OCCT's STEP translator writes B-Rep solids; the mesh
//!    kernel alone can only export tessellated geometry, which STEP
//!    AP214 does not standardize – hence B-Rep is a hard prerequisite.
//! 3. Interim **detection**: this module recognizes STEP/IGES files and
//!    reports a precise, actionable error instead of failing opaquely.
//!
//! [`StepFormat`] also defines the shared trait that the Phase-6 adapter
//! will implement, so the app's import/export plumbing can be wired today.

use crate::{IoError, Result};
use std::path::Path;

/// File extensions recognized as STEP.
pub const STEP_EXTENSIONS: [&str; 2] = ["step", "stp"];
/// File extensions recognized as IGES.
pub const IGES_EXTENSIONS: [&str; 2] = ["igs", "iges"];

/// The future STEP adapter contract (implemented in Phase 6 by the
/// `occt` cargo feature).
pub trait StepFormat {
    /// Import a STEP file as a tessellated body.
    fn import_step(&mut self, path: &Path) -> Result<forge_geometry::TriMesh>;
    /// Export bodies as a STEP file.
    fn export_step(&mut self, path: &Path, bodies: &[crate::ExportMesh]) -> Result<()>;
}

/// No-op adapter used until the kernel integration lands.
pub struct StepStub;

impl StepFormat for StepStub {
    fn import_step(&mut self, _path: &Path) -> Result<forge_geometry::TriMesh> {
        Err(unimplemented_step("import"))
    }

    fn export_step(&mut self, _path: &Path, _bodies: &[crate::ExportMesh]) -> Result<()> {
        Err(unimplemented_step("export"))
    }
}

fn unimplemented_step(direction: &str) -> IoError {
    IoError::Unsupported(format!(
        "STEP {direction} requires the B-Rep kernel integration (roadmap Phase 6); \
         mesh formats (STL/OBJ/glTF) and the native format are available today"
    ))
}

/// Detect whether `path` is a STEP or IGES file (by extension).
pub fn detect_cad_format(path: &Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())?;
    if STEP_EXTENSIONS.contains(&ext.as_str()) {
        Some("STEP")
    } else if IGES_EXTENSIONS.contains(&ext.as_str()) {
        Some("IGES")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_works() {
        assert_eq!(detect_cad_format(Path::new("/x/part.STEP")), Some("STEP"));
        assert_eq!(detect_cad_format(Path::new("/x/part.stp")), Some("STEP"));
        assert_eq!(detect_cad_format(Path::new("/x/part.igs")), Some("IGES"));
        assert_eq!(detect_cad_format(Path::new("/x/part.stl")), None);
    }

    #[test]
    fn stub_reports_roadmap() {
        let mut stub = StepStub;
        let err = stub.import_step(Path::new("x.step")).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Phase 6"));
    }
}
