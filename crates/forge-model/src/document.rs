//! The [`Document`]: parametric data only (no meshes).

use crate::feature::Feature;
use crate::tree::FeatureTree;
use forge_core::{FeatureId, IdAllocator, LengthUnit, ParamId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A named scalar parameter (lengths in mm, angles in radians).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    /// Parameter id.
    pub id: ParamId,
    /// Display name.
    pub name: String,
    /// Value (mm / rad depending on kind).
    pub value: f64,
    /// Whether the value is an angle (radians internally).
    pub is_angle: bool,
    /// Optional comment.
    pub note: String,
}

impl Param {
    /// Create a length parameter.
    pub fn length(name: impl Into<String>, mm: f64) -> Self {
        Self {
            id: ParamId::NONE,
            name: name.into(),
            value: mm,
            is_angle: false,
            note: String::new(),
        }
    }

    /// Create an angle parameter.
    pub fn angle(name: impl Into<String>, rad: f64) -> Self {
        Self {
            id: ParamId::NONE,
            name: name.into(),
            value: rad,
            is_angle: true,
            note: String::new(),
        }
    }
}

/// The parametric CAD document.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    /// Native format version (bumped on breaking schema changes).
    pub format_version: u32,
    /// Document name (also used as default file name).
    pub name: String,
    /// Model length unit (internal values are always mm).
    pub units: LengthUnit,
    /// The parametric feature tree.
    pub tree: FeatureTree,
    /// Named parameters table.
    pub params: BTreeMap<ParamId, Param>,
    /// Id allocator (feature ids).
    pub allocator: IdAllocator,
    /// Unsaved-changes flag (set by every mutating command).
    pub modified: bool,
}

impl Document {
    /// A new empty document.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: forge_core::NATIVE_FORMAT_VERSION,
            name: name.into(),
            units: LengthUnit::Millimeter,
            tree: FeatureTree::new(),
            params: BTreeMap::new(),
            allocator: IdAllocator::starting_at(1),
            modified: false,
        }
    }

    /// Allocate the next feature id.
    pub fn next_feature_id(&mut self) -> FeatureId {
        FeatureId::new(self.allocator.next_id())
    }

    /// Insert a feature into the tree.
    pub fn add_feature(&mut self, feature: Feature) -> crate::Result<FeatureId> {
        let id = self.next_feature_id();
        self.tree.insert(id, feature)?;
        self.modified = true;
        Ok(id)
    }

    /// Remove a feature.
    pub fn remove_feature(&mut self, id: FeatureId) -> crate::Result<Feature> {
        let node = self
            .tree
            .remove(id)
            .ok_or_else(|| crate::ModelError::MissingFeature(format!("{id}")))?;
        self.modified = true;
        Ok(node.feature)
    }

    /// Edit a feature.
    pub fn edit_feature(&mut self, id: FeatureId, feature: Feature) -> crate::Result<()> {
        self.tree.edit(id, feature)?;
        self.modified = true;
        Ok(())
    }

    /// Look up a feature.
    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.tree.get(id).map(|n| &n.feature)
    }

    /// Look up a sketch feature mutably.
    pub fn sketch_mut(&mut self, id: FeatureId) -> Option<&mut forge_sketch::Sketch> {
        let node = self.tree.get_mut(id)?;
        match &mut node.feature {
            Feature::Sketch(s) => Some(s),
            _ => None,
        }
    }

    /// Look up a sketch feature.
    pub fn sketch(&self, id: FeatureId) -> Option<&forge_sketch::Sketch> {
        match self.feature(id)? {
            Feature::Sketch(s) => Some(s),
            _ => None,
        }
    }

    /// Add or update a parameter.
    pub fn set_param(&mut self, id: ParamId, param: Param) {
        self.params.insert(id, param);
        self.modified = true;
    }
}
