//! ForgeCAD application entry point.

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
