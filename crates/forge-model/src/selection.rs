//! Selection state: what the user has picked in the viewport.

use forge_core::{BodyId, EdgeId, FaceId, VertexId};
use serde::{Deserialize, Serialize};

/// A single picked element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionItem {
    /// A whole body.
    Body(BodyId),
    /// A face of a body.
    Face {
        /// Owning body.
        body: BodyId,
        /// Face reference.
        face: FaceId,
    },
    /// An edge of a body.
    Edge {
        /// Owning body.
        body: BodyId,
        /// Edge reference.
        edge: EdgeId,
    },
    /// A vertex of a body.
    Vertex {
        /// Owning body.
        body: BodyId,
        /// Vertex reference.
        vertex: VertexId,
    },
}

impl SelectionItem {
    /// The body this item belongs to.
    pub fn body(&self) -> BodyId {
        match self {
            SelectionItem::Body(b) => *b,
            SelectionItem::Face { body, .. }
            | SelectionItem::Edge { body, .. }
            | SelectionItem::Vertex { body, .. } => *body,
        }
    }
}

/// The current selection (multi-select with Ctrl).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    /// Picked items, in pick order.
    pub items: Vec<SelectionItem>,
    /// Extra selection used by command targets (e.g. which body to cut).
    pub hovered: Option<SelectionItem>,
}

impl Selection {
    /// Empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the selection with one item.
    pub fn select(&mut self, item: SelectionItem) {
        self.items.clear();
        self.items.push(item);
    }

    /// Toggle an item (Ctrl-click behavior).
    pub fn toggle(&mut self, item: SelectionItem) {
        if let Some(pos) = self.items.iter().position(|i| *i == item) {
            self.items.remove(pos);
        } else {
            self.items.push(item);
        }
    }

    /// Clear.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// First selected body, if any.
    pub fn primary_body(&self) -> Option<BodyId> {
        self.items.first().map(|i| i.body())
    }

    /// All selected bodies (deduplicated).
    pub fn bodies(&self) -> Vec<BodyId> {
        let mut seen = std::collections::BTreeSet::new();
        self.items
            .iter()
            .map(|i| i.body())
            .filter(|b| seen.insert(*b))
            .collect()
    }

    /// Number of selected items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The first selected body translated into its source feature id.
    pub fn primary_feature(&self) -> Option<forge_core::FeatureId> {
        self.primary_body()
            .map(|b| forge_core::FeatureId::new(b.raw()))
    }

    /// `true` when exactly one planar face is selected (used to suggest
    /// "New Sketch" / "Extrude" context actions, FR-UI-02).
    pub fn single_face(&self) -> Option<(BodyId, FaceId)> {
        match self.items.as_slice() {
            [SelectionItem::Face { body, face }] => Some((*body, *face)),
            _ => None,
        }
    }
}
