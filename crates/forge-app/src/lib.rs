//! # forge-app
//!
//! The ForgeCAD desktop application shell (eframe + egui + wgpu).
//!
//! Threading model (see the SRS architecture chapter):
//! - **UI thread**: egui panels, camera, selection, the document.
//! - **Eval worker thread**: owns the geometry evaluation cache
//!   (`rayon` parallelism inside the kernel).
//! - **Tokio runtime**: background file I/O (exports).
//! - **GPU**: the multi-pass `forge-render` renderer driven through
//!   `egui_wgpu` paint callbacks.

pub mod app;
pub mod background;
pub mod gizmo;
pub mod palette;
pub mod picking;
pub mod ui;
pub mod viewport;

pub use app::ForgeApp;
