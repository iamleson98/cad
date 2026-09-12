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
    /// Display name (referenced by expressions and bindings).
    pub name: String,
    /// Optional expression driving the value (P-01). When set, `value` is
    /// a cache recomputed by [`Document::resolve_params`]; expressions may
    /// reference other parameters, e.g. `width = 2*th + 1`.
    #[serde(default)]
    pub expression: Option<String>,
    /// Value (mm / rad depending on kind; recomputed when `expression` is
    /// set).
    pub value: f64,
    /// Whether the value is an angle (radians internally).
    pub is_angle: bool,
    /// Optional comment.
    pub note: String,
}

impl Param {
    /// Create a length parameter with a fixed value.
    pub fn length(name: impl Into<String>, mm: f64) -> Self {
        Self {
            id: ParamId::NONE,
            name: name.into(),
            expression: None,
            value: mm,
            is_angle: false,
            note: String::new(),
        }
    }

    /// Create an angle parameter with a fixed value.
    pub fn angle(name: impl Into<String>, rad: f64) -> Self {
        Self {
            id: ParamId::NONE,
            name: name.into(),
            expression: None,
            value: rad,
            is_angle: true,
            note: String::new(),
        }
    }

    /// Create a parameter driven by an expression.
    pub fn expressed(
        name: impl Into<String>,
        expression: impl Into<String>,
        is_angle: bool,
    ) -> Self {
        Self {
            id: ParamId::NONE,
            name: name.into(),
            expression: Some(expression.into()),
            value: f64::NAN,
            is_angle,
            note: String::new(),
        }
    }
}

/// Which dimensional field of a feature a [`DimBinding`] drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DimField {
    /// `ExtrudeParams::distance`.
    ExtrudeDistance,
    /// `RevolveParams::angle`.
    RevolveAngle,
    /// `HoleParams::depth`.
    HoleDepth,
    /// `HoleParams::diameter`.
    HoleDiameter,
    /// `LinearPatternParams::spacing`.
    PatternSpacing,
    /// Mirror plane offset along its normal (from the origin).
    MirrorOffset,
    /// `PrimitiveParams::dims.x`.
    PrimitiveDimA,
    /// `ChamferParams::distance`.
    ChamferDistance,
    /// `FilletParams::radius`.
    FilletRadius,
    /// `ShellParams::thickness`.
    ShellThickness,
}

impl std::fmt::Display for DimField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DimField::ExtrudeDistance => "extrude distance",
            DimField::RevolveAngle => "revolve angle",
            DimField::HoleDepth => "hole depth",
            DimField::HoleDiameter => "hole diameter",
            DimField::PatternSpacing => "pattern spacing",
            DimField::MirrorOffset => "mirror offset",
            DimField::PrimitiveDimA => "dimension A",
            DimField::ChamferDistance => "chamfer distance",
            DimField::FilletRadius => "fillet radius",
            DimField::ShellThickness => "shell thickness",
        })
    }
}

/// A feature dimension driven by a parameter expression (P-01), e.g.
/// "extrude 3 distance = `plate_th`".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimBinding {
    /// The feature whose dimension is driven.
    pub feature: FeatureId,
    /// Which dimension.
    pub field: DimField,
    /// The expression (evaluated after the parameter table resolves).
    pub expression: String,
}

/// The parametric CAD document.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    /// Native format version (bumped on breaking schema changes).
    /// Absent in pre-versioning legacy files → 0, which the loader
    /// migrates to current (I-06).
    #[serde(default)]
    pub format_version: u32,
    /// Document name (also used as default file name).
    pub name: String,
    /// Model length unit (internal values are always mm).
    pub units: LengthUnit,
    /// The parametric feature tree.
    pub tree: FeatureTree,
    /// Named parameters table.
    pub params: BTreeMap<ParamId, Param>,
    /// Dimension bindings: feature dimensions driven by expressions (P-01).
    #[serde(default)]
    pub bindings: Vec<DimBinding>,
    /// Next parameter id (parameters have their own id space).
    #[serde(default)]
    next_param: u64,
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
            bindings: Vec::new(),
            next_param: 0,
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

    /// Remove a parameter (bindings referencing it will fail at resolve
    /// time with a clear message).
    pub fn remove_param(&mut self, id: ParamId) -> Option<Param> {
        self.modified = true;
        self.params.remove(&id)
    }

    /// Allocate a fresh parameter id.
    pub fn next_param_id(&mut self) -> ParamId {
        self.next_param += 1;
        ParamId::new(self.next_param)
    }

    /// P-01: resolve every expression parameter against the others, in
    /// dependency order. Plain (value) parameters act as roots. Returns an
    /// error naming the parameters stuck in a cycle or referencing
    /// unknown names; the document keeps the last-good values in that case.
    pub fn resolve_params(&mut self) -> crate::Result<()> {
        if self.params.is_empty() {
            return Ok(());
        }

        // Seed with plain parameters.
        let mut values: BTreeMap<String, f64> = BTreeMap::new();
        let mut pending: Vec<ParamId> = Vec::new();
        for (id, p) in &self.params {
            match &p.expression {
                None => {
                    values.insert(p.name.clone(), p.value);
                }
                Some(_) => pending.push(*id),
            }
        }

        let mut guard = 0usize;
        while !pending.is_empty() {
            guard += 1;
            if guard > self.params.len() + 1 {
                let stuck: Vec<String> = pending
                    .iter()
                    .filter_map(|id| self.params.get(id).map(|p| p.name.clone()))
                    .collect();
                return Err(crate::ModelError::Invalid(format!(
                    "parameter cycle or unknown reference: {}",
                    stuck.join(", ")
                )));
            }
            let mut progressed = false;
            let mut next_pending = Vec::new();
            for id in pending {
                let (name, expr) = {
                    let p = &self.params[&id];
                    (p.name.clone(), p.expression.clone().unwrap_or_default())
                };
                match crate::expr::eval(&expr, &values) {
                    Ok(v) => {
                        self.params.get_mut(&id).expect("just read").value = v;
                        values.insert(name, v);
                        progressed = true;
                    }
                    Err(e) => {
                        // Unknown-name errors are expected while upstream
                        // expressions are still pending; hard errors
                        // (syntax, domain) surface on the next round if
                        // they never clear.
                        if !e.message.starts_with("unknown name") {
                            return Err(crate::ModelError::Invalid(format!(
                                "parameter `{name}`: {}",
                                e.message
                            )));
                        }
                        next_pending.push(id);
                    }
                }
            }
            pending = next_pending;
            if !progressed && !pending.is_empty() {
                let stuck: Vec<String> = pending
                    .iter()
                    .filter_map(|id| self.params.get(id).map(|p| p.name.clone()))
                    .collect();
                return Err(crate::ModelError::Invalid(format!(
                    "parameter cycle or unknown reference: {}",
                    stuck.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// P-01: numeric overrides for feature dimensions, computed from the
    /// bindings after [`Document::resolve_params`]. Malformed bindings
    /// produce an error naming the feature and field.
    pub fn resolved_binding_values(&self) -> crate::Result<BTreeMap<(FeatureId, DimField), f64>> {
        if self.bindings.is_empty() {
            return Ok(BTreeMap::new());
        }
        let values: BTreeMap<String, f64> = self
            .params
            .values()
            .map(|p| (p.name.clone(), p.value))
            .collect();
        let mut out = BTreeMap::new();
        for b in &self.bindings {
            let v = crate::expr::eval(&b.expression, &values).map_err(|e| {
                crate::ModelError::Invalid(format!(
                    "binding for {} of feature {}: {}",
                    b.field, b.feature, e.message
                ))
            })?;
            out.insert((b.feature, b.field), v);
        }
        Ok(out)
    }

    /// Set (or, with `None`, remove) the binding of a feature field.
    pub fn set_binding(&mut self, feature: FeatureId, field: DimField, expression: Option<String>) {
        self.bindings
            .retain(|b| !(b.feature == feature && b.field == field));
        if let Some(expression) = expression {
            self.bindings.push(DimBinding {
                feature,
                field,
                expression,
            });
        }
        self.modified = true;
    }

    /// The binding of a feature field, if any.
    pub fn binding(&self, feature: FeatureId, field: DimField) -> Option<&DimBinding> {
        self.bindings
            .iter()
            .find(|b| b.feature == feature && b.field == field)
    }

    /// Mark every bound feature dirty (used when parameters change, since
    /// their values may feed any binding).
    pub fn mark_bindings_dirty(&mut self) {
        let ids: Vec<FeatureId> = self.bindings.iter().map(|b| b.feature).collect();
        for id in ids {
            self.tree.mark_dirty(id);
        }
    }
}
