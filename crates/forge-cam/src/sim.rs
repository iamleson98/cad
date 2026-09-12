//! Stock & material-removal simulation (C-05): a 2.5D heightfield model
//! of the remaining stock, updated per cutting move.
//!
//! The simulation answers the questions that make CAM trustworthy:
//! - *how much material is actually removed* (volume + percent),
//! - *does any move gouge the part* (sim height below the part surface),
//! - *what does the remaining stock look like* (mesh export for the
//!   viewport ghost).
//!
//! Model: the stock is a height grid initialized to the stock top. Every
//! cutting move stamps the tool footprint (flat disc or ball sphere)
//! along its path, lowering covered cells. Rapids cut nothing. Drill
//! cycles stamp to final depth. The classic 2.5D approximation —
//! undercuts are invisible, matching what a 3-axis machine can reach.

use crate::field::HeightField;
use crate::path::{Move, Toolpath};
use crate::setup::Stock;
use crate::tool::{Tool, ToolKind};
use forge_geometry::TriMesh;

/// Simulation outcome.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimReport {
    /// Removed volume (mm³).
    pub removed_volume: f64,
    /// Total stock volume (mm³, within the grid).
    pub stock_volume: f64,
    /// Removed fraction (0..1).
    pub removed_pct: f64,
    /// Cells where the simulation cut below the part surface (gouges).
    pub gouges: usize,
}

/// Remaining-stock simulator.
#[derive(Debug, Clone)]
pub struct MaterialSim {
    /// Remaining material top height per cell (starts at stock top).
    pub field: HeightField,
    /// Stock top (height the field started at).
    stock_top: f64,
    /// Part surface heights for gouge detection (None = no part).
    part: Option<HeightField>,
}

impl MaterialSim {
    /// New simulation over a stock at `res` grid resolution.
    pub fn new(stock: &Stock, res: f64) -> Self {
        let nx = ((stock.width() / res).ceil() as usize).clamp(2, 1000);
        let ny = ((stock.depth() / res).ceil() as usize).clamp(2, 1000);
        let field = HeightField {
            heights: vec![Some(stock.top()); nx * ny],
            nx,
            ny,
            cell: res,
            ox: stock.min[0] + res * 0.5,
            oy: stock.min[1] + res * 0.5,
            floor: stock.bottom(),
        };
        MaterialSim {
            field,
            stock_top: stock.top(),
            part: None,
        }
    }

    /// Attach the part surface for gouge detection (the raw rasterized
    /// field, no dilation, no leave).
    pub fn with_part(mut self, part: HeightField) -> Self {
        self.part = Some(part);
        self
    }

    /// Apply a whole toolpath with its tool.
    pub fn apply(&mut self, path: &Toolpath, tool: &Tool) {
        let mut prev: Option<[f64; 3]> = None;
        for m in &path.moves {
            match *m {
                // Rapids retract and reposition: they are never a cut
                // link, so the next plunge must not sweep from the last
                // cut position (which could cross the part).
                Move::Rapid { .. } => prev = None,
                Move::Feed { p } | Move::Plunge { p } => {
                    if let Some(a) = prev {
                        self.sweep(a, p, tool);
                    } else {
                        self.stamp(p, tool);
                    }
                    prev = Some(p);
                }
                Move::Drill { p, depth, .. } => {
                    self.stamp_disc(p[0], p[1], depth, tool.radius());
                    prev = Some([p[0], p[1], depth]);
                }
            }
        }
    }

    /// Sweep the tool from `a` to `b` (a cutting move): stamp the footprint
    /// along the segment, interpolating Z (handles ramps and plunges).
    fn sweep(&mut self, a: [f64; 3], b: [f64; 3], tool: &Tool) {
        let res = self.field.cell;
        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
        let steps = ((d / res).ceil() as usize).max(1);
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let x = a[0] + (b[0] - a[0]) * t;
            let y = a[1] + (b[1] - a[1]) * t;
            let z = a[2] + (b[2] - a[2]) * t;
            self.stamp([x, y, z], tool);
        }
    }

    /// Stamp the tool footprint at full depth at `p`.
    fn stamp(&mut self, p: [f64; 3], tool: &Tool) {
        match tool.kind {
            ToolKind::Ball => self.stamp_ball(p[0], p[1], p[2], tool.radius()),
            _ => self.stamp_disc(p[0], p[1], p[2], tool.radius()),
        }
    }

    /// Flat-disc stamp: cells within `r` drop to `z`.
    fn stamp_disc(&mut self, x: f64, y: f64, z: f64, r: f64) {
        let res = self.field.cell;
        let r_cells = (r / res).ceil() as i64;
        let (ci, cj) = self.field.world_to_cell(x, y);
        let r2 = r * r;
        for dj in -r_cells..=r_cells {
            for di in -r_cells..=r_cells {
                let dx = di as f64 * res;
                let dy = dj as f64 * res;
                if dx * dx + dy * dy > r2 + 1e-12 {
                    continue;
                }
                let i = ci as i64 + di;
                let j = cj as i64 + dj;
                if i < 0 || j < 0 || i >= self.field.nx as i64 || j >= self.field.ny as i64 {
                    continue;
                }
                let k = j as usize * self.field.nx + i as usize;
                if let Some(h) = self.field.heights[k] {
                    self.field.heights[k] = Some(h.min(z));
                }
            }
        }
    }

    /// Ball stamp: cell at distance `d` drops to `z − sqrt(r² − d²)`.
    fn stamp_ball(&mut self, x: f64, y: f64, z: f64, r: f64) {
        let res = self.field.cell;
        let r_cells = (r / res).ceil() as i64;
        let (ci, cj) = self.field.world_to_cell(x, y);
        let r2 = r * r;
        for dj in -r_cells..=r_cells {
            for di in -r_cells..=r_cells {
                let dx = di as f64 * res;
                let dy = dj as f64 * res;
                let d2 = dx * dx + dy * dy;
                if d2 > r2 + 1e-12 {
                    continue;
                }
                let i = ci as i64 + di;
                let j = cj as i64 + dj;
                if i < 0 || j < 0 || i >= self.field.nx as i64 || j >= self.field.ny as i64 {
                    continue;
                }
                let cut = z - (r2 - d2).sqrt();
                let k = j as usize * self.field.nx + i as usize;
                if let Some(h) = self.field.heights[k] {
                    self.field.heights[k] = Some(h.min(cut));
                }
            }
        }
    }

    /// Aggregate report: removed volume + gouge cells.
    pub fn report(&self) -> SimReport {
        let cell_area = self.field.cell * self.field.cell;
        let mut removed = 0.0;
        for h in self.field.heights.iter().flatten() {
            removed += (self.stock_top - h).max(0.0) * cell_area;
        }
        let stock_volume = self.field.nx as f64
            * self.field.ny as f64
            * cell_area
            * (self.stock_top - self.field.floor).max(0.0);
        let mut gouges = 0;
        if let Some(part) = &self.part {
            // Tolerance: sub-cell discretization of both grids can
            // misreport the tool's true footprint by up to one cell.
            let tol = self.field.cell.max(part.cell);
            for j in 0..self.field.ny {
                for i in 0..self.field.nx {
                    let Some(h) = self.field.heights[j * self.field.nx + i] else {
                        continue;
                    };
                    let (pi, pj) = part.world_to_cell(self.field.x(i), self.field.y(j));
                    if let Some(pz) = part.heights[pj * part.nx + pi] {
                        if h < pz - tol {
                            gouges += 1;
                        }
                    }
                }
            }
        }
        SimReport {
            removed_volume: removed,
            stock_volume,
            removed_pct: if stock_volume > 1e-9 {
                removed / stock_volume
            } else {
                0.0
            },
            gouges,
        }
    }

    /// Mesh of the remaining stock top surface (stepped quads, one per
    /// cell) for viewport rendering. Cells at the floor emit nothing (all
    /// removed); the mesh is open (top surface only) and rendered
    /// translucent.
    pub fn stock_mesh(&self) -> TriMesh {
        let res = self.field.cell;
        let mut mesh = TriMesh::default();
        for j in 0..self.field.ny {
            for i in 0..self.field.nx {
                let h = match self.field.heights[j * self.field.nx + i] {
                    Some(h) => h,
                    None => continue,
                };
                if (h - self.field.floor).abs() < 1e-9 {
                    continue; // fully removed
                }
                let x = self.field.ox + (i as f64 - 0.5) * res;
                let y = self.field.oy + (j as f64 - 0.5) * res;
                let x1 = x + res;
                let y1 = y + res;
                // Two triangles per cell, CCW when viewed from +Z.
                mesh.push_triangle(
                    forge_core::Point3::new(x, y, h),
                    forge_core::Point3::new(x1, y, h),
                    forge_core::Point3::new(x1, y1, h),
                );
                mesh.push_triangle(
                    forge_core::Point3::new(x, y, h),
                    forge_core::Point3::new(x1, y1, h),
                    forge_core::Point3::new(x, y1, h),
                );
            }
        }
        mesh.compute_vertex_normals();
        mesh
    }

    /// Number of simulation cells.
    pub fn cell_count(&self) -> usize {
        self.field.nx * self.field.ny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Feeds;
    use crate::setup::Setup;
    use crate::strategy::{self, Hole, RoughParams};
    use forge_core::{Point3, Vector3};
    use forge_geometry::primitives;

    fn boxmesh(w: f64, d: f64, h: f64) -> TriMesh {
        primitives::box_from_center_extents(Point3::new(0.0, 0.0, h / 2.0), Vector3::new(w, d, h))
    }

    #[test]
    fn untouched_stock_reports_zero_removal() {
        let stock = Stock::from_bounds([0.0, 0.0, 0.0], [10.0, 10.0, 5.0]);
        let sim = MaterialSim::new(&stock, 0.5);
        let r = sim.report();
        assert!((r.removed_volume - 0.0).abs() < 1e-9);
        assert_eq!(r.gouges, 0);
    }

    #[test]
    fn roughing_removes_stock_without_gouges() {
        // 6 mm boss (h=4) in a 12 mm stock; rough with the 6 mm flat mill.
        let mesh = boxmesh(6.0, 6.0, 4.0);
        let stock = Stock::around_mesh(&mesh, 3.0, 1.0).unwrap();
        let setup = Setup::from_stock(stock.clone());
        let tool = Tool::presets()[0].clone();
        let res = strategy::rough(
            &mesh,
            &setup,
            &tool,
            Feeds::default(),
            &RoughParams {
                stepdown: 2.0,
                stepover: 2.5,
                leave: 0.2,
                floor_z: None,
            },
        );
        // Raw part field (no dilation) for gouge detection.
        let part = HeightField::rasterize(
            &mesh,
            stock.min[0] + 0.25,
            stock.min[1] + 0.25,
            ((stock.width() / 0.25).ceil() as usize).clamp(2, 1000),
            ((stock.depth() / 0.25).ceil() as usize).clamp(2, 1000),
            0.25,
            stock.bottom(),
        );
        let mut sim = MaterialSim::new(&stock, 0.5).with_part(part);
        sim.apply(&res.path, &tool);
        let r = sim.report();
        assert!(r.removed_volume > 1.0, "removed {r:?}");
        assert!(r.removed_pct > 0.0 && r.removed_pct < 1.0, "{r:?}");
        // The roughing path is gouge-free by construction (leave 0.2).
        assert_eq!(r.gouges, 0, "gouges in a leave-0.2 roughing pass: {r:?}");
    }

    #[test]
    fn manual_stamp_below_part_counts_as_gouge() {
        let mesh = boxmesh(6.0, 6.0, 4.0);
        let stock = Stock::around_mesh(&mesh, 3.0, 1.0).unwrap();
        let part = HeightField::rasterize(
            &mesh,
            stock.min[0] + 0.25,
            stock.min[1] + 0.25,
            20,
            20,
            stock.width() / 20.0,
            stock.bottom(),
        );
        let mut sim = MaterialSim::new(&stock, 0.5).with_part(part);
        // Stamp directly over the boss top (z=4) below its surface.
        sim.stamp_disc(0.0, 0.0, 2.0, 3.0);
        let r = sim.report();
        assert!(r.gouges > 0, "expected gouges: {r:?}");
    }

    #[test]
    fn drill_stamps_hole_column() {
        let stock = Stock::from_bounds([0.0, 0.0, 0.0], [10.0, 10.0, 6.0]);
        let mut sim = MaterialSim::new(&stock, 0.5);
        let mut path = Toolpath::new("drill", 5, 2.5, Feeds::default());
        path.drill([5.0, 5.0, 0.0], 6.0, 1.0, None);
        let tool = Tool::presets()[4].clone(); // D5
        sim.apply(&path, &tool);
        let r = sim.report();
        let want_disc = std::f64::consts::PI * 2.5_f64.powi(2) * 5.0; // π r² h
        assert!(
            (r.removed_volume - want_disc).abs() / want_disc < 0.35,
            "removed {r:?} vs disc {want_disc}"
        );
        // The cell at the hole center is down to depth 1.
        let (i, j) = sim.field.world_to_cell(5.0, 5.0);
        assert!(
            (sim.field.z(i, j) - 1.0).abs() < 1e-9,
            "center z {}",
            sim.field.z(i, j)
        );
    }

    #[test]
    fn ball_stamp_cuts_spherical_profile() {
        let stock = Stock::from_bounds([0.0, 0.0, 0.0], [10.0, 10.0, 6.0]);
        let mut sim = MaterialSim::new(&stock, 0.25);
        // Ball r=2 centered at (5,5,5): directly under the axis cuts to 3.
        sim.stamp_ball(5.0, 5.0, 5.0, 2.0);
        let (i, j) = sim.field.world_to_cell(5.0, 5.0);
        assert!(
            (sim.field.z(i, j) - 3.0).abs() < 0.3,
            "axis z {}",
            sim.field.z(i, j)
        );
        // 1.5 mm off-axis: cut = 5 - sqrt(4-2.25) ≈ 2.68.
        let (i, j) = sim.field.world_to_cell(6.5, 5.0);
        let z = sim.field.z(i, j);
        assert!((z - (5.0 - (4.0 - 2.25_f64).sqrt())).abs() < 0.3, "z {z}");
    }

    #[test]
    fn rapids_remove_nothing() {
        let stock = Stock::from_bounds([0.0, 0.0, 0.0], [10.0, 10.0, 6.0]);
        let mut sim = MaterialSim::new(&stock, 0.5);
        let mut path = Toolpath::new("rapid only", 1, 3.0, Feeds::default());
        path.rapid([1.0, 1.0, 6.0]);
        path.rapid([5.0, 5.0, 6.0]);
        let tool = Tool::presets()[0].clone();
        sim.apply(&path, &tool);
        assert!((sim.report().removed_volume - 0.0).abs() < 1e-9);
    }

    #[test]
    fn stock_mesh_follows_the_field() {
        let stock = Stock::from_bounds([0.0, 0.0, 0.0], [10.0, 10.0, 6.0]);
        let mut sim = MaterialSim::new(&stock, 0.5);
        sim.stamp_disc(5.0, 5.0, 2.0, 2.0);
        let mesh = sim.stock_mesh();
        assert!(mesh.tri_count() > 100);
        // Volume of the stepped mesh ≈ stock − removed.
        let bbox = mesh.bbox();
        assert!(bbox.is_valid());
        assert!(bbox.max.z <= 6.0 + 1e-9 && bbox.min.z >= 2.0 - 1e-9);
    }

    #[test]
    fn hole_drilling_through_strategy_pipeline() {
        // End-to-end: hole feature → drill strategy → sim.
        let stock = Stock::from_bounds([-15.0, -15.0, 0.0], [15.0, 15.0, 10.0]);
        let setup = Setup::from_stock(stock.clone());
        let tool = Tool::presets()[4].clone();
        let holes = [Hole {
            center: [0.0, 0.0],
            top: 10.0,
            bottom: 0.0,
            diameter: 5.2,
        }];
        let res = strategy::drill(
            &setup,
            &tool,
            Feeds::default(),
            &holes,
            &strategy::DrillParams {
                peck: Some(2.0),
                dwell: 0.0,
            },
        );
        let mut sim = MaterialSim::new(&stock, 0.4);
        sim.apply(&res.path, &tool);
        let r = sim.report();
        assert!(r.removed_volume > 30.0, "drilled volume {r:?}");
    }
}
