//! The ForgeApp: main application state and update loop.

use crate::background::{EvalRequest, EvalResponse, EvalWorker, ExportDone};
use crate::palette::PaletteAction;
use crate::ui;
use crate::viewport;
use forge_core::{BodyId, FeatureId};
use forge_model::{Document, Evaluation, Selection, SelectionItem};
use forge_render::{Camera, RenderOptions, Renderer, Scene, SceneBody};
use forge_sketch::SolveReport;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Autosave interval (NFR-RES-03).
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);

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
    /// Whether an evaluation request is in flight.
    pub eval_pending: bool,
    /// Suppress duplicate eval requests during a single frame.
    eval_requested_this_frame: bool,

    /// Last sketch solver report (inspector display).
    pub last_sketch_report: Option<SolveReport>,

    /// Autosave path + timer.
    autosave_path: PathBuf,
    last_autosave: Instant,

    /// Document file path (Save).
    pub doc_path: Option<PathBuf>,

    /// Command palette state.
    pub palette_open: bool,
    pub palette_query: String,

    /// A pick result is awaited (poll the renderer each frame).
    pub pick_requested: bool,

    /// Status line text.
    pub status: String,

    /// Tokio runtime for background file I/O.
    tokio_rt: tokio::runtime::Runtime,
    /// Export job completion channel.
    export_tx: std::sync::mpsc::Sender<ExportDone>,
    /// Finished export jobs (drained into the status line).
    export_rx: std::sync::mpsc::Receiver<ExportDone>,

    /// FPS estimate.
    frame_times: Vec<f32>,
}

impl ForgeApp {
    /// Create the app from the eframe creation context.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // GPU renderer from eframe's wgpu state.
        let renderer = cc.wgpu_render_state.as_ref().map(|state| {
            Arc::new(Mutex::new(Renderer::new(
                state.device.clone(),
                state.queue.clone(),
                state.target_format,
            )))
        });

        let autosave_path = default_autosave_path();
        let mut doc = Document::new("untitled");
        let mut status = String::from("Welcome to ForgeCAD — Ctrl+Shift+P for the command palette");

        // Crash recovery (NFR-RES-03): restore the last autosave if any.
        if autosave_path.exists() {
            match forge_io::load_document(&autosave_path) {
                Ok(recovered) => {
                    doc = recovered;
                    status = format!(
                        "Recovered autosaved document \"{}\" ({} features)",
                        doc.name,
                        doc.tree.len()
                    );
                    doc.modified = true;
                }
                Err(e) => {
                    status = format!("Autosave found but unreadable: {e}");
                }
            }
        }

        let tokio_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (export_tx, export_rx) = std::sync::mpsc::channel();

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
            eval_pending: false,
            eval_requested_this_frame: false,
            last_sketch_report: None,
            autosave_path,
            last_autosave: Instant::now(),
            doc_path: None,
            palette_open: false,
            palette_query: String::new(),
            pick_requested: false,
            status,
            tokio_rt,
            export_tx,
            export_rx,
            frame_times: Vec::new(),
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
        let doc = self.doc.clone();
        if self.eval_worker.tx.send(EvalRequest::Evaluate(doc)).is_ok() {
            self.eval_pending = true;
        }
    }

    /// Poll the evaluation worker and refresh the scene.
    fn poll_evaluation(&mut self) {
        while let Ok(response) = self.eval_worker.rx.try_recv() {
            match response {
                EvalResponse::Done(ev) => {
                    self.eval_pending = false;
                    self.last_evaluation = Some(ev);
                    self.rebuild_scene();
                }
            }
        }
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

    /// Autosave if needed (NFR-RES-03).
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

    /// Save the native document (Ctrl+S / palette).
    pub fn save_native_dialog(&mut self) {
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

    /// Export meshes in the background (tokio spawn_blocking, FR-IO-*).
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

/// Where the autosave lives.
fn default_autosave_path() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("forgecad_autosave.forgecad");
    dir
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // FPS estimate.
        let dt = ctx.input(|i| i.unstable_dt);
        if dt > 0.0 {
            self.frame_times.push(dt);
            if self.frame_times.len() > 30 {
                self.frame_times.remove(0);
            }
        }

        // Poll background jobs.
        self.poll_evaluation();
        self.poll_pick();
        while let Ok(done) = self.export_rx.try_recv() {
            match done.result {
                Ok(()) => self.set_status(format!("Exported {}", done.path.display())),
                Err(e) => self.set_status(format!("Export {what} failed: {e}", what = done.what)),
            }
        }
        self.maybe_autosave();

        // Global shortcuts.
        ctx.input(|i| {
            let ctrl = i.modifiers.ctrl;
            let shift = i.modifiers.shift;
            if ctrl && shift && i.key_pressed(egui::Key::P) {
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
            }
            if ctrl && !shift && i.key_pressed(egui::Key::Z) {
                PaletteAction::Undo.run(self);
            }
            if (ctrl && i.key_pressed(egui::Key::Y))
                || (ctrl && shift && i.key_pressed(egui::Key::Z))
            {
                PaletteAction::Redo.run(self);
            }
            if ctrl && i.key_pressed(egui::Key::S) {
                PaletteAction::SaveNative.run(self);
            }
            if !ctrl {
                if i.key_pressed(egui::Key::F) {
                    self.camera.fit_to(&self.scene_bounds());
                }
                if i.key_pressed(egui::Key::P) {
                    self.camera.orthographic = !self.camera.orthographic;
                }
                if i.key_pressed(egui::Key::G) {
                    self.render_options.show_grid = !self.render_options.show_grid;
                }
                if i.key_pressed(egui::Key::E) {
                    self.render_options.show_edges = !self.render_options.show_edges;
                    self.scene.show_edges = self.render_options.show_edges;
                    self.scene.version += 1;
                }
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
        egui::Panel::left("tree")
            .default_size(230.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Feature tree");
                ui.separator();
                ui::tree_panel(ui, self);
                // P-01: the parameter table shares the left panel.
                ui.add_space(8.0);
                ui.separator();
                ui::params_panel(ui, self);
            });
        egui::Panel::right("inspector")
            .default_size(260.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Inspector");
                ui.separator();
                ui::inspector(ui, self);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            viewport::viewport_ui(ui, self);
        });

        ui::palette_overlay(&ctx, self);

        // Keep repainting while evaluating / picking (the 3D view is
        // continuously interactive at 60 FPS, NFR-PER-02).
        if self.eval_pending || self.pick_requested {
            ctx.request_repaint();
        }
        self.eval_requested_this_frame = false;
    }

    fn on_exit(&mut self) {
        // Final autosave on exit for crash resilience.
        if self.doc.modified {
            let _ = forge_io::save_document(&self.autosave_path, &self.doc);
        }
    }
}
