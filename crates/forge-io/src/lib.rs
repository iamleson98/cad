//! # forge-io
//!
//! Data interchange for ForgeCAD:
//! - [`stl`] – STL ASCII + binary, read and write
//! - [`obj`] – OBJ write + read
//! - [`gltf`] – glTF 2.0 write (embedded buffer)
//! - [`threemf`] – 3MF read + write (I-02, production 3D-print format)
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
pub mod threemf;

pub use native::{document_from_str, document_to_string, load_document, save_document};
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
    /// 3MF (I-02: OPC/ZIP package, the 3D-print production format).
    ThreeMf,
}

impl ExportFormat {
    /// Canonical file extension.
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Stl => "stl",
            ExportFormat::Obj => "obj",
            ExportFormat::Gltf => "gltf",
            ExportFormat::ThreeMf => "3mf",
        }
    }
}

/// Supported import formats (I-01/I-02: mesh bodies from interchange files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// STL (binary or ASCII, auto-detected).
    Stl,
    /// Wavefront OBJ.
    Obj,
    /// 3MF (I-02: all mesh objects of the package, transforms applied).
    ThreeMf,
}

impl ImportFormat {
    /// Detect the format from a file extension (case-insensitive).
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "stl" => Some(ImportFormat::Stl),
            "obj" => Some(ImportFormat::Obj),
            "3mf" => Some(ImportFormat::ThreeMf),
            _ => None,
        }
    }

    /// Canonical file extension.
    pub fn extension(&self) -> &'static str {
        match self {
            ImportFormat::Stl => "stl",
            ImportFormat::Obj => "obj",
            ImportFormat::ThreeMf => "3mf",
        }
    }
}

/// Import a mesh file (STL or OBJ) as a repaired, welded [`TriMesh`]
/// ready to become a mesh body feature (I-01).
///
/// The pipeline is: parse → weld (`WELD_EPS`) → drop degenerate triangles
/// → orient-consistency repair (BFS across shared edges + volume-sign
/// outward fix) → recompute vertex normals. Boolean workflows work on the
/// imported body immediately.
pub fn import_mesh(
    format: ImportFormat,
    path: &std::path::Path,
) -> Result<forge_geometry::TriMesh> {
    import_meshes(format, path).map(|mut all| all.remove(0).1)
}

/// Import every mesh of a file (I-02: 3MF packages may hold several
/// objects; STL/OBJ yield one). Same repair pipeline as
/// [`import_mesh`].
pub fn import_meshes(
    format: ImportFormat,
    path: &std::path::Path,
) -> Result<Vec<(String, forge_geometry::TriMesh)>> {
    if !path.is_file() {
        return Err(IoError::Malformed(format!(
            "file not found: {}",
            path.display()
        )));
    }
    import_meshes_bytes(format, &std::fs::read(path)?)
}

/// Import meshes from in-memory bytes (W-10: browser drag-and-drop on
/// wasm supplies bytes, not paths). Same repair pipeline and format
/// detection as [`import_meshes`].
pub fn import_meshes_bytes(
    format: ImportFormat,
    data: &[u8],
) -> Result<Vec<(String, forge_geometry::TriMesh)>> {
    match format {
        ImportFormat::Stl => stl::read_stl_bytes(data).map(|m| vec![("mesh".into(), m)]),
        ImportFormat::Obj => {
            obj::read_obj_bytes(&String::from_utf8_lossy(data)).map(|m| vec![("mesh".into(), m)])
        }
        ImportFormat::ThreeMf => threemf::read_3mf_bytes(data),
    }
}

/// Export meshes in the given format to `path`.
pub fn export(format: ExportFormat, path: &std::path::Path, meshes: &[ExportMesh]) -> Result<()> {
    let bytes = export_bytes(format, meshes)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Export meshes in the given format to an in-memory buffer (W-10:
/// browser download on wasm). Produces the exact same bytes as
/// [`export`].
pub fn export_bytes(format: ExportFormat, meshes: &[ExportMesh]) -> Result<Vec<u8>> {
    if meshes.is_empty() {
        return Err(IoError::Malformed("no meshes to export".into()));
    }
    match format {
        ExportFormat::Stl => Ok(stl::binary_stl_bytes(meshes)),
        ExportFormat::Obj => Ok(obj::obj_bytes(meshes)),
        ExportFormat::Gltf => gltf::gltf_bytes(meshes),
        ExportFormat::ThreeMf => threemf::three_mf_bytes(meshes),
    }
}
