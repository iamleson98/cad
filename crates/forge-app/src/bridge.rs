//! E2E test bridge: a channel between the running app and external test
//! drivers (the native headless harness + Playwright in a headless
//! browser).
//!
//! Compile gating: the module ALWAYS compiles (call sites stay clean),
//! but the machinery is real only in `cargo test` and debug builds —
//! which includes the trunk debug bundle the browser E2E suite runs
//! against — and a no-op in release builds, so shipped binaries carry
//! zero overhead (the stubs are dead code the optimizer removes).
//!
//! Data channels, refreshed every frame in `ForgeApp::ui_body`:
//! - **Widget registry**: every interactive widget (id, kind, label,
//!   rect, enabled). Tests click the REAL widgets at their REAL rects.
//! - **State snapshot**: a JSON summary of the app state (doc, selection,
//!   evaluation, tools) for assertions.
//! - **Heartbeat**: a frame counter. A stalled counter means the app
//!   stopped rendering — the headless driver detects hangs.
//! - **Errors**: wasm panics are captured by a hook; the native harness
//!   gets panics as test failures directly.
//! - **Action queue**: JS-side `window.__forgecad.action(name, payload)`
//!   pushes requests that `ForgeApp::ui_body` drains next frame — this
//!   drives features that need file bytes (imports) without a native
//!   file dialog.

#[cfg(any(test, debug_assertions))]
mod imp;
#[cfg(any(test, debug_assertions))]
pub use imp::*;

#[cfg(not(any(test, debug_assertions)))]
mod noop;
#[cfg(not(any(test, debug_assertions)))]
pub use noop::*;

/// One interactive widget, as laid out THIS frame (points, origin
/// top-left — identical to CSS pixels at `pixels_per_point == 1`).
#[derive(Debug, Clone)]
pub struct WidgetInfo {
    /// Stable test id, e.g. `tool:undo`, `btn:Apply`, `tree:Box 1`.
    pub id: String,
    /// Human-readable text (menus show their label).
    pub label: String,
    /// `button` / `menu` / `check` / `selectable` / `viewport` / `field`.
    pub kind: &'static str,
    /// `[min_x, min_y, max_x, max_y]` in points.
    pub rect: [f32; 4],
    pub enabled: bool,
}

impl WidgetInfo {
    /// Click center of the widget.
    pub fn center(&self) -> (f32, f32) {
        (
            (self.rect[0] + self.rect[2]) * 0.5,
            (self.rect[1] + self.rect[3]) * 0.5,
        )
    }
}

// ---------------------------------------------------------------------------
// Wrappers: UI call sites route through these so every interactive
// widget lands in the registry. They are byte-identical to the stock
// egui calls — the record is the only addition (a no-op in release).
// ---------------------------------------------------------------------------

/// `ui.button(...)` + registry record (id `btn:{label}`).
pub fn button(ui: &mut egui::Ui, label: impl Into<String> + Clone) -> egui::Response {
    let label = label.into();
    let r = ui.button(label.as_str());
    record(format!("btn:{label}"), label, "button", r.rect, r.enabled());
    r
}

/// `ui.checkbox(...)` + record (id `check:{label}`).
pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    let r = ui.checkbox(checked, label);
    record(
        format!("check:{label}"),
        label,
        "check",
        r.rect,
        r.enabled(),
    );
    r
}
