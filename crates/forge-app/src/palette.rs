//! Command palette (FR-UI-01) and palette actions.

use crate::app::ForgeApp;
use crate::icons;
use forge_core::{FeatureId, Point2, Point3, SketchId, Vector3};
use forge_geometry::ExtrudeDirection;
use forge_model::{
    BooleanFeature, CircularPatternParams, Command, DatumParams, ExtrudeOp, ExtrudeParams, Feature,
    HoleKind, HoleParams, LinearPatternParams, MirrorParams, PrimitiveKind, PrimitiveParams,
};
use forge_render::Scene;
use forge_sketch::{DatumPlane, Sketch, SketchPlane};

/// A palette entry.
pub struct PaletteEntry {
    /// Display label.
    pub label: &'static str,
    /// Keyword hint for fuzzy matching.
    pub keywords: &'static str,
    /// Lucide glyph for the palette row (crate::icons).
    pub icon: char,
    /// The action.
    pub action: PaletteAction,
}

/// All palette commands.
pub fn entries() -> Vec<PaletteEntry> {
    use PaletteAction::*;
    vec![
        PaletteEntry {
            label: "Add Box",
            keywords: "primitive solid box",
            icon: icons::BOX,
            action: AddBox,
        },
        PaletteEntry {
            label: "Add Sphere",
            keywords: "primitive solid sphere",
            icon: icons::SPHERE,
            action: AddSphere,
        },
        PaletteEntry {
            label: "Add Cylinder",
            keywords: "primitive solid cylinder",
            icon: icons::CYLINDER,
            action: AddCylinder,
        },
        PaletteEntry {
            label: "Add Cone",
            keywords: "primitive solid cone",
            icon: icons::CONE,
            action: AddCone,
        },
        PaletteEntry {
            label: "Add Torus",
            keywords: "primitive solid torus",
            icon: icons::TORUS,
            action: AddTorus,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on XY",
            keywords: "sketch draw rectangle profile",
            icon: icons::SKETCH,
            action: NewSketchXY,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on YZ",
            keywords: "sketch draw rectangle",
            icon: icons::SKETCH,
            action: NewSketchYZ,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on XZ",
            keywords: "sketch draw rectangle",
            icon: icons::SKETCH,
            action: NewSketchXZ,
        },
        PaletteEntry {
            label: "New Sketch: slot on XY",
            keywords: "sketch draw slot obround stadium hole",
            icon: icons::SKETCH,
            action: NewSketchSlotXY,
        },
        PaletteEntry {
            label: "New Sketch: hexagon on XY",
            keywords: "sketch draw polygon hexagon bolt",
            icon: icons::SKETCH,
            action: NewSketchHexXY,
        },
        PaletteEntry {
            label: "New Sketch: ellipse on XY",
            keywords: "sketch draw ellipse oval conic tilted",
            icon: icons::ELLIPSE,
            action: NewSketchEllipseXY,
        },
        PaletteEntry {
            label: "New datum plane: XY + 20 mm offset",
            keywords: "datum plane offset reference construction",
            icon: icons::DATUM,
            action: NewDatumOffsetXY,
        },
        PaletteEntry {
            label: "New datum plane: XY tilted 30\u{00b0}",
            keywords: "datum plane angle tilt reference",
            icon: icons::DATUM,
            action: NewDatumAngleXY,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on latest datum",
            keywords: "sketch datum carrier rectangle profile",
            icon: icons::SKETCH,
            action: NewSketchOnLatestDatum,
        },
        PaletteEntry {
            label: "Hole: drill latest body at latest sketch points",
            keywords: "hole drill bore counterbore countersink wizard",
            icon: icons::DRILL,
            action: HoleLastBody,
        },
        PaletteEntry {
            label: "Mirror latest body across latest datum",
            keywords: "mirror reflect datum plane symmetry",
            icon: icons::MIRROR,
            action: MirrorLastDatum,
        },
        PaletteEntry {
            label: "Extrude latest sketch (new body)",
            keywords: "extrude boss solid",
            icon: icons::EXTRUDE,
            action: ExtrudeLastSketch,
        },
        PaletteEntry {
            label: "Cut latest body with cylinder",
            keywords: "boolean cut drill hole",
            icon: icons::SUBTRACT,
            action: CutWithCylinder,
        },
        PaletteEntry {
            label: "Union two latest bodies",
            keywords: "boolean union combine",
            icon: icons::UNION,
            action: UnionLastTwo,
        },
        PaletteEntry {
            label: "Subtract two latest bodies",
            keywords: "boolean difference subtract",
            icon: icons::SUBTRACT,
            action: DifferenceLastTwo,
        },
        PaletteEntry {
            label: "Intersect two latest bodies",
            keywords: "boolean intersection",
            icon: icons::INTERSECT,
            action: IntersectLastTwo,
        },
        PaletteEntry {
            label: "Linear pattern latest body (x4)",
            keywords: "pattern array repeat copy linear",
            icon: icons::LINEAR_PATTERN,
            action: LinearPatternLast,
        },
        PaletteEntry {
            label: "Circular pattern latest body (x6)",
            keywords: "pattern array repeat copy circular polar",
            icon: icons::CIRCULAR_PATTERN,
            action: CircularPatternLast,
        },
        PaletteEntry {
            label: "Mirror latest body across YZ",
            keywords: "mirror reflect symmetry plane",
            icon: icons::MIRROR,
            action: MirrorLastYZ,
        },
        PaletteEntry {
            label: "Delete selected feature",
            keywords: "remove feature",
            icon: icons::DELETE,
            action: DeleteSelected,
        },
        PaletteEntry {
            label: "Suppress selected feature",
            keywords: "toggle suppress",
            icon: icons::EYE_OFF,
            action: SuppressSelected,
        },
        PaletteEntry {
            label: "Undo",
            keywords: "history revert",
            icon: icons::UNDO,
            action: Undo,
        },
        PaletteEntry {
            label: "Redo",
            keywords: "history forward",
            icon: icons::REDO,
            action: Redo,
        },
        PaletteEntry {
            label: "Fit view",
            keywords: "zoom frame camera",
            icon: icons::FIT,
            action: FitView,
        },
        PaletteEntry {
            label: "Toggle perspective / orthographic",
            keywords: "projection camera",
            icon: icons::AXIS_3D,
            action: ToggleProjection,
        },
        PaletteEntry {
            label: "Front view",
            keywords: "camera",
            icon: icons::CAMERA,
            action: FrontView,
        },
        PaletteEntry {
            label: "Top view",
            keywords: "camera",
            icon: icons::CAMERA,
            action: TopView,
        },
        PaletteEntry {
            label: "Right view",
            keywords: "camera",
            icon: icons::CAMERA,
            action: RightView,
        },
        PaletteEntry {
            label: "Isometric view",
            keywords: "camera iso",
            icon: icons::CAMERA,
            action: IsoView,
        },
        PaletteEntry {
            label: "Toggle ground grid",
            keywords: "grid",
            icon: icons::GRID,
            action: ToggleGrid,
        },
        PaletteEntry {
            label: "Toggle feature edges",
            keywords: "edges lines",
            icon: icons::EDGES,
            action: ToggleEdges,
        },
        PaletteEntry {
            label: "Save document (.forgecad)",
            keywords: "file save native",
            icon: icons::SAVE,
            action: SaveNative,
        },
        PaletteEntry {
            label: "Export STL",
            keywords: "file export mesh",
            icon: icons::EXPORT,
            action: ExportSTL,
        },
        PaletteEntry {
            label: "Export OBJ",
            keywords: "file export mesh",
            icon: icons::EXPORT,
            action: ExportOBJ,
        },
        PaletteEntry {
            label: "Export glTF",
            keywords: "file export mesh",
            icon: icons::EXPORT,
            action: ExportGLTF,
        },
        PaletteEntry {
            label: "Chamfer selected edges",
            keywords: "modify chamfer edge detail",
            icon: icons::EDGES,
            action: ChamferEdges,
        },
        PaletteEntry {
            label: "Fillet selected edges",
            keywords: "modify fillet round edge detail",
            icon: icons::COMBINE,
            action: FilletEdges,
        },
        PaletteEntry {
            label: "Shell body (open picked faces)",
            keywords: "modify shell hollow wall thickness",
            icon: icons::BODIES,
            action: ShellBody,
        },
        PaletteEntry {
            label: "Export 3MF",
            keywords: "file export mesh 3d print package zip",
            icon: icons::EXPORT,
            action: Export3MF,
        },
        PaletteEntry {
            label: "Import mesh from current directory (stl / obj / 3mf)",
            keywords: "file import mesh stl obj 3mf load",
            icon: icons::IMPORT,
            action: ImportMeshDir,
        },
        PaletteEntry {
            label: "Display: shaded",
            keywords: "view display mode shaded render",
            icon: icons::SHADED,
            action: DisplayShaded,
        },
        PaletteEntry {
            label: "Display: wireframe (hidden line)",
            keywords: "view display mode wireframe hidden line render",
            icon: icons::WIREFRAME,
            action: DisplayWireframe,
        },
        PaletteEntry {
            label: "Display: x-ray (translucent)",
            keywords: "view display mode xray ghost transparent render",
            icon: icons::XRAY,
            action: DisplayXRay,
        },
        PaletteEntry {
            label: "Section view: toggle cut plane",
            keywords: "view section clip cut slice",
            icon: icons::SECTION,
            action: ToggleSection,
        },
        PaletteEntry {
            label: "Measure: two-pick distance + angle",
            keywords: "measure distance angle inspect dimension",
            icon: icons::MEASURE,
            action: ToggleMeasure,
        },
        PaletteEntry {
            label: "Gizmo: move (translate) mode",
            keywords: "gizmo manipulator move translate drag handle",
            icon: icons::MOVE_3D,
            action: GizmoTranslate,
        },
        PaletteEntry {
            label: "Gizmo: rotate mode",
            keywords: "gizmo manipulator rotate spin ring handle",
            icon: icons::ROTATE_3D,
            action: GizmoRotate,
        },
        PaletteEntry {
            label: "Select: faces (sub-body picking)",
            keywords: "select picking face cluster sub-body",
            icon: icons::SKETCH_ON_FACE,
            action: PickFaces,
        },
        PaletteEntry {
            label: "Select: edges (sharp chains)",
            keywords: "select picking edge chain sub-body",
            icon: icons::EDGES,
            action: PickEdges,
        },
        PaletteEntry {
            label: "Select: vertices",
            keywords: "select picking vertex corner sub-body",
            icon: icons::TARGET,
            action: PickVertices,
        },
        PaletteEntry {
            label: "Select: bodies",
            keywords: "select picking whole body solid",
            icon: icons::BODIES,
            action: PickBodies,
        },
        PaletteEntry {
            label: "New sketch on selected face",
            keywords: "sketch face planar profile draw",
            icon: icons::SKETCH_ON_FACE,
            action: SketchOnSelectedFace,
        },
        PaletteEntry {
            label: "Mirror selected sketch entities (about first line)",
            keywords: "sketch mirror symmetric reflect copy s08",
            icon: icons::MIRROR,
            action: MirrorSelectedSketch,
        },
    ]
}

/// Palette actions.
#[derive(Clone, Copy, PartialEq)]
pub enum PaletteAction {
    AddBox,
    AddSphere,
    AddCylinder,
    AddCone,
    AddTorus,
    NewSketchXY,
    NewSketchYZ,
    NewSketchXZ,
    NewSketchSlotXY,
    NewSketchHexXY,
    NewSketchEllipseXY,
    NewDatumOffsetXY,
    NewDatumAngleXY,
    NewSketchOnLatestDatum,
    HoleLastBody,
    MirrorLastDatum,
    ExtrudeLastSketch,
    CutWithCylinder,
    ChamferEdges,
    FilletEdges,
    ShellBody,
    UnionLastTwo,
    DifferenceLastTwo,
    IntersectLastTwo,
    LinearPatternLast,
    CircularPatternLast,
    MirrorLastYZ,
    DeleteSelected,
    SuppressSelected,
    Undo,
    Redo,
    FitView,
    ToggleProjection,
    FrontView,
    TopView,
    RightView,
    IsoView,
    ToggleGrid,
    ToggleEdges,
    SaveNative,
    ExportSTL,
    ExportOBJ,
    ExportGLTF,
    /// I-02: 3MF export.
    Export3MF,
    /// Import a mesh from `import.stl` / `import.obj` in the current
    /// directory (I-01); drag-and-drop is the primary import path.
    ImportMeshDir,
    DisplayShaded,
    DisplayWireframe,
    DisplayXRay,
    ToggleSection,
    ToggleMeasure,
    /// W-01: gizmo translate mode.
    GizmoTranslate,
    /// W-01: gizmo rotate mode.
    GizmoRotate,
    /// W-04: pick whole bodies (default).
    PickBodies,
    /// W-04: pick coplanar face clusters.
    PickFaces,
    /// W-04: pick sharp-edge chains.
    PickEdges,
    /// W-04: pick vertices.
    PickVertices,
    /// W-04: create a sketch on the selected planar face.
    SketchOnSelectedFace,
    /// S-08: mirror the selected sketch's entities about its first line.
    MirrorSelectedSketch,
}

/// Simple subsequence fuzzy match score; `usize::MAX` means no match.
pub fn fuzzy_score(query: &str, label: &str, keywords: &str) -> Option<usize> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let hay = format!("{label} {keywords}").to_lowercase();
    // Subsequence match.
    let mut score = 0usize;
    let mut last = 0usize;
    for c in q.chars() {
        let idx = hay[last..].find(c)?;
        score += idx + 1; // earlier matches score better
        last += idx + 1;
    }
    Some(score)
}

impl PaletteAction {
    /// Execute the action against the app.
    pub fn run(self, app: &mut ForgeApp) {
        use PaletteAction::*;
        match self {
            AddBox => app.add_primitive(PrimitiveKind::Box, Vector3::new(40.0, 30.0, 20.0)),
            AddSphere => app.add_primitive(PrimitiveKind::Sphere, Vector3::new(20.0, 0.0, 0.0)),
            AddCylinder => {
                app.add_primitive(PrimitiveKind::Cylinder, Vector3::new(10.0, 30.0, 0.0))
            }
            AddCone => app.add_primitive(PrimitiveKind::Cone, Vector3::new(12.0, 6.0, 25.0)),
            AddTorus => app.add_primitive(PrimitiveKind::Torus, Vector3::new(20.0, 6.0, 0.0)),

            NewSketchXY => app.add_sketch_rect(DatumPlane::XY),
            NewSketchYZ => app.add_sketch_rect(DatumPlane::YZ),
            NewSketchXZ => app.add_sketch_rect(DatumPlane::XZ),
            NewSketchSlotXY => app.add_sketch_slot(DatumPlane::XY),
            NewSketchHexXY => app.add_sketch_polygon(DatumPlane::XY, 6),
            NewSketchEllipseXY => app.add_sketch_ellipse(DatumPlane::XY),

            NewDatumOffsetXY => app.add_datum(DatumParams::Offset {
                base: DatumPlane::XY,
                offset: 20.0,
            }),
            NewDatumAngleXY => app.add_datum(DatumParams::Angle {
                base: DatumPlane::XY,
                axis: 0,
                angle: 30_f64.to_radians(),
            }),
            NewSketchOnLatestDatum => app.add_sketch_on_latest_datum(),
            HoleLastBody => app.hole_last_body(),
            MirrorLastDatum => app.mirror_last_datum(),

            ExtrudeLastSketch => app.extrude_last_sketch(),
            CutWithCylinder => app.cut_with_cylinder(),
            ChamferEdges => app.apply_edge_detail(DetailKind::Chamfer),
            FilletEdges => app.apply_edge_detail(DetailKind::Fillet),
            ShellBody => app.apply_shell(),
            UnionLastTwo => app.boolean_last_two(forge_geometry::CsgOp::Union),
            DifferenceLastTwo => app.boolean_last_two(forge_geometry::CsgOp::Difference),
            IntersectLastTwo => app.boolean_last_two(forge_geometry::CsgOp::Intersection),

            LinearPatternLast => app.linear_pattern_last(),
            CircularPatternLast => app.circular_pattern_last(),
            MirrorLastYZ => app.mirror_last_yz(),

            DeleteSelected => app.delete_selected(),
            SuppressSelected => app.toggle_suppress_selected(),

            Undo => {
                if let Ok(desc) = app.commands.undo(&mut app.doc) {
                    app.set_status(format!("Undo: {desc}"));
                    app.request_evaluation();
                }
            }
            Redo => {
                if let Ok(desc) = app.commands.redo(&mut app.doc) {
                    app.set_status(format!("Redo: {desc}"));
                    app.request_evaluation();
                }
            }

            FitView => app.camera.fit_to(&app.scene_bounds()),
            ToggleProjection => app.camera.orthographic = !app.camera.orthographic,
            FrontView => {
                app.camera.yaw = std::f64::consts::FRAC_PI_2;
                app.camera.pitch = 0.0;
            }
            TopView => {
                app.camera.pitch = 1.5;
            }
            RightView => {
                app.camera.yaw = 0.0;
                app.camera.pitch = 0.0;
            }
            IsoView => {
                app.camera.yaw = -0.6;
                app.camera.pitch = 0.45;
            }

            ToggleGrid => app.render_options.show_grid = !app.render_options.show_grid,
            ToggleEdges => {
                app.render_options.show_edges = !app.render_options.show_edges;
                app.scene.show_edges = app.render_options.show_edges;
                app.scene.version += 1;
            }

            SaveNative => app.save_native_dialog(),
            ExportSTL => app.export_mesh(forge_io::ExportFormat::Stl),
            ExportOBJ => app.export_mesh(forge_io::ExportFormat::Obj),
            ExportGLTF => app.export_mesh(forge_io::ExportFormat::Gltf),
            Export3MF => app.export_mesh(forge_io::ExportFormat::ThreeMf),

            ImportMeshDir => {
                // wasm: there is no current directory — drag-and-drop is
                // the import path (W-10).
                #[cfg(target_arch = "wasm32")]
                {
                    // No filesystem on wasm — the arm ends here (the
                    // native branch below is cfg'd out).
                    app.set_status("Drag a .stl / .obj / .3mf file onto the window to import it");
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let candidates = [
                        cwd.join("import.stl"),
                        cwd.join("import.obj"),
                        cwd.join("import.3mf"),
                    ];
                    let Some(path) = candidates.iter().find(|p| p.is_file()) else {
                        app.set_status(
                            "No import.stl / import.obj / import.3mf in the current \
                             directory — or drag a file onto the window",
                        );
                        return;
                    };
                    app.import_file(path.clone());
                }
            }

            DisplayShaded => app.set_display_mode(forge_render::DisplayMode::Shaded),
            DisplayWireframe => app.set_display_mode(forge_render::DisplayMode::Wireframe),
            DisplayXRay => app.set_display_mode(forge_render::DisplayMode::XRay),

            GizmoTranslate => {
                app.gizmo_mode = crate::gizmo::GizmoMode::Translate;
                app.set_status(
                    "Gizmo: translate (T) — select a body, drag an axis arrow or a plane",
                );
            }
            GizmoRotate => {
                app.gizmo_mode = crate::gizmo::GizmoMode::Rotate;
                app.set_status(
                    "Gizmo: rotate (R) — select a body, drag a ring (5\u{00b0} snap, Shift = off)",
                );
            }

            PickBodies => {
                app.pick_mode = crate::picking::PickMode::Bodies;
                app.selection.clear();
                app.set_status("Picking bodies (whole solids)");
            }
            PickFaces => {
                app.pick_mode = crate::picking::PickMode::Faces;
                app.selection.clear();
                app.set_status("Picking faces — click a surface to select its coplanar patch");
            }
            PickEdges => {
                app.pick_mode = crate::picking::PickMode::Edges;
                app.selection.clear();
                app.set_status("Picking edges — click near a sharp edge (tangent chains)");
            }
            PickVertices => {
                app.pick_mode = crate::picking::PickMode::Vertices;
                app.selection.clear();
                app.set_status("Picking vertices — click near a corner");
            }
            SketchOnSelectedFace => app.add_sketch_on_selected_face(),
            MirrorSelectedSketch => app.mirror_selected_sketch(),

            ToggleSection => {
                let enabled = app.render_options.section.is_some();
                if enabled {
                    app.render_options.section = None;
                    app.set_status("Section view off");
                } else {
                    app.render_options.section = Some(forge_render::SectionPlane {
                        normal: [1.0, 0.0, 0.0],
                        offset: 0.0,
                    });
                    app.set_status("Section view on — adjust the plane in the viewport toolbar");
                }
            }

            ToggleMeasure => {
                app.measure_mode = !app.measure_mode;
                app.measure_picks.clear();
                app.measure_label = None;
                app.set_status(if app.measure_mode {
                    "Measure: click two surface points in the viewport"
                } else {
                    "Measure tool off"
                });
            }
        }
    }
}

impl ForgeApp {
    /// Add a primitive feature (recorded in the undo stack).
    pub(crate) fn add_primitive(&mut self, kind: PrimitiveKind, dims: Vector3) {
        let center = Point3::origin();
        let feature = Feature::Primitive(PrimitiveParams { kind, center, dims });
        match self.doc.add_feature(feature.clone()) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status(format!("Added {kind}"));
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Switch the viewport display mode (W-05). Wireframe pulls *all*
    /// feature edges (angle threshold 0 = every non-coplanar edge + all
    /// boundary edges) instead of just sharp ones, and forces the edge
    /// overlay on; leaving wireframe restores the 40° default.
    pub(crate) fn set_display_mode(&mut self, mode: forge_render::DisplayMode) {
        let wire = mode == forge_render::DisplayMode::Wireframe;
        self.render_options.display_mode = mode;
        if self.scene.show_edges != (wire || self.render_options.show_edges) {
            self.render_options.show_edges = wire || self.render_options.show_edges;
            self.scene.show_edges = self.render_options.show_edges;
        }
        self.scene.edge_angle_deg = if wire { 0.0 } else { 40.0 };
        self.scene.touch(); // rebuild line buffers for the new threshold
        self.set_status(format!("Display mode: {mode:?}"));
    }

    /// Add a sketch with a parametric rectangle on a datum plane.
    pub(crate) fn add_sketch_rect(&mut self, datum: DatumPlane) {
        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(sketch_id, "profile", SketchPlane::Datum { datum });
        sketch.add_rectangle(Point2::new(-15.0, -10.0), Point2::new(15.0, 10.0));
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status(format!("Sketch created on {datum:?}"));
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Add a sketch with a parametric slot on a datum plane.
    pub(crate) fn add_sketch_slot(&mut self, datum: DatumPlane) {
        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(sketch_id, "slot", SketchPlane::Datum { datum });
        if let Err(e) = sketch.add_slot(Point2::new(-12.0, 0.0), Point2::new(12.0, 0.0), 5.0) {
            self.set_status(format!("{e}"));
            return;
        }
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status("Slot sketch created (tangent, fully parametric)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Add a sketch with a native ellipse (S-03) on a datum plane.
    pub(crate) fn add_sketch_ellipse(&mut self, datum: DatumPlane) {
        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(sketch_id, "ellipse", SketchPlane::Datum { datum });
        sketch.add_ellipse(Point2::origin(), 15.0, 8.0, 30_f64.to_radians());
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status("Ellipse sketch created (5 DOF — constrain as needed)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Add a sketch with a regular polygon on a datum plane.
    pub(crate) fn add_sketch_polygon(&mut self, datum: DatumPlane, sides: usize) {
        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(sketch_id, "polygon", SketchPlane::Datum { datum });
        if let Err(e) = sketch.add_polygon(Point2::origin(), 15.0, sides, 0.0) {
            self.set_status(format!("{e}"));
            return;
        }
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status(format!("{sides}-gon sketch created (equal sides + angles)"));
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// The most recent body-producing feature (pattern/mirror seed).
    fn last_body_feature(&self) -> Option<FeatureId> {
        self.doc
            .tree
            .order()
            .iter()
            .rev()
            .find(|id| {
                matches!(
                    self.doc.feature(**id),
                    Some(Feature::Primitive(_))
                        | Some(Feature::Extrude(_))
                        | Some(Feature::Revolve(_))
                        | Some(Feature::Loft(_))
                        | Some(Feature::Sweep(_))
                        | Some(Feature::Boolean(_))
                        | Some(Feature::TransformBody { .. })
                        | Some(Feature::LinearPattern(_))
                        | Some(Feature::CircularPattern(_))
                        | Some(Feature::Mirror(_))
                )
            })
            .copied()
    }

    /// Linear pattern of the latest body: 4 instances along +X.
    pub(crate) fn linear_pattern_last(&mut self) {
        let Some(source) = self.last_body_feature() else {
            self.set_status("No body to pattern: add a solid first");
            return;
        };
        let feature = Feature::LinearPattern(LinearPatternParams {
            source,
            direction: Vector3::x(),
            count: 4,
            spacing: 50.0,
            symmetric: false,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Linear pattern added (edit count/spacing in the inspector)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Circular pattern of the latest body: 6 instances around Z.
    pub(crate) fn circular_pattern_last(&mut self) {
        let Some(source) = self.last_body_feature() else {
            self.set_status("No body to pattern: add a solid first");
            return;
        };
        // Put the rotation axis through the world origin so off-origin
        // bodies form a ring.
        let feature = Feature::CircularPattern(CircularPatternParams {
            source,
            axis_point: Point3::origin(),
            axis_dir: Vector3::z(),
            count: 6,
            angle: std::f64::consts::TAU,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Circular pattern added (edit count/angle in the inspector)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Mirror the latest body across the YZ plane (normal +X).
    pub(crate) fn mirror_last_yz(&mut self) {
        let Some(source) = self.last_body_feature() else {
            self.set_status("No body to mirror: add a solid first");
            return;
        };
        let feature = Feature::Mirror(MirrorParams {
            source,
            plane_point: Point3::origin(),
            plane_normal: Vector3::x(),
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Mirror added across the YZ plane");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Add a datum plane feature (D-01).
    pub(crate) fn add_datum(&mut self, params: DatumParams) {
        let feature = Feature::Datum(params);
        match self.doc.add_feature(feature.clone()) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status("Datum plane added (edit offset/tilt in the inspector)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// The most recent datum feature id.
    fn last_datum(&self) -> Option<FeatureId> {
        self.doc
            .tree
            .order()
            .iter()
            .rev()
            .find(|id| matches!(self.doc.feature(**id), Some(Feature::Datum(_))))
            .copied()
    }

    /// New sketch carried by the latest datum plane (D-01).
    pub(crate) fn add_sketch_on_latest_datum(&mut self) {
        let Some(datum) = self.last_datum() else {
            self.set_status("No datum plane: create one first (palette: New datum plane)");
            return;
        };
        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(
            sketch_id,
            "profile",
            SketchPlane::DatumRef { feature: datum },
        );
        sketch.add_rectangle(Point2::new(-15.0, -10.0), Point2::new(15.0, 10.0));
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status("Sketch created on the datum plane");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// New sketch carried by the selected planar face (W-04): the face
    /// cluster's plane (centroid + outward normal) carries a rectangle
    /// profile, ready for extrude/cut directly on the face.
    pub(crate) fn add_sketch_on_selected_face(&mut self) {
        let Some((body, face)) = self.selection.single_face() else {
            self.set_status("Select a planar face first (pick mode: Faces)");
            return;
        };
        // Resolve the face cluster from the last evaluation.
        let plane = self.last_evaluation.as_ref().and_then(|ev| {
            let b = ev.bodies.iter().find(|b| b.id == body)?;
            let cluster = b
                .mesh
                .face_cluster(face.raw() as usize, 1.0_f64.to_radians());
            let seed = cluster.first()?;
            let normal = b.mesh.triangle_normal(*seed)?;
            // Centroid of the cluster vertices.
            let mut centroid = forge_core::Vector3::zeros();
            let mut count = 0usize;
            for t in &cluster {
                for v in b.mesh.triangle_idx(*t) {
                    centroid += b.mesh.positions[v as usize].coords;
                    count += 1;
                }
            }
            let origin = Point3::from(centroid / count.max(1) as f64);
            forge_core::Plane::new(origin, normal)
        });
        let Some(plane) = plane else {
            self.set_status("Selected face is no longer valid (re-evaluate and re-pick)");
            return;
        };

        // Face extent: the rectangle is sized from the *cluster* bbox
        // (40% of the smallest extent, min 1 mm) so it stays on the face.
        let half = {
            let ev = self.last_evaluation.as_ref();
            let b = ev
                .and_then(|ev| ev.bodies.iter().find(|b| b.id == body))
                .expect("body re-checked above");
            let cluster = b
                .mesh
                .face_cluster(face.raw() as usize, 1.0_f64.to_radians());
            let mut bb_min = forge_core::Vector3::repeat(f64::INFINITY);
            let mut bb_max = forge_core::Vector3::repeat(f64::NEG_INFINITY);
            for t in &cluster {
                for v in b.mesh.triangle_idx(*t) {
                    let p = &b.mesh.positions[v as usize];
                    bb_min = bb_min.inf(&p.coords);
                    bb_max = bb_max.sup(&p.coords);
                }
            }
            let extents = bb_max - bb_min;
            (0.4 * extents.iter().copied().fold(f64::INFINITY, f64::min)).max(1.0)
        };

        let sketch_id = SketchId::new(self.doc.allocator.next_id());
        let mut sketch = Sketch::new(
            sketch_id,
            "face profile",
            SketchPlane::Face { plane, body, face },
        );
        sketch.add_rectangle(
            Point2::new(-half, -half * 0.66),
            Point2::new(half, half * 0.66),
        );
        match self.doc.add_feature(Feature::Sketch(sketch.clone())) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(Command::AddFeature { node }, &mut self.doc);
                }
                self.set_status("Sketch created on the selected face — extrude or cut from it");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// S-08: mirror the selected sketch's entities about its first line
    /// entity. The inspector offers per-line choice; this is the quick
    /// palette path.
    pub(crate) fn mirror_selected_sketch(&mut self) {
        let Some(id) = self.selection.primary_feature() else {
            self.set_status("Select a sketch feature first");
            return;
        };
        let Some(Feature::Sketch(sketch)) = self.doc.feature(id).cloned() else {
            self.set_status("Selected feature is not a sketch");
            return;
        };
        let Some(mirror) = sketch
            .entities
            .values()
            .find(|e| matches!(e, forge_sketch::SketchEntity::Line { .. }))
            .map(|e| e.id())
        else {
            self.set_status("Sketch has no line to mirror across");
            return;
        };
        let others: Vec<forge_core::EntityId> = sketch
            .entities
            .keys()
            .filter(|&&e| e != mirror)
            .copied()
            .collect();
        let mut target = sketch.clone();
        match target.mirror_entities(&others, mirror) {
            Ok(created) => {
                let report = target.solve().ok();
                if let Some(r) = &report {
                    self.last_sketch_report = Some(r.clone());
                }
                if self
                    .doc
                    .edit_feature(id, Feature::Sketch(target.clone()))
                    .is_ok()
                {
                    let _ = self.commands.execute(
                        Command::EditFeature {
                            id,
                            before: Box::new(Feature::Sketch(sketch)),
                            after: Box::new(Feature::Sketch(target)),
                        },
                        &mut self.doc,
                    );
                    self.request_evaluation();
                    self.set_status(format!(
                        "Mirrored {} entities about line {mirror}{}",
                        created.len(),
                        report
                            .filter(|r| !r.is_solved())
                            .map(|r| format!(" (solver: {})", r.status))
                            .unwrap_or_default()
                    ));
                }
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Hole wizard (F-04): drill the latest body at the points/circles of
    /// the latest sketch. If that sketch has no points, one is placed at
    /// the origin.
    pub(crate) fn hole_last_body(&mut self) {
        let target = self.last_body_feature();
        let profile = self
            .doc
            .tree
            .order()
            .iter()
            .rev()
            .find(|id| matches!(self.doc.feature(**id), Some(Feature::Sketch(_))))
            .copied();
        let (Some(target), Some(profile)) = (target, profile) else {
            self.set_status("Holes need a body and a sketch: add a solid and a sketch first");
            return;
        };
        // Ensure the placement sketch has at least one placement point.
        if let Some(sketch) = self.doc.sketch_mut(profile) {
            let has_placement = sketch.entities.values().any(|e| {
                matches!(
                    e,
                    forge_sketch::SketchEntity::Point { .. }
                        | forge_sketch::SketchEntity::Circle { .. }
                        | forge_sketch::SketchEntity::Arc { .. }
                )
            });
            if !has_placement {
                sketch.add_point(Point2::origin());
            }
        }
        let feature = Feature::Hole(HoleParams {
            profile,
            kind: HoleKind::Simple,
            diameter: 6.0,
            depth: 12.0,
            direction: ExtrudeDirection::Positive,
            counterbore_diameter: 11.0,
            counterbore_depth: 4.0,
            countersink_diameter: 10.0,
            countersink_angle: 90_f64.to_radians(),
            drill_point: false,
            drill_angle: 118_f64.to_radians(),
            target,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Hole added (edit diameter/depth/type in the inspector)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Mirror the latest body across the latest datum plane (D-01).
    pub(crate) fn mirror_last_datum(&mut self) {
        let Some(datum) = self.last_datum() else {
            self.set_status("No datum plane to mirror across: create one first");
            return;
        };
        let Some(source) = self.last_body_feature() else {
            self.set_status("No body to mirror: add a solid first");
            return;
        };
        let plane = match self.doc.feature(datum) {
            Some(Feature::Datum(d)) => d.to_plane(),
            _ => {
                self.set_status("datum feature is not a plane");
                return;
            }
        };
        let n: Vector3 = *plane.normal.as_ref();
        let feature = Feature::Mirror(MirrorParams {
            source,
            plane_point: plane.origin,
            plane_normal: n,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Mirror added across the datum plane");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Extrude the most recent sketch feature as a new body.
    pub(crate) fn extrude_last_sketch(&mut self) {
        let sketch_id = self
            .doc
            .tree
            .order()
            .iter()
            .rev()
            .find(|id| matches!(self.doc.feature(**id), Some(Feature::Sketch(_))))
            .copied();
        let Some(sketch_id) = sketch_id else {
            self.set_status("No sketch found: create one first (palette: New Sketch)");
            return;
        };
        let feature = Feature::Extrude(ExtrudeParams {
            profile: sketch_id,
            distance: 10.0,
            direction: ExtrudeDirection::Positive,
            operation: ExtrudeOp::New,
            target: FeatureId::NONE,
            draft_angle: 0.0,
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Extrude added (edit distance in the inspector)");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Drill a hole through the latest body with a boolean cylinder cut.
    pub(crate) fn cut_with_cylinder(&mut self) {
        let body = self
            .doc
            .tree
            .order()
            .iter()
            .rev()
            .find(|id| {
                matches!(
                    self.doc.feature(**id),
                    Some(Feature::Primitive(_))
                        | Some(Feature::Extrude(_))
                        | Some(Feature::Boolean(_))
                )
            })
            .copied();
        let Some(target) = body else {
            self.set_status("No body to cut: add a solid first");
            return;
        };
        let bb = self
            .last_evaluation
            .as_ref()
            .and_then(|ev| ev.bodies.iter().find(|b| b.source == target))
            .map(|b| b.mesh.bbox());
        let height = bb.map(|b| b.size().z * 3.0 + 20.0).unwrap_or(60.0);
        let base = bb.map(|b| b.min.z - 10.0).unwrap_or(-10.0);

        // Cylinder primitive…
        let cyl = PrimitiveParams {
            kind: PrimitiveKind::Cylinder,
            center: Point3::new(0.0, 0.0, base),
            dims: Vector3::new(6.0, height, 0.0),
        };
        let cyl_id = match self.doc.add_feature(Feature::Primitive(cyl)) {
            Ok(id) => id,
            Err(e) => {
                self.set_status(format!("{e}"));
                return;
            }
        };
        // …combined as a difference.
        let feature = Feature::Boolean(BooleanFeature {
            op: forge_geometry::CsgOp::Difference,
            operands: vec![target, cyl_id],
        });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status("Cylinder cut added");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Combine the two latest solid bodies with a boolean op.
    pub(crate) fn boolean_last_two(&mut self, op: forge_geometry::CsgOp) {
        let bodies: Vec<FeatureId> = self
            .doc
            .tree
            .order()
            .iter()
            .rev()
            .filter(|id| {
                matches!(
                    self.doc.feature(**id),
                    Some(Feature::Primitive(_))
                        | Some(Feature::Extrude(_))
                        | Some(Feature::Boolean(_))
                )
            })
            .copied()
            .take(2)
            .collect();
        if bodies.len() < 2 {
            self.set_status("Need two solid bodies for a boolean");
            return;
        }
        let mut operands = bodies;
        operands.reverse(); // chronological order
        let feature = Feature::Boolean(BooleanFeature { op, operands });
        match self.doc.add_feature(feature) {
            Ok(_id) => {
                self.set_status(format!("Boolean {op} added"));
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    pub(crate) fn delete_selected(&mut self) {
        let Some(id) = self.selection.primary_feature() else {
            self.set_status("Nothing selected");
            return;
        };
        let index = self
            .doc
            .tree
            .order()
            .iter()
            .position(|i| *i == id)
            .unwrap_or(0);
        match self.doc.remove_feature(id) {
            Ok(feature) => {
                let node = forge_model::FeatureNode {
                    id,
                    feature,
                    parents: Default::default(),
                    children: Default::default(),
                    suppressed: false,
                    dirty: true,
                };
                let _ = self
                    .commands
                    .execute(Command::RemoveFeature { node, index }, &mut self.doc);
                self.selection.clear();
                self.set_status("Feature deleted");
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    pub(crate) fn toggle_suppress_selected(&mut self) {
        let Some(id) = self.selection.primary_feature() else {
            self.set_status("Nothing selected");
            return;
        };
        let before = self.doc.tree.get(id).map(|n| n.suppressed).unwrap_or(false);
        let after = !before;
        if self.doc.tree.set_suppressed(id, after).is_ok() {
            let _ = self
                .commands
                .execute(Command::SetSuppressed { id, before, after }, &mut self.doc);
            self.set_status(if after {
                "Feature suppressed"
            } else {
                "Feature unsuppressed"
            });
            self.request_evaluation();
        }
    }

    /// Bounds of everything in the scene (for "fit view").
    pub(crate) fn scene_bounds(&self) -> forge_core::BBox3 {
        let mut bb = forge_core::BBox3::default();
        if let Some(ev) = &self.last_evaluation {
            for b in &ev.bodies {
                bb = bb.union(&b.mesh.bbox());
            }
        }
        bb
    }

    /// Default body colors (selection highlight, Scene::SELECTED).
    pub(crate) fn body_color(&self, source: forge_core::FeatureId) -> [f32; 4] {
        if self
            .selection
            .bodies()
            .iter()
            .any(|b| b.raw() == source.raw())
        {
            Scene::SELECTED
        } else {
            Scene::DEFAULT
        }
    }
}

/// Which edge-detail feature to apply (K-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailKind {
    Chamfer,
    Fillet,
}

impl ForgeApp {
    /// Create a chamfer/fillet feature from the current *edge* selection
    /// (K-03): resolve each selected edge chain back to geometry
    /// (EdgeSpec survives re-tessellation) and add the feature.
    pub(crate) fn apply_edge_detail(&mut self, kind: DetailKind) {
        use crate::picking::{CHAIN_TOL, EDGE_TOL};
        use forge_geometry::detail::EdgeSpec;
        use forge_model::SelectionItem;
        use forge_model::{ChamferParams, Feature, FilletParams};

        // Collect edge selections grouped by body.
        let mut edge_ids: Vec<(forge_core::BodyId, forge_core::EdgeId)> = Vec::new();
        for item in &self.selection.items {
            if let SelectionItem::Edge { body, edge } = item {
                edge_ids.push((*body, *edge));
            }
        }
        if edge_ids.is_empty() {
            self.set_status("Pick edges first (viewport pick mode: Edges)");
            return;
        }
        let Some(ev) = self.last_evaluation.as_ref() else {
            self.set_status("Evaluate a model first");
            return;
        };

        // Resolve each selected chain to EdgeSpecs.
        let mut by_target: std::collections::BTreeMap<forge_core::FeatureId, Vec<EdgeSpec>> =
            Default::default();
        for (body_id, edge_id) in edge_ids {
            let Some(body) = ev.bodies.iter().find(|b| b.id == body_id) else {
                continue;
            };
            // chain_id = min vertex index of the chain (picking.rs).
            let chains = body.mesh.sharp_edge_chains(EDGE_TOL, CHAIN_TOL);
            for chain in &chains {
                let cid = chain
                    .iter()
                    .flat_map(|e| e.iter())
                    .copied()
                    .map(u64::from)
                    .min()
                    .unwrap_or(u64::MAX);
                if cid != edge_id.raw() {
                    continue;
                }
                for [a, b] in chain {
                    let pa = body.mesh.positions[*a as usize];
                    let pb = body.mesh.positions[*b as usize];
                    by_target
                        .entry(body.source)
                        .or_default()
                        .push(EdgeSpec { a: pa, b: pb });
                }
            }
        }
        if by_target.is_empty() {
            self.set_status("Selected edges no longer exist (re-pick after edit)");
            return;
        }

        let mut added = 0;
        for (target, edges) in by_target {
            let feature = match kind {
                DetailKind::Chamfer => Feature::Chamfer(ChamferParams {
                    target,
                    edges,
                    distance: 1.0,
                }),
                DetailKind::Fillet => Feature::Fillet(FilletParams {
                    target,
                    edges,
                    radius: 1.0,
                    segments: 16,
                }),
            };
            match self.doc.add_feature(feature) {
                Ok(id) => {
                    if let Some(node) = self.doc.tree.get(id).cloned() {
                        let _ = self
                            .commands
                            .execute(forge_model::Command::AddFeature { node }, &mut self.doc);
                    }
                    added += 1;
                }
                Err(e) => {
                    self.set_status(format!("{e}"));
                    return;
                }
            }
        }
        self.selection.clear();
        self.set_status(format!(
            "{} applied to selected edges — tune it in the inspector",
            match kind {
                DetailKind::Chamfer => "Chamfer",
                DetailKind::Fillet => "Fillet",
            }
        ));
        let _ = added;
        self.request_evaluation();
    }
}

impl ForgeApp {
    /// Create a shell feature (F-05): hollow the latest body, opening
    /// the picked faces (plane snapshots from the current selection).
    pub(crate) fn apply_shell(&mut self) {
        use forge_geometry::FacePlane;
        use forge_model::{Feature, ShellParams};

        // Target: the body of the (single) selection, or the latest body.
        let ev = match self.last_evaluation.as_ref() {
            Some(ev) if !ev.bodies.is_empty() => ev.clone(),
            _ => {
                self.set_status("No body to shell: add a solid first");
                return;
            }
        };
        let target = self
            .selection
            .single_face()
            .and_then(|(bid, _)| ev.bodies.iter().find(|b| b.id == bid).map(|b| b.source));
        let target = target.unwrap_or_else(|| {
            // Latest solid-producing feature.
            self.doc
                .tree
                .order()
                .iter()
                .rev()
                .find(|id| {
                    matches!(
                        self.doc.feature(**id),
                        Some(Feature::Primitive(_))
                            | Some(Feature::Extrude(_))
                            | Some(Feature::Boolean(_))
                            | Some(Feature::Chamfer(_))
                            | Some(Feature::Fillet(_))
                    )
                })
                .copied()
                .unwrap_or(ev.bodies[0].source)
        });
        let Some(body) = ev.bodies.iter().find(|b| b.source == target) else {
            self.set_status("Shell target has no body");
            return;
        };

        // Open faces: every picked face of this body → plane snapshot.
        let mut open = Vec::new();
        for item in &self.selection.items {
            if let forge_model::SelectionItem::Face { body: bid, face } = item {
                if *bid == body.id {
                    let seed = face.raw() as usize;
                    let cluster = body.mesh.face_cluster(seed, 2.0);
                    if let Some(&tri) = cluster.first() {
                        if let Some(n) = body.mesh.triangle_normal(tri) {
                            let p = body.mesh.positions[body.mesh.triangle_idx(tri)[0] as usize];
                            open.push(FacePlane {
                                point: p,
                                normal: n,
                            });
                        }
                    }
                }
            }
        }
        let open_info = if open.is_empty() {
            "closed hollow".to_string()
        } else {
            format!("{} open face(s)", open.len())
        };

        let feature = Feature::Shell(ShellParams {
            target,
            thickness: 1.5,
            open,
        });
        match self.doc.add_feature(feature) {
            Ok(id) => {
                if let Some(node) = self.doc.tree.get(id).cloned() {
                    let _ = self
                        .commands
                        .execute(forge_model::Command::AddFeature { node }, &mut self.doc);
                }
                self.selection.clear();
                self.set_status(format!(
                    "Shell created ({open_info}) — tune thickness in the inspector"
                ));
                self.request_evaluation();
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }
}
