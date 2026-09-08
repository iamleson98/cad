//! CSG boolean operations on triangle meshes, built on [`crate::bsp`].
//!
//! Supported operations (FR-SM-03): union, difference (subtract) and
//! intersection. Inputs and outputs are closed, outward-oriented meshes.
//!
//! Precision note: the BSP approach operates on the *tessellated*
//! representation, so the result is exact at the triangle level but not at
//! the analytic-surface level. This is the documented v0.1 strategy; the
//! Phase-4 B-Rep kernel (see the roadmap) upgrades the semantics while
//! keeping this module as a fast preview path.

use crate::bsp::{BspNode, Polygon};
use crate::error::{GeometryError, Result};
use crate::mesh::TriMesh;

/// The three set operations on solids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CsgOp {
    /// A ∪ B.
    Union,
    /// A − B.
    Difference,
    /// A ∩ B.
    Intersection,
}

impl std::fmt::Display for CsgOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CsgOp::Union => "union",
            CsgOp::Difference => "difference",
            CsgOp::Intersection => "intersection",
        })
    }
}

/// Run a boolean operation on two closed meshes.
pub fn boolean(a: &TriMesh, b: &TriMesh, op: CsgOp) -> Result<TriMesh> {
    let polys_a = mesh_to_polygons(a);
    let polys_b = mesh_to_polygons(b);

    if polys_a.is_empty() && polys_b.is_empty() {
        return Err(GeometryError::EmptyResult("both operands empty".into()));
    }

    let tree_a = BspNode::build(polys_a);
    let tree_b = BspNode::build(polys_b);

    // Degenerate operand shortcuts (empty / degenerate meshes).
    if tree_b.is_none() {
        return match (tree_a, op) {
            (Some(a), CsgOp::Union | CsgOp::Difference) => polygons_to_mesh(a.into_polygons()),
            (Some(_), CsgOp::Intersection) => Ok(TriMesh::default()),
            (None, _) => Ok(TriMesh::default()),
        };
    }
    if tree_a.is_none() {
        return match (tree_b, op) {
            (Some(b), CsgOp::Union) => polygons_to_mesh(b.into_polygons()),
            (Some(_), _) => Ok(TriMesh::default()),
            (None, _) => Ok(TriMesh::default()),
        };
    }

    let (Some(mut a), Some(mut b)) = (tree_a, tree_b) else {
        unreachable!("emptiness handled above")
    };

    match op {
        CsgOp::Union => {
            // a.clipTo(b); b.clipTo(a); b.invert(); b.clipTo(a);
            // b.invert(); a.build(b.allPolygons());
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let extra = std::mem::take(&mut *b).into_polygons();
            a.add_polygons(extra);
        }
        CsgOp::Difference => {
            // a.invert(); a.clipTo(b); b.clipTo(a); b.invert();
            // b.clipTo(a); b.invert(); a.build(b.allPolygons());
            // a.invert();
            a.invert();
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let extra = std::mem::take(&mut *b).into_polygons();
            a.add_polygons(extra);
            a.invert();
        }
        CsgOp::Intersection => {
            // a.invert(); b.clipTo(a); b.invert(); a.clipTo(b);
            // b.clipTo(a); a.build(b.allPolygons()); a.invert();
            a.invert();
            b.clip_to(&a);
            b.invert();
            a.clip_to(&b);
            b.clip_to(&a);
            let extra = std::mem::take(&mut *b).into_polygons();
            a.add_polygons(extra);
            a.invert();
        }
    }

    polygons_to_mesh(a.into_polygons())
}

fn mesh_to_polygons(mesh: &TriMesh) -> Vec<Polygon> {
    mesh.triangles()
        .map(|[a, b, c]| Polygon::from_triangle(a, b, c))
        .collect()
}

fn polygons_to_mesh(polygons: Vec<Polygon>) -> Result<TriMesh> {
    let mut mesh = TriMesh::default();
    for poly in polygons {
        let n = poly.vertices.len();
        if n < 3 {
            continue;
        }
        // Polygons are convex: fan triangulation.
        let base = mesh.positions.len() as u32;
        mesh.positions.extend_from_slice(&poly.vertices);
        for k in 1..n - 1 {
            mesh.indices
                .extend_from_slice(&[base, base + k as u32, base + (k + 1) as u32]);
        }
    }
    if mesh.tri_count() == 0 {
        return Err(GeometryError::EmptyResult(
            "boolean operation produced no output".into(),
        ));
    }
    mesh.weld(crate::mesh::WELD_EPS);
    mesh.remove_degenerate(1e-12);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;
    use forge_core::{Point3, TessellationConfig, Vector3};

    const CFG: TessellationConfig = TessellationConfig {
        chord_tolerance_mm: 0.05,
        max_segment_angle_rad: 10.0_f64.to_radians(),
        max_segments_per_circle: 256,
        min_segments_per_circle: 32,
    };

    fn box_at(center: [f64; 3], size: [f64; 3]) -> TriMesh {
        primitives::box_from_center_extents(
            Point3::new(center[0], center[1], center[2]),
            Vector3::new(size[0], size[1], size[2]),
        )
    }

    #[test]
    fn union_of_two_boxes() {
        let a = box_at([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = box_at([5.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let c = boolean(&a, &b, CsgOp::Union).unwrap();
        let expected = 1000.0 + 1000.0 - 500.0;
        assert!(
            (c.volume_signed() - expected).abs() < 1e-6,
            "union volume {} vs {}",
            c.volume_signed(),
            expected
        );
    }

    #[test]
    fn subtract_disjoint_keeps_volume() {
        let a = box_at([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = box_at([100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let c = boolean(&a, &b, CsgOp::Difference).unwrap();
        assert!((c.volume_signed() - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn intersection_of_boxes() {
        let a = box_at([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = box_at([5.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let c = boolean(&a, &b, CsgOp::Intersection).unwrap();
        assert!((c.volume_signed() - 500.0).abs() < 1e-6);
    }

    #[test]
    fn box_minus_cylinder_hole() {
        // The classic CAD smoke test: a plate with a drilled hole.
        let plate = box_at([0.0, 0.0, 5.0], [40.0, 40.0, 10.0]);
        let hole = primitives::cylinder(Point3::new(0.0, 0.0, -1.0), 5.0, 12.0, &CFG);
        let result = boolean(&plate, &hole, CsgOp::Difference).unwrap();
        let expected = 16000.0 - std::f64::consts::PI * 25.0 * 10.0;
        let got = result.volume_signed();
        assert!(
            (got - expected).abs() / expected < 0.01,
            "drilled plate volume {got} vs {expected}"
        );
    }

    #[test]
    fn union_box_sphere() {
        let boxm = box_at([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        let sph = primitives::sphere(Point3::new(0.0, 0.0, 10.0), 10.0, &CFG);
        let c = boolean(&boxm, &sph, CsgOp::Union).unwrap();
        // Spherical cap volume above z=10 (half the sphere, chord = 10):
        // union = box + sphere - overlap; overlap = half box? Compute
        // roughly: overlap = sphere ∩ box = half of the sphere volume.
        let sphere_v = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        let expected = 8000.0 + sphere_v - sphere_v * 0.5;
        let got = c.volume_signed();
        assert!(
            (got - expected).abs() / expected < 0.03,
            "union volume {got} vs {expected}"
        );
    }
}
