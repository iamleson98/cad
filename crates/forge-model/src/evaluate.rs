//! Feature evaluation: turns the parametric tree into triangle meshes.
//!
//! - Only **dirty** features are re-evaluated; clean results come from the
//!   [`Evaluator`] cache (FR-SM-04 dynamic DAG re-evaluation).
//! - Every feature evaluation runs inside [`std::panic::catch_unwind`]
//!   (NFR-RES-03): kernel panics degrade to per-feature error messages.
//! - Boolean features *consume* their operands: consumed bodies are hidden
//!   from the render set while the boolean succeeds, and reappear when it
//!   is suppressed or fails.

use crate::document::Document;
use crate::feature::{ExtrudeOp, Feature, PrimitiveKind};
use forge_core::{BodyId, FeatureId, Point2, TessellationConfig, Transform};
use forge_geometry::{
    boolean, extrude, loft, primitives, revolve, sweep_along_path, CsgOp, Profile2D, TriMesh,
};
use forge_sketch::Sketch;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

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

        for id in doc.tree.order().to_vec() {
            let Some(node) = doc.tree.get(id) else {
                continue;
            };
            if node.suppressed {
                self.cache.remove(&id);
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
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.evaluate_feature(&*doc, id, &feature)
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

        let total_triangles = bodies.iter().map(|b| b.mesh.tri_count()).sum();
        Evaluation {
            bodies,
            errors,
            evaluated,
            reused,
            total_triangles,
            duration: start.elapsed(),
        }
    }

    /// Evaluate one feature. `Ok(None)` means "no body" (sketch features
    /// are handled by the caller through [`build_profiles`]).
    fn evaluate_feature(
        &mut self,
        doc: &Document,
        id: FeatureId,
        feature: &Feature,
    ) -> crate::Result<Option<CachedResult>> {
        let _ = id;
        let mesh: TriMesh = match feature {
            Feature::Sketch(_) => return Ok(None),

            Feature::Primitive(p) => {
                // Validate dims before touching the kernel (negative or NaN
                // sizes are user input errors, not kernel panics).
                let (a, b, c) = (p.dims.x, p.dims.y, p.dims.z);
                let ok = match p.kind {
                    PrimitiveKind::Box => a > 0.0 && b > 0.0 && c > 0.0,
                    PrimitiveKind::Sphere => a > 0.0,
                    PrimitiveKind::Cylinder => a > 0.0 && b > 0.0,
                    PrimitiveKind::Cone => a >= 0.0 && b >= 0.0 && c > 0.0,
                    PrimitiveKind::Torus => a > 0.0 && b > 0.0,
                };
                if !ok || p.dims.x.is_nan() || p.dims.y.is_nan() || p.dims.z.is_nan() {
                    return Err(crate::ModelError::Invalid(format!(
                        "invalid dimensions {:?} for {}",
                        p.dims, p.kind
                    )));
                }
                let cfg = &self.tessellation;
                match p.kind {
                    PrimitiveKind::Box => primitives::box_from_center_extents(p.center, p.dims),
                    PrimitiveKind::Sphere => primitives::sphere(p.center, p.dims.x, cfg),
                    PrimitiveKind::Cylinder => {
                        primitives::cylinder(p.center, p.dims.x, p.dims.y, cfg)
                    }
                    PrimitiveKind::Cone => {
                        primitives::cone(p.center, p.dims.x, p.dims.y, p.dims.z, cfg)
                    }
                    PrimitiveKind::Torus => primitives::torus(p.center, p.dims.x, p.dims.y, cfg),
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
                let plane = doc
                    .sketch(p.profile)
                    .map(|s| s.plane.to_plane())
                    .ok_or_else(|| crate::ModelError::MissingFeature(format!("{}", p.profile)))?;
                let mut solid = TriMesh::default();
                for profile in &profiles {
                    let mesh = extrude(
                        profile,
                        &plane,
                        &forge_geometry::ExtrudeParams {
                            distance: p.distance,
                            direction: p.direction,
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
                let plane = doc
                    .sketch(p.profile)
                    .map(|s| s.plane.to_plane())
                    .ok_or_else(|| crate::ModelError::MissingFeature(format!("{}", p.profile)))?;
                let solid = revolve(
                    &profile.outer,
                    &plane,
                    p.axis_start,
                    p.axis_end,
                    &forge_geometry::RevolveParams { angle: p.angle },
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
                    let plane = doc
                        .sketch(*s_id)
                        .map(|s| s.plane.to_plane())
                        .ok_or_else(|| crate::ModelError::MissingFeature(format!("{s_id}")))?;
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
                let plane = doc
                    .sketch(p.profile)
                    .map(|s| s.plane.to_plane())
                    .ok_or_else(|| crate::ModelError::MissingFeature(format!("{}", p.profile)))?;
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
            } => {
                let mesh = self.cached_body(*source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("source {} has no body", source))
                })?;
                let iso = forge_core::Transform::from_parts(
                    nalgebra::Translation3::new(translation.x, translation.y, translation.z),
                    nalgebra::UnitQuaternion::from_euler_angles(rotation.x, rotation.y, rotation.z),
                );
                let mut m = mesh.transformed(&iso);
                m.compute_vertex_normals();
                m
            }

            Feature::LinearPattern(p) => {
                let seed = self.cached_body(p.source).cloned().ok_or_else(|| {
                    crate::ModelError::MissingEntity(format!("seed {} has no body", p.source))
                })?;
                let instances = linear_instance_transforms(p, p.operation != ExtrudeOp::New)?;
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
                let mirrored = source.mirrored(&p.plane_point, &p.plane_normal);
                apply_operation(self, p.operation, p.target, mirrored)?
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
/// of the pattern result (SolidWorks feature-pattern semantics).
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
        _ => Vec::new(),
    }
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
}
