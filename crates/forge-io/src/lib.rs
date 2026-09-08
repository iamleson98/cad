//! # forge-io
//!
//! Data interchange for ForgeCAD:
//! - [`stl`] – STL ASCII + binary, read and write
//! - [`obj`] – OBJ write
//! - [`gltf`] – glTF 2.0 write (embedded buffer)
//! - [`native`] – versioned RON serialization of the parametric document
//! - [`step`] – STEP interface (trait-gated, Phase-6 roadmap)
//!
//! All mesh exports honor [`TessellationConfig`] via the caller's
//! evaluation pass (FR-IO-04: configurable tessellation density).

pub mod gltf;
pub mod native;
pub mod obj;
pub mod step;
pub mod stl;

pub use native::{load_document, save_document};
pub use step::{StepFormat, STEP_EXTENSIONS};

/// I/O error type.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// Filesystem error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Malformed data.
    #[error("malformed data: {0}")]
    Malformed(String),
    /// Unsupported format or version.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Native serialization error.
    #[error("serialization error: {0}")]
    Serde(String),
    /// Missing feature references.
    #[error(transparent)]
    Model(#[from] forge_model::ModelError),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, IoError>;

/// A named mesh to export.
#[derive(Debug, Clone)]
pub struct ExportMesh {
    /// Body name.
    pub name: String,
    /// Triangle soup (positions + flat indices).
    pub mesh: forge_geometry::TriMesh,
}

/// Supported export formats (dispatch helper for the app layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Binary STL.
    Stl,
    /// Wavefront OBJ.
    Obj,
    /// glTF 2.0 (embedded buffer).
    Gltf,
}

impl ExportFormat {
    /// Canonical file extension.
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Stl => "stl",
            ExportFormat::Obj => "obj",
            ExportFormat::Gltf => "gltf",
        }
    }
}

/// Export meshes in the given format to `path`.
pub fn export(format: ExportFormat, path: &std::path::Path, meshes: &[ExportMesh]) -> Result<()> {
    if meshes.is_empty() {
        return Err(IoError::Malformed("no meshes to export".into()));
    }
    match format {
        ExportFormat::Stl => stl::write_binary_stl(path, meshes),
        ExportFormat::Obj => obj::write_obj(path, meshes),
        ExportFormat::Gltf => gltf::write_gltf(path, meshes),
    }
}
