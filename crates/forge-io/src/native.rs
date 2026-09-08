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
    let data = ron::ser::to_string_pretty(doc, ron::ser::PrettyConfig::default())
        .map_err(|e| IoError::Serde(format!("ron encode: {e}")))?;
    std::fs::write(path, data)?;
    Ok(())
}

/// Load a document from RON, validating the format version.
pub fn load_document(path: &Path) -> Result<Document> {
    let data = std::fs::read_to_string(path)?;
    let doc: Document = ron::from_str(&data)
        .map_err(|e| IoError::Serde(format!("ron decode: {e}")))?;
    if doc.format_version > forge_core::NATIVE_FORMAT_VERSION {
        return Err(IoError::Unsupported(format!(
            "file format version {} is newer than supported {}",
            doc.format_version,
            forge_core::NATIVE_FORMAT_VERSION
        )));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_model::{
        Document, ExtrudeOp, ExtrudeParams, Feature, PrimitiveKind, PrimitiveParams,
    };
    use forge_core::{FeatureId, Point2, Point3, SketchId, Vector3};
    use forge_sketch::{DatumPlane, Sketch, SketchPlane};

    fn sample_doc() -> Document {
        let mut doc = Document::new("native-test");
        let mut sketch = Sketch::new(SketchId::new(1), "profile", SketchPlane::Datum {
            datum: DatumPlane::XY,
        });
        sketch.add_rectangle(Point2::new(-5.0, -5.0), Point2::new(5.0, 5.0));
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 12.0,
            direction: forge_geometry::ExtrudeDirection::Symmetric,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
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
        std::fs::remove_file(&path).ok();
    }
}
