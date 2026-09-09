//! Real bridge implementation (debug/test builds). See `super` for the
//! public API; `noop` provides the release-build stubs.

use super::WidgetInfo;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// Frame heartbeat.
static FRAME: AtomicU32 = AtomicU32::new(0);
/// Widgets laid out in the current frame.
static WIDGETS: Mutex<Vec<WidgetInfo>> = Mutex::new(Vec::new());
/// Last published state JSON.
static STATE: Mutex<String> = Mutex::new(String::new());
/// Captured errors (wasm panic hook / explicit reports).
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Queued test actions `(name, payload)`, drained by the app each frame.
static ACTIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Poison-tolerant: a panic mid-frame must not take the bridge down
    // with it — the error report matters more than the poisoned state.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start a frame: bump the heartbeat and clear the widget registry.
pub fn begin_frame() {
    FRAME.fetch_add(1, Ordering::Relaxed);
    lock(&WIDGETS).clear();
}

/// Current heartbeat value.
pub fn frame() -> u32 {
    FRAME.load(Ordering::Relaxed)
}

/// Record a widget (call after it is laid out this frame).
pub fn record(
    id: impl Into<String>,
    label: impl Into<String>,
    kind: &'static str,
    rect: egui::Rect,
    enabled: bool,
) {
    lock(&WIDGETS).push(WidgetInfo {
        id: id.into(),
        label: label.into(),
        kind,
        rect: [rect.min.x, rect.min.y, rect.max.x, rect.max.y],
        enabled,
    });
}

/// All widgets laid out in the current frame (clone).
pub fn widgets() -> Vec<WidgetInfo> {
    lock(&WIDGETS).clone()
}

/// Find the first widget whose id starts with `prefix`.
pub fn find_id(prefix: &str) -> Option<WidgetInfo> {
    widgets().into_iter().find(|w| w.id.starts_with(prefix))
}

/// Find the first widget whose label contains `needle`.
pub fn find_label(needle: &str) -> Option<WidgetInfo> {
    widgets().into_iter().find(|w| w.label.contains(needle))
}

/// Publish the app-state JSON snapshot (called at the end of
/// [`ForgeApp::ui_body`]).
pub fn publish_state(app: &crate::app::ForgeApp) {
    *lock(&STATE) = state_json(app);
}

/// Last published state JSON.
pub fn state() -> String {
    lock(&STATE).clone()
}

/// Record an error from outside the panic path (wasm hooks, actions).
pub fn record_error(msg: impl Into<String>) {
    lock(&ERRORS).push(msg.into());
}

/// All captured errors (clone).
pub fn errors() -> Vec<String> {
    lock(&ERRORS).clone()
}

/// Drop all captured errors (start of a test).
pub fn clear_errors() {
    lock(&ERRORS).clear();
}

/// Queue a test action for the next frame.
pub fn queue_action(name: &str, payload: &str) {
    lock(&ACTIONS).push((name.to_string(), payload.to_string()));
}

/// Drain queued test actions (called once per frame by the app).
pub fn drain_actions() -> Vec<(String, String)> {
    std::mem::take(&mut *lock(&ACTIONS))
}

// ---------------------------------------------------------------------------
// State snapshot JSON (hand-rolled — no serde_json dependency).
// ---------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn num(f: f64) -> String {
    if f.is_finite() {
        format!("{f:.3}")
    } else {
        format!("\"{f}\"") // NaN / inf as strings — valid JSON numbers only
    }
}

fn state_json(app: &crate::app::ForgeApp) -> String {
    let features = app.doc.tree.len();
    let bodies = app.last_evaluation.as_ref().map(|ev| ev.bodies.len());
    let mut s = String::with_capacity(512);
    s.push('{');
    s.push_str(&format!("\"frame\":{},", frame()));
    s.push_str(&format!("\"doc\":\"{}\",", esc(&app.doc.name)));
    s.push_str(&format!("\"modified\":{},", app.doc.modified));
    s.push_str(&format!("\"features\":{features},"));
    s.push_str(&format!(
        "\"bodies\":{},",
        bodies
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".into())
    ));
    s.push_str(&format!("\"selected\":{},", app.selection.items.len()));
    s.push_str(&format!("\"status\":\"{}\",", esc(&app.status)));
    s.push_str(&format!("\"evalPending\":{},", app.eval_pending));
    s.push_str(&format!("\"evalDone\":{},", app.eval_done_count));
    if let Some(d) = app.last_eval_duration {
        s.push_str(&format!("\"lastEvalMs\":{},", num(d.as_secs_f64() * 1e3)));
    }
    s.push_str(&format!("\"measureMode\":{},", app.measure_mode));
    s.push_str(&format!("\"measurePicks\":{},", app.measure_picks.len()));
    if let Some(l) = &app.measure_label {
        s.push_str(&format!("\"measureLabel\":\"{}\",", esc(l)));
    }
    s.push_str(&format!("\"gizmoMode\":\"{:?}\",", app.gizmo_mode));
    s.push_str(&format!("\"paletteOpen\":{},", app.palette_open));
    s.push_str(&format!("\"grid\":{},", app.render_options.show_grid));
    s.push_str(&format!("\"edges\":{},", app.render_options.show_edges));
    s.push_str(&format!("\"pickMode\":\"{:?}\",", app.pick_mode));
    if let Some(sec) = &app.render_options.section {
        s.push_str(&format!(
            "\"section\":{{\"normal\":[{},{},{}],\"offset\":{}}},",
            num(sec.normal[0]),
            num(sec.normal[1]),
            num(sec.normal[2]),
            num(sec.offset)
        ));
    }
    s.push_str(&format!(
        "\"camera\":{{\"distance\":{},\"yaw\":{},\"pitch\":{},\"orthographic\":{}}},",
        num(app.camera.distance),
        num(app.camera.yaw),
        num(app.camera.pitch),
        app.camera.orthographic
    ));
    s.push_str(&format!("\"errors\":{}", lock(&ERRORS).len()));
    // Trim a trailing comma before the closing brace (optional fields).
    if s.ends_with(',') {
        s.pop();
    }
    s.push('}');
    s
}

// ---------------------------------------------------------------------------
// wasm32: expose the bridge on `window.__forgecad` + panic capture.
// ---------------------------------------------------------------------------

/// Install the JS bridge and the panic hook (wasm debug builds). Called
/// from the wasm `main` before the WebRunner starts.
#[cfg(target_arch = "wasm32")]
pub fn install_js_bridge() {
    use wasm_bindgen::prelude::*;

    install_panic_hook();

    let Some(window) = web_sys::window() else {
        return;
    };
    let obj = js_sys::Object::new();

    // Each closure is leaked: the bridge lives for the page lifetime
    // (debug builds only — the production build never runs this).
    let keep = |name: &str, c: Closure<dyn Fn() -> JsValue>| {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(name), c.as_ref());
        std::mem::forget(c);
    };

    let frame = Closure::wrap(Box::new(|| JsValue::from(frame())) as Box<dyn Fn() -> JsValue>);
    keep("frame", frame);

    let widgets =
        Closure::wrap(Box::new(|| JsValue::from_str(&widgets_json())) as Box<dyn Fn() -> JsValue>);
    keep("widgets", widgets);

    let state = Closure::wrap(Box::new(|| JsValue::from_str(&state())) as Box<dyn Fn() -> JsValue>);
    keep("state", state);

    let errors = Closure::wrap(Box::new(|| {
        let list: Vec<String> = errors();
        JsValue::from_str(&errors_json(&list))
    }) as Box<dyn Fn() -> JsValue>);
    keep("errors", errors);

    let clear = Closure::wrap(Box::new(|| {
        clear_errors();
        JsValue::undefined()
    }) as Box<dyn Fn() -> JsValue>);
    keep("clearErrors", clear);

    // The 2-arg action closure needs its own leak helper (its Fn trait
    // type differs).
    let action = Closure::wrap(Box::new(|name: String, payload: String| {
        queue_action(&name, &payload);
        JsValue::undefined()
    }) as Box<dyn Fn(String, String) -> JsValue>);
    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("action"), action.as_ref());
    std::mem::forget(action);

    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__forgecad"), &obj);
}

/// Push panics into the error registry (they still reach the console via
/// the previous hook / eframe's handler, so Playwright sees them twice:
/// once in our registry, once as console errors).
#[cfg(target_arch = "wasm32")]
fn install_panic_hook() {
    use wasm_bindgen::JsValue;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("panic: {info}");
        record_error(msg.clone());
        // Also mirror to the window — headless drivers see it even if
        // the eframe runner swallows the unwinding panic.
        if let Some(window) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &window,
                &JsValue::from_str("__forgecadPanic"),
                &JsValue::from_str(&msg),
            );
        }
        previous(info);
    }));
}

#[cfg(any(target_arch = "wasm32", test))]
fn widgets_json() -> String {
    let list = widgets();
    let mut s = String::with_capacity(96 * list.len() + 8);
    s.push('[');
    for (i, w) in list.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":\"{}\",\"label\":\"{}\",\"kind\":\"{}\",\"rect\":[{},{},{},{}],\"enabled\":{}}}",
            esc(&w.id),
            esc(&w.label),
            w.kind,
            num(w.rect[0] as f64),
            num(w.rect[1] as f64),
            num(w.rect[2] as f64),
            num(w.rect[3] as f64),
            w.enabled
        ));
    }
    s.push(']');
    s
}

#[cfg(target_arch = "wasm32")]
fn errors_json(list: &[String]) -> String {
    let mut s = String::from("[");
    for (i, e) in list.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("\"{}\"", esc(e)));
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_json_separates_entries() {
        lock(&WIDGETS).clear();
        lock(&WIDGETS).push(WidgetInfo {
            id: "tool:undo".into(),
            label: "Undo".into(),
            kind: "button",
            rect: [0.0, 0.0, 30.0, 26.0],
            enabled: false,
        });
        lock(&WIDGETS).push(WidgetInfo {
            id: "btn:Apply".into(),
            label: "Apply".into(),
            kind: "button",
            rect: [40.0, 0.0, 80.0, 26.0],
            enabled: true,
        });
        let json = widgets_json();
        assert!(json.starts_with('[') && json.ends_with(']'));
        assert_eq!(json.matches('{').count(), 2);
        // Hand-rolled JSON must parse — spot-check the separator.
        assert!(
            json.contains("},{"),
            "entries must be comma-separated: {json}"
        );
    }
}
