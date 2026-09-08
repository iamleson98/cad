//! Feature definitions: the parametric operations of the tree.

use forge_core::{FeatureId, Plane, Point2, Point3, Vector3};
use forge_geometry::TriMesh;
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

/// Hole flavor (F-04), SolidWorks hole-wizard semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleKind {
    /// Straight cylindrical hole.
    Simple,
    /// Cylinder + a wider counterbore at the head side.
    Counterbore,
    /// Cylinder + a conical countersink at the head side.
    Countersink,
}

impl std::fmt::Display for HoleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HoleKind::Simple => "Simple",
            HoleKind::Counterbore => "Counterbore",
            HoleKind::Countersink => "Countersink",
        })
    }
}

/// Parameters of a hole feature (F-04): a compound boolean-cut stack
/// (cylinder + counterbore cylinder / countersink cone + optional drill
/// point) placed at every point/circle-center of the placement sketch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleParams {
    /// Sketch feature providing hole placements (points and circle
    /// centers; circle diameter is *not* used).
    pub profile: FeatureId,
    /// Hole kind.
    pub kind: HoleKind,
    /// Hole diameter (mm).
    pub diameter: f64,
    /// Hole depth measured from the sketch plane into the material (mm).
    pub depth: f64,
    /// Cut side relative to the sketch plane normal (Positive = into
    /// material on the +normal side, e.g. an XY sketch extruded up).
    pub direction: forge_geometry::ExtrudeDirection,
    /// Counterbore diameter (Counterbore only).
    pub counterbore_diameter: f64,
    /// Counterbore depth from the sketch plane (Counterbore only).
    pub counterbore_depth: f64,
    /// Countersink major diameter (Countersink only).
    pub countersink_diameter: f64,
    /// Countersink included angle, radians (Countersink only).
    pub countersink_angle: f64,
    /// Add a conical drill point at the hole bottom (118° typical).
    pub drill_point: bool,
    /// Drill point included angle, radians.
    pub drill_angle: f64,
    /// Target body to cut (holes are always cuts).
    pub target: FeatureId,
}

/// Construction of a user datum plane beyond the three standard datums
/// (D-01).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DatumParams {
    /// Plane offset from a standard datum along its normal.
    Offset {
        /// Base datum.
        base: forge_sketch::DatumPlane,
        /// Offset along the base normal (mm).
        offset: f64,
    },
    /// Plane tilted about a base-plane in-plane axis, through the origin.
    Angle {
        /// Base datum.
        base: forge_sketch::DatumPlane,
        /// Tilt about the base plane's local X (0) or Y (1) axis.
        axis: u8,
        /// Tilt angle (radians).
        angle: f64,
    },
}

impl DatumParams {
    /// The world-space plane of this datum.
    pub fn to_plane(&self) -> Plane {
        match *self {
            DatumParams::Offset { base, offset } => {
                let mut plane = base.to_plane();
                let n: Vector3 = *plane.normal.as_ref();
                plane.origin += n * offset;
                plane
            }
            DatumParams::Angle { base, axis, angle } => {
                let plane = base.to_plane();
                // In-plane axes (u x v = n) from the plane's own frame.
                let (u, v) = plane.basis();
                let axis_vec = if axis == 0 { u } else { v };
                let n: Vector3 = *plane.normal.as_ref();
                let normal = nalgebra::UnitQuaternion::from_axis_angle(
                    &nalgebra::Unit::new_unchecked(axis_vec),
                    angle,
                ) * n;
                Plane::new(Point3::origin(), normal).unwrap_or(plane)
            }
        }
    }
}

impl Default for DatumParams {
    fn default() -> Self {
        DatumParams::Offset {
            base: forge_sketch::DatumPlane::XY,
            offset: 0.0,
        }
    }
}

/// Parameters of an imported mesh body (I-01).
///
/// The mesh is *embedded* in the document (not referenced by path) so the
/// native file is self-contained and the body survives file moves. The
/// mesh arrives already repaired by `forge_io::import_mesh`: welded,
/// degenerate-free, consistently oriented, normals computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedMeshParams {
    /// Source file name (display only).
    pub source: String,
    /// The repaired triangle mesh (model units, mm).
    pub mesh: TriMesh,
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
    /// Compound hole cut at sketch placements (F-04).
    Hole(HoleParams),
    /// User datum plane (D-01): construction geometry for sketch carriers
    /// and mirror/pattern references. Produces no body.
    Datum(DatumParams),
    /// Imported mesh body (I-01): repaired STL/OBJ geometry as a body.
    ImportedMesh(ImportedMeshParams),
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
            Feature::Hole(h) => format!("{} Hole \u{2300}{:.1}", h.kind, h.diameter),
            Feature::Datum(d) => match d {
                DatumParams::Offset { base, offset } => {
                    format!("Datum: {base:?} +{offset:.1}mm")
                }
                DatumParams::Angle { base, angle, .. } => {
                    format!("Datum: {base:?} tilted {:.0}\u{00b0}", angle.to_degrees())
                }
            },
            Feature::ImportedMesh(p) => {
                format!("Import: {} ({} tris)", p.source, p.mesh.tri_count())
            }
        }
    }

    /// Feature ids this feature depends on (its DAG parents).
    pub fn dependencies(&self) -> Vec<FeatureId> {
        match self {
            Feature::Sketch(s) => s.plane.datum_ref().into_iter().collect(),
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
            Feature::Hole(p) => vec![p.profile, p.target],
            Feature::Datum(_) => Vec::new(),
            Feature::ImportedMesh(_) => Vec::new(),
        }
    }
}
