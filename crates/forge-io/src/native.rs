//! Native file format: versioned RON serialization of the parametric
//! document (FR-IO-05).
//!
//! The native format stores the *parametric feature tree*, sketches and
//! parameters – not meshes. File size stays small and reopening a document
//! reproduces the full edit history state.

use crate::{IoError, Result};
use forge_model::Document;
use std::path::Path;

/// Save the document as pretty RON.
pub fn save_document(path: &Path, doc: &Document) -> Result<()> {
    std::fs::write(path, document_to_string(doc)?)?;
    Ok(())
}

/// Serialize the document (pretty RON) into memory (wasm: browser
/// download; crash snapshot uses the same string).
pub fn document_to_string(doc: &Document) -> Result<String> {
    ron::ser::to_string_pretty(doc, ron::ser::PrettyConfig::default())
        .map_err(|e| IoError::Serde(format!("ron encode: {e}")))
}

/// Load a document from RON, validating the format version and migrating
/// older files to the current schema (I-06).
pub fn load_document(path: &Path) -> Result<Document> {
    let data = std::fs::read_to_string(path)?;
    document_from_str(&data)
}

/// Deserialize + migrate a document from RON text (wasm: dropped-file
/// bytes as UTF-8). Same validation path as [`load_document`].
pub fn document_from_str(data: &str) -> Result<Document> {
    let mut doc: Document =
        ron::from_str(data).map_err(|e| IoError::Serde(format!("ron decode: {e}")))?;
    if doc.format_version > forge_core::NATIVE_FORMAT_VERSION {
        return Err(IoError::Unsupported(format!(
            "file format version {} is newer than supported {}",
            doc.format_version,
            forge_core::NATIVE_FORMAT_VERSION
        )));
    }
    // Walk the stepwise migration pipeline up to the current version.
    forge_model::migrate_document(&mut doc)?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{FeatureId, Point2, Point3, SketchId, Vector3};
    use forge_model::{
        Document, ExtrudeOp, ExtrudeParams, Feature, PrimitiveKind, PrimitiveParams,
    };
    use forge_sketch::{DatumPlane, Sketch, SketchPlane};

    fn sample_doc() -> Document {
        let mut doc = Document::new("native-test");
        let mut sketch = Sketch::new(
            SketchId::new(1),
            "profile",
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
        );
        sketch.add_rectangle(Point2::new(-5.0, -5.0), Point2::new(5.0, 5.0));
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 12.0,
            direction: forge_geometry::ExtrudeDirection::Symmetric,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        }))
        .unwrap();
        doc.add_feature(Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Sphere,
            center: Point3::new(0.0, 0.0, 30.0),
            dims: Vector3::new(10.0, 0.0, 0.0),
        }))
        .unwrap();
        doc
    }

    #[test]
    fn ron_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_native.forgecad");
        let doc = sample_doc();
        save_document(&path, &doc).unwrap();
        let loaded = load_document(&path).unwrap();
        assert_eq!(loaded.name, "native-test");
        assert_eq!(loaded.tree.len(), 3);
        // The tree still evaluates identically after the roundtrip.
        let mut ev = forge_model::Evaluator::default();
        let result = ev.evaluate(&mut loaded.clone());
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bodies.len(), 2);
        // Current-version files stay current (no spurious migration).
        assert_eq!(loaded.format_version, forge_core::NATIVE_FORMAT_VERSION);
        std::fs::remove_file(&path).ok();
    }

    /// I-06: a hand-written v0 (pre-versioning) document — no
    /// `format_version` field, no `bindings`, no `next_param` — loads,
    /// migrates, and re-saves at the current version.
    #[test]
    fn legacy_v0_document_loads_and_migrates() {
        let legacy_ron = r#"
(
    name: "legacy doc",
    units: Millimeter,
    tree: (nodes: {}, order: []),
    params: {},
    allocator: (next: 1),
    modified: false,
)
"#;
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_legacy_v0.forgecad");
        std::fs::write(&path, legacy_ron).unwrap();
        let doc = load_document(&path).unwrap();
        assert_eq!(doc.name, "legacy doc");
        assert_eq!(
            doc.format_version,
            forge_core::NATIVE_FORMAT_VERSION,
            "migration must stamp the current version"
        );
        assert!(doc.bindings.is_empty());
        // Re-saving writes the current version explicitly.
        save_document(&path, &doc).unwrap();
        let reloaded = load_document(&path).unwrap();
        assert_eq!(reloaded.format_version, forge_core::NATIVE_FORMAT_VERSION);
        assert_eq!(reloaded.name, "legacy doc");
        std::fs::remove_file(&path).ok();
    }

    /// I-06: files from a newer ForgeCAD are rejected with a clear error
    /// instead of silently misreading the schema.
    #[test]
    fn newer_format_version_is_rejected() {
        let future_ron = r#"
(
    format_version: 9999,
    name: "from the future",
    units: Millimeter,
    tree: (nodes: {}, order: []),
    params: {},
    allocator: (next: 1),
    modified: false,
)
"#;
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_future.forgecad");
        std::fs::write(&path, future_ron).unwrap();
        let err = load_document(&path).expect_err("must be rejected");
        assert!(err.to_string().contains("newer"), "{err}");
        std::fs::remove_file(&path).ok();
    }
}
