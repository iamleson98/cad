//! Headless E2E harness (native, `cargo test -p forge-app`): drives the
//! REAL application — the exact same `ui_body` the desktop app runs —
//! against a bare `egui::Context`, injecting real pointer/keyboard
//! events at the REAL widget rects from the bridge registry.
//!
//! This is the fastest layer of the E2E strategy (compiles once with the
//! workspace, runs in seconds, deterministic): any panic on any button
//! click fails a test here. The browser layer (`e2e/`, Playwright +
//! trunk debug bundle) then covers the wasm/wGPU integration on top.
//!
//! Frame model: `Harness::frame` = one `ctx.run_ui` pass around
//! `ui_body`. A click is three frames (hover, press, release) — the
//! same event shape eframe feeds egui from the window system.

use crate::app::ForgeApp;
use crate::bridge::{self, WidgetInfo};
use egui::{pos2, vec2, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use std::sync::Mutex;

/// Harness tests run against GLOBAL bridge state (the registry the wasm
/// JS bridge also reads), so they must not interleave. Every test takes
/// this lock; the suite is fast (<2 s total), serialization is fine.
static SERIAL: Mutex<()> = Mutex::new(());

/// Serialize harness tests (call as `let _s = serial();` first).
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Harness screen: a comfortable desktop viewport (points).
pub const SCREEN: (f32, f32) = (1400.0, 900.0);

pub struct Harness {
    pub ctx: egui::Context,
    pub app: ForgeApp,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    /// Boot the app headlessly and settle the first evaluation.
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut h = Self {
            ctx,
            app: ForgeApp::new_headless(),
        };
        // Initial evaluation + auto-fit settle.
        h.wait_for_eval(500);
        h
    }

    fn raw_input(&self, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(SCREEN.0, SCREEN.1),
            )),
            events,
            ..Default::default()
        }
    }

    /// Run one frame with the given input events (empty = idle frame).
    pub fn frame(&mut self, events: Vec<Event>) {
        let raw = self.raw_input(events);
        let Harness { ctx, app } = self;
        let mut output = ctx.run_ui(raw, |ctx| {
            // Root replicating eframe's shell: a borderless central panel
            // carrying the app.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| app.ui_body(ui));
        });
        // Headless passes produce font/texture deltas nobody applies;
        // egui's Drop asserts unclaimed deltas — clear them (same trick
        // as theme.rs tests).
        output.textures_delta.clear();
    }

    /// Idle frames.
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    /// Run frames until the background evaluation settles, within a
    /// wall-clock budget in ms (the worker computes on another thread —
    /// frame count alone races it: 120 frames can pass in ~200 ms
    /// while a union needs ~300 ms of compute).
    /// Settled = every sent request answered (`eval_sent_count ==
    /// eval_done_count`). The boolean `eval_pending` misreports when
    /// several requests overlap: the FIRST response clears it while
    /// later ones still compute, and a stale `last_evaluation` reads
    /// as a missing body.
    pub fn wait_for_eval(&mut self, budget_ms: u64) -> bool {
        let settled = |h: &Self| h.app.eval_sent_count == h.app.eval_done_count;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(budget_ms);
        while std::time::Instant::now() < deadline {
            if settled(self) && self.app.eval_done_count > 0 {
                self.frame(vec![]);
                if settled(self) {
                    return true;
                }
            }
            self.frame(vec![]);
            // Give the worker thread wall time to compute.
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        settled(self) && self.app.eval_done_count > 0
    }

    // ---- Widget lookup -------------------------------------------------

    /// Bridge widget registry of the last frame.
    pub fn widgets(&self) -> Vec<WidgetInfo> {
        bridge::widgets()
    }

    /// Find by exact id.
    pub fn widget(&self, id: &str) -> WidgetInfo {
        self.widgets()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap_or_else(|| panic!("widget {id:?} not found this frame"))
    }

    /// Find by id prefix (first match).
    pub fn widget_prefix(&self, prefix: &str) -> WidgetInfo {
        bridge::find_id(prefix).unwrap_or_else(|| panic!("no widget with id prefix {prefix:?}"))
    }

    /// Find by label substring (first match).
    pub fn widget_label(&self, needle: &str) -> WidgetInfo {
        bridge::find_label(needle)
            .unwrap_or_else(|| panic!("no widget with label containing {needle:?}"))
    }

    pub fn has_widget(&self, id: &str) -> bool {
        self.widgets().iter().any(|w| w.id == id)
    }

    // ---- Pointer input --------------------------------------------------

    /// Full click at a position: hover, press, release (3 frames).
    pub fn click_pos(&mut self, pos: Pos2) {
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        }]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        }]);
    }

    /// Click a widget by exact id.
    pub fn click(&mut self, id: &str) {
        let w = self.widget(id);
        let (x, y) = w.center();
        assert!(w.enabled, "widget {id:?} is disabled");
        self.click_pos(pos2(x, y));
    }

    /// Click the first widget whose id starts with `prefix`.
    pub fn click_prefix(&mut self, prefix: &str) {
        let w = self.widget_prefix(prefix);
        let (x, y) = w.center();
        assert!(w.enabled, "widget {} is disabled", w.id);
        self.click_pos(pos2(x, y));
    }

    /// Click the first widget whose label contains `needle`.
    pub fn click_label(&mut self, needle: &str) {
        let w = self.widget_label(needle);
        let (x, y) = w.center();
        assert!(w.enabled, "widget {} is disabled", w.id);
        self.click_pos(pos2(x, y));
    }

    /// Left-drag between two points (orbit/pan/gizmo): press, move in
    /// steps, release.
    pub fn drag(&mut self, from: Pos2, to: Pos2, modifiers: Modifiers) {
        self.frame(vec![
            Event::PointerMoved(from),
            Event::ModifiersChanged(modifiers),
            Event::PointerButton {
                pos: from,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        let steps = 8;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let pos = pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            self.frame(vec![Event::PointerMoved(pos)]);
        }
        self.frame(vec![
            Event::ModifiersChanged(Modifiers::default()),
            Event::PointerButton {
                pos: to,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            },
        ]);
    }

    /// Mouse wheel scroll at the current hovered position.
    pub fn scroll(&mut self, delta_y: f32, at: Pos2) {
        self.frame(vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, delta_y),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::default(),
            },
        ]);
    }

    // ---- Keyboard input --------------------------------------------------

    /// Press and release a key with modifiers. egui's tracked modifier
    /// STATE only changes on `ModifiersChanged` events (the modifiers
    /// field on Key events is informational), so push the state first
    /// and clear it after — exactly what a real window system does.
    pub fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![
            Event::ModifiersChanged(modifiers),
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ]);
        self.frame(vec![
            Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            Event::ModifiersChanged(Modifiers::default()),
        ]);
    }

    /// Ctrl+Shift+key (command palette etc.).
    pub fn ctrl_shift(&mut self, key: Key) {
        self.key(key, Modifiers::CTRL | Modifiers::SHIFT);
    }

    /// Ctrl+key (undo, save etc.).
    pub fn ctrl(&mut self, key: Key) {
        self.key(key, Modifiers::CTRL);
    }

    /// Type text into whatever currently owns the keyboard focus.
    pub fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_string())]);
    }

    /// Enter.
    pub fn enter(&mut self) {
        self.key(Key::Enter, Modifiers::default());
    }

    /// Click a widget's center even when it is disabled (a no-op click:
    /// disabled widgets must never panic — the empty-stack undo case).
    pub fn click_anyway(&mut self, id: &str) {
        let w = self.widget(id);
        let (x, y) = w.center();
        self.click_pos(pos2(x, y));
    }

    // ---- Viewport helpers -------------------------------------------------

    /// Center of the 3D viewport widget.
    pub fn viewport_center(&self) -> Pos2 {
        let w = self.widget("viewport");
        let (x, y) = w.center();
        pos2(x, y)
    }

    /// A point in the viewport guaranteed to be over empty space (far
    /// corner away from the origin-centered default body).
    pub fn viewport_empty_corner(&self) -> Pos2 {
        let w = self.widget("viewport");
        pos2(w.rect[2] - 30.0, w.rect[3] - 30.0)
    }
}
