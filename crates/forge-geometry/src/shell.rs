//! Shell / hollow (F-05, mesh-approximate).
//!
//! Approach: plane-offset the mesh inward, then boolean-cut the cavity:
//!
//! 1. **inner mesh**: every vertex moves to the least-squares
//!    intersection of its adjacent face planes, each offset inward by
//!    `t` — this gives *uniform wall thickness* on polyhedral geometry
//!    (a box corner moves exactly `t·(1,1,1)`, unlike naive
//!    normal-offsetting which would give `t·√3`).
//! 2. **openings**: for each open face (matched by its plane snapshot),
//!    a prism is extruded through the face region from inside the inner
//!    solid to outside the outer surface, and unioned into the cavity.
//! 3. **shell** = `Difference(outer, cavity)`.
//!
//! Limits (documented): sharp concave interior corners can self-intersect
//! the inner offset (the classic polygon-offset failure); the evaluator
//! reports the resulting boolean error rather than crashing. Undercuts
//! are hollowed to the first surface from outside (3-axis semantics).

use crate::boolean::{boolean, CsgOp};
use crate::error::{GeometryError, Result};
use crate::mesh::TriMesh;
use forge_core::{Point3, Vector3};

/// Plane snapshot identifying an open face across re-tessellations.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FacePlane {
    /// A point on the face plane.
    pub point: Point3,
    /// Face normal (outward).
    pub normal: Vector3,
}

/// Hollow the solid to wall `thickness`, opening the faces matching
/// `open` (plane snapshots; empty = fully closed hollow).
pub fn shell(mesh: &TriMesh, thickness: f64, open: &[FacePlane]) -> Result<TriMesh> {
    if thickness <= 0.0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "shell thickness must be positive".into(),
        )));
    }
    if mesh.tri_count() == 0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "shell: body is empty".into(),
        )));
    }
    // 1. Inner offset mesh (uniform-thickness plane offset).
    let inner = offset_inward(mesh, thickness)?;
    if inner.tri_count() == 0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "shell: inner offset collapsed (body thinner than the walls?)".into(),
        )));
    }

    // 2. Openings: push the inner face's vertices outward along the face
    //    normal past the outer surface — the cavity grows *through* the
    //    opening, so the Difference carves it cleanly. Vertices shared
    //    with adjacent faces (the opening rim) stretch those faces along,
    //    which is exactly the wall-continuation semantics.
    let mut cavity = inner;
    for plane in open {
        let inner_plane = FacePlane {
            point: plane.point - plane.normal * thickness,
            normal: plane.normal,
        };
        open_face_push(&mut cavity, &inner_plane, thickness);
    }

    // 3. shell = outer − cavity.
    let shell = boolean(mesh, &cavity, CsgOp::Difference)?;
    Ok(shell)
}

/// Push the vertices of the face matching `plane` outward along its
/// normal by `t` + margin (through the outer surface). No-op when the
/// face is not found (re-eval moved it) — the shell stays closed there.
fn open_face_push(mesh: &mut TriMesh, plane: &FacePlane, t: f64) {
    let cos_tol = (2.0_f64).to_radians().cos();
    let mut vertices: std::collections::BTreeSet<u32> = Default::default();
    for tri in 0..mesh.tri_count() {
        let Some(n) = mesh.triangle_normal(tri) else {
            continue;
        };
        let p = mesh.positions[mesh.triangle_idx(tri)[0] as usize];
        let on_plane = (plane.normal.dot(&(p - plane.point))).abs() < 1e-4;
        if on_plane && n.dot(&plane.normal) > cos_tol {
            for v in mesh.triangle_idx(tri) {
                vertices.insert(v);
            }
        }
    }
    let push = plane.normal * (t + 2e-3);
    for v in vertices {
        mesh.positions[v as usize] += push;
    }
    mesh.normals = None;
}

/// Inward plane-offset: per vertex, intersect the adjacent face planes
/// (each moved inward by `t`) in the least-squares sense.
fn offset_inward(mesh: &TriMesh, t: f64) -> Result<TriMesh> {
    // Vertex → incident plane count (for the degenerate-vertex guard).
    let mut counts = vec![0usize; mesh.positions.len()];
    for tri in 0..mesh.tri_count() {
        let [a, b, c] = mesh.triangle_idx(tri);
        for v in [a, b, c] {
            counts[v as usize] += 1;
        }
    }
    let mut out = mesh.clone();
    // Normal equations per vertex: (Σ n nᵀ) x = Σ n·d with each plane
    // offset inward by t (n·x = n·p − t).
    let mut ata: Vec<[[f64; 3]; 3]> = vec![[[0.0; 3]; 3]; mesh.positions.len()];
    let mut atb: Vec<[f64; 3]> = vec![[0.0; 3]; mesh.positions.len()];
    let mut vn: Vec<Vector3> = vec![Vector3::zeros(); mesh.positions.len()];
    for tri in 0..mesh.tri_count() {
        let Some(n) = mesh.triangle_normal(tri) else {
            continue;
        };
        let [a, b, c] = mesh.triangle_idx(tri);
        let p = mesh.positions[a as usize];
        let d = n.dot(&(p - Point3::origin())) - t;
        for v in [a, b, c] {
            let vi = v as usize;
            let nv = [n.x, n.y, n.z];
            let bd = [n.x * d, n.y * d, n.z * d];
            for r in 0..3 {
                for c2 in 0..3 {
                    ata[vi][r][c2] += nv[r] * nv[c2];
                }
                atb[vi][r] += bd[r];
            }
            vn[vi] += n;
        }
    }
    for (i, p) in out.positions.iter_mut().enumerate() {
        if counts[i] == 0 {
            continue;
        }
        // Solve the 3×3 system; fall back to the vertex-normal offset.
        let solved = solve3(&ata[i], &atb[i]);
        match solved {
            Some(x) if x.iter().all(|v| v.is_finite()) => {
                *p = Point3::new(x[0], x[1], x[2]);
            }
            _ => {
                let n = vn[i].normalize();
                *p -= n * t;
            }
        }
    }
    out.normals = None;
    Ok(out)
}

/// Solve a symmetric 3×3 system (Gaussian elimination with partial
/// pivoting). Returns `None` on a (near-)singular system.
fn solve3(a: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<[f64; 3]> {
    let mut m = [a[0], a[1], a[2]];
    let mut rhs = *b;
    for col in 0..3 {
        // Pivot.
        let mut best = col;
        for r in col + 1..3 {
            if m[r][col].abs() > m[best][col].abs() {
                best = r;
            }
        }
        if m[best][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, best);
        rhs.swap(col, best);
        // Eliminate below.
        for r in col + 1..3 {
            let f = m[r][col] / m[col][col];
            let (head, tail) = m.split_at_mut(r);
            for (c2, slot) in tail[0].iter_mut().enumerate().skip(col) {
                *slot -= f * head[col][c2];
            }
            rhs[r] -= f * rhs[col];
        }
    }
    // Back-substitute.
    let mut x = [0.0; 3];
    for r in (0..3).rev() {
        let mut s = rhs[r];
        for c2 in r + 1..3 {
            s -= m[r][c2] * x[c2];
        }
        x[r] = s / m[r][r];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn boxmesh(w: f64, d: f64, h: f64) -> TriMesh {
        primitives::box_from_center_extents(Point3::new(0.0, 0.0, h / 2.0), Vector3::new(w, d, h))
    }

    const TOP: FacePlane = FacePlane {
        point: Point3::new(0.0, 0.0, 10.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };

    #[test]
    fn shell_box_closed_exact_volume() {
        // Closed hollow: outer 10³ = 1000, inner (10−2t)³ = 6³ = 216.
        let m = boxmesh(10.0, 10.0, 10.0);
        let out = shell(&m, 2.0, &[]).unwrap();
        let v = out.volume_signed();
        let want = 1000.0 - 6.0_f64.powi(3);
        assert!((v - want).abs() < 1.0, "volume {v} vs {want}");
    }

    #[test]
    fn shell_box_open_top_exact_volume() {
        // Open top: walls = closed hollow − top wall (6×6×2 = 72).
        let m = boxmesh(10.0, 10.0, 10.0);
        let out = shell(&m, 2.0, &[TOP]).unwrap();
        let v = out.volume_signed();
        let want = 1000.0 - 6.0_f64.powi(3) - 6.0 * 6.0 * 2.0;
        assert!((v - want).abs() < 2.0, "volume {v} vs {want}");
    }

    #[test]
    fn shell_rejects_bad_input() {
        let m = boxmesh(10.0, 10.0, 10.0);
        assert!(shell(&m, 0.0, &[]).is_err());
        assert!(shell(&m, -1.0, &[]).is_err());
        assert!(shell(&TriMesh::default(), 1.0, &[]).is_err());
        // Unknown face plane → graceful no-op (the shell stays closed
        // there; re-eval robustness beats a hard error).
        let ghost = FacePlane {
            point: Point3::new(100.0, 0.0, 0.0),
            normal: Vector3::x(),
        };
        let closed = shell(&m, 1.0, &[]).unwrap().volume_signed();
        let ghosted = shell(&m, 1.0, &[ghost]).unwrap().volume_signed();
        assert!(
            (closed - ghosted).abs() < 1e-9,
            "ghost face changed the result: {closed} vs {ghosted}"
        );
    }

    #[test]
    fn shell_too_thick_reports_cleanly() {
        // Thickness ≥ half the smallest dimension → the offset collapses
        // or inverts: either a clean error or an inverted-volume result.
        let m = boxmesh(10.0, 10.0, 10.0);
        let res = shell(&m, 6.0, &[]);
        match res {
            Err(e) => assert!(!format!("{e}").is_empty()),
            Ok(mesh) => {
                // Inverted/collapsed: volume far from sane.
                assert!(mesh.volume_signed().abs() < 500.0);
            }
        }
    }
}
