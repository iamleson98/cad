//! Release-build no-op stubs: same signatures as `imp`, zero behavior.
//! The optimizer removes the calls entirely; shipped binaries carry no
//! test machinery.

use super::WidgetInfo;

pub fn begin_frame() {}

pub fn frame() -> u32 {
    0
}

#[inline(always)]
pub fn record(
    _id: impl Into<String>,
    _label: impl Into<String>,
    _kind: &'static str,
    _rect: egui::Rect,
    _enabled: bool,
) {
}

pub fn widgets() -> Vec<WidgetInfo> {
    Vec::new()
}

pub fn find_id(_prefix: &str) -> Option<WidgetInfo> {
    None
}

pub fn find_label(_needle: &str) -> Option<WidgetInfo> {
    None
}

pub fn publish_state(_app: &crate::app::ForgeApp) {}

pub fn state() -> String {
    String::new()
}

pub fn record_error(_msg: impl Into<String>) {}

pub fn errors() -> Vec<String> {
    Vec::new()
}

pub fn clear_errors() {}

pub fn queue_action(_name: &str, _payload: &str) {}

pub fn drain_actions() -> Vec<(String, String)> {
    Vec::new()
}
