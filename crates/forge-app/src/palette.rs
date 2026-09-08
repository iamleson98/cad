//! Command palette (FR-UI-01) and palette actions.

use crate::app::ForgeApp;
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
            action: AddBox,
        },
        PaletteEntry {
            label: "Add Sphere",
            keywords: "primitive solid sphere",
            action: AddSphere,
        },
        PaletteEntry {
            label: "Add Cylinder",
            keywords: "primitive solid cylinder",
            action: AddCylinder,
        },
        PaletteEntry {
            label: "Add Cone",
            keywords: "primitive solid cone",
            action: AddCone,
        },
        PaletteEntry {
            label: "Add Torus",
            keywords: "primitive solid torus",
            action: AddTorus,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on XY",
            keywords: "sketch draw rectangle profile",
            action: NewSketchXY,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on YZ",
            keywords: "sketch draw rectangle",
            action: NewSketchYZ,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on XZ",
            keywords: "sketch draw rectangle",
            action: NewSketchXZ,
        },
        PaletteEntry {
            label: "New Sketch: slot on XY",
            keywords: "sketch draw slot obround stadium hole",
            action: NewSketchSlotXY,
        },
        PaletteEntry {
            label: "New Sketch: hexagon on XY",
            keywords: "sketch draw polygon hexagon bolt",
            action: NewSketchHexXY,
        },
        PaletteEntry {
            label: "New datum plane: XY + 20 mm offset",
            keywords: "datum plane offset reference construction",
            action: NewDatumOffsetXY,
        },
        PaletteEntry {
            label: "New datum plane: XY tilted 30\u{00b0}",
            keywords: "datum plane angle tilt reference",
            action: NewDatumAngleXY,
        },
        PaletteEntry {
            label: "New Sketch: rectangle on latest datum",
            keywords: "sketch datum carrier rectangle profile",
            action: NewSketchOnLatestDatum,
        },
        PaletteEntry {
            label: "Hole: drill latest body at latest sketch points",
            keywords: "hole drill bore counterbore countersink wizard",
            action: HoleLastBody,
        },
        PaletteEntry {
            label: "Mirror latest body across latest datum",
            keywords: "mirror reflect datum plane symmetry",
            action: MirrorLastDatum,
        },
        PaletteEntry {
            label: "Extrude latest sketch (new body)",
            keywords: "extrude boss solid",
            action: ExtrudeLastSketch,
        },
        PaletteEntry {
            label: "Cut latest body with cylinder",
            keywords: "boolean cut drill hole",
            action: CutWithCylinder,
        },
        PaletteEntry {
            label: "Union two latest bodies",
            keywords: "boolean union combine",
            action: UnionLastTwo,
        },
        PaletteEntry {
            label: "Subtract two latest bodies",
            keywords: "boolean difference subtract",
            action: DifferenceLastTwo,
        },
        PaletteEntry {
            label: "Intersect two latest bodies",
            keywords: "boolean intersection",
            action: IntersectLastTwo,
        },
        PaletteEntry {
            label: "Linear pattern latest body (x4)",
            keywords: "pattern array repeat copy linear",
            action: LinearPatternLast,
        },
        PaletteEntry {
            label: "Circular pattern latest body (x6)",
            keywords: "pattern array repeat copy circular polar",
            action: CircularPatternLast,
        },
        PaletteEntry {
            label: "Mirror latest body across YZ",
            keywords: "mirror reflect symmetry plane",
            action: MirrorLastYZ,
        },
        PaletteEntry {
            label: "Delete selected feature",
            keywords: "remove feature",
            action: DeleteSelected,
        },
        PaletteEntry {
            label: "Suppress selected feature",
            keywords: "toggle suppress",
            action: SuppressSelected,
        },
        PaletteEntry {
            label: "Undo",
            keywords: "history revert",
            action: Undo,
        },
        PaletteEntry {
            label: "Redo",
            keywords: "history forward",
            action: Redo,
        },
        PaletteEntry {
            label: "Fit view",
            keywords: "zoom frame camera",
            action: FitView,
        },
        PaletteEntry {
            label: "Toggle perspective / orthographic",
            keywords: "projection camera",
            action: ToggleProjection,
        },
        PaletteEntry {
            label: "Front view",
            keywords: "camera",
            action: FrontView,
        },
        PaletteEntry {
            label: "Top view",
            keywords: "camera",
            action: TopView,
        },
        PaletteEntry {
            label: "Right view",
            keywords: "camera",
            action: RightView,
        },
        PaletteEntry {
            label: "Isometric view",
            keywords: "camera iso",
            action: IsoView,
        },
        PaletteEntry {
            label: "Toggle ground grid",
            keywords: "grid",
            action: ToggleGrid,
        },
        PaletteEntry {
            label: "Toggle feature edges",
            keywords: "edges lines",
            action: ToggleEdges,
        },
        PaletteEntry {
            label: "Save document (.forgecad)",
            keywords: "file save native",
            action: SaveNative,
        },
        PaletteEntry {
            label: "Export STL",
            keywords: "file export mesh",
            action: ExportSTL,
        },
        PaletteEntry {
            label: "Export OBJ",
            keywords: "file export mesh",
            action: ExportOBJ,
        },
        PaletteEntry {
            label: "Export glTF",
            keywords: "file export mesh",
            action: ExportGLTF,
        },
        PaletteEntry {
            label: "Import mesh from current directory (import.stl / import.obj)",
            keywords: "file import mesh stl obj load",
            action: ImportMeshDir,
        },
        PaletteEntry {
            label: "Display: shaded",
            keywords: "view display mode shaded render",
            action: DisplayShaded,
        },
        PaletteEntry {
            label: "Display: wireframe (hidden line)",
            keywords: "view display mode wireframe hidden line render",
            action: DisplayWireframe,
        },
        PaletteEntry {
            label: "Display: x-ray (translucent)",
            keywords: "view display mode xray ghost transparent render",
            action: DisplayXRay,
        },
        PaletteEntry {
            label: "Section view: toggle cut plane",
            keywords: "view section clip cut slice",
            action: ToggleSection,
        },
        PaletteEntry {
            label: "Measure: two-pick distance + angle",
            keywords: "measure distance angle inspect dimension",
            action: ToggleMeasure,
        },
        PaletteEntry {
            label: "Gizmo: move (translate) mode",
            keywords: "gizmo manipulator move translate drag handle",
            action: GizmoTranslate,
        },
        PaletteEntry {
            label: "Gizmo: rotate mode",
            keywords: "gizmo manipulator rotate spin ring handle",
            action: GizmoRotate,
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
    NewDatumOffsetXY,
    NewDatumAngleXY,
    NewSketchOnLatestDatum,
    HoleLastBody,
    MirrorLastDatum,
    ExtrudeLastSketch,
    CutWithCylinder,
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

            ImportMeshDir => {
                let cwd = std::env::current_dir().unwrap_or_default();
                let candidates = [cwd.join("import.stl"), cwd.join("import.obj")];
                let Some(path) = candidates.iter().find(|p| p.is_file()) else {
                    app.set_status(
                        "No import.stl / import.obj in the current directory — \
                         or drag a file onto the window",
                    );
                    return;
                };
                app.import_file(path.clone());
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
