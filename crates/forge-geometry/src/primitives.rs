//! Analytic solid primitives, tessellated directly into [`TriMesh`].
//!
//! All producers return **closed, consistently outward-oriented** meshes,
//! which the tests verify via [`TriMesh::is_closed`] and
//! [`TriMesh::volume`].

use crate::mesh::TriMesh;
use forge_core::{Point3, TessellationConfig, Vector3};

/// Axis-aligned box from center and *full* extents `(dx, dy, dz)`.
pub fn box_from_center_extents(center: Point3, extents: Vector3) -> TriMesh {
    let h = extents * 0.5;
    let (cx, cy, cz) = (center.x, center.y, center.z);
    let (hx, hy, hz) = (h.x, h.y, h.z);

    // 8 corners, CCW winding when viewed from outside.
    let p = [
        // z- face (bottom, seen from -z: x right, y up)
        Point3::new(cx - hx, cy - hy, cz - hz),
        Point3::new(cx + hx, cy - hy, cz - hz),
        Point3::new(cx + hx, cy + hy, cz - hz),
        Point3::new(cx - hx, cy + hy, cz - hz),
        // z+ face (top)
        Point3::new(cx - hx, cy - hy, cz + hz),
        Point3::new(cx + hx, cy - hy, cz + hz),
        Point3::new(cx + hx, cy + hy, cz + hz),
        Point3::new(cx - hx, cy + hy, cz + hz),
    ];

    let quads: [[usize; 4]; 6] = [
        [4, 5, 6, 7], // top (+z)
        [1, 0, 3, 2], // bottom (-z)
        [0, 4, 7, 3], // -x
        [5, 1, 2, 6], // +x
        [3, 7, 6, 2], // +y
        [0, 1, 5, 4], // -y
    ];

    let mut mesh = TriMesh {
        positions: p.to_vec(),
        indices: Vec::with_capacity(quads.len() * 6),
        normals: None,
    };
    for [a, b, c, d] in quads {
        let (a, b, c, d) = (a as u32, b as u32, c as u32, d as u32);
        mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }
    mesh.compute_vertex_normals();
    mesh
}

/// UV sphere.
pub fn sphere(center: Point3, radius: f64, cfg: &TessellationConfig) -> TriMesh {
    let sectors = cfg.segments_for_circle(radius);
    let rings = (sectors / 2).max(3);

    let mut mesh = TriMesh::with_capacity((rings + 1) * (sectors + 1), rings * sectors * 2);

    // Vertex grid: (ring, sector), ring 0 = north pole (+y), ring `rings`
    // = south pole. phi is the polar angle from +y.
    for i in 0..=rings {
        let phi = std::f64::consts::PI * i as f64 / rings as f64; // 0..pi
        for j in 0..=sectors {
            let theta = std::f64::consts::TAU * j as f64 / sectors as f64;
            let n = Vector3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            mesh.positions.push(center + n * radius);
        }
    }
    for i in 0..rings {
        for j in 0..sectors {
            let a = i * (sectors + 1) + j;
            let b = a + sectors + 1;
            if i > 0 {
                mesh.indices
                    .extend_from_slice(&[a as u32, (a + 1) as u32, b as u32]);
            }
            if i < rings - 1 {
                mesh.indices
                    .extend_from_slice(&[(a + 1) as u32, (b + 1) as u32, b as u32]);
            }
        }
    }
    // Merge the duplicated seam column and pole vertices.
    mesh.weld(crate::mesh::WELD_EPS);
    mesh.compute_vertex_normals();
    mesh
}

/// Solid cylinder from base-center along `+z` (use a transform to orient).
pub fn cylinder(
    base_center: Point3,
    radius: f64,
    height: f64,
    cfg: &TessellationConfig,
) -> TriMesh {
    let n = cfg.segments_for_circle(radius);
    let mut mesh = TriMesh::with_capacity(2 * n + 2, 4 * n);

    // 0: bottom center, 1: top center.
    mesh.positions.push(base_center);
    mesh.positions
        .push(base_center + Vector3::new(0.0, 0.0, height));
    // Bottom ring then top ring.
    for k in 0..n {
        let ang = std::f64::consts::TAU * k as f64 / n as f64;
        let (c, s) = (ang.cos(), ang.sin());
        mesh.positions.push(Point3::new(
            base_center.x + radius * c,
            base_center.y + radius * s,
            base_center.z,
        ));
    }
    for k in 0..n {
        let ang = std::f64::consts::TAU * k as f64 / n as f64;
        let (c, s) = (ang.cos(), ang.sin());
        mesh.positions.push(Point3::new(
            base_center.x + radius * c,
            base_center.y + radius * s,
            base_center.z + height,
        ));
    }
    // Rings start at index 2 (bottom) and 2+n (top).
    let (rb, rt) = (2u32, (2 + n) as u32);
    for k in 0..n as u32 {
        let k1 = (k + 1) % n as u32;
        // Side quads (outward).
        mesh.indices.extend_from_slice(&[rb + k, rb + k1, rt + k1]);
        mesh.indices.extend_from_slice(&[rb + k, rt + k1, rt + k]);
        // Bottom cap (viewed from -z, so reversed winding).
        mesh.indices.extend_from_slice(&[0, rb + k1, rb + k]);
        // Top cap.
        mesh.indices.extend_from_slice(&[1, rt + k, rt + k1]);
    }
    mesh.compute_vertex_normals();
    mesh
}

/// Solid (truncated) cone from base-center along `+z`.
pub fn cone(
    base_center: Point3,
    base_radius: f64,
    top_radius: f64,
    height: f64,
    cfg: &TessellationConfig,
) -> TriMesh {
    let n = cfg.segments_for_circle(base_radius.max(top_radius));
    let mut mesh = TriMesh::with_capacity(2 * n + 2, 4 * n);

    mesh.positions.push(base_center);
    mesh.positions
        .push(base_center + Vector3::new(0.0, 0.0, height));
    for k in 0..n {
        let ang = std::f64::consts::TAU * k as f64 / n as f64;
        mesh.positions.push(Point3::new(
            base_center.x + base_radius * ang.cos(),
            base_center.y + base_radius * ang.sin(),
            base_center.z,
        ));
    }
    for k in 0..n {
        let ang = std::f64::consts::TAU * k as f64 / n as f64;
        mesh.positions.push(Point3::new(
            base_center.x + top_radius * ang.cos(),
            base_center.y + top_radius * ang.sin(),
            base_center.z + height,
        ));
    }
    let (rb, rt) = (2u32, (2 + n) as u32);
    for k in 0..n as u32 {
        let k1 = (k + 1) % n as u32;
        mesh.indices.extend_from_slice(&[rb + k, rb + k1, rt + k1]);
        mesh.indices.extend_from_slice(&[rb + k, rt + k1, rt + k]);
        if top_radius > 1e-12 {
            mesh.indices.extend_from_slice(&[1, rt + k, rt + k1]);
        }
        if base_radius > 1e-12 {
            mesh.indices.extend_from_slice(&[0, rb + k1, rb + k]);
        }
    }
    mesh.compute_vertex_normals();
    mesh
}

/// Torus around the `z` axis at `center`.
pub fn torus(
    center: Point3,
    major_radius: f64,
    minor_radius: f64,
    cfg: &TessellationConfig,
) -> TriMesh {
    let major = cfg.segments_for_circle(major_radius);
    let minor = cfg.segments_for_circle(minor_radius).max(3);
    let mut mesh = TriMesh::with_capacity(major * minor, major * minor * 2);

    for i in 0..major {
        let u = std::f64::consts::TAU * i as f64 / major as f64; // around z
        for j in 0..minor {
            let v = std::f64::consts::TAU * j as f64 / minor as f64; // around tube
            let x = (major_radius + minor_radius * v.cos()) * u.cos();
            let y = (major_radius + minor_radius * v.cos()) * u.sin();
            let z = minor_radius * v.sin();
            mesh.positions
                .push(Point3::new(center.x + x, center.y + y, center.z + z));
        }
    }
    let at = |i: usize, j: usize| -> u32 { ((i % major) * minor + (j % minor)) as u32 };
    for i in 0..major {
        for j in 0..minor {
            let a = at(i, j);
            let b = at(i + 1, j);
            let c = at(i + 1, j + 1);
            let d = at(i, j + 1);
            mesh.indices.extend_from_slice(&[a, b, c]);
            mesh.indices.extend_from_slice(&[a, c, d]);
        }
    }
    mesh.compute_vertex_normals();
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: TessellationConfig = TessellationConfig {
        chord_tolerance_mm: 0.02,
        max_segment_angle_rad: 8.0_f64.to_radians(),
        max_segments_per_circle: 512,
        min_segments_per_circle: 32,
    };

    #[test]
    fn sphere_volume_and_closure() {
        let m = sphere(Point3::origin(), 10.0, &CFG);
        assert!(m.is_closed());
        let v = m.volume().unwrap();
        let expected = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        assert!(
            (v - expected).abs() / expected < 0.02,
            "sphere volume {v} vs {expected}"
        );
    }

    #[test]
    fn cylinder_volume_and_closure() {
        let m = cylinder(Point3::origin(), 5.0, 20.0, &CFG);
        assert!(m.is_closed());
        let v = m.volume().unwrap();
        // The tessellated cylinder is an n-gon prism.
        let n = CFG.segments_for_circle(5.0) as f64;
        let expected = 0.5 * n * 25.0 * (std::f64::consts::TAU / n).sin() * 20.0;
        assert!(
            (v - expected).abs() < 1e-6,
            "cylinder volume {v} vs n-gon prism {expected}"
        );
    }

    #[test]
    fn cone_volume_and_closure() {
        let m = cone(Point3::origin(), 6.0, 3.0, 10.0, &CFG);
        assert!(m.is_closed());
        let v = m.volume().unwrap();
        let expected = std::f64::consts::PI * 10.0 / 3.0 * (36.0 + 18.0 + 9.0);
        assert!((v - expected).abs() / expected < 0.01, "{v} vs {expected}");
    }

    #[test]
    fn torus_volume_and_closure() {
        let m = torus(Point3::origin(), 10.0, 2.0, &CFG);
        assert!(m.is_closed());
        let v = m.volume().unwrap();
        let expected = 2.0 * std::f64::consts::PI.powi(2) * 10.0 * 4.0;
        assert!((v - expected).abs() / expected < 0.03, "{v} vs {expected}");
    }
}
