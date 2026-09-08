//! Command-pattern undo/redo with unbounded history (FR-UI-04).
//!
//! Commands store exact before/after data (not whole-document snapshots),
//! so memory grows with the number of *edits*, not their size. The
//! `Document` itself holds no meshes, which keeps this practical.

use crate::document::Document;
use crate::feature::Feature;
use crate::tree::FeatureNode;
use forge_core::{FeatureId, ParamId};
use serde::{Deserialize, Serialize};

/// One reversible edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// Feature added (stores the full node for exact restoration).
    AddFeature {
        /// The node that was added.
        node: FeatureNode,
    },
    /// Feature removed.
    RemoveFeature {
        /// The removed node.
        node: FeatureNode,
        /// Its position in the evaluation order.
        index: usize,
    },
    /// Feature edited (params replaced).
    EditFeature {
        /// Feature id.
        id: FeatureId,
        /// Feature data before the edit.
        before: Box<Feature>,
        /// Feature data after the edit.
        after: Box<Feature>,
    },
    /// Suppression toggled.
    SetSuppressed {
        /// Feature id.
        id: FeatureId,
        /// Value before.
        before: bool,
        /// Value after.
        after: bool,
    },
    /// Feature moved in the evaluation order.
    Reorder {
        /// Feature id.
        id: FeatureId,
        /// Position before.
        before: usize,
        /// Position after.
        after: usize,
    },
    /// Parameter created/changed/removed.
    SetParam {
        /// Parameter id.
        id: ParamId,
        /// Value before (None = did not exist).
        before: Option<crate::document::Param>,
        /// Value after (None = removed).
        after: Option<crate::document::Param>,
    },
}

impl Command {
    /// Short description for the undo/redo menu.
    pub fn describe(&self) -> String {
        match self {
            Command::AddFeature { node } => format!("Add {}", node.feature.label()),
            Command::RemoveFeature { node, .. } => format!("Delete {}", node.feature.label()),
            Command::EditFeature { after, .. } => format!("Edit {}", after.label()),
            Command::SetSuppressed { id, after, .. } => {
                format!("{} {id}", if *after { "Suppress" } else { "Unsuppress" })
            }
            Command::Reorder { .. } => "Reorder feature".into(),
            Command::SetParam { after, .. } => match after {
                Some(p) => format!("Set parameter {}", p.name),
                None => "Remove parameter".into(),
            },
        }
    }

    /// Apply the command's effect to the document.
    pub fn apply(&self, doc: &mut Document) -> crate::Result<()> {
        match self {
            Command::AddFeature { node } => {
                let id = node.id;
                doc.tree.insert_raw(node.clone())?;
                doc.modified = true;
                let _ = id;
                Ok(())
            }
            Command::RemoveFeature { node, .. } => {
                doc.tree.remove(node.id);
                doc.modified = true;
                Ok(())
            }
            Command::EditFeature { id, after, .. } => {
                doc.tree.edit(*id, (**after).clone())?;
                doc.modified = true;
                Ok(())
            }
            Command::SetSuppressed { id, after, .. } => {
                doc.tree.set_suppressed(*id, *after)?;
                doc.modified = true;
                Ok(())
            }
            Command::Reorder { id, after, .. } => {
                doc.tree.reorder(*id, *after)?;
                doc.modified = true;
                Ok(())
            }
            Command::SetParam { id, after, .. } => match after {
                Some(p) => {
                    doc.params.insert(*id, p.clone());
                    doc.modified = true;
                    Ok(())
                }
                None => {
                    doc.params.remove(id);
                    doc.modified = true;
                    Ok(())
                }
            },
        }
    }

    /// Revert the command's effect.
    pub fn revert(&self, doc: &mut Document) -> crate::Result<()> {
        match self {
            Command::AddFeature { node } => {
                doc.tree.remove(node.id);
                doc.modified = true;
                Ok(())
            }
            Command::RemoveFeature { node, index } => {
                doc.tree.insert_at(node.clone(), *index)?;
                doc.modified = true;
                Ok(())
            }
            Command::EditFeature { id, before, .. } => {
                doc.tree.edit(*id, (**before).clone())?;
                doc.modified = true;
                Ok(())
            }
            Command::SetSuppressed { id, before, .. } => {
                doc.tree.set_suppressed(*id, *before)?;
                doc.modified = true;
                Ok(())
            }
            Command::Reorder { id, before, .. } => {
                doc.tree.reorder(*id, *before)?;
                doc.modified = true;
                Ok(())
            }
            Command::SetParam { id, before, .. } => match before {
                Some(p) => {
                    doc.params.insert(*id, p.clone());
                    doc.modified = true;
                    Ok(())
                }
                None => {
                    doc.params.remove(id);
                    doc.modified = true;
                    Ok(())
                }
            },
        }
    }
}

/// Unbounded undo/redo stack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl CommandStack {
    /// New empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute and record a command.
    pub fn execute(&mut self, cmd: Command, doc: &mut Document) -> crate::Result<()> {
        cmd.apply(doc)?;
        self.undo.push(cmd);
        self.redo.clear(); // branching history is discarded
        Ok(())
    }

    /// Undo the most recent command. Returns its description.
    pub fn undo(&mut self, doc: &mut Document) -> crate::Result<String> {
        let cmd = self
            .undo
            .pop()
            .ok_or_else(|| crate::ModelError::EmptyStack("undo".into()))?;
        let desc = cmd.describe();
        cmd.revert(doc)?;
        self.redo.push(cmd);
        Ok(desc)
    }

    /// Redo the most recently undone command.
    pub fn redo(&mut self, doc: &mut Document) -> crate::Result<String> {
        let cmd = self
            .redo
            .pop()
            .ok_or_else(|| crate::ModelError::EmptyStack("redo".into()))?;
        let desc = cmd.describe();
        cmd.apply(doc)?;
        self.undo.push(cmd);
        Ok(desc)
    }

    /// Whether undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo depth (unbounded by design).
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Descriptions of the undo stack (most recent first).
    pub fn undo_labels(&self) -> Vec<String> {
        self.undo.iter().rev().map(|c| c.describe()).collect()
    }

    /// Clear all history (e.g. after loading a file).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{Feature, PrimitiveKind, PrimitiveParams};
    use forge_core::{Point3, Vector3};

    fn box_feature(size: f64) -> Feature {
        Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Box,
            center: Point3::origin(),
            dims: Vector3::new(size, size, size),
        })
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut doc = Document::new("t");
        let mut stack = CommandStack::new();

        // Add a feature.
        doc.add_feature(box_feature(10.0)).unwrap();
        let node = doc.tree.get(FeatureId::new(1)).cloned().unwrap();
        stack
            .execute(Command::AddFeature { node }, &mut doc)
            .unwrap();
        assert_eq!(doc.tree.len(), 1);
        assert!(stack.can_undo());

        // Edit it.
        stack
            .execute(
                Command::EditFeature {
                    id: FeatureId::new(1),
                    before: Box::new(box_feature(10.0)),
                    after: Box::new(box_feature(20.0)),
                },
                &mut doc,
            )
            .unwrap();

        // Undo twice, redo twice.
        stack.undo(&mut doc).unwrap();
        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.tree.len(), 0);
        stack.redo(&mut doc).unwrap();
        stack.redo(&mut doc).unwrap();
        assert_eq!(doc.tree.len(), 1);
        match doc.feature(FeatureId::new(1)).unwrap() {
            Feature::Primitive(p) => {
                assert!((p.dims.x - 20.0).abs() < 1e-9);
            }
            _ => panic!("expected primitive"),
        }
    }

    #[test]
    fn unbounded_depth() {
        let mut doc = Document::new("t");
        let mut stack = CommandStack::new();
        for i in 0..500 {
            doc.add_feature(box_feature(i as f64 + 1.0)).unwrap();
            let node = doc.tree.get(FeatureId::new((i + 1) as u64)).cloned().unwrap();
            stack.execute(Command::AddFeature { node }, &mut doc).unwrap();
        }
        assert_eq!(stack.undo_depth(), 500);
        for _ in 0..500 {
            stack.undo(&mut doc).unwrap();
        }
        assert_eq!(doc.tree.len(), 0);
        assert!(!stack.can_undo());
    }

    #[test]
    fn redo_branch_cleared_by_new_command() {
        let mut doc = Document::new("t");
        let mut stack = CommandStack::new();
        let id1 = doc.add_feature(box_feature(5.0)).unwrap();
        let node = doc.tree.get(id1).cloned().unwrap();
        stack.execute(Command::AddFeature { node }, &mut doc).unwrap();
        stack.undo(&mut doc).unwrap();
        assert!(stack.can_redo());
        // A new command discards the redo branch. Note: the id allocator
        // never rolls back, so this feature gets a *new* id.
        let id2 = doc.add_feature(box_feature(7.0)).unwrap();
        assert_ne!(id1, id2, "feature ids are never reused");
        let node2 = doc.tree.get(id2).cloned().unwrap();
        stack
            .execute(Command::AddFeature { node: node2 }, &mut doc)
            .unwrap();
        assert!(!stack.can_redo());
    }
}
