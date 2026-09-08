//! Native-format version migration pipeline (I-06).
//!
//! Documents written by older ForgeCAD builds must stay loadable for at
//! least one major release. The loader decodes whatever RON it can
//! (schema drift is absorbed by `#[serde(default)]` fields), then walks
//! the document **stepwise** — v0→v1, v1→v2, … — up to
//! [`forge_core::NATIVE_FORMAT_VERSION`]. Documents claiming a *newer*
//! version are rejected by the loader before this pipeline runs.
//!
//! Rules for contributors:
//! 1. One function per step, named `vN_to_vM`, kept small and
//!    deterministic.
//! 2. Never mutate data destructively — a migration must be idempotent:
//!    running it twice yields the same document.
//! 3. Add a test per step, plus a fixture in `forge-io`'s `native`
//!    module that round-trips a hand-written old-format file.

use crate::{Document, ModelError, Result};

/// Migrate a loaded document up to the current
/// [`forge_core::NATIVE_FORMAT_VERSION`].
pub fn migrate_document(doc: &mut Document) -> Result<()> {
    while doc.format_version < forge_core::NATIVE_FORMAT_VERSION {
        match doc.format_version {
            0 => v0_to_v1(doc)?,
            other => {
                return Err(ModelError::Invalid(format!(
                    "no migration path from format version {other}"
                )))
            }
        }
    }
    Ok(())
}

/// v0 → v1: documents from the pre-versioning era (the field did not
/// exist, so serde defaults it to 0).
///
/// The schema is already compatible through serde defaults; the
/// substantive repair is **allocator hygiene**: early builds could save
/// an allocator whose `next` counter sat below feature ids already
/// present in the tree, so the first feature added after loading would
/// collide with an existing id and silently corrupt references. The
/// migration reserves the allocator above every feature id in the tree.
fn v0_to_v1(doc: &mut Document) -> Result<()> {
    for id in doc.tree.order() {
        doc.allocator.reserve(id.raw());
    }
    doc.format_version = 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Feature, PrimitiveKind, PrimitiveParams};
    use forge_core::{Point3, Vector3};

    #[test]
    fn v0_allocator_collision_is_repaired() {
        let mut doc = Document::new("legacy");
        // One feature (id 1) already in the tree …
        doc.add_feature(Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Box,
            center: Point3::origin(),
            dims: Vector3::new(10.0, 10.0, 10.0),
        }))
        .unwrap();
        assert_eq!(doc.tree.order().len(), 1);
        let first_id = doc.tree.order()[0].raw();
        // … but the allocator was reset by an early bug: it would hand
        // out colliding ids.
        doc.allocator = forge_core::IdAllocator::starting_at(1);
        doc.format_version = 0;

        migrate_document(&mut doc).expect("migration");
        assert_eq!(doc.format_version, forge_core::NATIVE_FORMAT_VERSION);
        // The next issued id must clear every existing feature id.
        let next = doc.next_feature_id();
        assert!(next.raw() > first_id);
        assert!(doc.tree.order().iter().all(|id| *id != next));
    }

    #[test]
    fn current_version_documents_are_no_ops() {
        let mut doc = Document::new("current");
        doc.format_version = forge_core::NATIVE_FORMAT_VERSION;
        migrate_document(&mut doc).expect("migration");
        assert_eq!(doc.format_version, forge_core::NATIVE_FORMAT_VERSION);
        // Idempotence: migrating again changes nothing.
        let before = format!("{:?}", doc.tree.order());
        migrate_document(&mut doc).expect("migration");
        assert_eq!(before, format!("{:?}", doc.tree.order()));
    }

    #[test]
    fn unknown_version_has_no_path() {
        // A version with no registered step (but below "current", so the
        // loader would hand it to us) must fail loudly instead of looping.
        let mut doc = Document::new("weird");
        doc.format_version = 7;
        if forge_core::NATIVE_FORMAT_VERSION > 7 {
            assert!(migrate_document(&mut doc).is_err());
        } else {
            // When current grows past 7, a v7 step must exist.
            assert!(migrate_document(&mut doc).is_ok());
        }
    }
}
