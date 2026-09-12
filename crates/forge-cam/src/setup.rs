//! Machine setup: stock, work coordinate system, safety heights.

use forge_geometry::TriMesh;
use serde::{Deserialize, Serialize};

/// Stock definition (v1: rectangular billet, XY-aligned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stock {
    /// Stock min corner (mm, WCS).
    pub min: [f64; 3],
    /// Stock max corner (mm, WCS).
    pub max: [f64; 3],
}

impl Stock {
    /// Stock from explicit bounds.
    pub fn from_bounds(min: [f64; 3], max: [f64; 3]) -> Self {
        Stock { min, max }
    }

    /// Stock box enclosing a mesh, with XY margin (mm) and extra headroom
    /// above the part top (mm). Floor aligns with the mesh minimum Z.
    pub fn around_mesh(mesh: &TriMesh, xy_margin: f64, top_margin: f64) -> Option<Self> {
        let bb = mesh.bbox();
        if !bb.is_valid() {
            return None;
        }
        Some(Stock {
            min: [bb.min.x - xy_margin, bb.min.y - xy_margin, bb.min.z],
            max: [
                bb.max.x + xy_margin,
                bb.max.y + xy_margin,
                bb.max.z + top_margin,
            ],
        })
    }

    /// XY width.
    pub fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }

    /// XY depth.
    pub fn depth(&self) -> f64 {
        self.max[1] - self.min[1]
    }

    /// Z height.
    pub fn height(&self) -> f64 {
        self.max[2] - self.min[2]
    }

    /// Top of the stock.
    pub fn top(&self) -> f64 {
        self.max[2]
    }

    /// Bottom of the stock.
    pub fn bottom(&self) -> f64 {
        self.min[2]
    }
}

/// Complete machine setup for a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    /// Stock billet.
    pub stock: Stock,
    /// Clearance / retract height above everything (mm, absolute Z).
    pub safe_z: f64,
    /// Default rapid rate (mm/min).
    pub rapid_rate: f64,
}

impl Setup {
    /// Setup from stock: safe Z = stock top + 5 mm.
    pub fn from_stock(stock: Stock) -> Self {
        let safe_z = stock.top() + 5.0;
        Setup {
            stock,
            safe_z,
            rapid_rate: 5000.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Point3, Vector3};
    use forge_geometry::primitives;

    #[test]
    fn stock_around_mesh_with_margins() {
        let mesh = primitives::box_from_center_extents(
            Point3::new(5.0, 5.0, 2.5),
            Vector3::new(10.0, 8.0, 5.0),
        );
        let s = Stock::around_mesh(&mesh, 2.0, 1.0).unwrap();
        assert_eq!(s.min, [-2.0, -1.0, 0.0]);
        assert_eq!(s.max, [12.0, 11.0, 6.0]);
        assert!((s.width() - 14.0).abs() < 1e-9);
        assert!((s.height() - 6.0).abs() < 1e-9);
        let setup = Setup::from_stock(s);
        assert!((setup.safe_z - 11.0).abs() < 1e-9);
    }

    #[test]
    fn empty_mesh_has_no_stock() {
        let mesh = TriMesh::default();
        assert!(Stock::around_mesh(&mesh, 1.0, 1.0).is_none());
    }
}
