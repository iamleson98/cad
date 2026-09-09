//! ForgeCAD entry point: native desktop app **and** wasm browser build
//! (W-10) — same binary target, selected by `cfg`.
//!
//! - Native: crash reporter + env_logger + `eframe::run_native`.
//! - wasm: `WebLogger` to the browser console + `WebRunner` on the
//!   `#forgecad_canvas` element (trunk processes this bin's wasm and
//!   injects the bootstrap script into `index.html`).

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    const CANVAS_ID: &str = "forgecad_canvas";

    // Redirect `log` to console.log and friends:
    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    // E2E bridge (debug builds only): exposes window.__forgecad (widget
    // registry, state, heartbeat, errors, action queue) for the
    // headless-browser test suite. Compiled out of release bundles.
    #[cfg(debug_assertions)]
    forge_app::bridge::install_js_bridge();

    let window = web_sys::window().expect("no browser window");
    let document = window.document().expect("no document");
    let canvas = document
        .get_element_by_id(CANVAS_ID)
        .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .unwrap_or_else(|| panic!("canvas #{CANVAS_ID} not found"));

    let mut web_options = eframe::WebOptions::default();
    // Debug (E2E) bundles force the WebGL2 backend: the Dawn software
    // WebGPU adapter in headless Chromium loses the device under the
    // multi-pass renderer (wgpu createBuffer panics), while SwiftShader
    // WebGL2 is rock solid. Production keeps the default backend
    // selection (WebGPU when available, WebGL2 otherwise).
    #[cfg(debug_assertions)]
    if let egui_wgpu::WgpuSetup::CreateNew(ref mut create_new) = web_options.wgpu_options.wgpu_setup
    {
        create_new.instance_descriptor.backends = wgpu::Backends::GL;
    }
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(forge_app::ForgeApp::new(cc)))),
            )
            .await
            .expect("failed to start ForgeCAD (WebRunner)");
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    // PR-01: crash reporter first — any later panic (init included)
    // produces a report + document snapshot.
    forge_app::install_hook();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("ForgeCAD — Rust/WebGPU 3D CAD"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "ForgeCAD",
        options,
        Box::new(|cc| Ok(Box::new(forge_app::ForgeApp::new(cc)))),
    )
}
