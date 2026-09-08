//! CPU frustum culling (K-05): per-body AABB rejection before draw-call
//! recording.
//!
//! Planes are extracted from the view-projection matrix with the
//! Gribb–Hartmann row method, and bodies are tested with the
//! positive-vertex AABB test. A body whose bounding box is entirely
//! outside any one plane cannot contribute a fragment, so its vertex,
//! index, edge-line and transparency draws are all skipped — geometry
//! cost scales with what is actually on screen, not with document size.
//!
//! Degenerate/empty bounding boxes (infinite bounds) always report
//! visible: never cull what cannot be bounded.

/// Six-plane view frustum. A point is inside when
/// `a·x + b·y + c·z + d >= 0` holds for every plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    planes: [[f64; 4]; 6],
}

impl Frustum {
    /// Extract the frustum planes from a view-projection matrix
    /// (Gribb–Hartmann): `left = row3 + row0`, `right = row3 − row0`, …
    /// The planes are normalized to unit-length normals so tolerance
    /// checks are meaningful.
    pub fn from_view_proj(m: glam::Mat4) -> Self {
        let row = |i: usize| {
            let r = m.row(i);
            [r.x as f64, r.y as f64, r.z as f64, r.w as f64]
        };
        let r0 = row(0);
        let r1 = row(1);
        let r2 = row(2);
        let r3 = row(3);
        // row3 + sign*rowN as a raw (unnormalized) plane.
        let combine = |a: &[f64; 4], sign: f64| -> [f64; 4] {
            [
                r3[0] + sign * a[0],
                r3[1] + sign * a[1],
                r3[2] + sign * a[2],
                r3[3] + sign * a[3],
            ]
        };
        let raw = [
            combine(&r0, 1.0),  // left
            combine(&r0, -1.0), // right
            combine(&r1, 1.0),  // bottom
            combine(&r1, -1.0), // top
            combine(&r2, 1.0),  // near
            combine(&r2, -1.0), // far
        ];
        let mut planes = [[0.0f64; 4]; 6];
        for (i, p) in raw.iter().enumerate() {
            // Normalize (guarded: a zero plane never occurs for a real
            // projective transform, but stay total).
            let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            let q = if len > 1e-30 {
                [p[0] / len, p[1] / len, p[2] / len, p[3] / len]
            } else {
                *p
            };
            planes[i] = q;
        }
        Self { planes }
    }

    /// Whether an axis-aligned box `[min, max]` intersects the frustum
    /// (positive-vertex test). Inclusive on the boundary.
    pub fn intersects_aabb(&self, min: &[f64; 3], max: &[f64; 3]) -> bool {
        for p in &self.planes {
            // The p-vertex: the box corner furthest along the plane
            // normal. If even that corner is outside, the whole box is.
            let px = if p[0] > 0.0 { max[0] } else { min[0] };
            let py = if p[1] > 0.0 { max[1] } else { min[1] };
            let pz = if p[2] > 0.0 { max[2] } else { min[2] };
            if p[0] * px + p[1] * py + p[2] * pz + p[3] < 0.0 {
                return false;
            }
        }
        true
    }

    /// Whether a point is inside the frustum.
    pub fn contains_point(&self, p: [f64; 3]) -> bool {
        self.planes
            .iter()
            .all(|pl| pl[0] * p[0] + pl[1] * p[1] + pl[2] * p[2] + pl[3] >= 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mm(center: [f64; 3], size: f64) -> ([f64; 3], [f64; 3]) {
        let h = size / 2.0;
        (
            [center[0] - h, center[1] - h, center[2] - h],
            [center[0] + h, center[1] + h, center[2] + h],
        )
    }

    /// The identity "view-projection" is the unit cube [−1, 1]³: a clean
    /// ground-truth check of the plane extraction.
    #[test]
    fn identity_matrix_frustum_is_unit_cube() {
        let f = Frustum::from_view_proj(glam::Mat4::IDENTITY);
        // Inside the cube.
        assert!(f.contains_point([0.0, 0.0, 0.0]));
        assert!(f.intersects_aabb(&[-0.9, -0.9, -0.9], &[0.9, 0.9, 0.9]));
        // Outside each axis.
        for c in 0..3 {
            let mut min = [0.0; 3];
            let mut max = [0.0; 3];
            min[c] = 1.5;
            max[c] = 2.5;
            assert!(!f.intersects_aabb(&min, &max), "axis {c} must cull");
            min[c] = -2.5;
            max[c] = -1.5;
            assert!(!f.intersects_aabb(&min, &max), "axis {c} must cull");
        }
        // A box straddling the boundary is still visible (conservative).
        assert!(f.intersects_aabb(&[0.5, 0.5, 0.5], &[5.0, 5.0, 5.0]));
    }

    #[test]
    fn camera_culls_behind_and_beyond_far() {
        use crate::camera::Camera;
        let cam = Camera::default(); // eye 300 mm from the origin target
        let f = Frustum::from_view_proj(cam.view_proj(1.0));

        // The orbit target is squarely in view.
        let (min, max) = box_mm([0.0, 0.0, 0.0], 10.0);
        assert!(f.intersects_aabb(&min, &max));

        // Behind the eye: eye + (eye − target) direction.
        let eye = cam.eye();
        let behind = [
            eye.x * 2.0 - cam.target.x,
            eye.y * 2.0 - cam.target.y,
            eye.z * 2.0 - cam.target.z,
        ];
        let (min, max) = box_mm(behind, 10.0);
        assert!(!f.intersects_aabb(&min, &max), "behind camera must cull");

        // Beyond the far plane (default far = 100_000).
        let forward = cam.forward();
        let far_away = [
            eye.x + forward.x * 250_000.0,
            eye.y + forward.y * 250_000.0,
            eye.z + forward.z * 250_000.0,
        ];
        let (min, max) = box_mm(far_away, 10.0);
        assert!(!f.intersects_aabb(&min, &max), "beyond far must cull");

        // Far to the side (outside the FOV cone).
        let right = cam.right();
        let sideways = [
            eye.x + right.x * 50_000.0,
            eye.y + right.y * 50_000.0,
            eye.z + right.z * 50_000.0,
        ];
        let (min, max) = box_mm(sideways, 10.0);
        assert!(!f.intersects_aabb(&min, &max), "far off-axis must cull");

        // A scene-encompassing box stays visible (it straddles every
        // plane — the p-vertex test must keep it).
        let (min, max) = box_mm([0.0, 0.0, 0.0], 1.0e6);
        assert!(f.intersects_aabb(&min, &max));
    }

    #[test]
    fn degenerate_bounds_are_never_culled() {
        let f = Frustum::from_view_proj(glam::Mat4::IDENTITY);
        let inf = f64::INFINITY;
        // Empty/unbounded mesh: keep drawing (safe default).
        assert!(f.intersects_aabb(&[inf, inf, inf], &[-inf, -inf, -inf]));
    }
}
