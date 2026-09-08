//! # forge-geometry
//!
//! The computational geometry engine of ForgeCAD.
//!
//! Design decisions (see the SRS / architecture document for context):
//!
//! - **Mesh-first kernel.** v0.1 represents solids as closed, welded,
//!   consistently-oriented triangle meshes. This is the representation the
//!   WebGPU renderer consumes directly and is enough for extrude / revolve
//!   / loft / sweep / boolean modeling at interactive speeds. The B-Rep
//!   kernel (the roadmap's Phase 4) will sit *behind* the same public API.
//! - **`f64` everywhere** (NFR-PREC-01); the renderer narrows to `f32` at
//!   buffer upload time.
//! - **`rayon`** parallelism for normal computation and (in
//!   `forge-model`) parallel feature evaluation.
//!
//! Module map:
//! - [`mesh`] – [`TriMesh`] data structure + derived quantities
//! - [`triangulate`] – 2D ear clipping with hole bridging
//! - [`primitives`] – box / sphere / cylinder / cone / torus
//! - [`sweep`] – extrude, revolve, loft, path sweep
//! - [`boolean`] + [`bsp`] – CSG union / difference / intersection
//! - [`bvh`] – CPU raycasting (picking fallback)
//! - [`detail`] – fillet / chamfer / shell / offset (Phase-4 stubs)

pub mod boolean;
pub mod bsp;
pub mod bvh;
pub mod detail;
pub mod error;
pub mod mesh;
pub mod primitives;
pub mod sweep;
pub mod triangulate;

pub use boolean::{boolean, CsgOp};
pub use bvh::{Bvh, RayHit};
pub use error::GeometryError;
pub use mesh::{TriMesh, WELD_EPS};
pub use sweep::{
    extrude, loft, revolve, sweep_along_path, ExtrudeDirection, ExtrudeParams, Profile2D,
    RevolveParams,
};
pub use triangulate::{triangulate_with_holes, Triangulation};

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Plane, Point2, Point3, TessellationConfig, Vector3};

    /// End-to-end kernel pipeline: profile -> extrude -> boolean cut.
    #[test]
    fn plate_with_hole_pipeline() {
        let cfg = TessellationConfig::default();
        // 60 x 40 rectangle.
        let outer = vec![
            Point2::new(-30.0, -20.0),
            Point2::new(30.0, -20.0),
            Point2::new(30.0, 20.0),
            Point2::new(-30.0, 20.0),
        ];
        let hole = triangulate::circle_points(Point2::origin(), 8.0, 64);
        let profile = Profile2D::new(outer, vec![hole]).expect("valid profile");

        let plate = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: 10.0,
                direction: ExtrudeDirection::Symmetric,
                draft_angle: 0.0,
            },
        )
        .expect("extrude");

        assert!(plate.is_closed());
        // The hole is tessellated to a 64-gon.
        let hole_area = 0.5 * 64.0 * 64.0 * (std::f64::consts::TAU / 64.0).sin();
        let expected = (2400.0 - hole_area) * 10.0;
        let got = plate.volume().expect("closed");
        assert!(
            (got - expected).abs() / expected < 1e-9,
            "{got} vs {expected}"
        );

        // Drill a second hole with a boolean cylinder cut.
        let drill = primitives::cylinder(Point3::new(15.0, 0.0, -20.0), 5.0, 40.0, &cfg);
        let result = boolean(&plate, &drill, CsgOp::Difference).expect("cut");
        let expected2 = expected - std::f64::consts::PI * 25.0 * 10.0;
        let got2 = result.volume_signed();
        assert!(
            (got2 - expected2).abs() / expected2 < 0.01,
            "{got2} vs {expected2}"
        );
    }

    #[test]
    fn transformed_mesh_volume_is_preserved() {
        let m =
            primitives::box_from_center_extents(Point3::origin(), Vector3::new(10.0, 10.0, 10.0));
        let iso = forge_core::Transform::from_parts(
            nalgebra::Translation3::new(100.0, -50.0, 7.0),
            nalgebra::UnitQuaternion::from_euler_angles(30.0_f64.to_radians(), 45.0, 60.0),
        );
        let t = m.transformed(&iso);
        assert!((t.volume().unwrap() - 1000.0).abs() < 1e-6);
    }
}
