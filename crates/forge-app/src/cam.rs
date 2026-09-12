//! CAM workspace state (C-03/C-04): tool library, operations, computation,
//! G-code, and viewport overlays. UI lives in `ui.rs::cam_panel`; this
//! module stays UI-free and unit-testable.

use forge_cam::path::{Feeds, Toolpath};
use forge_cam::post::{self, PostOptions};
use forge_cam::setup::{Setup, Stock};
use forge_cam::strategy::{self, Hole, RoughParams, WaterlineParams};
use forge_cam::tool::{Tool, ToolLibrary};
use forge_geometry::TriMesh;
use forge_model::{Document, Evaluation, Feature};
use forge_render::{OverlayLines, Scene};
use serde::{Deserialize, Serialize};

/// Strategy kinds surfaced in the CAM panel (Inventor-CAM equivalents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CamStrategyKind {
    /// Adaptive-style raster roughing (clear everything above a floor).
    Rough,
    /// Face the top down to a level.
    Face,
    /// Waterline (constant-Z) wall/slope finishing.
    Waterline,
    /// Peck-drill recognized holes (C-06).
    Drill,
}

impl CamStrategyKind {
    /// Short label.
    pub fn label(self) -> &'static str {
        match self {
            CamStrategyKind::Rough => "Rough",
            CamStrategyKind::Face => "Face",
            CamStrategyKind::Waterline => "Waterline",
            CamStrategyKind::Drill => "Drill",
        }
    }

    /// All kinds (panel buttons).
    pub const ALL: [CamStrategyKind; 4] = [Self::Rough, Self::Face, Self::Waterline, Self::Drill];
}

/// One CAM operation (serializable: params only; results are recomputed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamOp {
    /// Stable op id.
    pub id: u32,
    /// Suppressed ops stay listed but are skipped (like feature suppress).
    pub enabled: bool,
    /// Strategy.
    pub kind: CamStrategyKind,
    /// Tool id in the library.
    pub tool_id: u32,
    /// Stepdown (rough/face/waterline: Z per pass; drill: unused).
    pub stepdown: f64,
    /// Stepover (rough/face: raster spacing).
    pub stepover: f64,
    /// Stock to leave (mm).
    pub leave: f64,
    /// Explicit floor / face target Z (rough & face; `None` = default).
    pub floor_z: Option<f64>,
    /// Peck depth for drilling (`None` = G81 single shot).
    pub peck: Option<f64>,
    /// Drill dwell (seconds).
    pub dwell: f64,

    /// Computed toolpath (not serialized; recomputed on demand).
    #[serde(skip)]
    pub result: Option<Toolpath>,
    /// Warnings from the last compute (not serialized).
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl CamOp {
    /// New op with strategy-appropriate defaults for `tool`.
    fn new(id: u32, kind: CamStrategyKind, tool: &Tool) -> Self {
        let (stepdown, stepover) = match kind {
            CamStrategyKind::Rough => (1.5, tool.diameter * 0.4),
            CamStrategyKind::Face => (1.0, tool.diameter * 0.65),
            CamStrategyKind::Waterline => (0.5, tool.diameter * 0.4),
            CamStrategyKind::Drill => (1.0, 0.0),
        };
        CamOp {
            id,
            enabled: true,
            kind,
            tool_id: tool.id,
            stepdown,
            stepover,
            leave: match kind {
                CamStrategyKind::Rough => 0.2,
                _ => 0.0,
            },
            floor_z: None,
            peck: if kind == CamStrategyKind::Drill {
                Some(1.0)
            } else {
                None
            },
            dwell: 0.0,
            result: None,
            warnings: Vec::new(),
        }
    }
}

/// CAM workspace: everything the CAM panel needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamState {
    /// Tool library (document-local, serializable).
    pub library: ToolLibrary,
    /// Operations in order.
    pub ops: Vec<CamOp>,
    next_op_id: u32,
    /// Show the CAM dock.
    #[serde(default)]
    pub panel_open: bool,
    /// Draw the stock ghost box.
    #[serde(default = "default_true")]
    pub show_stock: bool,
    /// Draw feed moves.
    #[serde(default = "default_true")]
    pub show_toolpaths: bool,
    /// Draw rapid moves.
    #[serde(default)]
    pub show_rapids: bool,
    /// XY stock margin around the part bounds (mm).
    pub stock_margin: f64,
    /// Stock top margin above the part (mm).
    pub stock_top_margin: f64,
    /// Result of the last compute (status line).
    #[serde(skip)]
    pub last_status: Option<String>,
    /// Stock captured at the last compute (ghost overlay).
    #[serde(skip)]
    pub last_stock: Option<Stock>,
    /// Cached G-code of the last export (not serialized).
    #[serde(skip)]
    pub gcode: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for CamState {
    fn default() -> Self {
        CamState {
            library: ToolLibrary::starter(),
            ops: Vec::new(),
            next_op_id: 0,
            panel_open: false,
            show_stock: true,
            show_toolpaths: true,
            show_rapids: false,
            stock_margin: 3.0,
            stock_top_margin: 1.0,
            last_status: None,
            last_stock: None,
            gcode: None,
        }
    }
}

impl CamState {
    /// Add an operation of `kind` using the first tool (or a default).
    pub fn add_op(&mut self, kind: CamStrategyKind) -> u32 {
        let tool = self
            .library
            .tools
            .first()
            .cloned()
            .unwrap_or_else(|| Tool::presets()[0].clone());
        let id = self.next_op_id;
        self.next_op_id += 1;
        let op = CamOp::new(id, kind, &tool);
        self.ops.push(op);
        id
    }

    /// Remove an operation by id.
    pub fn remove_op(&mut self, id: u32) -> bool {
        let before = self.ops.len();
        self.ops.retain(|o| o.id != id);
        self.ops.len() != before
    }

    /// Merged part mesh of all visible bodies (the CAM "part"). Mesh
    /// concatenation is exact for the heightfield kernel (it takes the
    /// max surface per cell — overlaps are harmless).
    pub fn part_mesh(ev: &Evaluation) -> TriMesh {
        let mut mesh = TriMesh::default();
        for b in &ev.bodies {
            let base = mesh.positions.len() as u32;
            mesh.positions.extend_from_slice(&b.mesh.positions);
            mesh.indices.extend(b.mesh.indices.iter().map(|i| i + base));
            mesh.normals = None;
        }
        mesh
    }

    /// Stock around the part bounds with the state's margins.
    pub fn stock_for(&self, part: &TriMesh) -> Option<Stock> {
        Stock::around_mesh(part, self.stock_margin, self.stock_top_margin)
    }

    /// All holes recognized from the document's hole features (C-06).
    pub fn holes_from_doc(doc: &Document) -> Vec<Hole> {
        let mut holes = Vec::new();
        for node in doc.tree.nodes_in_order() {
            if node.suppressed {
                continue;
            }
            if let Feature::Hole(p) = &node.feature {
                if node.suppressed {
                    continue;
                }
                if let Ok(placements) = forge_model::hole_placements(doc, p) {
                    for (center, diameter, top, bottom) in placements {
                        holes.push(Hole {
                            center: [center.x, center.y],
                            top,
                            bottom,
                            diameter,
                        });
                    }
                }
            }
        }
        holes
    }

    /// Feeds for an op from its tool's defaults.
    fn feeds_for(library: &ToolLibrary, op: &CamOp) -> Feeds {
        let t = library.get(op.tool_id);
        Feeds {
            feed: t.map(|t| t.default_feed).unwrap_or(1000.0),
            plunge: t.map(|t| t.default_plunge).unwrap_or(300.0),
            rapid: 5000.0,
            rpm: t.map(|t| t.default_rpm).unwrap_or(10000.0),
        }
    }

    /// (Re)compute every enabled operation against the evaluated part.
    /// Returns a status line for the panel.
    pub fn compute(&mut self, doc: &Document, ev: &Evaluation) -> String {
        self.gcode = None;
        let part = Self::part_mesh(ev);
        if part.tri_count() == 0 {
            self.last_status = Some("CAM: no bodies to machine".into());
            return self.last_status.clone().unwrap();
        }
        let Some(stock) = self.stock_for(&part) else {
            self.last_status = Some("CAM: part has no bounds".into());
            return self.last_status.clone().unwrap();
        };
        let setup = Setup::from_stock(stock.clone());
        let holes = Self::holes_from_doc(doc);
        self.last_stock = Some(stock);
        let mut total_cut = 0.0;
        let mut total_time = 0.0;
        let mut computed = 0;
        let library = self.library.clone();
        let library = &library;
        for op in self.ops.iter_mut() {
            if !op.enabled {
                op.result = None;
                op.warnings.clear();
                continue;
            }
            let tool = self
                .library
                .get(op.tool_id)
                .cloned()
                .unwrap_or_else(|| Tool::presets()[0].clone());
            let feeds = Self::feeds_for(library, op);
            let res = match op.kind {
                CamStrategyKind::Rough => strategy::rough(
                    &part,
                    &setup,
                    &tool,
                    feeds,
                    &RoughParams {
                        stepdown: op.stepdown,
                        stepover: op.stepover,
                        leave: op.leave,
                        floor_z: op.floor_z,
                    },
                ),
                CamStrategyKind::Face => {
                    strategy::face(&part, &setup, &tool, feeds, op.floor_z, op.leave)
                }
                CamStrategyKind::Waterline => strategy::waterline(
                    &part,
                    &setup,
                    &tool,
                    feeds,
                    &WaterlineParams {
                        stepdown: op.stepdown,
                        leave: op.leave,
                    },
                ),
                CamStrategyKind::Drill => strategy::drill(
                    &setup,
                    &tool,
                    feeds,
                    &holes,
                    &strategy::DrillParams {
                        peck: op.peck,
                        dwell: op.dwell,
                    },
                ),
            };
            let (cut, time) = (res.path.cut_length(), res.path.time_minutes());
            total_cut += cut;
            total_time += time;
            computed += 1;
            op.warnings = res.warnings;
            op.result = Some(res.path);
        }
        let status = if computed == 0 {
            "CAM: no operations (add Rough / Face / Waterline / Drill)".to_string()
        } else {
            format!(
                "CAM: {computed} ops — cut {total_cut:.0} mm, est. {total_time:.1} min, {} holes",
                holes.len()
            )
        };
        self.last_status = Some(status.clone());
        status
    }

    /// G-code for all computed ops (cached).
    pub fn gcode(&mut self) -> Option<String> {
        if self.gcode.is_some() {
            return self.gcode.clone();
        }
        let paths: Vec<Toolpath> = self.ops.iter().filter_map(|o| o.result.clone()).collect();
        if !paths.iter().any(|p| p.moves.iter().any(|m| m.cuts())) {
            return None;
        }
        let g = post::post(&paths, &PostOptions::default());
        self.gcode = Some(g.clone());
        Some(g)
    }

    /// Viewport overlays: stock ghost + per-op feed/rapid polylines.
    pub fn build_overlays(&self, scene: &mut Scene) {
        let mut overlays = Vec::new();
        if self.show_stock {
            if let Some(stock) = &self.last_stock {
                overlays.push(stock_overlay(stock));
            }
        }
        if self.show_toolpaths || self.show_rapids {
            for op in &self.ops {
                let Some(path) = &op.result else { continue };
                if self.show_toolpaths {
                    let segs: Vec<([f32; 3], [f32; 3])> = path
                        .cut_segments()
                        .into_iter()
                        .map(|(a, b)| (to_f32(a), to_f32(b)))
                        .collect();
                    if !segs.is_empty() {
                        overlays.push(OverlayLines {
                            segments: segs,
                            color: Scene::CAM_FEED,
                        });
                    }
                }
                if self.show_rapids {
                    let segs: Vec<([f32; 3], [f32; 3])> = path
                        .rapid_segments()
                        .into_iter()
                        .map(|(a, b)| (to_f32(a), to_f32(b)))
                        .collect();
                    if !segs.is_empty() {
                        overlays.push(OverlayLines {
                            segments: segs,
                            color: Scene::CAM_RAPID,
                        });
                    }
                }
            }
        }
        if scene.overlays != overlays {
            scene.overlays = overlays;
            scene.version += 1;
        }
    }

    /// Clear all results (document changed → toolpaths stale).
    pub fn clear_results(&mut self) {
        for op in self.ops.iter_mut() {
            op.result = None;
            op.warnings.clear();
        }
        self.gcode = None;
        self.last_stock = None;
        self.last_status = None;
    }
}

fn to_f32(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

/// Stock box edge overlay (12 edges, faint).
fn stock_overlay(stock: &Stock) -> OverlayLines {
    let [mn, mx] = [&stock.min, &stock.max];
    let pts = |i: f64, j: f64, k: f64| [i as f32, j as f32, k as f32];
    let c = [
        pts(mn[0], mn[1], mn[2]),
        pts(mx[0], mn[1], mn[2]),
        pts(mx[0], mx[1], mn[2]),
        pts(mn[0], mx[1], mn[2]),
        pts(mn[0], mn[1], mx[2]),
        pts(mx[0], mn[1], mx[2]),
        pts(mx[0], mx[1], mx[2]),
        pts(mn[0], mx[1], mx[2]),
    ];
    let e = |a: usize, b: usize| (c[a], c[b]);
    let segments = vec![
        e(0, 1),
        e(1, 2),
        e(2, 3),
        e(3, 0), // bottom
        e(4, 5),
        e(5, 6),
        e(6, 7),
        e(7, 4), // top
        e(0, 4),
        e(1, 5),
        e(2, 6),
        e(3, 7), // pillars
    ];
    OverlayLines {
        segments,
        color: Scene::CAM_STOCK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Point3, Vector3};
    use forge_geometry::primitives;
    use forge_model::Document;

    /// Synthetic evaluation with one box body.
    fn eval_with_box() -> (Document, Evaluation) {
        let doc = Document::new("cam-test");
        let mut ev = Evaluation::default();
        let mesh = primitives::box_from_center_extents(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(6.0, 6.0, 4.0),
        );
        ev.bodies.push(forge_model::EvalBody {
            id: forge_core::BodyId::new(1),
            name: "Boss".into(),
            mesh,
            source: forge_core::FeatureId::new(1),
        });
        (doc, ev)
    }

    #[test]
    fn add_remove_ops() {
        let mut cam = CamState::default();
        let a = cam.add_op(CamStrategyKind::Rough);
        let b = cam.add_op(CamStrategyKind::Waterline);
        assert_eq!(cam.ops.len(), 2);
        assert_ne!(a, b);
        assert!(cam.remove_op(a));
        assert!(!cam.remove_op(a));
        assert_eq!(cam.ops.len(), 1);
        assert_eq!(cam.ops[0].id, b);
    }

    #[test]
    fn compute_rough_produces_path_and_status() {
        let (doc, ev) = eval_with_box();
        let mut cam = CamState::default();
        cam.add_op(CamStrategyKind::Rough);
        let status = cam.compute(&doc, &ev);
        assert!(status.contains("CAM: 1 ops"), "{status}");
        assert!(cam.ops[0].result.is_some());
        let path = cam.ops[0].result.as_ref().unwrap();
        assert!(path.cut_length() > 10.0);
        // G-code generation works and is cached.
        let g = cam.gcode().expect("gcode");
        assert!(g.contains("T1 M6"));
        assert!(g.contains("M30"));
        let again = cam.gcode().unwrap();
        assert_eq!(g, again);
    }

    #[test]
    fn compute_with_no_bodies_reports_cleanly() {
        let doc = Document::new("empty");
        let ev = Evaluation::default();
        let mut cam = CamState::default();
        cam.add_op(CamStrategyKind::Rough);
        let status = cam.compute(&doc, &ev);
        assert_eq!(status, "CAM: no bodies to machine");
        assert!(cam.ops[0].result.is_none());
    }

    #[test]
    fn suppressed_ops_are_skipped() {
        let (doc, ev) = eval_with_box();
        let mut cam = CamState::default();
        let id = cam.add_op(CamStrategyKind::Rough);
        cam.ops[0].enabled = false;
        let status = cam.compute(&doc, &ev);
        assert!(status.contains("no operations"), "{status}");
        assert!(cam
            .ops
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .result
            .is_none());
    }

    #[test]
    fn drill_op_recognizes_hole_features() {
        // Build a document with a hole feature and verify recognition.
        let doc = doc_with_hole();
        let holes = CamState::holes_from_doc(&doc);
        assert_eq!(holes.len(), 2, "two placements (point + circle center)");
        assert!((holes[0].diameter - 5.0).abs() < 1e-9);
        assert!(holes.iter().all(|h| h.top > h.bottom));
    }

    /// A plate with a top-face sketch carrying a point and a circle,
    /// consumed by a hole feature (C-06 recognition fixture).
    fn doc_with_hole() -> Document {
        use forge_core::{Point2, SketchId};
        use forge_model::{
            DatumParams, Feature, HoleKind, HoleParams, PrimitiveKind, PrimitiveParams,
        };
        use forge_sketch::{Sketch, SketchPlane};
        let mut doc = Document::new("cam-hole");
        let plate = doc
            .add_feature(Feature::Primitive(PrimitiveParams {
                kind: PrimitiveKind::Box,
                center: Point3::origin(),
                dims: Vector3::new(30.0, 30.0, 10.0),
            }))
            .unwrap();
        let datum = doc
            .add_feature(Feature::Datum(DatumParams::Offset {
                base: forge_sketch::DatumPlane::XY,
                offset: 5.0,
            }))
            .unwrap();
        let sid = SketchId::new(doc.allocator.next_id());
        let mut sketch = Sketch::new(sid, "holes", SketchPlane::DatumRef { feature: datum });
        sketch.add_point(Point2::origin());
        sketch.add_circle(Point2::new(10.0, 0.0), 2.5);
        let profile = doc.add_feature(Feature::Sketch(sketch)).unwrap();
        doc.add_feature(Feature::Hole(HoleParams {
            profile,
            kind: HoleKind::Simple,
            diameter: 5.0,
            depth: 12.0,
            direction: forge_geometry::ExtrudeDirection::Negative,
            counterbore_diameter: 8.0,
            counterbore_depth: 2.0,
            countersink_diameter: 8.0,
            countersink_angle: 90f64.to_radians(),
            drill_point: false,
            drill_angle: 118f64.to_radians(),
            target: plate,
        }))
        .unwrap();
        doc
    }

    #[test]
    fn overlays_built_after_compute() {
        let (doc, ev) = eval_with_box();
        let mut cam = CamState::default();
        cam.add_op(CamStrategyKind::Rough);
        cam.compute(&doc, &ev);
        let mut scene = Scene::new();
        cam.build_overlays(&mut scene);
        assert!(!scene.overlays.is_empty(), "stock + toolpath overlays");
        assert!(scene.overlays.iter().any(|o| o.color == Scene::CAM_FEED));
        assert!(scene.overlays.iter().any(|o| o.color == Scene::CAM_STOCK));
    }

    #[test]
    fn clear_results_resets() {
        let (doc, ev) = eval_with_box();
        let mut cam = CamState::default();
        cam.add_op(CamStrategyKind::Rough);
        cam.compute(&doc, &ev);
        cam.clear_results();
        assert!(cam.ops[0].result.is_none());
        assert!(cam.gcode.is_none());
    }

    #[test]
    fn cam_state_serializes_ron() {
        let mut cam = CamState::default();
        cam.add_op(CamStrategyKind::Face);
        let s = ron::to_string(&cam).unwrap();
        let back: CamState = ron::from_str(&s).unwrap();
        assert_eq!(back.ops.len(), 1);
        assert_eq!(back.ops[0].kind, CamStrategyKind::Face);
        assert_eq!(back.library.tools.len(), cam.library.tools.len());
    }
}
