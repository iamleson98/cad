//! Feature evaluation: turns the parametric tree into triangle meshes.
//!
//! - Only **dirty** features are re-evaluated; clean results come from the
//!   [`Evaluator`] cache (FR-SM-04 dynamic DAG re-evaluation).
//! - Every feature evaluation runs inside [`std::panic::catch_unwind`]
//!   (NFR-RES-03): kernel panics degrade to per-feature error messages.
//! - Boolean features *consume* their operands: consumed bodies are hidden
//!   from the render set while the boolean succeeds, and reappear when it
//!   is suppressed or fails.

use crate::document::{DimField, Document};
use crate::feature::{ExtrudeOp, Feature, HoleKind, PrimitiveKind};
use forge_core::time::Instant;
use forge_core::{BodyId, FeatureId, Point2, Point3, TessellationConfig, Transform};
use forge_geometry::{
    boolean, extrude, loft, primitives, revolve, sweep_along_path, CsgOp, Profile2D, TriMesh,
};
use forge_sketch::{Sketch, SketchEntity, SketchPlane, SolveReport};
use std::collections::{BTreeMap, BTreeSet};

/// A cached evaluation result for one feature.
#[derive(Debug, Clone)]
pub enum CachedResult {
    /// A solid body (tessellated).
    Body(TriMesh),
    /// Solved sketch profiles (outer + holes per island).
    Profiles(Vec<Profile2D>),
}

/// One visible body of an evaluation.
#[derive(Debug, Clone)]
pub struct EvalBody {
    /// Body id (== the id of the feature that produced it).
    pub id: BodyId,
    /// Display name.
    pub name: String,
    /// The tessellated mesh.
    pub mesh: TriMesh,
    /// Producing feature.
    pub source: FeatureId,
}

/// Outcome of a full evaluation pass.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Visible bodies in tree order.
    pub bodies: Vec<EvalBody>,
    /// Per-feature error messages (empty string values are cleared).
    pub errors: BTreeMap<FeatureId, String>,
    /// Features (re-)evaluated in this pass.
    pub evaluated: usize,
    /// Features reused from cache.
    pub reused: usize,
    /// Total triangle count of visible bodies.
    pub total_triangles: usize,
    /// Wall-clock duration of the pass.
    pub duration: std::time::Duration,
    /// Sketch solver reports (S-05 diagnostics: DOF readout, conflicts).
    pub sketch_reports: BTreeMap<FeatureId, SolveReport>,
}

impl Evaluation {
    /// Total visible triangle count.
    pub fn triangle_count(&self) -> usize {
        self.bodies.iter().map(|b| b.mesh.tri_count()).sum()
    }
}

/// Persistent evaluation cache.
#[derive(Debug, Clone, Default)]
pub struct Evaluator {
    cache: BTreeMap<FeatureId, CachedResult>,
    /// Last solver report per sketch feature (S-05 diagnostics).
    sketch_reports: BTreeMap<FeatureId, SolveReport>,
    /// Tessellation quality used for all features.
    pub tessellation: TessellationConfig,
}

impl Evaluator {
    /// Cached result of a feature (if any).
    pub fn cached(&self, id: FeatureId) -> Option<&CachedResult> {
        self.cache.get(&id)
    }

    /// Cached body of a feature.
    pub fn cached_body(&self, id: FeatureId) -> Option<&TriMesh> {
        match self.cache.get(&id) {
            Some(CachedResult::Body(m)) => Some(m),
            _ => None,
        }
    }

    /// Cached profiles of a sketch feature.
    pub fn cached_profiles(&self, id: FeatureId) -> Option<&Vec<Profile2D>> {
        match self.cache.get(&id) {
            Some(CachedResult::Profiles(p)) => Some(p),
            _ => None,
        }
    }

    /// Drop all cached results (e.g. after tessellation settings changed).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.sketch_reports.clear();
    }

    /// Evaluate the document.
    ///
    /// Clears the dirty flag of every feature that was evaluated
    /// successfully (or reused from cache); features with errors stay
    /// dirty so the next pass retries them.
    pub fn evaluate(&mut self, doc: &mut Document) -> Evaluation {
        let start = Instant::now();
        let mut errors: BTreeMap<FeatureId, String> = BTreeMap::new();
        let mut consumed: BTreeSet<FeatureId> = BTreeSet::new();
        let mut body_order: Vec<FeatureId> = Vec::new();
        let mut evaluated = 0usize;
        let mut reused = 0usize;

        // P-01: resolve parameter expressions, then dimension bindings
        // into numeric overrides, before evaluating any feature.
        let mut param_error: Option<String> = None;
        let overrides = match doc.resolve_params().map(|_| doc.resolved_binding_values()) {
            Ok(Ok(map)) => map,
            Ok(Err(e)) | Err(e) => {
                // Parameter failures surface on every bound feature after
                // the loop (a successful evaluation with last-good stored
                // values must not clear the message); evaluation continues
                // with the stored numeric values.
                param_error = Some(format!("{e}"));
                BTreeMap::new()
            }
        };

        for id in doc.tree.order().to_vec() {
            let Some(node) = doc.tree.get(id) else {
                continue;
            };
            if node.suppressed {
                self.cache.remove(&id);
                self.sketch_reports.remove(&id);
                errors.remove(&id);
                continue;
            }

            if !node.dirty && self.cache.contains_key(&id) {
                reused += 1;
                if let Some(CachedResult::Body(_)) = self.cache.get(&id) {
                    body_order.push(id);
                }
                continue;
            }

            evaluated += 1;
            // Panic containment (NFR-RES-03).
            let feature = node.feature.clone();
            let overrides_ref = &overrides;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.evaluate_feature(&*doc, id, &feature, overrides_ref)
            }));
            let mut success = false;
            match outcome {
                Ok(Ok(Some(result))) => {
                    errors.remove(&id);
                    success = true;
                    if let CachedResult::Body(_) = &result {
                        body_order.push(id);
                    }
                    // Track operand / target consumption.
                    for consumed_id in consumed_targets(&feature) {
                        consumed.insert(consumed_id);
                    }
                    self.cache.insert(id, result);
                }
                Ok(Ok(None)) => {
                    // Sketch feature: solve, then extract profiles.
                    if let Feature::Sketch(sketch) = &feature {
                        let mut solved = sketch.clone();
                        let report = solved.solve().ok();
                        if let Some(r) = &report {
                            self.sketch_reports.insert(id, r.clone());
                        }
                        match build_profiles(&solved, &self.tessellation) {
                            Ok(profiles) => {
                                success = true;
                                match &report {
                                    Some(r) if !r.is_solved() => {
                                        errors.insert(
                                            id,
                                            format!(
                                                "sketch not fully solved ({}, residual {:.2e})",
                                                r.status, r.residual
                                            ),
                                        );
                                    }
                                    _ => {
                                        errors.remove(&id);
                                    }
                                }
                                self.cache.insert(id, CachedResult::Profiles(profiles));
                            }
                            Err(e) => {
                                errors.insert(id, format!("{e}"));
                                self.cache.remove(&id);
                            }
                        }
                    } else if let Feature::Datum(datum) = &feature {
                        // D-01: datums are construction geometry: no body,
                        // no profiles; they only act as sketch carriers and
                        // mirror references. Validation: the plane must be
                        // constructible.
                        let _ = datum.to_plane();
                        errors.remove(&id);
                        success = true;
                    } else {
                        self.cache.remove(&id);
                        errors.insert(id, "feature produced no result".into());
                    }
                }
                Ok(Err(msg)) => {
                    self.cache.remove(&id);
                    errors.insert(id, format!("{msg}"));
                }
                Err(panic) => {
                    self.cache.remove(&id);
                    let msg = panic_message(&panic);
                    errors.insert(id, format!("kernel panic: {msg}"));
                }
            }
            if success {
                doc.tree.clear_dirty(id);
            }
        }

        // Assemble visible bodies (skip bodies consumed by booleans).
        let mut bodies = Vec::new();
        for id in body_order {
            if consumed.contains(&id) {
                continue;
            }
            if let Some(CachedResult::Body(mesh)) = self.cache.get(&id) {
                let name = doc
                    .tree
                    .get(id)
                    .map(|n| n.feature.label())
                    .unwrap_or_else(|| id.to_string());
                bodies.push(EvalBody {
                    id: BodyId::new(id.raw()),
                    name,
                    mesh: mesh.clone(),
                    source: id,
                });
            }
        }

        // P-01: attach parameter/binding failures to the bound features
        // *after* the loop so a successful evaluation (with last-good
        // stored values) does not clear them.
        if let Some(e) = &param_error {
            for b in &doc.bindings {
                errors
                    .entry(b.feature)
                    .and_modify(|old| *old = format!("{old}; {e}"))
                    .or_insert_with(|| e.clone());
            }
        }

        let total_triangles = bodies.iter().map(|b| b.mesh.tri_count()).sum();
        // S-05: reports of living sketch features only.
        let live: BTreeSet<FeatureId> = doc.tree.order().iter().copied().collect();
        self.sketch_reports.retain(|id, _| live.contains(id));
        let sketch_reports = self.sketch_reports.clone();
        Evaluation {
            bodies,
            errors,
            evaluated,
            reused,
            total_triangles,
            duration: start.elapsed(),
            sketch_reports,
        }
    }

    /// Evaluate one feature. `Ok(None)` means "no body" (sketch and datum
    /// features are handled by the caller).
    ///
    /// `overrides` carries P-01 dimension-binding values (already resolved
    /// against the parameter table).
    fn evaluate_feature(
        &mut self,
        doc: &Document,
        id: FeatureId,
        feature: &Feature,
        overrides: &BTreeMap<(FeatureId, DimField), f64>,
    ) -> crate::Result<Option<CachedResult>> {
        // P-01: dimension override lookup for this feature.
        let dim = |field: DimField, stored: f64| -> f64 {
            overrides
                .get(&(id, field))
                .copied()
                .filter(|v| v.is_finite())
                .unwrap_or(stored)
        };
        let mesh: TriMesh = match feature {
            Feature::Sketch(_) | Feature::Datum(_) => return Ok(None),

            Feature::Primitive(p) => {
                // Validate dims before touching the kernel (negative or NaN
                // sizes are user input errors, not kernel panics). The
                // dimension-A binding (P-01) may override dims.x.
                let dim_a = dim(DimField::PrimitiveDimA, p.dims.x);
                let dims = forge_core::Vector3::new(dim_a, p.dims.y, p.dims.z);
                let (a, b, c) = (dims.x, dims.y, dims.z);
                let ok = match p.kind {
                    PrimitiveKind::Box => a > 0.0 && b > 0.0 && c > 0.0,
                    PrimitiveKind::Sphere => a > 0.0,
                    PrimitiveKind::Cylinder => a > 0.0 && b > 0.0,
                    PrimitiveKind::Cone => a >= 0.0 && b >= 0.0 && c > 0.0,
                    PrimitiveKind::Torus => a > 0.0 && b > 0.0,
                };
                if !ok || dims.x.is_nan() || dims.y.is_nan() || dims.z.is_nan() {
                    return Err(crate::ModelError::Invalid(format!(
                        "invalid dimensions {dims:?} for {}",
                        p.kind
                    )));
                }
                let cfg = &self.tessellation;
                match p.kind {
                    PrimitiveKind::Box => primitives::box_from_center_extents(p.center, dims),
                    PrimitiveKind::Sphere => primitives::sphere(p.center, dims.x, cfg),
                    PrimitiveKind::Cylinder => primitives::cylinder(p.center, dims.x, dims.y, cfg),
                    PrimitiveKind::Cone => primitives::cone(p.center, dims.x, dims.y, dims.z, cfg),
                    PrimitiveKind::Torus => primitives::torus(p.center, dims.x, dims.y, cfg),
                }
            }

            Feature::Extrude(p) => {
                let profiles = self.cached_profiles(p.profile).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!(
                        "profile feature {} has no solved profiles",
                        p.profile
                    ))
                })?;
                if profiles.is_empty() {
                    return Err(crate::ModelError::Invalid(
                        "sketch has no closed contours".into(),
                    ));
                }
                let plane = resolve_plane(doc, p.profile)?;
                let distance = dim(DimField::ExtrudeDistance, p.distance);
                if !(distance.is_finite() && distance > 0.0) {
                    return Err(crate::ModelError::Invalid(format!(
                        "extrude distance must be positive, got {distance}"
                    )));
                }
                let mut solid = TriMesh::default();
                for profile in &profiles {
                    let mesh = extrude(
                        profile,
                        &plane,
                        &forge_geometry::ExtrudeParams {
                            distance,
                            direction: p.direction,
                            draft_angle: p.draft_angle,
                        },
                    )?;
                    solid.merge(&mesh);
                }
                apply_operation(self, p.operation, p.target, solid)?
            }

            Feature::Revolve(p) => {
                let profiles = self.cached_profiles(p.profile).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!(
                        "profile feature {} has no solved profiles",
                        p.profile
                    ))
                })?;
                let profile = profiles
                    .first()
                    .ok_or_else(|| crate::ModelError::Invalid("empty profile".into()))?;
                let plane = resolve_plane(doc, p.profile)?;
                let angle = dim(DimField::RevolveAngle, p.angle);
                let solid = revolve(
                    &profile.outer,
                    &plane,
                    p.axis_start,
                    p.axis_end,
                    &forge_geometry::RevolveParams { angle },
                    &self.tessellation,
                )?;
                apply_operation(self, p.operation, p.target, solid)?
            }

            Feature::Loft(p) => {
                let mut sections = Vec::new();
                let mut planes = Vec::new();
                for s_id in &p.sections {
                    let profiles = self.cached_profiles(*s_id).cloned().ok_or_else(|| {
                        crate::ModelError::MissingEntity(format!(
                            "section {} has no profiles",
                            s_id
                        ))
                    })?;
                    let profile = profiles
                        .first()
                        .ok_or_else(|| crate::ModelError::Invalid("empty loft section".into()))?;
                    let plane = resolve_plane(doc, *s_id)?;
                    sections.push(profile.clone());
                    planes.push(plane);
                }
                loft(&sections, &planes)?
            }

            Feature::Sweep(p) => {
                let profiles = self.cached_profiles(p.profile).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!(
                        "profile feature {} has no solved profiles",
                        p.profile
                    ))
                })?;
                let profile = profiles
                    .first()
                    .ok_or_else(|| crate::ModelError::Invalid("empty profile".into()))?;
                let plane = resolve_plane(doc, p.profile)?;
                sweep_along_path(&profile.outer, &plane, &p.path)?
            }

            Feature::Boolean(b) => {
                let mut operands = Vec::new();
                for op_id in &b.operands {
                    let mesh = self.cached_body(*op_id).cloned().ok_or_else(|| {
                        crate::ModelError::MissingEntity(format!("operand {} has no body", op_id))
                    })?;
                    operands.push(mesh);
                }
                let first = operands
                    .first()
                    .cloned()
                    .ok_or_else(|| crate::ModelError::Invalid("empty boolean".into()))?;
                let mut result = first;
                for other in operands.iter().skip(1) {
                    result = boolean(&result, other, b.op)?;
                }
                result
            }

            Feature::TransformBody {
                source,
                translation,
                rotation,
                pivot,
            } => {
                let mesh = self.cached_body(*source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("source {} has no body", source))
                })?;
                // p' = pivot + R·(p − pivot) + translation: rotation spins
                // about the pivot, translation is a pure world offset.
                let rot =
                    nalgebra::UnitQuaternion::from_euler_angles(rotation.x, rotation.y, rotation.z);
                let iso = forge_core::Transform::from_parts(
                    nalgebra::Translation3::from(pivot.coords + translation),
                    rot,
                ) * forge_core::Transform::from_parts(
                    nalgebra::Translation3::from(-pivot.coords),
                    nalgebra::UnitQuaternion::identity(),
                );
                let mut m = mesh.transformed(&iso);
                m.compute_vertex_normals();
                m
            }

            Feature::LinearPattern(p) => {
                let seed = self.cached_body(p.source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("seed {} has no body", p.source))
                })?;
                let spacing = dim(DimField::PatternSpacing, p.spacing);
                let mut p2 = p.clone();
                p2.spacing = spacing;
                if !(spacing.is_finite() && spacing > 0.0) {
                    return Err(crate::ModelError::Invalid(format!(
                        "pattern spacing must be positive, got {spacing}"
                    )));
                }
                let instances = linear_instance_transforms(&p2, p.operation != ExtrudeOp::New)?;
                self.apply_instances(&instances, p.operation, p.target, &seed)?
            }

            Feature::CircularPattern(p) => {
                let seed = self.cached_body(p.source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("seed {} has no body", p.source))
                })?;
                let instances = circular_instance_transforms(p, p.operation != ExtrudeOp::New)?;
                self.apply_instances(&instances, p.operation, p.target, &seed)?
            }

            Feature::Mirror(p) => {
                let source = self.cached_body(p.source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("source {} has no body", p.source))
                })?;
                if p.plane_normal.norm() < 1e-12 {
                    return Err(crate::ModelError::Invalid(
                        "mirror plane normal is degenerate".into(),
                    ));
                }
                let mut p2 = p.clone();
                // P-01: the MirrorOffset binding drives the plane's offset
                // along its own normal (measured from the origin).
                if let Some(offset) = overrides.get(&(id, DimField::MirrorOffset)) {
                    let n = p2.plane_normal.normalize();
                    p2.plane_point = forge_core::Point3::origin() + n * *offset;
                }
                let mirrored = source.mirrored(&p2.plane_point, &p2.plane_normal);
                apply_operation(self, p2.operation, p2.target, mirrored)?
            }

            Feature::Hole(p) => {
                let depth = dim(DimField::HoleDepth, p.depth);
                let diameter = dim(DimField::HoleDiameter, p.diameter);
                let mut p2 = p.clone();
                p2.depth = depth;
                p2.diameter = diameter;
                let tool = build_hole_tool(doc, &p2, &self.tessellation)?;
                if tool.tri_count() == 0 {
                    return Err(crate::ModelError::Invalid(
                        "hole tool is empty (check placements and dimensions)".into(),
                    ));
                }
                let target_mesh = self.cached_body(p.target).ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!(
                        "hole target {} has no body",
                        p.target
                    ))
                })?;
                boolean(target_mesh, &tool, CsgOp::Difference)?
            }

            Feature::ImportedMesh(p) => {
                if p.mesh.tri_count() == 0 {
                    return Err(crate::ModelError::Invalid(format!(
                        "imported mesh {} is empty",
                        p.source
                    )));
                }
                let mut mesh = p.mesh.clone();
                mesh.ensure_normals();
                mesh
            }
        };

        Ok(Some(CachedResult::Body(mesh)))
    }
}

/// Apply a Join/Cut operation against a target body.
fn apply_operation(
    evaluator: &Evaluator,
    op: ExtrudeOp,
    target: FeatureId,
    solid: TriMesh,
) -> crate::Result<TriMesh> {
    match op {
        ExtrudeOp::New => Ok(solid),
        ExtrudeOp::Join | ExtrudeOp::Cut => {
            let target_mesh = evaluator.cached_body(target).ok_or_else(|| {
                crate::ModelError::MissingEntity(format!("target {target} has no body"))
            })?;
            let csg_op = if op == ExtrudeOp::Join {
                CsgOp::Union
            } else {
                CsgOp::Difference
            };
            Ok(boolean(target_mesh, &solid, csg_op)?)
        }
    }
}

/// Feature ids hidden once a feature successfully produced a body:
/// boolean operands, and Join/Cut targets (their geometry lives on in
/// the consuming feature's result).
///
/// Join/Cut **patterns** also hide their seed: the seed instance is part
/// of the pattern result (SolidWorks feature-pattern semantics). Holes
/// consume their cut target (F-04).
fn consumed_targets(feature: &Feature) -> Vec<FeatureId> {
    fn join_cut_target(op: ExtrudeOp, target: FeatureId) -> Option<FeatureId> {
        (op != ExtrudeOp::New && !target.is_none()).then_some(target)
    }
    match feature {
        Feature::Boolean(b) => b.operands.clone(),
        Feature::Extrude(p) => join_cut_target(p.operation, p.target).into_iter().collect(),
        Feature::Revolve(p) => join_cut_target(p.operation, p.target).into_iter().collect(),
        Feature::LinearPattern(p) => {
            let mut v = join_cut_target(p.operation, p.target)
                .into_iter()
                .collect::<Vec<_>>();
            if p.operation != ExtrudeOp::New {
                v.push(p.source);
            }
            v
        }
        Feature::CircularPattern(p) => {
            let mut v = join_cut_target(p.operation, p.target)
                .into_iter()
                .collect::<Vec<_>>();
            if p.operation != ExtrudeOp::New {
                v.push(p.source);
            }
            v
        }
        Feature::Mirror(p) => join_cut_target(p.operation, p.target).into_iter().collect(),
        Feature::Hole(p) => (!p.target.is_none())
            .then_some(p.target)
            .into_iter()
            .collect(),
        // W-01: a transform *moves* its source (the source body is
        // replaced by the transformed one).
        Feature::TransformBody { source, .. } => vec![*source],
        _ => Vec::new(),
    }
}

/// Resolve a sketch feature's plane, following datum references (D-01).
/// World-space placements of a hole feature's centers (C-06 CAM hole
/// recognition): `[(world center, hole diameter, top Z, bottom Z)]`.
///
/// The top is the sketch plane Z (entry face), the bottom the plane minus
/// the hole depth along the cut direction.
pub fn hole_placements(
    doc: &Document,
    p: &crate::HoleParams,
) -> crate::Result<Vec<(forge_core::Point3, f64, f64, f64)>> {
    let sketch = doc
        .sketch(p.profile)
        .ok_or_else(|| crate::ModelError::MissingFeature(format!("{}", p.profile)))?;
    let placements: Vec<forge_core::Point2> = sketch
        .entities
        .values()
        .filter_map(|e| match e {
            forge_sketch::SketchEntity::Point { p, .. } => Some(*p),
            forge_sketch::SketchEntity::Circle { center, .. }
            | forge_sketch::SketchEntity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    if placements.is_empty() {
        return Ok(Vec::new());
    }
    let plane = resolve_plane(doc, p.profile)?;
    let n: forge_core::Vector3 = *plane.normal.as_ref();
    let sign = match p.direction {
        forge_geometry::ExtrudeDirection::Positive => 1.0,
        forge_geometry::ExtrudeDirection::Negative => -1.0,
        forge_geometry::ExtrudeDirection::Symmetric => 1.0,
    };
    let depth = p.depth;
    Ok(placements
        .into_iter()
        .map(|pt| {
            let world = plane.to_world(pt);
            // Hole axis: from the sketch plane, `sign` half-spaces along
            // the plane normal, length = depth.
            let entry = world.z;
            let far = world.z + sign * depth * n.z;
            // CAM drills vertically: entry from above (the higher face).
            let top = entry.max(far);
            let bottom = entry.min(far);
            (
                forge_core::Point3::new(world.x, world.y, top),
                p.diameter,
                top,
                bottom,
            )
        })
        .collect())
}

fn resolve_plane(doc: &Document, sketch_id: FeatureId) -> crate::Result<forge_core::Plane> {
    let sketch = doc
        .sketch(sketch_id)
        .ok_or_else(|| crate::ModelError::MissingFeature(format!("{sketch_id}")))?;
    match &sketch.plane {
        SketchPlane::DatumRef { feature } => {
            let node = doc.tree.get(*feature).ok_or_else(|| {
                crate::ModelError::MissingFeature(format!(
                    "sketch {sketch_id} references missing datum {feature}"
                ))
            })?;
            if node.suppressed {
                return Err(crate::ModelError::Invalid(format!(
                    "sketch {sketch_id} lives on suppressed datum {feature}"
                )));
            }
            match &node.feature {
                Feature::Datum(d) => Ok(d.to_plane()),
                other => Err(crate::ModelError::Invalid(format!(
                    "feature {feature} ({}) is not a datum plane",
                    other.label()
                ))),
            }
        }
        other => Ok(other.to_plane()),
    }
}

/// Rotation mapping unit vector `from` onto `to` (anti-parallel safe).
fn rotation_from_to(
    from: forge_core::Vector3,
    to: forge_core::Vector3,
) -> nalgebra::UnitQuaternion<f64> {
    if let Some(q) = nalgebra::UnitQuaternion::rotation_between(&from, &to) {
        return q;
    }
    // Anti-parallel: 180° around any perpendicular axis.
    let perp = from.cross(&forge_core::Vector3::z());
    let axis = if perp.norm() < 1e-9 {
        forge_core::Vector3::x()
    } else {
        perp.normalize()
    };
    nalgebra::UnitQuaternion::from_axis_angle(
        &nalgebra::Unit::new_unchecked(axis),
        std::f64::consts::PI,
    )
}

/// Build the revolved cross-section (r, t) of one hole, where `t` measures
/// distance into the material from the entry plane and `r` the radius.
/// `lead` extends the tool above the entry plane so the boolean never sees
/// a coplanar face at the surface.
fn hole_profile(p: &crate::HoleParams, lead: f64) -> crate::Result<Vec<Point2>> {
    let d = p.diameter;
    if !(d.is_finite() && d > 0.0) {
        return Err(crate::ModelError::Invalid(format!(
            "hole diameter must be positive, got {d}"
        )));
    }
    if !(p.depth.is_finite() && p.depth > 0.0) {
        return Err(crate::ModelError::Invalid(format!(
            "hole depth must be positive, got {}",
            p.depth
        )));
    }
    let r = d / 2.0;
    let mut pts: Vec<Point2> = Vec::with_capacity(8);
    match p.kind {
        HoleKind::Simple => {
            // axis start -> (r, -lead) -> (r, depth) -> drill apex
            pts.push(Point2::new(0.0, -lead));
            pts.push(Point2::new(r, -lead));
            pts.push(Point2::new(r, p.depth));
        }
        HoleKind::Counterbore => {
            if p.counterbore_diameter < d {
                return Err(crate::ModelError::Invalid(format!(
                    "counterbore diameter {} must be >= hole diameter {d}",
                    p.counterbore_diameter
                )));
            }
            if !(p.counterbore_depth > 0.0 && p.counterbore_depth < p.depth) {
                return Err(crate::ModelError::Invalid(format!(
                    "counterbore depth {} must be in (0, hole depth {})",
                    p.counterbore_depth, p.depth
                )));
            }
            let cb_r = p.counterbore_diameter / 2.0;
            pts.push(Point2::new(0.0, -lead));
            pts.push(Point2::new(cb_r, -lead));
            pts.push(Point2::new(cb_r, p.counterbore_depth));
            pts.push(Point2::new(r, p.counterbore_depth));
            pts.push(Point2::new(r, p.depth));
        }
        HoleKind::Countersink => {
            if p.countersink_diameter < d {
                return Err(crate::ModelError::Invalid(format!(
                    "countersink diameter {} must be >= hole diameter {d}",
                    p.countersink_diameter
                )));
            }
            let half = p.countersink_angle / 2.0;
            if !(half > 1e-6 && half < std::f64::consts::FRAC_PI_2 - 1e-6) {
                return Err(crate::ModelError::Invalid(format!(
                    "countersink angle {:.1}° is out of range",
                    p.countersink_angle.to_degrees()
                )));
            }
            let cs_r = p.countersink_diameter / 2.0;
            let cs_h = (cs_r - r) / half.tan();
            if cs_h >= p.depth {
                return Err(crate::ModelError::Invalid(
                    "countersink is deeper than the hole".into(),
                ));
            }
            pts.push(Point2::new(0.0, -lead));
            pts.push(Point2::new(cs_r, -lead));
            pts.push(Point2::new(cs_r, 0.0));
            pts.push(Point2::new(r, cs_h));
            pts.push(Point2::new(r, p.depth));
        }
    }
    if p.drill_point {
        let half = p.drill_angle / 2.0;
        if !(half > 1e-6 && half < std::f64::consts::FRAC_PI_2 - 1e-6) {
            return Err(crate::ModelError::Invalid(format!(
                "drill point angle {:.1}° is out of range",
                p.drill_angle.to_degrees()
            )));
        }
        let dp_h = r / half.tan();
        // Cone from (r, depth) to the apex (0, depth + dp_h).
        pts.push(Point2::new(0.0, p.depth + dp_h));
    } else {
        // Flat bottom: descend to the axis before closing along it.
        pts.push(Point2::new(0.0, p.depth));
    }
    // Close along the axis back to the start.
    pts.push(Point2::new(0.0, -lead));
    Ok(pts)
}

/// Build the compound hole tool (F-04): one watertight solid of
/// revolution per placement, oriented along the hole axis (the sketch
/// plane normal, on the side given by `direction`). Segments of the
/// counterbore/countersink/drill-point stack are a *single* revolve
/// profile, so the tool has no internal boolean interfaces.
fn build_hole_tool(
    doc: &Document,
    p: &crate::HoleParams,
    cfg: &TessellationConfig,
) -> crate::Result<TriMesh> {
    let sketch = doc
        .sketch(p.profile)
        .ok_or_else(|| crate::ModelError::MissingFeature(format!("{}", p.profile)))?;
    // Placements: sketch points and circle/arc centers.
    let placements: Vec<Point2> = sketch
        .entities
        .values()
        .filter_map(|e| match e {
            SketchEntity::Point { p, .. } => Some(*p),
            SketchEntity::Circle { center, .. } | SketchEntity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    if placements.is_empty() {
        return Err(crate::ModelError::Invalid(
            "hole sketch has no placement points (add points or circles)".into(),
        ));
    }
    let plane = resolve_plane(doc, p.profile)?;
    let n: forge_core::Vector3 = *plane.normal.as_ref();

    let signs: Vec<f64> = match p.direction {
        forge_geometry::ExtrudeDirection::Positive => vec![1.0],
        forge_geometry::ExtrudeDirection::Negative => vec![-1.0],
        forge_geometry::ExtrudeDirection::Symmetric => vec![1.0, -1.0],
    };

    // Overshoot above the entry plane (in air) avoids coplanar boolean
    // faces at the surface.
    const LEAD: f64 = 1.0;
    let profile = hole_profile(p, LEAD)?;

    // Revolve the (r, t) profile around the world +Y axis on an XY-plane
    // at the origin (to_world maps (r, t) to (r, t, 0); the revolve axis
    // through 2D (0,0)-(0,1) is the world Y axis), then rigidly move each
    // tool to its placement and direction.
    let rev_plane = forge_core::Plane::new(Point3::origin(), forge_core::Vector3::z())
        .expect("z axis is a valid plane normal");
    let blank = revolve(
        &profile,
        &rev_plane,
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0),
        &forge_geometry::RevolveParams {
            angle: std::f64::consts::TAU,
        },
        cfg,
    )?;

    let mut tool = TriMesh::default();
    for placement in &placements {
        for &sign in &signs {
            let axis = n * sign;
            let anchor = plane.to_world(*placement);
            let rotation = rotation_from_to(forge_core::Vector3::y(), axis);
            let iso = Transform::from_parts(nalgebra::Translation3::from(anchor.coords), rotation);
            let mut mesh = blank.transformed(&iso);
            mesh.compute_vertex_normals();
            tool.merge(&mesh);
        }
    }
    Ok(tool)
}

/// Rigid transforms of the pattern copies. For `New` the seed itself is
/// excluded (it stays visible as the source body, like a SolidWorks body
/// pattern); for Join/Cut the seed instance is included (feature-pattern
/// semantics: the target is cut/joined at every instance).
fn linear_instance_transforms(
    p: &crate::LinearPatternParams,
    include_seed: bool,
) -> crate::Result<Vec<Transform>> {
    if p.count < 2 {
        return Err(crate::ModelError::Invalid(
            "a pattern needs at least 2 instances".into(),
        ));
    }
    if p.direction.norm() < 1e-12 {
        return Err(crate::ModelError::Invalid(
            "pattern direction is degenerate".into(),
        ));
    }
    let dir = p.direction.normalize();
    let count = p.count;
    let offsets: Vec<f64> = if p.symmetric {
        // (i - (count-1)/2) * spacing; the seed (offset 0) is included or
        // excluded by the caller.
        (0..count)
            .map(|i| (i as f64 - (count as f64 - 1.0) * 0.5) * p.spacing)
            .filter(|o| include_seed || o.abs() > 1e-9)
            .collect()
    } else {
        let first = if include_seed { 0 } else { 1 };
        (first..count).map(|i| i as f64 * p.spacing).collect()
    };
    Ok(offsets
        .into_iter()
        .map(|o| {
            Transform::from_parts(
                nalgebra::Translation3::from(dir * o),
                nalgebra::UnitQuaternion::identity(),
            )
        })
        .collect())
}

/// Rigid rotations of the pattern instances about the axis through
/// `axis_point` along `axis_dir`.
fn circular_instance_transforms(
    p: &crate::CircularPatternParams,
    include_seed: bool,
) -> crate::Result<Vec<Transform>> {
    if p.count < 2 {
        return Err(crate::ModelError::Invalid(
            "a pattern needs at least 2 instances".into(),
        ));
    }
    if p.axis_dir.norm() < 1e-12 {
        return Err(crate::ModelError::Invalid(
            "pattern axis is degenerate".into(),
        ));
    }
    if !p.angle.is_finite() || p.angle <= 1e-12 {
        return Err(crate::ModelError::Invalid(
            "pattern angle must be positive".into(),
        ));
    }
    let axis = nalgebra::Unit::new_normalize(p.axis_dir);
    let count = p.count;
    let first = if include_seed { 0 } else { 1 };
    // Full circle: equal pitch TAU/count (k = count would coincide with the
    // seed). Partial span: k*angle/(count-1) so the last copy lands at
    // `angle` (SolidWorks "equal spacing" semantics).
    let angles: Vec<f64> = if p.angle >= std::f64::consts::TAU - 1e-9 {
        (first..count)
            .map(|k| std::f64::consts::TAU * k as f64 / count as f64)
            .collect()
    } else {
        (first..count)
            .map(|k| p.angle * k as f64 / (count as f64 - 1.0))
            .collect()
    };
    Ok(angles
        .into_iter()
        .map(|a| {
            let rot = nalgebra::Rotation3::from_axis_angle(&axis, a);
            // p' = R(p - c) + c  =>  translation = c - R*c
            let translation = nalgebra::Translation3::from(
                p.axis_point.coords - rot.transform_point(&p.axis_point).coords,
            );
            Transform::from_parts(
                translation,
                nalgebra::UnitQuaternion::from_rotation_matrix(&rot),
            )
        })
        .collect())
}

impl Evaluator {
    /// Combine pattern instances with a target (`Join`/`Cut`) or into a
    /// standalone body (`New`).
    fn apply_instances(
        &self,
        instances: &[Transform],
        op: ExtrudeOp,
        target: FeatureId,
        seed: &TriMesh,
    ) -> crate::Result<TriMesh> {
        let instantiate = |t: &Transform| -> TriMesh {
            let mut m = seed.transformed(t);
            m.compute_vertex_normals();
            m
        };
        match op {
            ExtrudeOp::New => {
                let mut result: Option<TriMesh> = None;
                for t in instances {
                    let m = instantiate(t);
                    result = Some(match result {
                        None => m,
                        Some(r) => boolean(&r, &m, CsgOp::Union)?,
                    });
                }
                result
                    .ok_or_else(|| crate::ModelError::Invalid("pattern produced no copies".into()))
            }
            ExtrudeOp::Join | ExtrudeOp::Cut => {
                let mut result = self.cached_body(target).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("target {} has no body", target))
                })?;
                let csg_op = if op == ExtrudeOp::Join {
                    CsgOp::Union
                } else {
                    CsgOp::Difference
                };
                for t in instances {
                    result = boolean(&result, &instantiate(t), csg_op)?;
                }
                Ok(result)
            }
        }
    }
}

/// Convert a solved sketch into profiles (islands with holes), classifying
/// contours by containment depth (0 = island, 1 = hole).
pub fn build_profiles(sketch: &Sketch, cfg: &TessellationConfig) -> crate::Result<Vec<Profile2D>> {
    let contours = sketch.profile_contours(cfg);
    if contours.is_empty() {
        return Ok(Vec::new());
    }

    // Containment matrix (approximate: test one representative vertex).
    let contains = |outer: &Vec<Point2>, inner: &Vec<Point2>| -> bool {
        let p = inner[0];
        point_in_polygon(p, outer)
    };

    let mut depth = vec![0usize; contours.len()];
    for i in 0..contours.len() {
        for j in 0..contours.len() {
            if i != j && contains(&contours[i], &contours[j]) {
                depth[j] += 1;
            }
        }
    }

    let mut profiles = Vec::new();
    for (i, c) in contours.iter().enumerate() {
        if depth[i] == 0 {
            let holes: Vec<Vec<Point2>> = contours
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i && depth[*j] == 1 && contains(c, &contours[*j]))
                .map(|(_, h)| h.clone())
                .collect();
            profiles.push(Profile2D::new(c.clone(), holes)?);
        }
    }
    // Contours nested deeper than one level are ignored (documented
    // limitation; the ear-clip bridger supports one hole level per island).
    Ok(profiles)
}

/// Winding-parity point-in-polygon test.
fn point_in_polygon(p: Point2, poly: &[Point2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::feature::{ExtrudeOp, ExtrudeParams, Feature, PrimitiveKind, PrimitiveParams};
    use crate::{Command, CommandStack, DatumParams, DimField, HoleKind, HoleParams, Param};
    use forge_core::{FeatureId, Point3, SketchId, Vector3};
    use forge_sketch::{DatumPlane, Sketch, SketchPlane};

    fn doc_with_box() -> Document {
        let mut doc = Document::new("test");
        doc.add_feature(Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Box,
            center: Point3::origin(),
            dims: Vector3::new(10.0, 10.0, 10.0),
        }))
        .unwrap();
        doc
    }

    #[test]
    fn evaluates_primitive_body() {
        let mut doc = doc_with_box();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert_eq!(result.bodies.len(), 1);
        assert!(result.total_triangles >= 12);
        assert!(result.errors.is_empty());
        assert_eq!(result.evaluated, 1);
        // Second pass reuses the cache.
        let result2 = ev.evaluate(&mut doc);
        assert_eq!(result2.reused, 1);
        assert_eq!(result2.evaluated, 0);
    }

    // ---- Imported mesh bodies (I-01) ------------------------------------

    fn import_box(doc: &mut Document, dims: Vector3) -> FeatureId {
        let mesh = forge_geometry::primitives::box_from_center_extents(Point3::origin(), dims);
        doc.add_feature(Feature::ImportedMesh(crate::ImportedMeshParams {
            source: "test_box.stl".into(),
            mesh,
        }))
        .unwrap()
    }

    #[test]
    fn evaluates_imported_mesh_body_with_normals() {
        let mut doc = Document::new("import");
        import_box(&mut doc, Vector3::new(3.0, 4.0, 5.0));
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bodies.len(), 1);
        let body = &result.bodies[0];
        assert_eq!(body.mesh.tri_count(), 12);
        assert!(
            body.mesh.normals.is_some(),
            "evaluation ensures normals for rendering"
        );
        assert!((body.mesh.volume_signed() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn boolean_cut_works_on_imported_body() {
        // The point of I-01: imported meshes enter the boolean workflow.
        let mut doc = Document::new("import-boolean");
        let imported = import_box(&mut doc, Vector3::new(20.0, 20.0, 20.0));
        let tool = add_box(
            &mut doc,
            "tool",
            Point3::origin(),
            Vector3::new(10.0, 10.0, 10.0),
        );
        doc.add_feature(Feature::Boolean(crate::BooleanFeature {
            op: forge_geometry::CsgOp::Difference,
            operands: vec![imported, tool],
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // The boolean consumed both operands into one body.
        assert_eq!(result.bodies.len(), 1);
        let v = result.bodies[0].mesh.volume_signed();
        assert!(
            (v - (20.0 * 20.0 * 20.0 - 10.0 * 10.0 * 10.0)).abs() < 1e-6,
            "cut volume {v}"
        );
    }

    #[test]
    fn imported_mesh_serializes_roundtrip_ron() {
        // Serialization check without forge-io (no circular dep): the
        // feature round-trips through RON in-place.
        let mesh = forge_geometry::primitives::box_from_center_extents(
            Point3::origin(),
            Vector3::new(2.0, 3.0, 4.0),
        );
        let feature = Feature::ImportedMesh(crate::ImportedMeshParams {
            source: "box.stl".into(),
            mesh: mesh.clone(),
        });
        let ron = ron::to_string(&feature).unwrap();
        let back: Feature = ron::from_str(&ron).unwrap();
        match back {
            Feature::ImportedMesh(p) => {
                assert_eq!(p.source, "box.stl");
                assert_eq!(p.mesh.tri_count(), mesh.tri_count());
                assert!((p.mesh.volume_signed() - 24.0).abs() < 1e-9);
            }
            other => panic!("wrong feature {other:?}"),
        }
    }

    // ---- Patterns & mirror (F-01/02/03) --------------------------------

    fn add_box(doc: &mut Document, name: &str, center: Point3, dims: Vector3) -> FeatureId {
        doc.add_feature(Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Box,
            center,
            dims,
        }))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn mirror_new_body_preserves_volume() {
        let mut doc = Document::new("mirror");
        let box_id = add_box(
            &mut doc,
            "seed",
            Point3::new(15.0, 0.0, 0.0),
            Vector3::new(4.0, 6.0, 8.0),
        );
        doc.add_feature(Feature::Mirror(crate::MirrorParams {
            source: box_id,
            plane_point: Point3::origin(),
            plane_normal: Vector3::x(),
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Seed + mirrored copy are both visible.
        assert_eq!(result.bodies.len(), 2);
        let v = result
            .bodies
            .iter()
            .map(|b| b.mesh.volume_signed())
            .sum::<f64>();
        assert!((v - 2.0 * 4.0 * 6.0 * 8.0).abs() < 1e-6, "total volume {v}");
        // The mirrored copy sits on the -X side: bbox min x ~ 13.
        let mirrored = &result.bodies[1];
        let bb = mirrored.mesh.bbox();
        assert!((bb.max.x + 13.0).abs() < 1e-6, "bbox {:?}", bb);
        assert!((bb.min.x + 17.0).abs() < 1e-6, "bbox {:?}", bb);
    }

    #[test]
    fn mirror_join_doubles_symmetric_body() {
        let mut doc = Document::new("mirror-join");
        let half = add_box(
            &mut doc,
            "half",
            Point3::new(5.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 10.0),
        );
        let joined = doc
            .add_feature(Feature::Mirror(crate::MirrorParams {
                source: half,
                plane_point: Point3::origin(),
                plane_normal: Vector3::x(),
                operation: ExtrudeOp::Join,
                target: half,
            }))
            .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Target consumed, single symmetric body remains.
        assert_eq!(result.bodies.len(), 1);
        assert_eq!(result.bodies[0].source, joined);
        let v = result.bodies[0].mesh.volume_signed();
        assert!((v - 2000.0).abs() < 1e-6, "volume {v}");
        // The seam boxes share the x=0 face; union must stay closed.
        assert!(result.bodies[0].mesh.is_closed());
    }

    #[test]
    fn linear_pattern_new_disjoint_instances() {
        let mut doc = Document::new("linpat");
        let seed = add_box(
            &mut doc,
            "seed",
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc.add_feature(Feature::LinearPattern(crate::LinearPatternParams {
            source: seed,
            direction: Vector3::x(),
            count: 4,
            spacing: 10.0,
            symmetric: false,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Seed + one pattern body (3 copies unioned).
        assert_eq!(result.bodies.len(), 2);
        let pattern = result.bodies.last().unwrap();
        let v = pattern.mesh.volume_signed();
        assert!((v - 3.0 * 8.0).abs() < 1e-6, "pattern volume {v}");
        // Copies at +10, +20, +30; last one reaches x = 30 + 1.
        let bb = pattern.mesh.bbox();
        assert!((bb.max.x - 31.0).abs() < 1e-6, "bbox {:?}", bb);
        assert!((bb.min.x - 9.0).abs() < 1e-6, "bbox {:?}", bb);
        assert!(pattern.mesh.is_closed());
    }

    #[test]
    fn linear_pattern_symmetric_spreads_both_sides() {
        let mut doc = Document::new("linpat-sym");
        let seed = add_box(
            &mut doc,
            "seed",
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc.add_feature(Feature::LinearPattern(crate::LinearPatternParams {
            source: seed,
            direction: Vector3::y(),
            count: 5,
            spacing: 4.0,
            symmetric: true,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let pattern = result.bodies.last().unwrap();
        // 4 copies (±2·spacing, ±spacing) around the seed.
        let v = pattern.mesh.volume_signed();
        assert!((v - 4.0 * 8.0).abs() < 1e-6, "volume {v}");
        let bb = pattern.mesh.bbox();
        assert!((bb.max.y - 9.0).abs() < 1e-6, "bbox {:?}", bb);
        assert!((bb.min.y + 9.0).abs() < 1e-6, "bbox {:?}", bb);
    }

    #[test]
    fn linear_pattern_cuts_target() {
        // Pattern of punch bodies cut from a plate.
        let mut doc = Document::new("linpat-cut");
        let plate = add_box(
            &mut doc,
            "plate",
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(40.0, 10.0, 10.0),
        );
        let hole = add_box(
            &mut doc,
            "hole",
            Point3::new(-10.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 12.0),
        );
        doc.add_feature(Feature::LinearPattern(crate::LinearPatternParams {
            source: hole,
            direction: Vector3::x(),
            count: 3,
            spacing: 10.0,
            symmetric: false,
            operation: ExtrudeOp::Cut,
            target: plate,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Cut target and seed are consumed; only the pattern result remains.
        assert_eq!(result.bodies.len(), 1);
        let v = result.bodies[0].mesh.volume_signed();
        // Instances at x = -10 (seed), 0, +10: three 2×2 through-holes.
        assert!(
            (v - (4000.0 - 3.0 * 2.0 * 2.0 * 10.0)).abs() < 1e-6,
            "volume {v}"
        );
        // NOTE: no is_closed() here - BSP cuts leave collinear T-junction
        // seams (volume-exact, topologically open); tracked as roadmap K-02.
    }

    #[test]
    fn circular_pattern_full_circle_volume() {
        let mut doc = Document::new("circpat");
        let seed = add_box(
            &mut doc,
            "seed",
            Point3::new(10.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc.add_feature(Feature::CircularPattern(crate::CircularPatternParams {
            source: seed,
            axis_point: Point3::origin(),
            axis_dir: Vector3::z(),
            count: 6,
            angle: std::f64::consts::TAU,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let pattern = result.bodies.last().unwrap();
        // 5 rotated copies, disjoint from the seed and from each other
        // (chord distance between centers is 10, box is 2).
        let v = pattern.mesh.volume_signed();
        assert!((v - 5.0 * 8.0).abs() < 1e-6, "volume {v}");
        // Copies at 60°..300° (the seed at 0° is a separate body):
        // - 180° copy: min.x = -11
        // - 60°/300° copies: max.x = 11·cos60° + 1·sin60° = 5.5 + √3/2
        // - 60°/120° copies: max.y = 11·sin60° + 1·cos60°
        let (c60, s60) = (0.5_f64, 3_f64.sqrt() / 2.0);
        let bb = pattern.mesh.bbox();
        assert!((bb.min.x + 11.0).abs() < 1e-6, "bbox {:?}", bb);
        assert!(
            (bb.max.x - (11.0 * c60 + s60)).abs() < 1e-6,
            "bbox {:?}",
            bb
        );
        assert!(
            (bb.max.y - (11.0 * s60 + c60)).abs() < 1e-6,
            "bbox {:?}",
            bb
        );
        assert!(
            (bb.min.y + (11.0 * s60 + c60)).abs() < 1e-6,
            "bbox {:?}",
            bb
        );
    }

    // ---- Transform body (W-01: move semantics + pivot) ------------------

    #[test]
    fn transform_body_consumes_source_and_moves() {
        let mut doc = Document::new("transform");
        let source = add_box(
            &mut doc,
            "box",
            Point3::new(5.0, 0.0, 0.0),
            Vector3::new(2.0, 4.0, 6.0),
        );
        doc.add_feature(Feature::TransformBody {
            source,
            translation: Vector3::new(10.0, -3.0, 1.0),
            rotation: Vector3::new(0.0, 0.0, 0.0),
            pivot: Point3::origin(),
        })
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Move semantics: the source is consumed, exactly one body remains.
        assert_eq!(result.bodies.len(), 1, "source must be consumed");
        let body = &result.bodies[0];
        assert_eq!(body.id, BodyId::new(2));
        assert!((body.mesh.volume_signed() - 48.0).abs() < 1e-9);
        // The bbox center moved by the translation.
        let bb = body.mesh.bbox();
        let center = bb.center();
        assert!(
            (center - Point3::new(15.0, -3.0, 1.0)).norm() < 1e-9,
            "center {center:?}"
        );
    }

    #[test]
    fn transform_body_rotates_about_pivot() {
        let mut doc = Document::new("transform-pivot");
        let source = add_box(
            &mut doc,
            "box",
            Point3::new(10.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        // 90° about Z, pivot = the box center: the box spins in place.
        doc.add_feature(Feature::TransformBody {
            source,
            translation: Vector3::zeros(),
            rotation: Vector3::new(0.0, 0.0, std::f64::consts::FRAC_PI_2),
            pivot: Point3::new(10.0, 0.0, 0.0),
        })
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let body = &result.bodies[0];
        let bb = body.mesh.bbox();
        assert!(
            (bb.center() - Point3::new(10.0, 0.0, 0.0)).norm() < 1e-9,
            "spins about its own center, {:?}",
            bb.center()
        );
        assert!((body.mesh.volume_signed() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn transform_body_pivot_default_is_origin() {
        // Old files serialize without `pivot`; it must default to the
        // origin (backward compatibility with pre-W-01 documents).
        let with_pivot = Feature::TransformBody {
            source: FeatureId::new(7),
            translation: Vector3::new(1.0, 2.0, 3.0),
            rotation: Vector3::new(0.1, 0.2, 0.3),
            pivot: Point3::new(4.0, 5.0, 6.0),
        };
        let s = ron::to_string(&with_pivot).expect("serialize");
        // Strip the trailing `pivot: <value>` — `pivot` is the last field,
        // so everything between it and the final `)` is the value.
        let i = s.find("pivot").expect("pivot present");
        let close = s[i..].rfind(')').expect("variant close");
        let old_text = format!("{}{}", &s[..i], &s[i + close..]);
        assert!(!old_text.contains("pivot"), "stripped text: {old_text}");

        let old: Feature = ron::from_str(&old_text).expect("parse old format");
        match old {
            Feature::TransformBody {
                source,
                translation,
                rotation,
                pivot,
            } => {
                assert_eq!(source, FeatureId::new(7));
                assert!((translation - Vector3::new(1.0, 2.0, 3.0)).norm() < 1e-12);
                assert!((rotation - Vector3::new(0.1, 0.2, 0.3)).norm() < 1e-12);
                assert!(
                    (pivot - Point3::origin()).norm() < 1e-12,
                    "missing pivot must default to the origin"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn circular_pattern_partial_span_angle() {
        let mut doc = Document::new("circpat-partial");
        let seed = add_box(
            &mut doc,
            "seed",
            Point3::new(10.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc.add_feature(Feature::CircularPattern(crate::CircularPatternParams {
            source: seed,
            axis_point: Point3::new(0.0, 0.0, 5.0),
            axis_dir: Vector3::new(0.0, 0.0, 1.0),
            count: 3,
            angle: std::f64::consts::FRAC_PI_2, // 0, 45, 90 degrees
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Pattern body = copies at 45° and 90° (seed at 0° is separate).
        let pattern = result.bodies.last().unwrap();
        let bb = pattern.mesh.bbox();
        // 90° copy: center (0, 10), reaches y = 11.
        assert!((bb.max.y - 11.0).abs() < 1e-6, "bbox {:?}", bb);
        // Nothing below the X axis (all copies in the first quadrant).
        assert!(bb.min.y > -1e-6, "bbox {:?}", bb);
    }

    #[test]
    fn pattern_validation_errors() {
        let mut doc = Document::new("bad");
        let seed = add_box(
            &mut doc,
            "seed",
            Point3::origin(),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc.add_feature(Feature::LinearPattern(crate::LinearPatternParams {
            source: seed,
            direction: Vector3::new(0.0, 0.0, 0.0), // degenerate
            count: 3,
            spacing: 5.0,
            symmetric: false,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(
            result.errors.values().any(|e| e.contains("degenerate")),
            "{:?}",
            result.errors
        );

        let mut doc2 = Document::new("bad2");
        let seed2 = add_box(
            &mut doc2,
            "seed",
            Point3::origin(),
            Vector3::new(2.0, 2.0, 2.0),
        );
        doc2.add_feature(Feature::Mirror(crate::MirrorParams {
            source: seed2,
            plane_point: Point3::origin(),
            plane_normal: Vector3::new(0.0, 0.0, 0.0), // degenerate
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        }))
        .unwrap();
        let mut ev2 = Evaluator::default();
        let result2 = ev2.evaluate(&mut doc2);
        assert!(
            result2.errors.values().any(|e| e.contains("degenerate")),
            "{:?}",
            result2.errors
        );
    }

    #[test]
    fn extrude_cut_consumes_target() {
        let mut doc = Document::new("cut");
        let plate = add_box(
            &mut doc,
            "plate",
            Point3::origin(),
            Vector3::new(10.0, 10.0, 10.0),
        );
        let mut sketch = Sketch::new(
            SketchId::new(1),
            "s",
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
        );
        sketch.add_rectangle(
            forge_core::Point2::new(-1.0, -1.0),
            forge_core::Point2::new(1.0, 1.0),
        );
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 20.0,
            direction: forge_geometry::ExtrudeDirection::Symmetric,
            operation: ExtrudeOp::Cut,
            target: plate,
            draft_angle: 0.0,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // The raw plate is hidden; only the drilled result is visible.
        assert_eq!(result.bodies.len(), 1);
        let v = result.bodies[0].mesh.volume_signed();
        // 2×2 square hole through the 10 mm plate.
        assert!((v - 960.0).abs() < 1e-6, "volume {v}");
    }

    #[test]
    fn sketch_extrude_pipeline() {
        let mut doc = Document::new("pipe");
        let mut sketch = Sketch::new(
            SketchId::new(1),
            "s1",
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
        );
        sketch.add_rectangle(Point2::new(-10.0, -10.0), Point2::new(10.0, 10.0));
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        let extrude_id = doc
            .add_feature(Feature::Extrude(ExtrudeParams {
                profile: sketch_id,
                distance: 5.0,
                direction: forge_geometry::ExtrudeDirection::Positive,
                operation: ExtrudeOp::New,
                target: FeatureId::NONE,
                draft_angle: 0.0,
            }))
            .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bodies.len(), 1);
        let body = &result.bodies[0];
        assert_eq!(body.id.raw(), extrude_id.raw());
        let v = body.mesh.volume_signed();
        assert!((v - 400.0 * 5.0).abs() < 1e-6, "volume {v}");
    }

    // ---- F-06: draft / taper through the model layer --------------------

    #[test]
    fn extrude_with_draft_frustum_volume() {
        // 20x20 sketch extruded 5 mm with a 10° draft: the top cap scales
        // toward the section centroid; the frustum volume follows
        // V = h/3 · (A1 + A2 + √(A1·A2)) with k from the mean vertex
        // radius (4 corners at 10√2 → r_mean = 10√2).
        let mut doc = Document::new("draft");
        let mut sketch = Sketch::new(
            SketchId::new(1),
            "s",
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
        );
        sketch.add_rectangle(Point2::new(-10.0, -10.0), Point2::new(10.0, 10.0));
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        let h = 5.0;
        let draft = 10.0_f64.to_radians();
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: h,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: draft,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        let r_mean = 10.0 * std::f64::consts::SQRT_2;
        let k = 1.0 - draft.tan() * h / r_mean;
        let (a1, a2) = (400.0, 400.0 * k * k);
        let want = h / 3.0 * (a1 + a2 + (a1 * a2).sqrt());
        assert!((v - want).abs() < 1e-9, "volume {v} vs {want}");
    }

    #[test]
    fn extrude_draft_defaults_to_zero_for_old_files() {
        // Pre-F-06 RON has no `draft_angle` field: it must deserialize to
        // 0.0 (straight walls, behavior unchanged).
        let feature = Feature::Extrude(ExtrudeParams {
            profile: FeatureId::new(3),
            distance: 12.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.5,
        });
        let s = ron::to_string(&feature).expect("serialize");
        // Remove the trailing field verbatim (old files have no
        // `draft_angle`): compact RON emits `,draft_angle:0.5` exactly.
        let old_text = s.replace(",draft_angle:0.5", "");
        assert_ne!(old_text, s, "field fragment not found in: {s}");
        let old: Feature = ron::from_str(&old_text).expect("parse old format");
        match old {
            Feature::Extrude(p) => assert_eq!(p.draft_angle, 0.0),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn dirty_repropagation_on_edit() {
        let mut doc = doc_with_box();
        let mut ev = Evaluator::default();
        ev.evaluate(&mut doc);
        // Edit the box -> re-evaluation happens.
        let id = FeatureId::new(1);
        doc.edit_feature(
            id,
            Feature::Primitive(PrimitiveParams {
                kind: PrimitiveKind::Box,
                center: Point3::origin(),
                dims: Vector3::new(20.0, 10.0, 10.0),
            }),
        )
        .unwrap();
        let result = ev.evaluate(&mut doc);
        assert_eq!(result.evaluated, 1);
        let v = result.bodies[0].mesh.volume_signed();
        assert!((v - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn boolean_consumes_operands() {
        let mut doc = Document::new("bool");
        let a = doc
            .add_feature(Feature::Primitive(PrimitiveParams {
                kind: PrimitiveKind::Box,
                center: Point3::origin(),
                dims: Vector3::new(10.0, 10.0, 10.0),
            }))
            .unwrap();
        let b = doc
            .add_feature(Feature::Primitive(PrimitiveParams {
                kind: PrimitiveKind::Sphere,
                center: Point3::new(0.0, 0.0, 5.0),
                dims: Vector3::new(6.0, 0.0, 0.0),
            }))
            .unwrap();
        doc.add_feature(Feature::Boolean(crate::BooleanFeature {
            op: CsgOp::Union,
            operands: vec![a, b],
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bodies.len(), 1, "operands are consumed");
        let v = result.bodies[0].mesh.volume_signed();
        // Union of a 1000 box with a half-buried sphere.
        assert!(v > 1000.0 && v < 2000.0, "volume {v}");

        // Suppressing the boolean reveals both operands again.
        doc.tree.set_suppressed(FeatureId::new(3), true).unwrap();
        let result2 = ev.evaluate(&mut doc);
        assert_eq!(result2.bodies.len(), 2);
    }

    #[test]
    fn invalid_primitive_reports_error() {
        let mut doc = Document::new("bad");
        doc.add_feature(Feature::Primitive(PrimitiveParams {
            kind: PrimitiveKind::Cylinder,
            center: Point3::origin(),
            dims: Vector3::new(5.0, -20.0, 0.0), // negative height
        }))
        .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert_eq!(result.bodies.len(), 0);
        assert!(!result.errors.is_empty());
    }

    // ---- P-01: parameters, expressions, dimension bindings ------------

    #[test]
    fn parameter_expressions_resolve_in_dependency_order() {
        let mut doc = Document::new("params");
        let th = doc.next_param_id();
        doc.set_param(th, Param::length("th", 5.0));
        let width = doc.next_param_id();
        doc.set_param(width, Param::expressed("width", "2*th + 1", false));
        let depth = doc.next_param_id();
        doc.set_param(depth, Param::expressed("depth", "width / 2", false));
        doc.resolve_params().expect("resolve");
        assert!((doc.params[&width].value - 11.0).abs() < 1e-12);
        assert!((doc.params[&depth].value - 5.5).abs() < 1e-12);
    }

    #[test]
    fn parameter_cycle_is_reported() {
        let mut doc = Document::new("cycle");
        let a = doc.next_param_id();
        doc.set_param(a, Param::expressed("a", "b + 1", false));
        let b = doc.next_param_id();
        doc.set_param(b, Param::expressed("b", "a + 1", false));
        let err = doc.resolve_params().expect_err("cycle must fail");
        let msg = format!("{err}");
        assert!(msg.contains("cycle"), "message: {msg}");
    }

    fn add_rect_sketch(doc: &mut Document, plane: SketchPlane, w: f64, h: f64) -> FeatureId {
        let sid = SketchId::new(doc.allocator.next_id());
        let mut sketch = Sketch::new(sid, "profile", plane);
        sketch.add_rectangle(
            Point2::new(-w / 2.0, -h / 2.0),
            Point2::new(w / 2.0, h / 2.0),
        );
        doc.add_feature(Feature::Sketch(sketch))
            .expect("add sketch")
    }

    #[test]
    fn dimension_binding_drives_extrude_and_re_evaluates() {
        let mut doc = Document::new("bound");
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
            10.0,
            10.0,
        );
        let extrude_id = doc
            .add_feature(Feature::Extrude(ExtrudeParams {
                profile: sketch_id,
                distance: 5.0,
                direction: forge_geometry::ExtrudeDirection::Positive,
                operation: ExtrudeOp::New,
                target: FeatureId::NONE,
                draft_angle: 0.0,
            }))
            .unwrap();

        // Parameter table: h = 12, bound to the extrude distance.
        let pid = doc.next_param_id();
        let mut p = Param::length("h", 12.0);
        p.id = pid;
        doc.set_param(pid, p.clone());
        doc.set_binding(extrude_id, DimField::ExtrudeDistance, Some("h".into()));
        doc.mark_bindings_dirty();

        let mut ev = Evaluator::default();
        let r1 = ev.evaluate(&mut doc);
        assert!(r1.errors.is_empty(), "{:?}", r1.errors);
        let v1 = r1.bodies[0].mesh.volume_signed();
        assert!((v1 - 10.0 * 10.0 * 12.0).abs() < 1e-6, "volume {v1}");

        // Change the parameter through the undoable command: the bound
        // feature must re-evaluate (not be reused from cache).
        let mut p2 = Param::length("h", 20.0);
        p2.id = pid;
        let mut commands = CommandStack::new();
        commands
            .execute(
                Command::SetParam {
                    id: pid,
                    before: Some(p),
                    after: Some(p2),
                },
                &mut doc,
            )
            .expect("set param");

        let r2 = ev.evaluate(&mut doc);
        let v2 = r2.bodies[0].mesh.volume_signed();
        assert!((v2 - 10.0 * 10.0 * 20.0).abs() < 1e-6, "volume {v2}");
        // The volume change proves the extrude re-evaluated (a reused
        // cache entry would still show 1200).
        assert!(r2.evaluated >= 1, "extrude re-evaluated: {}", r2.evaluated);

        // Undo restores h = 12.
        commands.undo(&mut doc).expect("undo");
        let r3 = ev.evaluate(&mut doc);
        let v3 = r3.bodies[0].mesh.volume_signed();
        assert!((v3 - 1200.0).abs() < 1e-6, "volume {v3}");
    }

    #[test]
    fn binding_errors_surface_on_the_feature() {
        let mut doc = Document::new("bad-binding");
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
            10.0,
            10.0,
        );
        let extrude_id = doc
            .add_feature(Feature::Extrude(ExtrudeParams {
                profile: sketch_id,
                distance: 5.0,
                direction: forge_geometry::ExtrudeDirection::Positive,
                operation: ExtrudeOp::New,
                target: FeatureId::NONE,
                draft_angle: 0.0,
            }))
            .unwrap();
        // Binding references a parameter that does not exist.
        doc.set_binding(
            extrude_id,
            DimField::ExtrudeDistance,
            Some("no_such_param".into()),
        );
        doc.mark_bindings_dirty();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        let msg = result.errors.get(&extrude_id).cloned().unwrap_or_default();
        assert!(msg.contains("no_such_param"), "error was: {msg}");
    }

    // ---- D-01: datum planes as sketch carriers -------------------------

    #[test]
    fn datum_offset_carries_sketch_and_extrude() {
        let mut doc = Document::new("datum");
        let datum_id = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::DatumRef { feature: datum_id },
            10.0,
            10.0,
        );
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 5.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // The datum itself produces no body; the extrusion sits on [5, 10].
        assert_eq!(result.bodies.len(), 1);
        let bb = result.bodies[0].mesh.bbox();
        assert!((bb.min.z - 5.0).abs() < 1e-6, "bbox {bb:?}");
        assert!((bb.max.z - 10.0).abs() < 1e-6, "bbox {bb:?}");
        // Evaluation order: datum before sketch (dependency edge).
        let order: Vec<FeatureId> = result.bodies.iter().map(|b| b.source).collect();
        assert_eq!(order.len(), 1);
    }

    #[test]
    fn editing_datum_re_evaluates_dependent_sketch() {
        let mut doc = Document::new("datum-edit");
        let datum_id = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::DatumRef { feature: datum_id },
            10.0,
            10.0,
        );
        let extrude_id = doc
            .add_feature(Feature::Extrude(ExtrudeParams {
                profile: sketch_id,
                distance: 5.0,
                direction: forge_geometry::ExtrudeDirection::Positive,
                operation: ExtrudeOp::New,
                target: FeatureId::NONE,
                draft_angle: 0.0,
            }))
            .unwrap();

        let mut ev = Evaluator::default();
        let r1 = ev.evaluate(&mut doc);
        assert!(r1.errors.is_empty(), "{:?}", r1.errors);
        let bb1 = r1.bodies[0].mesh.bbox();
        assert!((bb1.min.z - 5.0).abs() < 1e-6);

        // Move the datum to z = 10: the extrusion must follow.
        doc.edit_feature(
            datum_id,
            Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 10.0,
            }),
        )
        .unwrap();
        let r2 = ev.evaluate(&mut doc);
        assert!(r2.errors.is_empty(), "{:?}", r2.errors);
        let bb2 = r2.bodies[0].mesh.bbox();
        assert!((bb2.min.z - 10.0).abs() < 1e-6, "bbox {bb2:?}");
        assert!((bb2.max.z - 15.0).abs() < 1e-6, "bbox {bb2:?}");
        // The dependent chain actually re-evaluated.
        assert!(r2.evaluated >= 3, "evaluated {}", r2.evaluated);
        let _ = extrude_id;
    }

    #[test]
    fn datum_tilt_keeps_volume_invariant() {
        let mut doc = Document::new("datum-tilt");
        let datum_id = doc
            .add_feature(Feature::Datum(DatumParams::Angle {
                base: DatumPlane::XY,
                axis: 0,
                angle: 30f64.to_radians(),
            }))
            .unwrap();
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::DatumRef { feature: datum_id },
            10.0,
            10.0,
        );
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 5.0,
            direction: forge_geometry::ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        }))
        .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        assert!((v - 500.0).abs() < 1e-6, "tilted extrusion volume {v}");
    }

    #[test]
    fn suppressed_datum_reports_clear_error() {
        let mut doc = Document::new("datum-suppressed");
        let datum_id = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sketch_id = add_rect_sketch(
            &mut doc,
            SketchPlane::DatumRef { feature: datum_id },
            10.0,
            10.0,
        );
        let extrude_id = doc
            .add_feature(Feature::Extrude(ExtrudeParams {
                profile: sketch_id,
                distance: 5.0,
                direction: forge_geometry::ExtrudeDirection::Positive,
                operation: ExtrudeOp::New,
                target: FeatureId::NONE,
                draft_angle: 0.0,
            }))
            .unwrap();
        doc.tree.set_suppressed(datum_id, true).unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        let msg = result.errors.get(&extrude_id).cloned().unwrap_or_default();
        assert!(msg.contains("suppressed datum"), "error was: {msg}");
    }

    // ---- F-04: hole features -------------------------------------------

    /// Cross-section area of the revolved tool at radius `r`: an n-gon
    /// with the segment count the tessellator picks for the tool's max
    /// radius (BSP booleans are volume-exact against this discretization).
    fn ngon_area(steps: usize, r: f64) -> f64 {
        0.5 * steps as f64 * r * r * (std::f64::consts::TAU / steps as f64).sin()
    }

    fn tool_steps(max_r: f64) -> usize {
        TessellationConfig::default().steps_for_arc(max_r, std::f64::consts::TAU)
    }

    fn hole_params(profile: FeatureId, target: FeatureId, kind: HoleKind) -> HoleParams {
        HoleParams {
            profile,
            kind,
            diameter: 6.0,
            depth: 12.0, // through the 10 mm plate
            direction: forge_geometry::ExtrudeDirection::Negative,
            counterbore_diameter: 11.0,
            counterbore_depth: 4.0,
            countersink_diameter: 10.0,
            countersink_angle: 90f64.to_radians(),
            drill_point: false,
            drill_angle: 118f64.to_radians(),
            target,
        }
    }

    /// A 30 x 30 x 10 plate with its top face on a datum at z = +5 (D-01
    /// doubles as the hole entry plane).
    fn plate_with_top_sketch(doc: &mut Document) -> (FeatureId, FeatureId) {
        let plate = add_box(
            doc,
            "plate",
            Point3::origin(),
            Vector3::new(30.0, 30.0, 10.0),
        );
        let datum = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sid = SketchId::new(doc.allocator.next_id());
        let mut sketch = Sketch::new(sid, "holes", SketchPlane::DatumRef { feature: datum });
        sketch.add_point(Point2::origin());
        let profile = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        (plate, profile)
    }

    #[test]
    fn simple_hole_volume_exact() {
        let mut doc = Document::new("hole-simple");
        let (plate, profile) = plate_with_top_sketch(&mut doc);
        doc.add_feature(Feature::Hole(hole_params(profile, plate, HoleKind::Simple)))
            .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.bodies.len(), 1, "plate consumed into the cut result");
        let v = result.bodies[0].mesh.volume_signed();
        let n = tool_steps(3.0);
        let expected = 30.0 * 30.0 * 10.0 - ngon_area(n, 3.0) * 10.0;
        assert!((v - expected).abs() < 1e-6, "volume {v} vs {expected}");
    }

    #[test]
    fn counterbore_hole_volume_exact() {
        let mut doc = Document::new("hole-cb");
        let (plate, profile) = plate_with_top_sketch(&mut doc);
        doc.add_feature(Feature::Hole(hole_params(
            profile,
            plate,
            HoleKind::Counterbore,
        )))
        .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        // cb: r 5.5 over 4 mm; shaft: r 3 over the remaining 6 mm. The
        // revolve uses the segment count of the max radius (5.5).
        let n = tool_steps(5.5);
        let cut = ngon_area(n, 5.5) * 4.0 + ngon_area(n, 3.0) * 6.0;
        let expected = 30.0 * 30.0 * 10.0 - cut;
        assert!((v - expected).abs() < 1e-6, "volume {v} vs {expected}");
    }

    #[test]
    fn countersink_hole_volume_exact() {
        let mut doc = Document::new("hole-cs");
        let (plate, profile) = plate_with_top_sketch(&mut doc);
        doc.add_feature(Feature::Hole(hole_params(
            profile,
            plate,
            HoleKind::Countersink,
        )))
        .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        // 90° included angle: cone height = (5 - 3) / tan(45°) = 2.
        // Polygonal frustum of similar n-gons: h/3 * (A1 + A2 + sqrt(A1 A2)).
        let n = tool_steps(5.0);
        let a_top = ngon_area(n, 5.0);
        let a_bot = ngon_area(n, 3.0);
        let frustum = 2.0 / 3.0 * (a_top + a_bot + (a_top * a_bot).sqrt());
        let shaft = a_bot * 8.0;
        let expected = 30.0 * 30.0 * 10.0 - frustum - shaft;
        assert!((v - expected).abs() < 1e-6, "volume {v} vs {expected}");
    }

    #[test]
    fn drill_point_adds_cone_volume() {
        let mut doc = Document::new("hole-drill");
        let (plate, profile) = plate_with_top_sketch(&mut doc);
        let mut p = hole_params(profile, plate, HoleKind::Simple);
        p.depth = 6.0; // blind hole, drill point fully inside the plate
        p.drill_point = true;
        doc.add_feature(Feature::Hole(p)).unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        let dp_h = 3.0 / (59f64.to_radians()).tan();
        let n = tool_steps(3.0);
        let a = ngon_area(n, 3.0);
        let cone = dp_h / 3.0 * a; // similar n-gons tapering to a point
        let shaft = a * 6.0;
        let expected = 30.0 * 30.0 * 10.0 - shaft - cone;
        assert!((v - expected).abs() < 1e-5, "volume {v} vs {expected}");
    }

    #[test]
    fn holes_cut_at_every_placement_point() {
        let mut doc = Document::new("hole-multi");
        let plate = add_box(
            &mut doc,
            "plate",
            Point3::origin(),
            Vector3::new(30.0, 30.0, 10.0),
        );
        let datum = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sid = SketchId::new(doc.allocator.next_id());
        let mut sketch = Sketch::new(sid, "holes", SketchPlane::DatumRef { feature: datum });
        sketch.add_point(Point2::new(-8.0, -8.0));
        sketch.add_point(Point2::new(8.0, 8.0));
        // A circle also contributes its center as a placement.
        sketch.add_circle(Point2::new(8.0, -8.0), 2.0);
        let profile = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        doc.add_feature(Feature::Hole(hole_params(profile, plate, HoleKind::Simple)))
            .unwrap();
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        let n = tool_steps(3.0);
        let expected = 30.0 * 30.0 * 10.0 - 3.0 * ngon_area(n, 3.0) * 10.0;
        assert!((v - expected).abs() < 1e-6, "volume {v} vs {expected}");
    }

    // ---- S-05: sketch diagnostics in the evaluation ---------------------

    #[test]
    fn sketch_reports_are_collected() {
        let mut doc = Document::new("dof");
        add_rect_sketch(
            &mut doc,
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
            10.0,
            10.0,
        );
        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert_eq!(result.sketch_reports.len(), 1);
        let (id, report) = result.sketch_reports.iter().next().unwrap();
        // A rectangle macro: 4 lines x 4 DOF = 16 DOF; 4 coincidences
        // (2 equations each) + 2 horizontal + 2 vertical = 12 equations
        // -> 4 DOF remaining (translation + width + height).
        assert_eq!(report.dof, 16);
        assert_eq!(report.equations, 12);
        assert_eq!(report.dof_balance, 4);
        let _ = id;
    }
}

#[cfg(test)]
mod ellipse_extrude_tests {
    use super::*;
    use crate::{ExtrudeOp, ExtrudeParams};
    use forge_core::{Point2, SketchId};
    use forge_geometry::ExtrudeDirection;
    use forge_sketch::{DatumPlane, Sketch};

    /// S-03 end-to-end: extruding a native ellipse yields a body whose
    /// volume approximates pi*rx*ry*h (tessellation-limited accuracy).
    #[test]
    fn extruded_ellipse_volume() {
        let mut doc = Document::new("ellipse");
        let mut sketch = Sketch::new(
            SketchId::new(1),
            "e",
            SketchPlane::Datum {
                datum: DatumPlane::XY,
            },
        );
        sketch.add_ellipse(Point2::origin(), 12.0, 7.0, 0.4);
        let sketch_id = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        let h = 4.0;
        doc.add_feature(Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: h,
            direction: ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        }))
        .unwrap();

        let mut ev = Evaluator::default();
        let result = ev.evaluate(&mut doc);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let v = result.bodies[0].mesh.volume_signed();
        let want = std::f64::consts::PI * 12.0 * 7.0 * h;
        // Polygonal approximation of the ellipse undershoots slightly;
        // default tessellation keeps it well inside 1%.
        assert!(
            (v - want).abs() / want < 0.01,
            "volume {v:.4} vs pi*rx*ry*h {want:.4}"
        );
    }
}
