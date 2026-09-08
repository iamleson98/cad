//! Feature tree: a directed acyclic graph of parametric operations with
//! topological ordering, dirty propagation, suppression and reordering.

use crate::feature::Feature;
use forge_core::FeatureId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One node of the feature tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureNode {
    /// Feature id.
    pub id: FeatureId,
    /// The feature data.
    pub feature: Feature,
    /// Parent features (dependencies).
    pub parents: BTreeSet<FeatureId>,
    /// Child features (features that depend on this one).
    pub children: BTreeSet<FeatureId>,
    /// Suppressed features are skipped during evaluation (FR-SM-04).
    pub suppressed: bool,
    /// Dirty flag: the feature (or an ancestor) changed since the last
    /// successful evaluation.
    pub dirty: bool,
}

/// The feature tree / DAG.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureTree {
    nodes: BTreeMap<FeatureId, FeatureNode>,
    /// Evaluation order (topological, also the display order).
    order: Vec<FeatureId>,
}

impl FeatureTree {
    /// Empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Node lookup.
    pub fn get(&self, id: FeatureId) -> Option<&FeatureNode> {
        self.nodes.get(&id)
    }

    /// Mutable node lookup.
    pub fn get_mut(&mut self, id: FeatureId) -> Option<&mut FeatureNode> {
        self.nodes.get_mut(&id)
    }

    /// All nodes in evaluation order.
    pub fn nodes_in_order(&self) -> impl Iterator<Item = &FeatureNode> {
        self.order.iter().filter_map(move |id| self.nodes.get(id))
    }

    /// The evaluation order (ids).
    pub fn order(&self) -> &[FeatureId] {
        &self.order
    }

    /// Number of features.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Insert a feature; dependencies become parents. The feature is
    /// appended at the end of the order (after its dependencies).
    pub fn insert(&mut self, id: FeatureId, feature: Feature) -> crate::Result<()> {
        let deps = feature.dependencies();
        for d in &deps {
            if !self.nodes.contains_key(d) {
                return Err(crate::ModelError::MissingFeature(format!("{d}")));
            }
        }
        // Cycle safety: inserting after all dependencies cannot create a
        // cycle in the order, but verify reachable sets anyway.
        for d in &deps {
            if self.reachable(*d).contains(&id) {
                return Err(crate::ModelError::Cycle(format!("{id}")));
            }
        }
        let parents: BTreeSet<FeatureId> = deps.into_iter().collect();
        for p in &parents {
            self.nodes
                .get_mut(p)
                .expect("checked above")
                .children
                .insert(id);
        }
        let node = FeatureNode {
            id,
            feature,
            parents,
            children: BTreeSet::new(),
            suppressed: false,
            dirty: true,
        };
        self.nodes.insert(id, node);
        self.order.push(id);
        self.retopologize();
        Ok(())
    }

    /// Insert a pre-built node (undo path): relinks parents and children
    /// from the stored node data.
    pub fn insert_raw(&mut self, node: FeatureNode) -> crate::Result<()> {
        let id = node.id;
        for p in &node.parents {
            if let Some(parent) = self.nodes.get_mut(p) {
                parent.children.insert(id);
            }
        }
        for c in &node.children {
            if let Some(child) = self.nodes.get_mut(c) {
                child.parents.insert(id);
                child.dirty = true;
            }
        }
        self.order.push(id);
        self.nodes.insert(id, node);
        self.retopologize();
        Ok(())
    }

    /// Insert a pre-built node at a specific position of the order
    /// (undo of a removal).
    pub fn insert_at(&mut self, node: FeatureNode, index: usize) -> crate::Result<()> {
        let id = node.id;
        self.insert_raw(node)?;
        let cur = self
            .order
            .iter()
            .position(|o| *o == id)
            .ok_or_else(|| crate::ModelError::MissingFeature(format!("{id}")))?;
        let target = index.min(self.order.len() - 1);
        if cur != target {
            self.order.remove(cur);
            self.order.insert(target, id);
        }
        if !self.order_valid() {
            self.retopologize();
        }
        Ok(())
    }

    /// Remove a feature and unlink it (its dependents keep their
    /// references and will report errors unless fixed).
    pub fn remove(&mut self, id: FeatureId) -> Option<FeatureNode> {
        let node = self.nodes.remove(&id)?;
        for p in &node.parents {
            if let Some(parent) = self.nodes.get_mut(p) {
                parent.children.remove(&id);
            }
        }
        for c in &node.children {
            if let Some(child) = self.nodes.get_mut(c) {
                child.parents.remove(&id);
                child.dirty = true;
            }
        }
        self.order.retain(|o| *o != id);
        Some(node)
    }

    /// Replace a feature's data (edit). Marks it and all descendants dirty.
    pub fn edit(&mut self, id: FeatureId, feature: Feature) -> crate::Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| crate::ModelError::MissingFeature(format!("{id}")))?;
        node.feature = feature;
        node.dirty = true;
        let children: Vec<FeatureId> = node.children.iter().copied().collect();
        self.mark_descendants_dirty(&children);
        self.retopologize();
        Ok(())
    }

    /// Toggle suppression (FR-SM-04). Descendants re-evaluate because their
    /// inputs may disappear.
    pub fn set_suppressed(&mut self, id: FeatureId, suppressed: bool) -> crate::Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| crate::ModelError::MissingFeature(format!("{id}")))?;
        node.suppressed = suppressed;
        node.dirty = true;
        let children: Vec<FeatureId> = node.children.iter().copied().collect();
        self.mark_descendants_dirty(&children);
        Ok(())
    }

    /// Mark a single feature dirty (descendants follow through the normal
    /// dirty propagation during the next evaluation). Used by parameter
    /// edits: bound dimensions change, so the feature must re-evaluate.
    pub fn mark_dirty(&mut self, id: FeatureId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.dirty = true;
            let children: Vec<FeatureId> = node.children.iter().copied().collect();
            self.mark_descendants_dirty(&children);
        }
    }

    /// Move a feature within the evaluation order. Constraints: the feature
    /// must stay after its parents and before its children.
    pub fn reorder(&mut self, id: FeatureId, new_index: usize) -> crate::Result<()> {
        let old_index = self
            .order
            .iter()
            .position(|o| *o == id)
            .ok_or_else(|| crate::ModelError::MissingFeature(format!("{id}")))?;
        if new_index >= self.order.len() {
            return Err(crate::ModelError::Invalid("index out of range".into()));
        }
        // Temporarily apply, then validate.
        let id_val = self.order.remove(old_index);
        self.order.insert(new_index.min(self.order.len()), id_val);
        if self.order_valid() {
            let affected: Vec<FeatureId> = self.order.clone();
            self.mark_descendants_dirty(&affected);
            Ok(())
        } else {
            // Roll back.
            self.order.remove(new_index.min(self.order.len() - 1));
            self.order.insert(old_index, id);
            Err(crate::ModelError::Invalid(
                "reorder would violate dependencies".into(),
            ))
        }
    }

    /// Check that the current order respects every edge (parents first).
    fn order_valid(&self) -> bool {
        let position: BTreeMap<FeatureId, usize> = self
            .order
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        for (id, node) in &self.nodes {
            if let Some(pos) = position.get(id) {
                for p in &node.parents {
                    match position.get(p) {
                        Some(pp) if pp < pos => {}
                        _ => return false,
                    }
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Recompute a valid topological order (stable: preserves current
    /// order as much as possible).
    fn retopologize(&mut self) {
        let mut remaining: BTreeMap<FeatureId, usize> = self
            .nodes
            .iter()
            .map(|(id, n)| (*id, n.parents.len()))
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        // Kahn's algorithm over a priority that prefers the previous order.
        let mut round = 0;
        while !remaining.is_empty() && round < self.nodes.len() + 1 {
            round += 1;
            let ready: Vec<FeatureId> = remaining
                .iter()
                .filter(|(_, indeg)| **indeg == 0)
                .map(|(id, _)| *id)
                .collect();
            if ready.is_empty() {
                break; // cycle (should not happen; inserts validate)
            }
            for id in ready {
                remaining.remove(&id);
                order.push(id);
                if let Some(node) = self.nodes.get(&id) {
                    for c in &node.children {
                        if let Some(deg) = remaining.get_mut(c) {
                            *deg -= 1;
                        }
                    }
                }
            }
        }
        if remaining.is_empty() {
            // Stable interleave: keep prior relative order when possible.
            let mut merged = Vec::with_capacity(order.len());
            let mut pushed = BTreeSet::new();
            for id in &self.order {
                if order.contains(id) && !pushed.contains(id) {
                    merged.push(*id);
                    pushed.insert(*id);
                }
            }
            for id in &order {
                if !pushed.contains(id) {
                    merged.push(*id);
                    pushed.insert(*id);
                }
            }
            self.order = merged;
        }
    }

    fn mark_descendants_dirty(&mut self, ids: &[FeatureId]) {
        let mut queue: Vec<FeatureId> = ids.to_vec();
        while let Some(id) = queue.pop() {
            if let Some(node) = self.nodes.get_mut(&id) {
                if !node.dirty {
                    node.dirty = true;
                    queue.extend(node.children.iter().copied());
                } else {
                    // Already dirty: children were already propagated.
                    queue.extend(node.children.iter().copied());
                }
            }
        }
    }

    /// All features reachable from `id` (transitive children).
    fn reachable(&self, id: FeatureId) -> BTreeSet<FeatureId> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            if seen.insert(cur) {
                if let Some(node) = self.nodes.get(&cur) {
                    stack.extend(node.children.iter().copied());
                }
            }
        }
        seen
    }

    /// Features with a pending (dirty) evaluation.
    pub fn dirty_features(&self) -> Vec<FeatureId> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.dirty && !n.suppressed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Clear the dirty flag after successful evaluation.
    pub fn clear_dirty(&mut self, id: FeatureId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.dirty = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{PrimitiveKind, PrimitiveParams};
    use forge_core::{Point3, Vector3};

    fn prim(id: FeatureId, name: &str) -> (FeatureId, Feature) {
        let _ = name;
        (
            id,
            Feature::Primitive(PrimitiveParams {
                kind: PrimitiveKind::Box,
                center: Point3::origin(),
                dims: Vector3::new(1.0, 1.0, 1.0),
            }),
        )
    }

    #[test]
    fn insert_and_order() {
        let mut tree = FeatureTree::new();
        let (id1, f1) = prim(FeatureId::new(1), "box1");
        let (id2, f2) = prim(FeatureId::new(2), "box2");
        tree.insert(id1, f1).unwrap();
        tree.insert(id2, f2).unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(tree.order(), &[id1, id2]);
    }

    #[test]
    fn dependencies_come_first() {
        let mut tree = FeatureTree::new();
        let (id1, f1) = prim(FeatureId::new(1), "base");
        tree.insert(id1, f1).unwrap();
        let ext = Feature::Extrude(crate::ExtrudeParams {
            profile: FeatureId::new(1),
            distance: 10.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: crate::ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        });
        let id2 = FeatureId::new(2);
        tree.insert(id2, ext).unwrap();
        let pos1 = tree.order().iter().position(|i| *i == id1).unwrap();
        let pos2 = tree.order().iter().position(|i| *i == id2).unwrap();
        assert!(pos1 < pos2);
        assert!(tree.get(id2).unwrap().parents.contains(&id1));
        assert!(tree.get(id1).unwrap().children.contains(&id2));
    }

    #[test]
    fn missing_dependency_rejected() {
        let mut tree = FeatureTree::new();
        let ext = Feature::Extrude(crate::ExtrudeParams {
            profile: FeatureId::new(99),
            distance: 10.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: crate::ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        });
        assert!(tree.insert(FeatureId::new(1), ext).is_err());
    }

    #[test]
    fn edit_marks_descendants_dirty() {
        let mut tree = FeatureTree::new();
        let (id1, f1) = prim(FeatureId::new(1), "base");
        tree.insert(id1, f1).unwrap();
        tree.clear_dirty(id1);
        let ext = Feature::Extrude(crate::ExtrudeParams {
            profile: id1,
            distance: 10.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: crate::ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        });
        let id2 = FeatureId::new(2);
        tree.insert(id2, ext).unwrap();
        tree.clear_dirty(id2);
        assert!(tree.dirty_features().is_empty());
        // Edit the base: both features become dirty again.
        let (_, f1b) = prim(id1, "base2");
        tree.edit(id1, f1b).unwrap();
        assert!(tree.get(id1).unwrap().dirty);
        assert!(tree.get(id2).unwrap().dirty);
    }

    #[test]
    fn suppression_toggles() {
        let mut tree = FeatureTree::new();
        let (id1, f1) = prim(FeatureId::new(1), "base");
        tree.insert(id1, f1).unwrap();
        tree.set_suppressed(id1, true).unwrap();
        assert!(tree.get(id1).unwrap().suppressed);
        assert!(!tree.dirty_features().contains(&id1));
    }

    #[test]
    fn remove_unlinks() {
        let mut tree = FeatureTree::new();
        let (id1, f1) = prim(FeatureId::new(1), "base");
        let (id2, f2) = prim(FeatureId::new(2), "other");
        tree.insert(id1, f1).unwrap();
        tree.insert(id2, f2).unwrap();
        assert!(tree.get(id1).is_some());
        tree.remove(id1);
        assert!(tree.get(id1).is_none());
        assert_eq!(tree.len(), 1);
    }
}
