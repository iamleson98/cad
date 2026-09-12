//! The ForgeApp: main application state and update loop.

use crate::background::{EvalRequest, EvalResponse, EvalWorker, ExportDone, ImportDone};
use crate::gizmo::{GizmoDrag, GizmoHandle, GizmoMode};
use crate::palette::PaletteAction;
use crate::picking::PickMode;
use crate::ui;
use crate::viewport;
use forge_core::{BodyId, FeatureId, Point3};
use forge_geometry::Bvh;
use forge_model::{Document, Evaluation, Selection, SelectionItem};
use forge_render::{Camera, RenderOptions, Renderer, Scene, SceneBody};
use forge_sketch::SolveReport;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

/// Autosave interval (NFR-RES-03).
#[cfg(not(target_arch = "wasm32"))]
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);

/// One measurement pick (W-08): a surface hit point and its face normal.
#[derive(Debug, Clone, Copy)]
pub struct MeasurePick {
    /// World-space hit point.
    pub point: Point3,
    /// Unit face normal at the hit.
    pub normal: forge_core::Vector3,
}

/// The application.
pub struct ForgeApp {
    /// Parametric document (UI thread only).
    pub doc: Document,
    /// Undo/redo stack.
    pub commands: forge_model::CommandStack,
    /// Current selection.
    pub selection: Selection,
    /// Viewport camera.
    pub camera: Camera,
    /// Render options.
    pub render_options: RenderOptions,
    /// Render scene (rebuilt after each evaluation).
    pub scene: Scene,

    /// GPU renderer (created once wgpu is available).
    renderer: Option<Arc<Mutex<Renderer>>>,

    /// Background evaluation worker.
    pub eval_worker: EvalWorker,
    /// Last completed evaluation.
    pub last_evaluation: Option<Evaluation>,
    /// Wall duration of the last completed evaluation (PR-05).
    pub last_eval_duration: Option<std::time::Duration>,
    /// Whether an evaluation request is in flight.
    pub eval_pending: bool,
    /// Suppress duplicate eval requests during a single frame.
    eval_requested_this_frame: bool,

    /// Last sketch solver report (inspector display).
    pub last_sketch_report: Option<SolveReport>,

    /// S-08: mirror-line entity chosen in the sketch inspector.
    pub mirror_line_pick: Option<forge_core::EntityId>,

    /// Autosave path + timer.
    #[cfg(not(target_arch = "wasm32"))]
    autosave_path: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    last_autosave: Instant,

    /// Document file path (Save).
    pub doc_path: Option<PathBuf>,

    /// Command palette state.
    pub palette_open: bool,
    pub palette_query: String,
    /// Highlighted row in the palette list (↑/↓ navigation).
    pub palette_cursor: usize,

    /// A pick result is awaited (poll the renderer each frame).
    pub pick_requested: bool,

    /// Measurement tool (W-08): picking surface points.
    pub measure_mode: bool,
    /// CAM workspace (C-03/C-04): tool library, operations, toolpaths.
    pub cam: crate::cam::CamState,
    /// Selected CAM operation index in the panel.
    pub cam_selected_op: usize,
    /// Picked surface points (0..=2, in click order).
    pub measure_picks: Vec<MeasurePick>,
    /// Label text of the finished measurement (distance + angle).
    pub measure_label: Option<String>,
    /// Per-body BVHs for measurement raycasts (rebuilt on evaluation).
    measure_bvhs: Vec<(BodyId, Bvh)>,
    measure_bvhs_stale: bool,

    /// 3D drag manipulator (W-01): translate or rotate handles.
    pub gizmo_mode: GizmoMode,
    /// Active gizmo drag (W-01); the drag owns its undo commit.
    pub gizmo_drag: Option<GizmoDrag>,
    /// Hovered gizmo handle this frame (W-01 highlight).
    pub(crate) gizmo_hover: Option<GizmoHandle>,
    /// The primary button is (or was, until this frame's click check) held
    /// on a gizmo handle: pick-clicks are suppressed (W-01).
    pub(crate) gizmo_press_on_handle: bool,

    /// Picking granularity (W-04): bodies / faces / edges / vertices.
    pub pick_mode: PickMode,

    /// Status line text.
    pub status: String,

    /// Tokio runtime for background file I/O.
    /// Async file-I/O runtime for export/import jobs (native only:
    /// wasm exports synchronously, W-10).
    #[cfg(not(target_arch = "wasm32"))]
    tokio_rt: tokio::runtime::Runtime,
    /// Export job completion channel.
    export_tx: std::sync::mpsc::Sender<ExportDone>,
    /// Finished export jobs (drained into the status line).
    export_rx: std::sync::mpsc::Receiver<ExportDone>,
    /// Import job completion channel (I-01).
    import_tx: std::sync::mpsc::Sender<ImportDone>,
    /// Finished import jobs (drained into the feature tree).
    import_rx: std::sync::mpsc::Receiver<ImportDone>,
    /// Paths already imported this session (drag-and-drop dedup —
    /// `raw.dropped_files` persists across frames after a drop).
    imported_paths: std::collections::HashSet<PathBuf>,

    /// FPS estimate.
    /// Rolling frame-time samples (PR-05 status stats).
    pub frame_times: Vec<f32>,
    /// Completed background evaluations (E2E assertions: the bridge
    /// state snapshot exposes it as `evalDone`).
    pub eval_done_count: u64,
    /// Evaluation requests sent (E2E settle detection: the app is
    /// settled when `eval_done_count == eval_sent_count`; the boolean
    /// `eval_pending` is cleared by any response and misreports when
    /// several requests are in flight).
    pub eval_sent_count: u64,
}

impl ForgeApp {
    /// Create the app from the eframe creation context.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Fonts (Inter + Lucide icons) and the ForgeCAD dark theme —
        // before the first frame so nothing flashes the egui default.
        crate::theme::install(&cc.egui_ctx);

        // GPU renderer from eframe's wgpu state.
        let renderer = cc.wgpu_render_state.as_ref().map(|state| {
            Arc::new(Mutex::new(Renderer::new(
                state.device.clone(),
                state.queue.clone(),
                state.target_format,
            )))
        });

        // Autosave destination — native only (wasm has no temp dir).
        #[cfg(not(target_arch = "wasm32"))]
        let autosave_path = default_autosave_path();
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut doc = Document::new("untitled");
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut status = String::from("Welcome to ForgeCAD — Ctrl+Shift+P for the command palette");

        // Crash recovery (NFR-RES-03 + PR-01): restore the freshest of
        // the crash snapshot (refreshed after every mutation) and the
        // 2-minute autosave. Native only — wasm has no disk to recover
        // from (browser persistence is a documented TODO).
        #[cfg(not(target_arch = "wasm32"))]
        let recovery = (|| -> Option<(PathBuf, &'static str)> {
            let crash_snapshot = crate::crash::latest_snapshot();
            let crash_newer = crash_snapshot
                .as_ref()
                .and_then(|p| p.metadata().ok().and_then(|m| m.modified().ok()))
                .zip(
                    autosave_path
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok()),
                )
                .map(|(c, a)| c > a)
                .unwrap_or(false);
            if crash_newer {
                return crash_snapshot.map(|p| (p, "crash snapshot"));
            }
            if autosave_path.exists() {
                return Some((autosave_path.clone(), "autosave"));
            }
            None
        })();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((path, source)) = recovery {
            match forge_io::load_document(&path) {
                Ok(recovered) => {
                    doc = recovered;
                    status = format!(
                        "Recovered \"{}\" from {source} ({} features)",
                        doc.name,
                        doc.tree.len()
                    );
                    doc.modified = true;
                }
                Err(e) => {
                    status = format!("{source} found but unreadable: {e}");
                }
            }
        }

        Self::assemble(doc, status, renderer)
    }

    /// Headless construction for the E2E harness (`cargo test`): a
    /// pristine document, no crash/autosave recovery (tests must be
    /// deterministic regardless of machine state) and no GPU renderer
    /// (the panels run against a bare egui context).
    #[cfg(any(test, debug_assertions))]
    pub fn new_headless() -> Self {
        Self::assemble(
            Document::new("untitled"),
            String::from("Welcome to ForgeCAD — Ctrl+Shift+P for the command palette"),
            None,
        )
    }

    /// Shared constructor tail: everything that does not depend on the
    /// eframe creation context.
    fn assemble(doc: Document, status: String, renderer: Option<Arc<Mutex<Renderer>>>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let autosave_path = default_autosave_path();
        #[cfg(not(target_arch = "wasm32"))]
        let tokio_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (export_tx, export_rx) = std::sync::mpsc::channel();
        let (import_tx, import_rx) = std::sync::mpsc::channel();

        let mut app = Self {
            doc,
            commands: forge_model::CommandStack::new(),
            selection: Selection::new(),
            camera: Camera::default(),
            render_options: RenderOptions::default(),
            scene: Scene::new(),
            renderer,
            eval_worker: EvalWorker::spawn(),
            last_evaluation: None,
            last_eval_duration: None,
            eval_pending: false,
            eval_requested_this_frame: false,
            last_sketch_report: None,
            #[cfg(not(target_arch = "wasm32"))]
            autosave_path,
            #[cfg(not(target_arch = "wasm32"))]
            last_autosave: Instant::now(),
            doc_path: None,
            palette_open: false,
            palette_query: String::new(),
            palette_cursor: 0,
            pick_requested: false,
            measure_mode: false,
            cam: crate::cam::CamState::default(),
            cam_selected_op: 0,
            measure_picks: Vec::new(),
            measure_label: None,
            measure_bvhs: Vec::new(),
            measure_bvhs_stale: true,
            mirror_line_pick: None,
            gizmo_mode: GizmoMode::default(),
            gizmo_drag: None,
            gizmo_hover: None,
            gizmo_press_on_handle: false,
            pick_mode: PickMode::default(),
            status,
            #[cfg(not(target_arch = "wasm32"))]
            tokio_rt,
            export_tx,
            export_rx,
            import_tx,
            import_rx,
            imported_paths: std::collections::HashSet::new(),
            frame_times: Vec::new(),
            eval_done_count: 0,
            eval_sent_count: 0,
        };
        app.request_evaluation();
        app
    }

    /// The renderer, if wgpu is available.
    pub fn renderer(&self) -> Option<Arc<Mutex<Renderer>>> {
        self.renderer.clone()
    }

    /// Set the status line.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }

    /// Request a background evaluation of the current document.
    pub fn request_evaluation(&mut self) {
        if self.eval_requested_this_frame {
            return;
        }
        self.eval_requested_this_frame = true;
        // PR-01: freshest-possible crash recovery — the snapshot is the
        // parametric data only (no meshes), so this is cheap.
        crate::crash::snapshot_document(&self.doc);
        let doc = self.doc.clone();
        if self.eval_worker.tx.send(EvalRequest::Evaluate(doc)).is_ok() {
            self.eval_pending = true;
            self.eval_sent_count += 1;
        }
    }

    /// Poll the evaluation worker and refresh the scene.
    fn poll_evaluation(&mut self) {
        while let Ok(response) = self.eval_worker.rx.try_recv() {
            match response {
                EvalResponse::Done(ev, duration) => {
                    self.eval_pending = false;
                    self.eval_done_count += 1;
                    // CAM results are computed against one evaluation
                    // snapshot: a new evaluation invalidates them (C-03).
                    let cam_stale = self.cam_has_stale_results();
                    if cam_stale {
                        self.cam.clear_results();
                        self.cam.build_overlays(&mut self.scene);
                    }
                    self.last_evaluation = Some(ev);
                    self.last_eval_duration = Some(duration);
                    self.measure_bvhs_stale = true;
                    self.rebuild_scene();
                }
            }
        }
    }

    /// True when CAM toolpaths exist but were computed against an older
    /// evaluation (they must be dropped before the new one lands).
    fn cam_has_stale_results(&self) -> bool {
        self.cam.ops.iter().any(|o| o.result.is_some()) || self.cam.gcode.is_some()
    }

    /// Rebuild the render scene from the last evaluation.
    fn rebuild_scene(&mut self) {
        let mut bodies = Vec::new();
        if let Some(ev) = &self.last_evaluation {
            for b in &ev.bodies {
                bodies.push(SceneBody {
                    id: b.id,
                    name: b.name.clone(),
                    mesh: b.mesh.clone(),
                    style: forge_render::BodyStyle {
                        color: self.body_color(b.source),
                        transparent: false,
                    },
                });
            }
        }
        self.scene.set_bodies(bodies);
        // Auto-fit on the first non-empty evaluation.
        if self
            .last_evaluation
            .as_ref()
            .map(|ev| !ev.bodies.is_empty())
            .unwrap_or(false)
            && self.camera.distance == 300.0
            && self.camera.target == forge_core::Point3::origin()
        {
            self.camera.fit_to(&self.scene_bounds());
        }
    }

    /// Poll the GPU pick result and update the selection.
    fn poll_pick(&mut self) {
        if !self.pick_requested {
            return;
        }
        let Some(renderer) = self.renderer.clone() else {
            return;
        };
        let result = renderer.lock().ok().and_then(|mut r| r.poll_pick());
        let Some(result) = result else { return };
        self.pick_requested = false;
        match result.body {
            Some(raw) => {
                let item = SelectionItem::Body(BodyId::new(raw));
                if self.selection.items.contains(&item) {
                    self.selection.toggle(item);
                } else {
                    self.selection.select(item);
                }
                let name = self
                    .doc
                    .feature(FeatureId::new(raw))
                    .map(|f| f.label())
                    .unwrap_or_else(|| format!("body {raw}"));
                self.set_status(format!("Selected {name}"));
            }
            None => {
                self.selection.clear();
                self.set_status("Selection cleared");
            }
        }
    }

    /// Autosave if needed (NFR-RES-03). Native only — wasm has no
    /// writable document path (browser persistence is a TODO item).
    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_autosave(&mut self) {
        if self.doc.modified && self.last_autosave.elapsed() > AUTOSAVE_INTERVAL {
            match forge_io::save_document(&self.autosave_path, &self.doc) {
                Ok(()) => {
                    self.doc.modified = false;
                    self.set_status("Autosaved");
                }
                Err(e) => self.set_status(format!("Autosave failed: {e}")),
            }
            self.last_autosave = Instant::now();
        }
    }

    /// Run the CAM compute (C-03): strategies against the last evaluation.
    pub fn cam_compute(&mut self) {
        let Some(ev) = self.last_evaluation.clone() else {
            self.set_status("CAM: evaluate a model first");
            return;
        };
        let status = self.cam.compute(&self.doc, &ev);
        self.cam.build_overlays(&mut self.scene);
        self.set_status(status);
    }

    /// Export the CAM G-code (C-02): native writes a `.nc` file, wasm
    /// triggers a browser download.
    pub fn cam_export_gcode(&mut self) {
        let Some(gcode) = self.cam.gcode() else {
            self.set_status("CAM: compute toolpaths first");
            return;
        };
        let name = format!("{}.nc", self.doc.name.replace(char::is_whitespace, "_"));
        #[cfg(target_arch = "wasm32")]
        {
            let result = crate::web::download_bytes(&name, gcode.into_bytes(), "text/plain");
            let msg = match result {
                Ok(()) => format!("G-code exported: {name}"),
                Err(e) => format!("G-code export failed: {e}"),
            };
            self.set_status(msg);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut path = std::env::current_dir().unwrap_or_default();
            path.push(&name);
            let what = "G-code".to_string();
            let export_tx = self.export_tx.clone();
            self.set_status("Exporting G-code…");
            let bytes = gcode.into_bytes();
            self.tokio_rt.spawn_blocking(move || {
                let result: Result<(), String> =
                    std::fs::write(&path, bytes).map_err(|e| format!("{e}"));
                let _ = export_tx.send(ExportDone { what, path, result });
            });
        }
    }

    /// Save the native document (Ctrl+S / palette). On wasm this is a
    /// browser download of the `.forgecad` file (W-10).
    pub fn save_native_dialog(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            let name = format!(
                "{}.forgecad",
                self.doc.name.replace(char::is_whitespace, "_")
            );
            let result = forge_io::document_to_string(&self.doc)
                .map_err(|e| format!("{e}"))
                .and_then(|text| {
                    crate::web::download_bytes(
                        &name,
                        text.into_bytes(),
                        crate::web::mime_for("forgecad"),
                    )
                });
            match result {
                Ok(()) => {
                    self.doc.modified = false;
                    self.set_status(format!("Saved {name} (download)"));
                }
                Err(e) => self.set_status(format!("Save failed: {e}")),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.doc_path.clone().unwrap_or_else(|| {
                let mut p = std::env::current_dir().unwrap_or_default();
                let name = self.doc.name.replace(char::is_whitespace, "_");
                p.push(format!("{name}.forgecad"));
                p
            });
            match forge_io::save_document(&path, &self.doc) {
                Ok(()) => {
                    self.doc_path = Some(path.clone());
                    self.doc.modified = false;
                    self.set_status(format!("Saved {}", path.display()));
                }
                Err(e) => self.set_status(format!("Save failed: {e}")),
            }
        }
    }

    /// Export meshes (FR-IO-*): tokio background job on native, a
    /// synchronous browser download on wasm (W-10).
    pub fn export_mesh(&mut self, format: forge_io::ExportFormat) {
        let Some(ev) = &self.last_evaluation else {
            self.set_status("Nothing to export: evaluate a model first");
            return;
        };
        if ev.bodies.is_empty() {
            self.set_status("No bodies to export");
            return;
        }
        let meshes: Vec<forge_io::ExportMesh> = ev
            .bodies
            .iter()
            .map(|b| forge_io::ExportMesh {
                name: b.name.clone(),
                mesh: b.mesh.clone(),
            })
            .collect();
        let what = format!("{format:?}");

        #[cfg(target_arch = "wasm32")]
        {
            let name = format!(
                "{}.{ext}",
                self.doc.name.replace(char::is_whitespace, "_"),
                ext = format.extension()
            );
            let result = forge_io::export_bytes(format, &meshes)
                .map_err(|e| format!("{e}"))
                .and_then(|bytes| {
                    crate::web::download_bytes(
                        &name,
                        bytes,
                        crate::web::mime_for(format.extension()),
                    )
                });
            let _ = self.export_tx.send(ExportDone {
                what,
                path: PathBuf::from(&name),
                result,
            });
            // (The native branch below is cfg'd out on wasm — control
            // simply falls out of the function here.)
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let export_tx = self.export_tx.clone();
            let mut dir = std::env::current_dir().unwrap_or_default();
            dir.push(format!(
                "{}.{ext}",
                self.doc.name.replace(char::is_whitespace, "_"),
                ext = format.extension()
            ));

            self.set_status(format!("Exporting {what}…"));
            self.tokio_rt.spawn_blocking(move || {
                let path = dir;
                let result = forge_io::export(format, &path, &meshes);
                let _ = export_tx.send(ExportDone {
                    what,
                    path: path.clone(),
                    result: result.map_err(|e| format!("{e}")),
                });
            });
        }
    }

    /// Kick off a background mesh import (I-01/I-02): parse + weld +
    /// repair on a blocking thread; the finished meshes arrive via
    /// `import_rx` and are added to the feature tree in
    /// [`Self::poll_imports`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_file(&mut self, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(forge_io::ImportFormat::from_extension);
        let Some(format) = ext else {
            self.set_status(format!(
                "Unsupported import format: {} (use .stl, .obj or .3mf)",
                path.display()
            ));
            return;
        };
        let import_tx = self.import_tx.clone();
        self.set_status(format!("Importing {}…", path.display()));
        self.tokio_rt.spawn_blocking(move || {
            let result = forge_io::import_meshes(format, &path).map_err(|e| format!("{e}"));
            let _ = import_tx.send(ImportDone { path, result });
        });
    }

    /// wasm32: import a dropped mesh. Browsers expose file contents
    /// only through async APIs, so the bytes are awaited off-frame and
    /// the finished import arrives through `import_rx` (W-10).
    #[cfg(target_arch = "wasm32")]
    fn import_dropped(&mut self, handle: egui::DroppedFileHandle, path: PathBuf) {
        let Some(format) = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(forge_io::ImportFormat::from_extension)
        else {
            self.set_status(format!(
                "Unsupported import format: {} (use .stl, .obj or .3mf)",
                path.display()
            ));
            return;
        };
        let import_tx = self.import_tx.clone();
        self.set_status(format!("Importing {}…", path.display()));
        wasm_bindgen_futures::spawn_local(async move {
            let result = match handle.bytes_async().await {
                Ok(bytes) => {
                    forge_io::import_meshes_bytes(format, &bytes).map_err(|e| format!("{e}"))
                }
                Err(e) => Err(e),
            };
            let _ = import_tx.send(ImportDone { path, result });
        });
    }

    /// Add finished imports as `ImportedMesh` features (I-01; one per
    /// object for multi-object 3MF files, I-02).
    fn add_imported_meshes(
        &mut self,
        path: &std::path::Path,
        meshes: Vec<(String, forge_geometry::TriMesh)>,
    ) {
        let file_stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mesh")
            .to_string();
        let multi = meshes.len() > 1;
        let mut added = 0usize;
        for (object_name, mesh) in meshes {
            // Single-object imports keep the plain file name; multi-object
            // 3MFs get "file:object" so the tree stays readable.
            let source = if multi {
                format!("{file_stem}:{object_name}")
            } else {
                file_stem.clone()
            };
            let feature = forge_model::Feature::ImportedMesh(forge_model::ImportedMeshParams {
                source,
                mesh,
            });
            match self.doc.add_feature(feature.clone()) {
                Ok(id) => {
                    if let Some(node) = self.doc.tree.get(id).cloned() {
                        let _ = self
                            .commands
                            .execute(forge_model::Command::AddFeature { node }, &mut self.doc);
                    }
                    added += 1;
                    self.request_evaluation();
                }
                Err(e) => self.set_status(format!("{e}")),
            }
        }
        if added > 0 {
            self.set_status(format!(
                "Imported {added} bod{} from {file_stem}",
                if added == 1 { "y" } else { "ies" }
            ));
        }
    }

    /// Drain finished background imports and add them to the tree.
    fn poll_imports(&mut self) {
        while let Ok(done) = self.import_rx.try_recv() {
            match done.result {
                Ok(meshes) => self.add_imported_meshes(&done.path, meshes),
                Err(e) => self.set_status(format!("Import {} failed: {e}", done.path.display())),
            }
        }
    }

    /// Apply a queued E2E test action (see [`crate::bridge::queue_action`]).
    /// Actions are drained at the top of [`Self::ui_body`] every frame;
    /// the queue is only ever filled in debug/test builds (the release
    /// drain is a no-op returning nothing).
    fn apply_test_action(&mut self, name: &str, payload: &str) {
        match name {
            // "import" with payload "<ext>:<file contents>" — the E2E
            // replacement for a native file dialog / browser drop: feed
            // mesh bytes straight into the I-01 import pipeline.
            "import" => {
                let (ext, bytes) = payload
                    .split_once(':')
                    .map(|(e, b)| (e.to_string(), b.as_bytes().to_vec()))
                    .unwrap_or_default();
                let format = forge_io::ImportFormat::from_extension(&ext);
                let Some(format) = format else {
                    self.set_status(format!("test import: unsupported .{ext}"));
                    return;
                };
                match forge_io::import_meshes_bytes(format, &bytes) {
                    Ok(meshes) => self.add_imported_meshes(
                        &std::path::PathBuf::from(format!("test_import.{ext}")),
                        meshes,
                    ),
                    Err(e) => self.set_status(format!("test import failed: {e}")),
                }
            }
            // Smoke channel: prove the queue round-trips (harness test).
            "status" => self.set_status(format!("action:{payload}")),
            other => self.set_status(format!("unknown test action: {other}")),
        }
    }

    // ---- Measurement tool (W-08) ----------------------------------------

    /// Rebuild per-body BVHs after an evaluation change.
    fn rebuild_measure_bvhs(&mut self) {
        let bvhs: Vec<(BodyId, Bvh)> = self
            .last_evaluation
            .iter()
            .flat_map(|ev| ev.bodies.iter())
            .map(|b| {
                (
                    b.id,
                    Bvh::from_mesh_positions(&b.mesh.positions, &b.mesh.indices),
                )
            })
            .collect();
        self.measure_bvhs = bvhs;
        self.measure_bvhs_stale = false;
    }

    /// Raycast every visible body (W-04 / W-08): nearest hit as
    /// `(body index, ray distance t, triangle index)`.
    pub fn raycast_bodies(&mut self, ndc: (f64, f64), aspect: f64) -> Option<(usize, f64, usize)> {
        if self.measure_bvhs_stale {
            self.rebuild_measure_bvhs();
        }
        let ray = self.camera.ray_through_ndc(ndc, aspect)?;
        let far = self.camera.far;
        let mut best: Option<(usize, f64, usize)> = None;
        if let Some(ev) = &self.last_evaluation {
            for (i, (body, (_, bvh))) in ev.bodies.iter().zip(self.measure_bvhs.iter()).enumerate()
            {
                if let Some(hit) = bvh.ray_cast(&body.mesh.positions, &body.mesh.indices, &ray, far)
                {
                    if best.map(|(_, t, _)| hit.t < t).unwrap_or(true) {
                        best = Some((i, hit.t, hit.triangle as usize));
                    }
                }
            }
        }
        best
    }

    /// A viewport click in measure mode: raycast every body, record the
    /// surface hit (point + face normal). Empty space restarts the pick.
    pub fn measure_click(&mut self, ndc: (f64, f64), aspect: f64) {
        let best = self.raycast_bodies(ndc, aspect);

        let Some((body_idx, t, tri)) = best else {
            // Clicked empty space: restart the measurement.
            self.measure_picks.clear();
            self.measure_label = None;
            self.set_status("Measure: pick a first surface point");
            return;
        };

        let ray = self.camera.ray_through_ndc(ndc, aspect);
        let Some(ray) = ray else { return };
        let point = ray.at(t);
        let normal = self
            .last_evaluation
            .as_ref()
            .and_then(|ev| ev.bodies.get(body_idx))
            .and_then(|b| b.mesh.triangle_normal(tri))
            .unwrap_or_else(forge_core::Vector3::z);

        // Third click starts a fresh measurement.
        if self.measure_picks.len() >= 2 {
            self.measure_picks.clear();
            self.measure_label = None;
        }
        self.measure_picks.push(MeasurePick { point, normal });

        if self.measure_picks.len() == 2 {
            let [a, b] = [self.measure_picks[0], self.measure_picks[1]];
            let dist = (b.point - a.point).norm();
            let cos = a.normal.dot(&b.normal).clamp(-1.0, 1.0);
            let angle = cos.acos().to_degrees();
            let label = format!("{dist:.3} mm \u{00b7} normals {angle:.1}\u{00b0}");
            self.set_status(format!("Measure: {label}"));
            self.measure_label = Some(label);
        } else {
            self.set_status("Measure: pick a second surface point");
        }
    }
}

/// Where the autosave lives.
#[cfg(not(target_arch = "wasm32"))]
fn default_autosave_path() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("forgecad_autosave.forgecad");
    dir
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_body(ui);
    }

    fn on_exit(&mut self) {
        // Final autosave on exit for crash resilience (native only).
        #[cfg(not(target_arch = "wasm32"))]
        if self.doc.modified {
            let _ = forge_io::save_document(&self.autosave_path, &self.doc);
        }
    }
}

impl ForgeApp {
    /// The full per-frame body: input polling, panels, viewport, palette.
    /// [`eframe::App::ui`] delegates here, and the headless E2E harness
    /// (cargo test / debug bridge) drives the exact same code against a
    /// bare `egui::Context` — one code path, tested two ways.
    pub(crate) fn ui_body(&mut self, ui: &mut egui::Ui) {
        // E2E bridge (debug/test builds only — compiled out of release):
        // frame heartbeat + widget registry reset + queued test actions.
        crate::bridge::begin_frame();
        for (name, payload) in crate::bridge::drain_actions() {
            self.apply_test_action(&name, &payload);
        }

        let ctx = ui.ctx().clone();
        // FPS estimate.
        let dt = ctx.input(|i| i.unstable_dt);
        if dt > 0.0 {
            self.frame_times.push(dt);
            if self.frame_times.len() > 30 {
                self.frame_times.remove(0);
            }
        }

        // Poll background jobs. On wasm the evaluation worker runs
        // synchronously here (no threads, W-10).
        #[cfg(target_arch = "wasm32")]
        self.eval_worker.process_pending();
        self.poll_evaluation();
        self.poll_pick();
        self.poll_imports();
        while let Ok(done) = self.export_rx.try_recv() {
            match done.result {
                Ok(()) => self.set_status(format!("Exported {}", done.path.display())),
                Err(e) => self.set_status(format!("Export {what} failed: {e}", what = done.what)),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.maybe_autosave();

        // Drag-and-drop mesh import (I-01): drop .stl/.obj files anywhere
        // on the window to add them as mesh bodies. `raw.dropped_files`
        // persists after the drop, so paths are imported once per session
        // (re-import deliberately via the palette command instead).
        // On wasm browsers supply bytes, not local paths — the file-name
        // path still dedupes, contents are read async (W-10).
        let dropped: Vec<(PathBuf, egui::DroppedFileHandle)> = ctx
            .input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| (f.path().to_path_buf(), f.clone()))
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .filter(|(p, _)| !self.imported_paths.contains(p))
            .collect();
        for (path, handle) in dropped {
            self.imported_paths.insert(path.clone());
            #[cfg(target_arch = "wasm32")]
            self.import_dropped(handle, path);
            #[cfg(not(target_arch = "wasm32"))]
            {
                drop(handle);
                self.import_file(path);
            }
        }

        // Global shortcuts.
        // Chord matching uses the per-EVENT modifiers, not the
        // end-of-frame `i.modifiers` state: when a fast key chord
        // (modifier downs + key + modifier ups) coalesces into a single
        // frame — common in the browser and for fast typists — the
        // tracked state is already back to default when the app reads
        // it, and the shortcut is silently lost (the E2E suite caught
        // this as a flaky Ctrl+Shift+P).
        let chord = |i: &egui::InputState, key: egui::Key, ctrl: bool, shift: bool| {
            i.events.iter().any(|e| match e {
                egui::Event::Key {
                    key: k,
                    pressed: true,
                    modifiers: m,
                    ..
                } => k == &key && m.ctrl == ctrl && m.shift == shift,
                _ => false,
            })
        };
        // Also `ctx.input` takes the context WRITE lock; calling
        // `ctx.egui_wants_keyboard_input()` (a READ lock) inside the
        // closure self-deadlocks — capture the flag BEFORE instead.
        let wants_keyboard = ctx.egui_wants_keyboard_input();
        ctx.input(|i| {
            if chord(i, egui::Key::P, true, true) {
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
                self.palette_cursor = 0;
            }
            if chord(i, egui::Key::Z, true, false) {
                PaletteAction::Undo.run(self);
            }
            if chord(i, egui::Key::Y, true, false) || chord(i, egui::Key::Z, true, true) {
                PaletteAction::Redo.run(self);
            }
            if chord(i, egui::Key::S, true, false) {
                PaletteAction::SaveNative.run(self);
            }
            if chord(i, egui::Key::F, false, false) {
                self.camera.fit_to(&self.scene_bounds());
            }
            if chord(i, egui::Key::P, false, false) {
                self.camera.orthographic = !self.camera.orthographic;
            }
            if chord(i, egui::Key::G, false, false) {
                self.render_options.show_grid = !self.render_options.show_grid;
            }
            if chord(i, egui::Key::E, false, false) {
                self.render_options.show_edges = !self.render_options.show_edges;
                self.scene.show_edges = self.render_options.show_edges;
                self.scene.version += 1;
            }
            // W-01 gizmo mode shortcuts (not while typing in a field).
            if chord(i, egui::Key::T, false, false) && !wants_keyboard {
                self.gizmo_mode = GizmoMode::Translate;
                self.set_status("Gizmo: translate (T) — grab an axis arrow or plane");
            }
            if chord(i, egui::Key::R, false, false) && !wants_keyboard {
                self.gizmo_mode = GizmoMode::Rotate;
                self.set_status("Gizmo: rotate (R) — grab a ring");
            }
        });

        // Scene sync: colors follow the selection.
        let selected: Vec<BodyId> = self.selection.bodies();
        if let Some(ev) = &self.last_evaluation {
            for (i, b) in ev.bodies.iter().enumerate() {
                if let Some(body) = self.scene.bodies.get_mut(i) {
                    let is_sel = selected.iter().any(|s| s.raw() == b.id.raw());
                    let want = if is_sel {
                        Scene::SELECTED
                    } else {
                        Scene::DEFAULT
                    };
                    if body.style.color != want {
                        body.style.color = want;
                        self.scene.version += 1;
                    }
                }
            }
        }

        // Layout.
        egui::Panel::top("toolbar").resizable(false).show(ui, |ui| {
            ui::toolbar(ui, self);
        });
        egui::Panel::bottom("status")
            .resizable(false)
            .show(ui, |ui| {
                ui::status_bar(ui, self);
            });
        if self.cam.panel_open {
            egui::Panel::bottom("cam")
                .default_size(220.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui::cam_panel(ui, self);
                });
        }
        egui::Panel::left("tree")
            .default_size(230.0)
            .resizable(true)
            .show(ui, |ui| {
                // One shared scroll area for the whole left panel (tree +
                // params): everything stays reachable by scrolling, the
                // parameter panel no longer gets pushed off-screen by the
                // feature tree (E2E regression: the Add-parameter button
                // sat at y=972 on a 900 px screen).
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        ui::tree_panel(ui, self);
                        // P-01: the parameter table shares the left panel.
                        ui.add_space(8.0);
                        ui.separator();
                        ui::params_panel(ui, self);
                    });
            });
        egui::Panel::right("inspector")
            .default_size(260.0)
            .resizable(true)
            .show(ui, |ui| {
                ui::inspector(ui, self);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            viewport::viewport_ui(ui, self);
        });

        ui::palette_overlay(&ctx, self);

        // Keep repainting while evaluating / picking / dragging the gizmo
        // (the 3D view is continuously interactive at 60 FPS, NFR-PER-02).
        if self.eval_pending || self.pick_requested || self.gizmo_drag.is_some() {
            ctx.request_repaint();
        }
        self.eval_requested_this_frame = false;
        // Release the gizmo press flag once the button is up *and* the
        // click check of this frame has run (W-01).
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.gizmo_press_on_handle = false;
        }

        // E2E bridge: publish the state snapshot for this frame.
        crate::bridge::publish_state(self);
    }
}
