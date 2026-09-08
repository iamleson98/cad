//! # forge-app
//!
//! The ForgeCAD application shell (eframe + egui + wgpu): native desktop
//! binary **and** wasm32 browser build (W-10).
//!
//! Threading model (see the SRS architecture chapter):
//! - **UI thread**: egui panels, camera, selection, the document.
//! - **Eval worker thread**: owns the geometry evaluation cache
//!   (`rayon` parallelism inside the kernel). On wasm (no threads) the
//!   worker runs synchronously in [`background::EvalWorker`] instead.
//! - **Tokio runtime**: background file I/O (exports), native only.
//! - **GPU**: the multi-pass `forge-render` renderer driven through
//!   `egui_wgpu` paint callbacks.

pub mod app;
pub mod background;
pub mod crash;
pub mod gizmo;
pub mod icons;
pub mod palette;
pub mod picking;
pub mod theme;
pub mod ui;
pub mod viewport;

/// Browser-only helpers (downloads, W-10).
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::ForgeApp;
pub use crash::install_hook;
