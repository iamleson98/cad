//! Heightfield kernel: the bridge between triangle meshes and 2.5D toolpaths.
//!
//! A [`HeightField`] is a regular XY grid storing, per cell, the highest
//! part-surface Z (or "no part" = the depth-limit floor). It is the standard
//! input representation for 3-axis CAM:
//!
//! - **rasterization**: each triangle is projected to XY and its plane Z is
//!   splatted into covered cells (robust, no ray casting, parallel over
//!   batches emitting sparse touched-cell lists),
//! - **dilation**: `max`-filter over the tool footprint (flat disc, or the
//!   spherical lift `h + sqrt(r² − d²)` for ball tools) yields the minimal
//!   legal tool-center Z per cell — the *cutter-location field* (CL field),
//! - **contouring**: marching-squares over the CL field at a finish Z gives
//!   closed waterline toolpaths.
//!
//! Undercuts (overhangs) are intentionally ignored: classic 2.5D machining
//! cannot reach them, so the field records the *first* surface from the top,
//! exactly like a physical Z probe would.

use forge_core::Point3;
use forge_geometry::TriMesh;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// A regular-grid height sample set.
#[derive(Debug, Clone)]
pub struct HeightField {
    /// Cell values: top-surface Z at the cell center, or `None` when the
    /// part does not cover the cell (air down to `floor`).
    pub heights: Vec<Option<f64>>,
    /// Samples per X axis.
    pub nx: usize,
    /// Samples per Y axis.
    pub ny: usize,
    /// World-space cell size (mm).
    pub cell: f64,
    /// Grid origin (center of cell `[0,0]`).
    pub ox: f64,
    /// Grid origin Y.
    pub oy: f64,
    /// Depth limit where no part exists (stock bottom or a strategy floor).
    pub floor: f64,
}

impl HeightField {
    /// Rasterize `mesh` into a grid of `nx × ny` cells (origin = center of
    /// cell `[0,0]`, spacing `cell`). Cells not covered by any triangle are
    /// "air" and resolve to `floor` when sampled.
    ///
    /// Parallelization is batch-based: each triangle batch emits a sparse
    /// `(cell_index, z)` list (max-merged within the batch), and the lists
    /// are merged sequentially at the end. Memory stays
    /// O(covered cells), not O(batches × grid).
    pub fn rasterize(
        mesh: &TriMesh,
        ox: f64,
        oy: f64,
        nx: usize,
        ny: usize,
        cell: f64,
        floor: f64,
    ) -> Self {
        let mut field = HeightField {
            heights: vec![None; nx * ny],
            nx,
            ny,
            cell,
            ox,
            oy,
            floor,
        };
        let tris: Vec<[Point3; 3]> = mesh.triangles().collect();
        let batch = (tris.len() / 8).max(1);
        let partials: Vec<Vec<(usize, f64)>> = {
            let batches: Vec<&[[Point3; 3]]> = tris.chunks(batch).collect();
            #[cfg(not(target_arch = "wasm32"))]
            let mapped: Vec<Vec<(usize, f64)>> = batches
                .into_par_iter()
                .map(|c| raster_batch(c, ox, oy, nx, ny, cell))
                .collect();
            #[cfg(target_arch = "wasm32")]
            let mapped: Vec<Vec<(usize, f64)>> = batches
                .into_iter()
                .map(|c| raster_batch(c, ox, oy, nx, ny, cell))
                .collect();
            mapped
        };
        for part in partials {
            for (k, z) in part {
                field.heights[k] = Some(match field.heights[k] {
                    Some(prev) => prev.max(z),
                    None => z,
                });
            }
        }
        field
    }

    /// Sample the obstacle height at a cell (air → floor).
    #[inline]
    pub fn z(&self, i: usize, j: usize) -> f64 {
        self.heights[j * self.nx + i].unwrap_or(self.floor)
    }

    /// True when the part covers the cell.
    #[inline]
    pub fn covered(&self, i: usize, j: usize) -> bool {
        self.heights[j * self.nx + i].is_some()
    }

    /// World X of cell `i`.
    #[inline]
    pub fn x(&self, i: usize) -> f64 {
        self.ox + i as f64 * self.cell
    }

    /// World Y of cell `j`.
    #[inline]
    pub fn y(&self, j: usize) -> f64 {
        self.oy + j as f64 * self.cell
    }

    /// World XY → cell indices (clamped).
    pub fn world_to_cell(&self, x: f64, y: f64) -> (usize, usize) {
        let i = (((x - self.ox) / self.cell).round() as i64).clamp(0, self.nx as i64 - 1);
        let j = (((y - self.oy) / self.cell).round() as i64).clamp(0, self.ny as i64 - 1);
        (i as usize, j as usize)
    }

    /// Highest part Z in the whole field (air ignored; empty → floor).
    pub fn top(&self) -> f64 {
        self.heights
            .iter()
            .flatten()
            .copied()
            .fold(self.floor, f64::max)
    }

    /// Dilate by a flat-end disc of `radius`: per cell, the max obstacle
    /// height over the tool footprint (air contributes its floor Z).
    pub fn dilate_flat(&self, radius: f64) -> HeightField {
        self.dilate_impl(radius, false)
    }

    /// Dilate for a ball-nose: an obstacle at distance `d < r` lifts the
    /// tool center by `sqrt(r² − d²)`; air cells contribute floor + 0
    /// (equator contact).
    pub fn dilate_ball(&self, radius: f64) -> HeightField {
        self.dilate_impl(radius, true)
    }

    fn dilate_impl(&self, radius: f64, ball: bool) -> HeightField {
        let r_cells = (radius / self.cell).ceil() as i64;
        let r2 = radius * radius;
        // Precompute the disc footprint offsets (cell space + ball lift).
        let mut offs: Vec<(i64, i64, f64)> = Vec::new();
        for dj in -r_cells..=r_cells {
            for di in -r_cells..=r_cells {
                let dx = di as f64 * self.cell;
                let dy = dj as f64 * self.cell;
                let d2 = dx * dx + dy * dy;
                if d2 <= r2 + 1e-12 {
                    let lift = if ball { (r2 - d2).sqrt() } else { 0.0 };
                    offs.push((di, dj, lift));
                }
            }
        }
        let src = self;
        let heights: Vec<Option<f64>> = {
            let compute = |k: usize| -> Option<f64> {
                let i = (k % src.nx) as i64;
                let j = (k / src.nx) as i64;
                let mut best = f64::NEG_INFINITY;
                for &(di, dj, lift) in &offs {
                    let ii = i + di;
                    let jj = j + dj;
                    if ii < 0 || jj < 0 || ii >= src.nx as i64 || jj >= src.ny as i64 {
                        continue;
                    }
                    let h = src.z(ii as usize, jj as usize) + lift;
                    if h > best {
                        best = h;
                    }
                }
                best.is_finite().then_some(best)
            };
            #[cfg(not(target_arch = "wasm32"))]
            let out: Vec<Option<f64>> = (0..src.nx * src.ny).into_par_iter().map(compute).collect();
            #[cfg(target_arch = "wasm32")]
            let out: Vec<Option<f64>> = (0..src.nx * src.ny).map(compute).collect();
            out
        };
        HeightField {
            heights,
            nx: src.nx,
            ny: src.ny,
            cell: src.cell,
            ox: src.ox,
            oy: src.oy,
            floor: src.floor,
        }
    }

    /// Marching-squares isocontours of the field at level `z`, as closed or
    /// open polylines in world XY. Cells exactly at the level count as
    /// below (`> z` is above).
    pub fn contours(&self, z: f64) -> Vec<Vec<[f64; 2]>> {
        march(self, z)
    }
}

/// Rasterize one triangle batch into a sparse touched-cell list:
/// `(cell_index, z)`, max-merged within the batch.
fn raster_batch(
    tris: &[[Point3; 3]],
    ox: f64,
    oy: f64,
    nx: usize,
    ny: usize,
    cell: f64,
) -> Vec<(usize, f64)> {
    let mut touched: Vec<(usize, f64)> = Vec::new();
    for tri in tris {
        let [a, b, c] = *tri;
        // Vertical triangles carry no XY footprint.
        let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if area.abs() < 1e-14 {
            continue;
        }
        // Plane Z(x,y) via barycentric coordinates of the XY projection.
        let z_at = |x: f64, y: f64| -> f64 {
            let w0 = ((b.x - x) * (c.y - y) - (c.x - x) * (b.y - y)) / area;
            let w1 = ((c.x - x) * (a.y - y) - (a.x - x) * (c.y - y)) / area;
            let w2 = 1.0 - w0 - w1;
            w0 * a.z + w1 * b.z + w2 * c.z
        };
        let (i0, i1) = world_to_range(a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x), ox, cell, nx);
        let (j0, j1) = world_to_range(a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y), oy, cell, ny);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let x = ox + i as f64 * cell;
                let y = oy + j as f64 * cell;
                if point_in_tri_xy(x, y, &a, &b, &c) {
                    let z = z_at(x, y);
                    let idx = j as usize * nx + i as usize;
                    touched.push((idx, z));
                }
            }
        }
    }
    // Sort by cell index, then max-merge duplicates.
    touched.sort_unstable_by_key(|&(k, _)| k);
    touched.dedup_by(|a, b| {
        if a.0 == b.0 {
            b.1 = b.1.max(a.1);
            true
        } else {
            false
        }
    });
    touched
}

/// XY point-in-triangle (projected, inclusive edges).
fn point_in_tri_xy(x: f64, y: f64, a: &Point3, b: &Point3, c: &Point3) -> bool {
    let d1 = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
    let d2 = (c.x - b.x) * (y - b.y) - (c.y - b.y) * (x - b.x);
    let d3 = (a.x - c.x) * (y - c.y) - (a.y - c.y) * (x - c.x);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// World interval → inclusive cell index range (clamped into the grid).
fn world_to_range(min: f64, max: f64, origin: f64, cell: f64, n: usize) -> (i64, i64) {
    let lo = (((min - origin) / cell).floor() as i64).clamp(0, n as i64 - 1);
    let hi = (((max - origin) / cell).ceil() as i64).clamp(0, n as i64 - 1);
    (lo, hi.max(lo))
}

/// Marching squares with linear interpolation and contour stitching.
///
/// Each dual node (between four cells) emits segments via the standard
/// 16-case lookup; segments are stitched into polylines with a hash grid
/// over endpoints (O(n) expected).
fn march(field: &HeightField, z: f64) -> Vec<Vec<[f64; 2]>> {
    let (nx, ny, cell, ox, oy) = (field.nx, field.ny, field.cell, field.ox, field.oy);
    // Above/below classification per cell (air → floor side).
    let above: Vec<bool> = (0..nx * ny)
        .map(|k| field.heights[k].map(|h| h > z).unwrap_or(field.floor > z))
        .collect();
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let idx = |i: usize, j: usize| j * nx + i;
            let c = [
                above[idx(i, j)],
                above[idx(i + 1, j)],
                above[idx(i + 1, j + 1)],
                above[idx(i, j + 1)],
            ];
            let code =
                (c[0] as u8) | ((c[1] as u8) << 1) | ((c[2] as u8) << 2) | ((c[3] as u8) << 3);
            if code == 0 || code == 15 {
                continue;
            }
            let x0 = ox + i as f64 * cell;
            let y0 = oy + j as f64 * cell;
            let x1 = x0 + cell;
            let y1 = y0 + cell;
            let h00 = field.z(i, j);
            let h10 = field.z(i + 1, j);
            let h11 = field.z(i + 1, j + 1);
            let h01 = field.z(i, j + 1);
            let lerp = |ha: f64, hb: f64| -> f64 {
                let t = if (hb - ha).abs() < 1e-12 {
                    0.5
                } else {
                    ((z - ha) / (hb - ha)).clamp(0.0, 1.0)
                };
                t * cell
            };
            // Edge crossing points (bottom, right, top, left).
            let b = [x0 + lerp(h00, h10), y0];
            let r = [x1, y0 + lerp(h10, h11)];
            let t = [x0 + lerp(h01, h11), y1];
            let l = [x0, y0 + lerp(h00, h01)];
            let mut seg = |a: [f64; 2], b: [f64; 2]| segs.push((a, b));
            // Saddle cases 5/10 use the center sample for consistent pairing.
            let center_avg = (h00 + h10 + h11 + h01) * 0.25;
            let center_above = center_avg > z;
            match code {
                1 | 14 => seg(l, b),
                2 | 13 => seg(b, r),
                3 | 12 => seg(l, r),
                4 | 11 => seg(t, r),
                6 | 9 => seg(b, t),
                7 | 8 => seg(l, t),
                5 => {
                    // c0, c2 above.
                    if center_above {
                        seg(b, r); // below corner c1 isolated
                        seg(l, t); // below corner c3 isolated
                    } else {
                        seg(l, b); // above corner c0 isolated
                        seg(t, r); // above corner c2 isolated
                    }
                }
                10 => {
                    // c1, c3 above.
                    if center_above {
                        seg(l, b); // below corner c0 isolated
                        seg(t, r); // below corner c2 isolated
                    } else {
                        seg(b, r); // above corner c1 isolated
                        seg(l, t); // above corner c3 isolated
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    stitch(segs, cell * 0.05)
}

/// Stitch undirected segments into polylines by endpoint proximity.
///
/// Hash-grid lookup on both segment endpoints (3×3 bucket neighbourhood
/// for seam tolerance); closed loops (head meets tail) drop the duplicated
/// endpoint.
fn stitch(segs: Vec<([f64; 2], [f64; 2])>, eps: f64) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    let bucket =
        |p: &[f64; 2]| -> (i64, i64) { ((p[0] / eps).round() as i64, (p[1] / eps).round() as i64) };
    // endpoint key → (segment index, is_start_endpoint)
    let mut by_endpoint: HashMap<(i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        by_endpoint.entry(bucket(a)).or_default().push((k, true));
        by_endpoint.entry(bucket(b)).or_default().push((k, false));
    }
    let neighbourhood = |k: (i64, i64)| -> Vec<(i64, i64)> {
        let mut v = Vec::with_capacity(9);
        for dj in -1..=1i64 {
            for di in -1..=1i64 {
                v.push((k.0 + di, k.1 + dj));
            }
        }
        v
    };
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (s, e) = segs[start];
        let mut line = vec![s, e];
        'extend: loop {
            let tail = *line.last().expect("non-empty");
            // Closed?
            if dist(&line[0], &tail) < eps {
                line.pop();
                break 'extend;
            }
            let key = bucket(&tail);
            let mut found: Option<(usize, [f64; 2])> = None;
            'search: for nb in neighbourhood(key) {
                if let Some(cands) = by_endpoint.get(&nb) {
                    for &(k, is_start) in cands {
                        if used[k] {
                            continue;
                        }
                        let (a, b) = segs[k];
                        let (match_p, other) = if is_start { (a, b) } else { (b, a) };
                        if dist(&match_p, &tail) < eps {
                            found = Some((k, other));
                            break 'search;
                        }
                    }
                }
            }
            match found {
                Some((k, next)) => {
                    used[k] = true;
                    line.push(next);
                }
                None => break 'extend,
            }
        }
        if line.len() >= 2 {
            out.push(line);
        }
    }
    out
}

fn dist(a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::Vector3;
    use forge_geometry::primitives;

    fn boxmesh(cx: f64, cy: f64, w: f64, d: f64, z0: f64, z1: f64) -> TriMesh {
        primitives::box_from_center_extents(
            Point3::new(cx, cy, (z0 + z1) / 2.0),
            Vector3::new(w, d, z1 - z0),
        )
    }

    #[test]
    fn rasterize_box_top_surface() {
        // A 4x4x2 box centered at origin, z 0..2.
        let mesh = boxmesh(0.0, 0.0, 4.0, 4.0, 0.0, 2.0);
        let f = HeightField::rasterize(&mesh, -6.0, -6.0, 25, 25, 0.5, -1.0);
        // Interior cell at world (0,0): cell (12,12).
        assert_eq!(f.z(12, 12), 2.0);
        assert!(f.covered(12, 12));
        // Outside the footprint: air → floor.
        assert!(!f.covered(0, 0));
        assert_eq!(f.z(0, 0), -1.0);
        assert_eq!(f.top(), 2.0);
        // Coverage count ≈ footprint area / cell² (4×4mm → 8×8 cells).
        let covered: usize = f.heights.iter().flatten().count();
        assert!((covered as i64 - 64).abs() <= 20, "covered {covered}");
    }

    #[test]
    fn rasterize_handles_multi_batch() {
        // Many triangles across multiple batches merge consistently.
        let mesh = boxmesh(1.0, -1.0, 3.0, 5.0, 0.0, 1.5);
        let f = HeightField::rasterize(&mesh, -8.0, -8.0, 33, 33, 0.5, -2.0);
        let (i, j) = f.world_to_cell(1.0, -1.0);
        assert!((f.z(i, j) - 1.5).abs() < 1e-9, "z {}", f.z(i, j));
        assert!((f.top() - 1.5).abs() < 1e-9, "top {}", f.top());
    }

    #[test]
    fn flat_dilation_lifts_edges_by_obstacle() {
        let mesh = boxmesh(0.0, 0.0, 4.0, 4.0, 0.0, 2.0);
        let f = HeightField::rasterize(&mesh, -6.0, -6.0, 25, 25, 0.5, -1.0);
        let d = f.dilate_flat(1.0); // 2 mm tool
                                    // Interior: obstacle 2.0 → 2.0.
        assert_eq!(d.z(12, 12), 2.0);
        // 0.75mm outside east face: disc radius 1.0 still sees the box top.
        let (i, j) = f.world_to_cell(2.75, 0.0);
        assert!((d.z(i, j) - 2.0).abs() < 1e-9, "z_out {}", d.z(i, j));
        // 3mm outside east face: disc sees only air → floor.
        let (i, j) = f.world_to_cell(5.0, 0.0);
        assert_eq!(d.z(i, j), -1.0);
    }

    #[test]
    fn ball_dilation_adds_lift() {
        let mesh = boxmesh(0.0, 0.0, 4.0, 4.0, 0.0, 2.0);
        let f = HeightField::rasterize(&mesh, -6.0, -6.0, 25, 25, 0.5, -1.0);
        let d = f.dilate_ball(1.0);
        // Directly over the box: lift = r → 3.0.
        assert!((d.z(12, 12) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn waterline_around_box_is_closed_loop_near_offset() {
        let mesh = boxmesh(0.0, 0.0, 4.0, 4.0, 0.0, 2.0);
        let f = HeightField::rasterize(&mesh, -8.0, -8.0, 65, 65, 0.25, -1.0);
        let d = f.dilate_flat(1.0);
        // Tool-center waterline at z = 1.5 (below the 2.0 top, so the
        // CL=2 plateau is "above"): a closed loop ≈ box footprint offset
        // outward by the 1 mm tool radius → |x| or |y| ≈ 3.0.
        let contours = d.contours(1.5);
        assert!(!contours.is_empty());
        let total: usize = contours.iter().map(|c| c.len()).sum();
        assert!(total > 8, "total points {total}");
        for line in &contours {
            assert!(line.len() >= 3, "degenerate contour {line:?}");
            for p in line {
                let ax = p[0].abs();
                let ay = p[1].abs();
                assert!(
                    (ax - 3.0).abs() < 0.5 || (ay - 3.0).abs() < 0.5,
                    "point {p:?} not on tool-center offset boundary"
                );
                assert!(ax < 3.6 && ay < 3.6, "point {p:?} escapes offset square");
            }
        }
    }

    #[test]
    fn rasterize_many_triangles_stays_fast_and_correct() {
        // A cylinder: enough triangles to span multiple batches.
        let cfg = forge_core::TessellationConfig::default();
        let cyl = primitives::cylinder(Point3::origin(), 2.0, 4.0, &cfg);
        let f = HeightField::rasterize(&cyl, -3.0, -3.0, 61, 61, 0.1, -1.0);
        assert!((f.top() - 4.0).abs() < 1e-9);
        let (i, j) = f.world_to_cell(0.0, 0.0);
        assert!((f.z(i, j) - 4.0).abs() < 1e-9, "axis z {}", f.z(i, j));
    }
}
