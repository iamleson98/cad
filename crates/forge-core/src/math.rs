//! `f64` CAD math built on `nalgebra`.
//!
//! Design rule (NFR-PREC-01): **all** geometric computation happens in
//! double precision. The renderer converts to `f32` at upload time only
//! (`forge-render` uses `glam` for that). This module is the single place
//! where the shared aliases are defined.

use nalgebra::Unit;
use serde::{Deserialize, Serialize};

/// 2D point in the sketch plane (model units, `f64`).
pub type Point2 = nalgebra::Point2<f64>;
/// 3D point in model space (model units, `f64`).
pub type Point3 = nalgebra::Point3<f64>;
/// 2D direction/offset vector (`f64`).
pub type Vector2 = nalgebra::Vector2<f64>;
/// 3D direction/offset vector (`f64`).
pub type Vector3 = nalgebra::Vector3<f64>;
/// Unit-length 3D direction.
pub type UnitVector3 = nalgebra::Unit<nalgebra::Vector3<f64>>;
/// Rigid-body transform (rotation + translation), `f64`.
pub type Transform = nalgebra::Isometry3<f64>;

/// A plane in 3D space, stored as point + unit normal.
///
/// Planes are used as sketch carriers (datum planes or planar faces of
/// existing geometry) and as splitting planes in the CSG kernel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    /// Any point on the plane.
    pub origin: Point3,
    /// Unit normal of the plane.
    pub normal: UnitVector3,
}

impl Plane {
    /// Create a plane, normalizing `normal`. Returns `None` for the zero
    /// vector.
    pub fn new(origin: Point3, normal: Vector3) -> Option<Self> {
        Unit::try_new(normal, 1e-12).map(|n| Self { origin, normal: n })
    }

    /// Signed distance of `p` from the plane (positive = along the normal).
    pub fn signed_distance(&self, p: &Point3) -> f64 {
        (p - self.origin).dot(self.normal.as_ref())
    }

    /// Project `p` onto the plane.
    pub fn project(&self, p: &Point3) -> Point3 {
        let n: Vector3 = *self.normal.as_ref();
        *p - n * self.signed_distance(p)
    }

    /// Build an orthonormal 2D coordinate system on the plane.
    /// Returns `(u, v)` basis vectors such that `u × v = normal`.
    pub fn basis(&self) -> (Vector3, Vector3) {
        // Pick the world axis least aligned with the normal for a stable
        // reference, then Gram-Schmidt.
        let n: Vector3 = *self.normal.as_ref();
        let helper = if n.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let u = (helper - n * helper.dot(&n)).normalize();
        let v = n.cross(&u);
        (u, v)
    }

    /// Convert a plane-local 2D point to world 3D.
    pub fn to_world(&self, local: Point2) -> Point3 {
        let (u, v) = self.basis();
        self.origin + u * local.x + v * local.y
    }

    /// Convert a world 3D point to plane-local 2D coordinates.
    pub fn to_local(&self, world: Point3) -> Point2 {
        let (u, v) = self.basis();
        let d = world - self.origin;
        Point2::new(d.dot(&u), d.dot(&v))
    }
}

impl Default for Plane {
    fn default() -> Self {
        Plane::new(Point3::origin(), Vector3::z()).expect("z-axis is unit length")
    }
}

/// An axis-aligned bounding box in model space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox3 {
    /// Minimum corner.
    pub min: Point3,
    /// Maximum corner.
    pub max: Point3,
}

impl Default for BBox3 {
    fn default() -> Self {
        Self {
            min: Point3::new(f64::MAX, f64::MAX, f64::MAX),
            max: Point3::new(f64::MIN, f64::MIN, f64::MIN),
        }
    }
}

impl BBox3 {
    /// Bounding box of a single point.
    pub fn from_point(p: Point3) -> Self {
        Self { min: p, max: p }
    }

    /// Bounding box containing all points of `pts` (empty if `pts` is empty).
    pub fn from_points(pts: impl IntoIterator<Item = Point3>) -> Self {
        let mut bb = BBox3::default();
        for p in pts {
            bb.extend(&p);
        }
        bb
    }

    /// Extend the box to contain `p`.
    pub fn extend(&mut self, p: &Point3) {
        self.min = self.min.inf(p);
        self.max = self.max.sup(p);
    }

    /// Union with another box.
    pub fn union(&self, other: &BBox3) -> BBox3 {
        BBox3 {
            min: self.min.inf(&other.min),
            max: self.max.sup(&other.max),
        }
    }

    /// `true` if the box contains at least one point.
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }

    /// Center point (midpoint of min/max). Invalid (NaN) on an empty box.
    pub fn center(&self) -> Point3 {
        Point3::from((self.min.coords + self.max.coords) * 0.5)
    }

    /// Extent (width, height, depth).
    pub fn size(&self) -> Vector3 {
        self.max - self.min
    }

    /// Half-diagonal radius around the center; used for camera framing.
    pub fn radius(&self) -> f64 {
        self.size().norm() * 0.5
    }

    /// Conservative ray/box intersection test (slab method). Returns the hit
    /// distance along the ray if any slab is hit.
    pub fn ray_cast(&self, ray: &Ray3) -> Option<f64> {
        if !self.is_valid() {
            return None;
        }
        let mut tmin = f64::MIN;
        let mut tmax = f64::MAX;
        for i in 0..3 {
            let (o, d, mn, mx) = (
                ray.origin[i],
                ray.direction[i],
                self.min[i],
                self.max[i],
            );
            if d.abs() < 1e-12 {
                if o < mn || o > mx {
                    return None;
                }
            } else {
                let mut t1 = (mn - o) / d;
                let mut t2 = (mx - o) / d;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                tmin = tmin.max(t1);
                tmax = tmax.min(t2);
                if tmin > tmax {
                    return None;
                }
            }
        }
        Some(if tmin >= 0.0 { tmin } else { tmax.max(0.0) })
    }
}

/// A ray in model space, used for CPU picking (BVH raycast) and queries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray3 {
    /// Ray origin.
    pub origin: Point3,
    /// Unit ray direction.
    pub direction: UnitVector3,
}

impl Ray3 {
    /// Create a ray; returns `None` if `direction` cannot be normalized.
    pub fn new(origin: Point3, direction: Vector3) -> Option<Self> {
        Unit::try_new(direction, 1e-12).map(|d| Self { origin, direction: d })
    }

    /// Point at distance `t` along the ray.
    pub fn at(&self, t: f64) -> Point3 {
        self.origin + self.direction.as_ref() * t
    }
}

/// Intersection of a ray with a triangle (Möller–Trumbore), `f64` version.
/// Returns the distance along the ray and the barycentric `(u, v)` of the
/// hit point on the triangle.
pub fn ray_triangle(
    ray: &Ray3,
    a: &Point3,
    b: &Point3,
    c: &Point3,
) -> Option<(f64, f64, f64)> {
    let e1 = *b - *a;
    let e2 = *c - *a;
    let dir: Vector3 = *ray.direction.as_ref();
    let pvec = dir.cross(&e2);
    let det = e1.dot(&pvec);
    if det.abs() < 1e-12 {
        return None; // ray parallel to triangle plane
    }
    let inv_det = 1.0 / det;
    let tvec = ray.origin - *a;
    let u = tvec.dot(&pvec) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(&e1);
    let v = dir.dot(&qvec) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(&qvec) * inv_det;
    if t < 1e-9 {
        return None;
    }
    Some((t, u, v))
}

/// Linear interpolation helper.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn plane_roundtrip_local_world() {
        let plane =
            Plane::new(Point3::new(1.0, 2.0, 3.0), Vector3::new(1.0, 1.0, 0.0))
                .expect("non-degenerate");
        let p = Point3::new(4.0, 5.0, 6.0);
        // Roundtrip is exact for points on the plane; general points must be
        // projected first.
        let q = plane.project(&p);
        let local = plane.to_local(q);
        let back = plane.to_world(local);
        assert_abs_diff_eq!(back.x, q.x, epsilon = 1e-12);
        assert_abs_diff_eq!(back.y, q.y, epsilon = 1e-12);
        assert_abs_diff_eq!(back.z, q.z, epsilon = 1e-12);
        assert_abs_diff_eq!(
            plane.signed_distance(&plane.project(&p)),
            0.0,
            epsilon = 1e-12
        );
    }

    #[test]
    fn plane_basis_is_right_handed() {
        let plane = Plane::default();
        let (u, v) = plane.basis();
        let n: Vector3 = *plane.normal.as_ref();
        assert_abs_diff_eq!((u.cross(&v) - n).norm(), 0.0, epsilon = 1e-12);
    }

    #[test]
    fn bbox_ray_cast_hits() {
        let bb = BBox3 {
            min: Point3::new(-1.0, -1.0, -1.0),
            max: Point3::new(1.0, 1.0, 1.0),
        };
        let ray = Ray3::new(Point3::new(-5.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("unit");
        assert_abs_diff_eq!(bb.ray_cast(&ray).unwrap(), 4.0, epsilon = 1e-9);
        let miss = Ray3::new(Point3::new(-5.0, 5.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("unit");
        assert!(bb.ray_cast(&miss).is_none());
    }

    #[test]
    fn ray_triangle_basic() {
        let a = Point3::origin();
        let b = Point3::new(1.0, 0.0, 0.0);
        let c = Point3::new(0.0, 1.0, 0.0);
        let ray = Ray3::new(Point3::new(0.25, 0.25, 5.0), Vector3::new(0.0, 0.0, -1.0))
            .expect("unit");
        let (t, _u, _v) = ray_triangle(&ray, &a, &b, &c).expect("must hit");
        assert_abs_diff_eq!(t, 5.0, epsilon = 1e-12);
        let miss = Ray3::new(Point3::new(2.0, 2.0, 5.0), Vector3::new(0.0, 0.0, -1.0))
            .expect("unit");
        assert!(ray_triangle(&miss, &a, &b, &c).is_none());
    }
}
