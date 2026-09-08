//! Feature definitions: the parametric operations of the tree.

use forge_core::{FeatureId, Point2, Point3, Vector3};
use forge_sketch::Sketch;
use serde::{Deserialize, Serialize};

/// How an extrusion interacts with existing bodies (boss vs cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtrudeOp {
    /// Create a new independent body.
    New,
    /// Union with the target body.
    Join,
    /// Subtract from the target body.
    Cut,
}

impl std::fmt::Display for ExtrudeOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ExtrudeOp::New => "New body",
            ExtrudeOp::Join => "Join",
            ExtrudeOp::Cut => "Cut",
        })
    }
}

/// Parameters of an extrude feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtrudeParams {
    /// Sketch feature providing the profile.
    pub profile: FeatureId,
    /// Extrusion distance (mm, positive).
    pub distance: f64,
    /// Extrusion side relative to the sketch plane normal.
    pub direction: forge_geometry::ExtrudeDirection,
    /// Interaction with existing bodies.
    pub operation: ExtrudeOp,
    /// Target body feature for Join/Cut (ignored for New).
    pub target: FeatureId,
}

/// Parameters of a revolve feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevolveParams {
    /// Sketch feature providing the profile contour.
    pub profile: FeatureId,
    /// Axis through two sketch-local points.
    pub axis_start: Point2,
    /// Axis end point.
    pub axis_end: Point2,
    /// Revolution angle (radians, `(0, 2pi]`).
    pub angle: f64,
    /// Interaction with existing bodies.
    pub operation: ExtrudeOp,
    /// Target body feature for Join/Cut.
    pub target: FeatureId,
}

/// Parameters of a loft feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoftParams {
    /// Sketch features providing the sections, bottom to top.
    pub sections: Vec<FeatureId>,
}

/// Parameters of a sweep-along-path feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepParams {
    /// Sketch feature providing the profile.
    pub profile: FeatureId,
    /// Path stations in world space (3D polyline).
    pub path: Vec<Point3>,
}

/// Analytic primitive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
    /// Axis-aligned box.
    Box,
    /// UV sphere.
    Sphere,
    /// Cylinder.
    Cylinder,
    /// Truncated cone.
    Cone,
    /// Torus.
    Torus,
}

impl std::fmt::Display for PrimitiveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PrimitiveKind::Box => "Box",
            PrimitiveKind::Sphere => "Sphere",
            PrimitiveKind::Cylinder => "Cylinder",
            PrimitiveKind::Cone => "Cone",
            PrimitiveKind::Torus => "Torus",
        })
    }
}

/// Parameters of a primitive feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveParams {
    /// Which primitive.
    pub kind: PrimitiveKind,
    /// Center position (world).
    pub center: Point3,
    /// Size / radius fields, interpreted per kind:
    /// - Box: full extents `(dx, dy, dz)`
    /// - Sphere: `[0]` = radius
    /// - Cylinder: `[0]` = radius, `[1]` = height (along +z)
    /// - Cone: `[0]` = base radius, `[1]` = top radius, `[2]` = height
    /// - Torus: `[0]` = major radius, `[1]` = minor radius
    pub dims: Vector3,
}

/// Parameters of a boolean feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BooleanFeature {
    /// Set operation.
    pub op: forge_geometry::CsgOp,
    /// Operand body features (first is the base for Difference).
    pub operands: Vec<FeatureId>,
}

/// Parameters of a linear pattern feature (F-02).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearPatternParams {
    /// Seed body feature whose instances are patterned.
    pub source: FeatureId,
    /// Pattern direction (world space, normalized during evaluation).
    pub direction: Vector3,
    /// Total instance count, including the seed at offset 0.
    pub count: usize,
    /// Spacing between consecutive instances (mm).
    pub spacing: f64,
    /// Distribute instances symmetrically on both sides of the seed.
    pub symmetric: bool,
    /// Interaction with existing bodies.
    pub operation: ExtrudeOp,
    /// Target body feature for Join/Cut (ignored for New).
    pub target: FeatureId,
}

/// Parameters of a circular pattern feature (F-03).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircularPatternParams {
    /// Seed body feature whose instances are patterned.
    pub source: FeatureId,
    /// A point on the rotation axis (world space).
    pub axis_point: Point3,
    /// Rotation axis direction (world space, normalized during evaluation).
    pub axis_dir: Vector3,
    /// Total instance count, including the seed at angle 0.
    pub count: usize,
    /// Total angular span of the pattern (radians). `TAU` = full circle.
    pub angle: f64,
    /// Interaction with existing bodies.
    pub operation: ExtrudeOp,
    /// Target body feature for Join/Cut (ignored for New).
    pub target: FeatureId,
}

/// Parameters of a mirror feature (F-01).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorParams {
    /// Source body feature to mirror.
    pub source: FeatureId,
    /// A point on the mirror plane (world space).
    pub plane_point: Point3,
    /// Mirror plane normal (world space, normalized during evaluation).
    pub plane_normal: Vector3,
    /// Interaction with existing bodies.
    pub operation: ExtrudeOp,
    /// Target body feature for Join/Cut (ignored for New).
    pub target: FeatureId,
}

/// A node of the parametric feature tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Feature {
    /// A 2D sketch on a plane (profile carrier).
    Sketch(Sketch),
    /// Extrude a sketch profile.
    Extrude(ExtrudeParams),
    /// Revolve a sketch profile.
    Revolve(RevolveParams),
    /// Loft between sketch sections.
    Loft(LoftParams),
    /// Sweep a profile along a 3D path.
    Sweep(SweepParams),
    /// Analytic primitive.
    Primitive(PrimitiveParams),
    /// Boolean combination of bodies.
    Boolean(BooleanFeature),
    /// Transform a body (rigid move).
    TransformBody {
        /// Source body feature.
        source: FeatureId,
        /// Translation.
        translation: Vector3,
        /// Euler angles (XYZ, radians).
        rotation: Vector3,
    },
    /// Replicate a body along a direction (F-02).
    LinearPattern(LinearPatternParams),
    /// Replicate a body around an axis (F-03).
    CircularPattern(CircularPatternParams),
    /// Mirror a body across a plane (F-01).
    Mirror(MirrorParams),
}

impl Feature {
    /// Short UI label.
    pub fn label(&self) -> String {
        match self {
            Feature::Sketch(s) => format!("Sketch: {}", s.name),
            Feature::Extrude(p) => format!("Extrude {} ({})", p.distance, p.operation),
            Feature::Revolve(p) => {
                format!("Revolve {:.0}\u{00b0}", p.angle.to_degrees())
            }
            Feature::Loft(p) => format!("Loft ({} sections)", p.sections.len()),
            Feature::Sweep(_) => "Sweep".into(),
            Feature::Primitive(p) => format!("Primitive: {}", p.kind),
            Feature::Boolean(b) => format!("Boolean: {}", b.op),
            Feature::TransformBody { .. } => "Transform".into(),
            Feature::LinearPattern(p) => {
                format!("Linear Pattern x{}", p.count.max(1))
            }
            Feature::CircularPattern(p) => {
                format!("Circular Pattern x{}", p.count.max(1))
            }
            Feature::Mirror(_) => "Mirror".into(),
        }
    }

    /// Feature ids this feature depends on (its DAG parents).
    pub fn dependencies(&self) -> Vec<FeatureId> {
        match self {
            Feature::Sketch(_) => Vec::new(),
            Feature::Extrude(p) => {
                let mut d = vec![p.profile];
                if p.operation != ExtrudeOp::New && !p.target.is_none() {
                    d.push(p.target);
                }
                d
            }
            Feature::Revolve(p) => {
                let mut d = vec![p.profile];
                if p.operation != ExtrudeOp::New && !p.target.is_none() {
                    d.push(p.target);
                }
                d
            }
            Feature::Loft(p) => p.sections.clone(),
            Feature::Sweep(p) => vec![p.profile],
            Feature::Primitive(_) => Vec::new(),
            Feature::Boolean(b) => b.operands.clone(),
            Feature::TransformBody { source, .. } => vec![*source],
            Feature::LinearPattern(p) => {
                let mut d = vec![p.source];
                if p.operation != ExtrudeOp::New && !p.target.is_none() {
                    d.push(p.target);
                }
                d
            }
            Feature::CircularPattern(p) => {
                let mut d = vec![p.source];
                if p.operation != ExtrudeOp::New && !p.target.is_none() {
                    d.push(p.target);
                }
                d
            }
            Feature::Mirror(p) => {
                let mut d = vec![p.source];
                if p.operation != ExtrudeOp::New && !p.target.is_none() {
                    d.push(p.target);
                }
                d
            }
        }
    }
}
