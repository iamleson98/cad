//! 2.5D machining strategies over the heightfield kernel.
//!
//! All strategies are pure functions of (part mesh, setup, tool, params) →
//! [`Toolpath`] + warnings. They share the heightfield pipeline:
//!
//! ```text
//! part mesh ─rasterize→ field ─(+leave)→ dilate(tool) → CL field
//!   roughing:  zigzag raster of "CL ≤ level" cells per Z level
//!   waterline: marching-squares contours of the CL field per Z level
//!   facing:    roughing with floor = face target Z
//!   drilling:  explicit hole list → peck cycles
//! ```
//!
//! Semantics notes (v1, documented limits):
//! - roughing enters with a vertical plunge (ramp/helical entry is on the
//!   roadmap),
//! - rest material inside the tool-radius band around walls is left for
//!   smaller tools / the waterline finisher,
//! - flat floors are cleared by roughing; the waterline finisher only
//!   handles slopes and walls,
//! - undercuts are invisible to 3-axis CAM by construction (the field
//!   records the first surface from the top).

use crate::field::HeightField;
use crate::path::{Feeds, Toolpath};
use crate::setup::Setup;
use crate::tool::Tool;
use forge_geometry::TriMesh;
use serde::{Deserialize, Serialize};

/// A hole to drill (from feature recognition or manual input).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    /// Hole axis center in XY (WCS mm).
    pub center: [f64; 2],
    /// Top of the hole (start of the cut, mm Z).
    pub top: f64,
    /// Bottom of the hole (final depth, mm Z).
    pub bottom: f64,
    /// Hole diameter (mm) — matched against the tool.
    pub diameter: f64,
}

/// Roughing / clearing parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoughParams {
    /// Stepdown between Z levels (mm).
    pub stepdown: f64,
    /// Stepover between raster rows (mm).
    pub stepover: f64,
    /// Stock to leave on walls/floors (mm).
    pub leave: f64,
    /// Floor Z the operation cuts to (default: stock bottom).
    pub floor_z: Option<f64>,
}

impl Default for RoughParams {
    fn default() -> Self {
        RoughParams {
            stepdown: 1.5,
            stepover: 2.0,
            leave: 0.2,
            floor_z: None,
        }
    }
}

/// Waterline finishing parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterlineParams {
    /// Stepdown between finish Z levels (mm).
    pub stepdown: f64,
    /// Stock to leave (mm, usually 0).
    pub leave: f64,
}

impl Default for WaterlineParams {
    fn default() -> Self {
        WaterlineParams {
            stepdown: 0.5,
            leave: 0.0,
        }
    }
}

/// Drilling parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrillParams {
    /// Peck depth (`None` = single-shot G81, `Some(d)` = G83).
    pub peck: Option<f64>,
    /// Dwell at hole bottom (seconds) — chip break.
    pub dwell: f64,
}

impl Default for DrillParams {
    fn default() -> Self {
        DrillParams {
            peck: Some(1.0),
            dwell: 0.0,
        }
    }
}

/// Result of one strategy computation.
#[derive(Debug, Clone)]
pub struct CamResult {
    /// The generated toolpath.
    pub path: Toolpath,
    /// Non-fatal warnings (tool too big for a hole, empty region…).
    pub warnings: Vec<String>,
}

/// Field sampling resolution for a strategy (mm), coarsened so the grid
/// stays within a cell budget (≤ 1000 cells/axis ⇒ ≤ 16 MB grids).
fn resolution(stepover: f64, fine: bool, width: f64, depth: f64) -> f64 {
    let base = if fine {
        stepover * 0.25
    } else {
        stepover * 0.5
    };
    let res = base.clamp(0.05, 0.4);
    res.max(width / 1000.0).max(depth / 1000.0)
}

/// Build the cutter-location field for a tool with `leave` on all
/// surfaces (obstacles *and* the floor).
fn cl_field(
    mesh: &TriMesh,
    stock: &crate::setup::Stock,
    tool: &Tool,
    leave: f64,
    res: f64,
    floor: f64,
) -> HeightField {
    let nx = ((stock.width() / res).ceil() as usize).clamp(2, 1000);
    let ny = ((stock.depth() / res).ceil() as usize).clamp(2, 1000);
    let ox = stock.min[0] + res * 0.5;
    let oy = stock.min[1] + res * 0.5;
    let mut lifted = HeightField::rasterize(mesh, ox, oy, nx, ny, res, floor);
    // Stock-to-leave: raise every surface (incl. floor) by `leave`.
    lifted.floor += leave;
    for h in lifted.heights.iter_mut() {
        *h = h.map(|z| z + leave);
    }
    match tool.kind {
        crate::tool::ToolKind::Ball => lifted.dilate_ball(tool.radius()),
        _ => lifted.dilate_flat(tool.radius()),
    }
}

/// True when the straight XY segment `a → b` at level `z` stays in
/// tool-safe space (CL ≤ z along the segment), sampled every `res`.
fn segment_clear(cl: &HeightField, a: [f64; 2], b: [f64; 2], z: f64, res: f64) -> bool {
    let d = (b[0] - a[0]).hypot(b[1] - a[1]);
    let steps = ((d / res).ceil() as usize).max(1);
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        let x = a[0] + (b[0] - a[0]) * t;
        let y = a[1] + (b[1] - a[1]) * t;
        let (i, j) = cl.world_to_cell(x, y);
        if cl.z(i, j) > z + 1e-9 {
            return false;
        }
    }
    true
}

/// Zigzag raster roughing of everything above `floor` that the tool can
/// reach without gouging the part (+`leave`). With `floor_z = Some(z)` this
/// is exactly *facing* (plane the stock down to z).
pub fn rough(
    mesh: &TriMesh,
    setup: &Setup,
    tool: &Tool,
    feeds: Feeds,
    p: &RoughParams,
) -> CamResult {
    let stock = &setup.stock;
    let floor = p.floor_z.unwrap_or(stock.bottom()).max(stock.bottom());
    let res = resolution(p.stepover, false, stock.width(), stock.depth());
    let cl = cl_field(mesh, stock, tool, p.leave, res, floor);
    let mut path = Toolpath::new(
        format!("Rough {:.1}mm @ {:.1} DOC", tool.diameter, p.stepdown),
        tool.id + 1,
        tool.radius(),
        feeds,
    );
    let mut warnings = Vec::new();
    // Levels: stock top − k·stepdown, down to floor.
    let stepdown = p.stepdown.max(0.05);
    let mut levels: Vec<f64> = Vec::new();
    let mut z = stock.top() - stepdown;
    while z > floor + 1e-9 {
        levels.push(z);
        z -= stepdown;
    }
    if levels.last() != Some(&floor) {
        levels.push(floor);
    }
    // Raster rows on the grid: step = stepover / res cells.
    let row_step = ((p.stepover / res).round() as usize).max(1);
    let mut tool_down = false;
    for &level in &levels {
        let mut row_cut = false;
        let mut j = 0usize;
        let mut reverse = false;
        while j < cl.ny {
            // Find runs of allowed cells in this row.
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut start: Option<usize> = None;
            for i in 0..cl.nx {
                let allowed = cl.z(i, j) <= level + 1e-9;
                if allowed {
                    if start.is_none() {
                        start = Some(i);
                    }
                } else if let Some(s) = start.take() {
                    runs.push((s, i - 1));
                }
            }
            if let Some(s) = start.take() {
                runs.push((s, cl.nx - 1));
            }
            if !runs.is_empty() {
                if reverse {
                    runs.reverse();
                }
                for &(i0, i1) in &runs {
                    let x_in = cl.x(i0);
                    let x_out = cl.x(i1);
                    let (x_start, x_end) = if reverse {
                        (x_out, x_in)
                    } else {
                        (x_in, x_out)
                    };
                    let y = cl.y(j);
                    // Stay-down link: previous cut ended at the same level
                    // and the straight XY segment to the new run start is
                    // verified collision-free on the CL field.
                    let last_cut = path.moves.last().filter(|m| m.cuts()).map(|m| m.target());
                    let link = tool_down
                        && last_cut
                            .map(|t| (t[2] - level).abs() < 1e-9)
                            .unwrap_or(false)
                        && last_cut
                            .map(|t| segment_clear(&cl, [t[0], t[1]], [x_start, y], level, res))
                            .unwrap_or(false);
                    if link {
                        path.feed([x_start, y, level]);
                    } else {
                        path.rapid([x_start, y, setup.safe_z]);
                        path.plunge([x_start, y, level]);
                        tool_down = true;
                    }
                    path.feed([x_end, y, level]);
                    row_cut = true;
                }
                reverse = !reverse;
            }
            j += row_step;
        }
        // Retract between levels.
        if tool_down {
            if let Some(last) = path.moves.last().map(|m| m.target()) {
                path.rapid([last[0], last[1], setup.safe_z]);
            }
            tool_down = false;
        }
        if !row_cut {
            warnings.push(format!(
                "Level z={level:.2}: nothing to clear (tool too large or already at floor)"
            ));
        }
    }
    if path.moves.is_empty() {
        warnings.push("Roughing produced no moves: check floor/stock bounds and tool size".into());
    }
    CamResult { path, warnings }
}

/// *Facing*: plane the stock down to `target` (default: highest part
/// surface), avoiding islands that rise above the target.
pub fn face(
    mesh: &TriMesh,
    setup: &Setup,
    tool: &Tool,
    feeds: Feeds,
    target: Option<f64>,
    leave: f64,
) -> CamResult {
    let target = target.unwrap_or_else(|| {
        let bb = mesh.bbox();
        if bb.is_valid() {
            bb.max.z
        } else {
            setup.stock.top()
        }
    });
    let p = RoughParams {
        stepdown: (setup.stock.top() - target).clamp(0.5, 3.0),
        stepover: tool.diameter * 0.65,
        leave,
        floor_z: Some(target),
    };
    let mut res = rough(mesh, setup, tool, feeds, &p);
    res.path.label = format!("Face {:.1}mm to z {:.2}", tool.diameter, target);
    res
}

/// Waterline (constant-Z) finishing: contour the CL field at successive
/// levels; slopes and walls get a constant-Z finish pass.
pub fn waterline(
    mesh: &TriMesh,
    setup: &Setup,
    tool: &Tool,
    feeds: Feeds,
    p: &WaterlineParams,
) -> CamResult {
    let stock = &setup.stock;
    let floor = stock.bottom();
    let res = resolution(p.stepdown, true, stock.width(), stock.depth());
    let cl = cl_field(mesh, stock, tool, p.leave, res, floor);
    let mut path = Toolpath::new(
        format!("Waterline {:.1}mm @ {:.2} step", tool.diameter, p.stepdown),
        tool.id + 1,
        tool.radius(),
        feeds,
    );
    let mut warnings = Vec::new();
    let stepdown = p.stepdown.max(0.05);
    let top = cl.top();
    let mut levels: Vec<f64> = Vec::new();
    let mut z = top - stepdown;
    while z > floor + 1e-9 {
        levels.push(z);
        z -= stepdown;
    }
    levels.push(floor);
    let mut tool_down = false;
    for &level in &levels {
        let mut contours = cl.contours(level);
        // Drop degenerate slivers.
        contours.retain(|c| c.len() >= 4 && polyline_len(c) > tool.diameter * 0.5);
        // Outer (longer) contours first.
        contours.sort_by(|a, b| polyline_len(b).total_cmp(&polyline_len(a)));
        for line in &contours {
            let pts = orient_loop(line, &cl, level);
            if pts.len() < 3 {
                continue;
            }
            let (x0, y0) = (pts[0][0], pts[0][1]);
            let last_xy = path.moves.last().map(|m| [m.target()[0], m.target()[1]]);
            let same_level = path
                .moves
                .last()
                .map(|m| m.cuts() && (m.target()[2] - level).abs() < 1e-9)
                .unwrap_or(false);
            let stay_down = tool_down
                && same_level
                && last_xy
                    .map(|t| (t[0] - x0).hypot(t[1] - y0) < tool.diameter * 3.0)
                    .unwrap_or(false)
                && last_xy
                    .map(|t| segment_clear(&cl, t, [x0, y0], level, res))
                    .unwrap_or(false);
            if stay_down {
                if let Some(t) = last_xy {
                    if (t[0] - x0).hypot(t[1] - y0) > 1e-9 {
                        path.feed([x0, y0, level]);
                    }
                }
            } else {
                if tool_down {
                    if let Some(t) = last_xy {
                        path.rapid([t[0], t[1], setup.safe_z]);
                    }
                }
                path.rapid([x0, y0, setup.safe_z]);
                path.plunge([x0, y0, level]);
                tool_down = true;
            }
            for pt in pts.iter().skip(1) {
                path.feed([pt[0], pt[1], level]);
            }
            // Close the loop (stitched loops are open polylines).
            path.feed([x0, y0, level]);
        }
    }
    if tool_down {
        if let Some(last) = path.moves.last().map(|m| m.target()) {
            path.rapid([last[0], last[1], setup.safe_z]);
        }
    }
    if path.moves.is_empty() {
        warnings.push(
            "Waterline produced no moves: part may be flat (walls below stepdown resolution)"
                .into(),
        );
    }
    CamResult { path, warnings }
}

/// Drilling: peck cycles at hole centers, nearest-neighbor order.
///
/// Holes whose diameter is smaller than the tool are skipped with a
/// warning; oversize holes are still drilled at center (pilot semantics).
pub fn drill(
    setup: &Setup,
    tool: &Tool,
    feeds: Feeds,
    holes: &[Hole],
    p: &DrillParams,
) -> CamResult {
    let mut path = Toolpath::new(
        format!("Drill {:.1}mm x{}", tool.diameter, holes.len()),
        tool.id + 1,
        tool.radius(),
        feeds,
    );
    let mut warnings = Vec::new();
    let mut open: Vec<Hole> = holes.to_vec();
    // Nearest-neighbour ordering from the stock origin.
    let origin = [setup.stock.min[0], setup.stock.min[1]];
    let mut cursor = origin;
    let mut ordered = Vec::new();
    while !open.is_empty() {
        let (best_i, _) = open
            .iter()
            .enumerate()
            .map(|(i, h)| (i, (h.center[0] - cursor[0]).hypot(h.center[1] - cursor[1])))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("non-empty");
        let h = open.remove(best_i);
        cursor = h.center;
        ordered.push(h);
    }
    let mut tool_down = false;
    for h in &ordered {
        if h.diameter + 1e-6 < tool.diameter {
            warnings.push(format!(
                "Hole at ({:.1},{:.1}) D{:.1} skipped: drill D{:.1} does not fit",
                h.center[0], h.center[1], h.diameter, tool.diameter
            ));
            continue;
        }
        tool_down = true;
        // Retract, position over the hole, then the drill cycle.
        path.rapid([h.center[0], h.center[1], setup.safe_z]);
        path.rapid([h.center[0], h.center[1], h.top + 0.5]);
        path.drill([h.center[0], h.center[1], h.top], h.top, h.bottom, p.peck);
    }
    if tool_down {
        if let Some(last) = path.moves.last().map(|m| m.target()) {
            path.rapid([last[0], last[1], setup.safe_z]);
        }
    }
    if path.moves.is_empty() {
        warnings.push("No drillable holes (all skipped or empty input)".into());
    }
    CamResult { path, warnings }
}

/// Signed area of a closed XY polyline (shoelace).
fn signed_area(pts: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        a += pts[i][0] * pts[j][1] - pts[j][0] * pts[i][1];
    }
    a * 0.5
}

/// Total polyline length (open: n−1 segments).
fn polyline_len(pts: &[[f64; 2]]) -> f64 {
    pts.windows(2)
        .map(|w| (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]))
        .sum()
}

/// Normalize loop orientation so the material side (field above the
/// level) stays consistent: loops enclosing above-material run CCW, loops
/// enclosing air (pockets) run CW. This yields consistent climb milling.
fn orient_loop(pts: &[[f64; 2]], cl: &HeightField, level: f64) -> Vec<[f64; 2]> {
    let area = signed_area(pts);
    let (mut cx, mut cy) = (0.0, 0.0);
    for p in pts {
        cx += p[0];
        cy += p[1];
    }
    cx /= pts.len() as f64;
    cy /= pts.len() as f64;
    let (i, j) = cl.world_to_cell(cx, cy);
    let inside_above = cl.z(i, j) > level;
    let ccw = area > 0.0;
    if ccw == inside_above {
        pts.to_vec()
    } else {
        let mut r = pts.to_vec();
        r.reverse();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Move;
    use crate::setup::Stock;
    use forge_core::{Point3, Vector3};
    use forge_geometry::primitives;

    fn boxmesh(w: f64, d: f64, h: f64) -> TriMesh {
        primitives::box_from_center_extents(Point3::new(0.0, 0.0, h / 2.0), Vector3::new(w, d, h))
    }

    fn setup_for(mesh: &TriMesh) -> Setup {
        let stock = Stock::around_mesh(mesh, 3.0, 1.0).unwrap();
        Setup::from_stock(stock)
    }

    fn tool6() -> Tool {
        Tool::presets()[0].clone() // 6 mm flat 3FL
    }

    #[test]
    fn roughing_clears_pocket_around_boss() {
        // 6 mm boss (h=4) centered in a 12 mm stock: ring pocket to floor.
        let mesh = boxmesh(6.0, 6.0, 4.0);
        let setup = setup_for(&mesh);
        let p = RoughParams {
            stepdown: 2.0,
            stepover: 2.5,
            leave: 0.1,
            floor_z: None,
        };
        let res = rough(&mesh, &setup, &tool6(), Feeds::default(), &p);
        assert!(!res.path.moves.is_empty());
        assert!(
            res.path.cut_length() > 50.0,
            "cut len {}",
            res.path.cut_length()
        );
        // Every cut target must respect the CL field: no cut inside the
        // tool-radius band around the boss below its top.
        for m in &res.path.moves {
            if !m.cuts() {
                continue;
            }
            let t = m.target();
            let r = t[0].hypot(t[1]);
            if r < 3.0 && t[2] < 3.5 {
                panic!("gouge: cut at r={r} z={}", t[2]);
            }
        }
        assert!(res.path.time_minutes() > 0.0);
    }

    #[test]
    fn roughing_floors_respect_floor_z() {
        let mesh = boxmesh(6.0, 6.0, 4.0);
        let setup = setup_for(&mesh);
        let p = RoughParams {
            stepdown: 1.0,
            stepover: 2.5,
            leave: 0.0,
            floor_z: Some(1.0),
        };
        let res = rough(&mesh, &setup, &tool6(), Feeds::default(), &p);
        let (lo, _) = res.path.cut_z_bounds().expect("bounds");
        assert!(lo >= 1.0 - 1e-6, "cut below floor: {lo}");
    }

    #[test]
    fn facing_is_a_single_level_raster() {
        let mesh = boxmesh(8.0, 8.0, 2.0);
        let setup = setup_for(&mesh);
        let res = face(&mesh, &setup, &tool6(), Feeds::default(), None, 0.0);
        assert!(!res.path.moves.is_empty());
        let (lo, hi) = res.path.cut_z_bounds().expect("bounds");
        assert!((hi - 2.0).abs() < 1e-6, "face level {hi}");
        assert!((lo - 2.0).abs() < 1e-6, "face floor {lo}");
        assert!(res.path.label.starts_with("Face"));
    }

    #[test]
    fn waterline_wraps_boss_with_closed_loops() {
        let mesh = boxmesh(6.0, 6.0, 4.0);
        let setup = setup_for(&mesh);
        let p = WaterlineParams {
            stepdown: 1.0,
            leave: 0.0,
        };
        let res = waterline(&mesh, &setup, &tool6(), Feeds::default(), &p);
        assert!(!res.path.moves.is_empty(), "waterline empty");
        let feeds: Vec<&Move> = res.path.moves.iter().filter(|m| m.cuts()).collect();
        assert!(feeds.len() > 20, "feed moves {}", feeds.len());
        for m in &feeds {
            assert!(m.target()[2] > setup.stock.bottom() - 1e-9);
        }
    }

    #[test]
    fn drilling_orders_and_pecks() {
        let mesh = boxmesh(20.0, 20.0, 5.0);
        let setup = setup_for(&mesh);
        let drill_tool = Tool::presets()[4].clone(); // D5
        let holes = vec![
            Hole {
                center: [5.0, 5.0],
                top: 5.0,
                bottom: 0.0,
                diameter: 5.2,
            },
            Hole {
                center: [-5.0, -5.0],
                top: 5.0,
                bottom: 2.0,
                diameter: 6.0,
            },
            Hole {
                center: [0.0, 0.0],
                top: 5.0,
                bottom: 0.0,
                diameter: 4.0,
            }, // too small
        ];
        let p = DrillParams {
            peck: Some(1.5),
            dwell: 0.0,
        };
        let res = drill(&setup, &drill_tool, Feeds::default(), &holes, &p);
        let cycles = res
            .path
            .moves
            .iter()
            .filter(|m| matches!(m, Move::Drill { .. }))
            .count();
        assert_eq!(cycles, 2, "skipped hole must not drill");
        assert!(
            res.warnings.iter().any(|w| w.contains("skipped")),
            "{:?}",
            res.warnings
        );
        // Nearest-neighbor from stock min corner (negative coords).
        let first_rapid = res.path.moves.iter().find_map(|m| match m {
            Move::Rapid { p } => Some(*p),
            _ => None,
        });
        let fr = first_rapid.expect("rapid");
        assert!(
            fr[0] < 0.0 && fr[1] < 0.0,
            "first hole should be the SW one, got {fr:?}"
        );
    }

    #[test]
    fn strategies_are_deterministic() {
        let mesh = boxmesh(7.0, 5.0, 3.0);
        let setup = setup_for(&mesh);
        let p = RoughParams::default();
        let a = rough(&mesh, &setup, &tool6(), Feeds::default(), &p);
        let b = rough(&mesh, &setup, &tool6(), Feeds::default(), &p);
        assert_eq!(a.path.moves.len(), b.path.moves.len());
        assert_eq!(a.path.cut_length(), b.path.cut_length());
    }
}
