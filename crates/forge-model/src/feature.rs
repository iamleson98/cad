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
        }
    }
}
