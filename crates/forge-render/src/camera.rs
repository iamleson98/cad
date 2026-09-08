//! Orbit camera: perspective/orthographic with pan/zoom/fit and CPU
//! ray generation.
//!
//! All camera math is `f64` (NFR-PREC-01); matrices convert to `f32`
//! (`glam`) at uniform-upload time.

use forge_core::{BBox3, Point3, Ray3, Vector3};

/// Interactive viewport camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Orbit center (world).
    pub target: Point3,
    /// Distance from the eye to the target.
    pub distance: f64,
    /// Horizontal orbit angle (radians, around +Z).
    pub yaw: f64,
    /// Vertical orbit angle (radians, clamped to avoid flipping).
    pub pitch: f64,
    /// Vertical field of view (degrees).
    pub fov_deg: f64,
    /// Near plane (mm).
    pub near: f64,
    /// Far plane (mm).
    pub far: f64,
    /// Orthographic projection toggle (FR-RD-04).
    pub orthographic: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Point3::origin(),
            distance: 300.0,
            yaw: -0.6,
            pitch: 0.45,
            fov_deg: 45.0,
            near: 0.1,
            far: 100_000.0,
            orthographic: false,
        }
    }
}

impl Camera {
    /// Camera eye position.
    pub fn eye(&self) -> Point3 {
        let (sy, cy) = (self.yaw.sin(), self.yaw.cos());
        let (sp, cp) = (self.pitch.sin(), self.pitch.cos());
        // Orbit in a Z-up CAD convention.
        let dir = Vector3::new(cp * cy, cp * sy, sp);
        self.target + dir * self.distance
    }

    /// Orbit around the target (radians).
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.yaw += dx;
        self.pitch = (self.pitch + dy).clamp(-1.5533, 1.5533); // ~89deg
    }

    /// Pan the target parallel to the view plane (screen-space delta in
    /// world units per pixel distance).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let scale = if self.orthographic {
            self.ortho_half_height() * 2.0
        } else {
            2.0 * self.distance * (self.fov_deg.to_radians() * 0.5).tan()
        };
        let right = self.right();
        let up = self.up();
        let (dx, dy) = (dx * scale, dy * scale);
        self.target -= right * dx;
        self.target += up * dy;
    }

    /// Zoom (multiplicative factor on the distance / ortho size).
    pub fn zoom(&mut self, factor: f64) {
        self.distance = (self.distance / factor).clamp(self.near * 10.0, self.far * 0.5);
    }

    /// Half height of the orthographic frustum.
    pub fn ortho_half_height(&self) -> f64 {
        self.distance * (self.fov_deg.to_radians() * 0.5).tan()
    }

    /// Frame the given bounding box (FR-RD-04 "fit to view").
    pub fn fit_to(&mut self, bb: &BBox3) {
        if !bb.is_valid() {
            return;
        }
        self.target = bb.center();
        self.distance = (bb.radius() * 2.2).max(10.0);
    }

    /// Forward direction (towards the target).
    pub fn forward(&self) -> Vector3 {
        (self.target - self.eye()).normalize()
    }

    /// Up vector in view space.
    pub fn up(&self) -> Vector3 {
        let forward = self.forward();
        let world_up = Vector3::z();
        let right = self.right();
        // Handle the degenerate case (looking straight along ±Z).
        let up = world_up - forward * world_up.dot(&forward);
        if up.norm() < 1e-9 {
            right.cross(&forward)
        } else {
            up.normalize()
        }
    }

    /// Right vector in view space.
    pub fn right(&self) -> Vector3 {
        let forward = self.forward();
        let world_up = Vector3::z();
        let right = world_up.cross(&forward);
        if right.norm() < 1e-9 {
            // Looking straight up/down: derive right from yaw.
            Vector3::new(-self.yaw.sin(), self.yaw.cos(), 0.0)
        } else {
            right.normalize()
        }
    }

    /// View matrix (right-handed, eye looking at target).
    fn view_matrix(&self) -> glam::Mat4 {
        let eye = self.eye();
        glam::Mat4::look_at_rh(
            glam::Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32),
            glam::Vec3::new(
                self.target.x as f32,
                self.target.y as f32,
                self.target.z as f32,
            ),
            glam::Vec3::Z,
        )
    }

    /// Projection matrix for the given aspect ratio.
    fn proj_matrix(&self, aspect: f64) -> glam::Mat4 {
        if self.orthographic {
            let half_h = self.ortho_half_height() as f32;
            let half_w = half_h * aspect as f32;
            glam::Mat4::orthographic_rh(
                -half_w,
                half_w,
                -half_h,
                half_h,
                self.near as f32,
                self.far as f32,
            )
        } else {
            glam::Mat4::perspective_rh(
                self.fov_deg.to_radians() as f32,
                aspect as f32,
                self.near as f32,
                self.far as f32,
            )
        }
    }

    /// View-projection matrix (f32, GPU representation).
    pub fn view_proj(&self, aspect: f64) -> glam::Mat4 {
        self.proj_matrix(aspect) * self.view_matrix()
    }

    /// Inverse view-projection (f64, CPU picking math).
    pub fn inv_view_proj(&self, aspect: f64) -> Option<nalgebra::Matrix4<f64>> {
        let cols = self.view_proj(aspect).to_cols_array();
        let mut m = nalgebra::Matrix4::<f64>::identity();
        for c in 0..4 {
            for r in 0..4 {
                m[(r, c)] = cols[c * 4 + r] as f64;
            }
        }
        m.try_inverse()
    }

    /// World-space ray through the given NDC point `(x, y)` in `[-1, 1]`
    /// (CPU picking fallback, FR-RD-02).
    pub fn ray_through_ndc(&self, ndc: (f64, f64), aspect: f64) -> Option<Ray3> {
        let inv = self.inv_view_proj(aspect)?;
        let p_near = inv * nalgebra::Point4::new(ndc.0, ndc.1, 0.0, 1.0);
        let p_far = inv * nalgebra::Point4::new(ndc.0, ndc.1, 1.0, 1.0);
        if p_near.w.abs() < 1e-12 || p_far.w.abs() < 1e-12 {
            return None;
        }
        let near = Point3::new(
            p_near.x / p_near.w,
            p_near.y / p_near.w,
            p_near.z / p_near.w,
        );
        let far = Point3::new(p_far.x / p_far.w, p_far.y / p_far.w, p_far.z / p_far.w);
        Ray3::new(near, far - near)
    }

    /// Packed uniform (GPU).
    pub fn uniform(&self, aspect: f64, viewport: (f64, f64)) -> CameraUniform {
        let eye = self.eye();
        let view = self.view_matrix();
        let light = glam::Vec3::new(0.5, -0.6, 0.8).normalize();
        CameraUniform {
            view_proj: self.view_proj(aspect).to_cols_array_2d(),
            view: view.to_cols_array_2d(),
            eye_pos: [
                eye.x as f32,
                eye.y as f32,
                eye.z as f32,
                if self.orthographic { 1.0 } else { 0.0 },
            ],
            light_dir: [light.x, light.y, light.z, 0.0],
            depth_params: [
                self.near as f32,
                self.far as f32,
                1.0 / viewport.0 as f32,
                1.0 / viewport.1 as f32,
            ],
        }
    }
}

/// GPU camera uniform block.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    /// view-projection matrix.
    pub view_proj: [[f32; 4]; 4],
    /// view matrix.
    pub view: [[f32; 4]; 4],
    /// Eye position + ortho flag.
    pub eye_pos: [f32; 4],
    /// Directional light direction.
    pub light_dir: [f32; 4],
    /// near, far, 1/width, 1/height (pixels).
    pub depth_params: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::Point3;

    #[test]
    fn orbit_keeps_distance() {
        let mut cam = Camera::default();
        let d = cam.distance;
        cam.orbit(0.5, -0.3);
        assert!((cam.eye() - cam.target).norm() - d < 1e-9);
    }

    #[test]
    fn ray_through_center_hits_target() {
        let cam = Camera {
            target: Point3::origin(),
            distance: 100.0,
            ..Default::default()
        };
        let ray = cam.ray_through_ndc((0.0, 0.0), 1.0).expect("invertible");
        // The center ray passes near the orbit target.
        let t = (cam.target - ray.origin).dot(&ray.direction);
        let closest = ray.at(t);
        assert!(t > 0.0);
        // f32 GPU matrices round-tripped through f64: micrometer-level
        // accuracy is more than enough for picking.
        assert!(
            (closest - cam.target).norm() < 1e-3,
            "closest {closest:?} vs target {:?}",
            cam.target
        );
    }

    #[test]
    fn fit_to_bbox() {
        let mut cam = Camera::default();
        cam.fit_to(&BBox3 {
            min: Point3::new(-10.0, -10.0, -10.0),
            max: Point3::new(10.0, 10.0, 10.0),
        });
        assert!((cam.target - Point3::origin()).norm() < 1e-9);
        assert!(cam.distance > 10.0);
    }
}
