//! # forge-render
//!
//! The WebGPU rendering engine of ForgeCAD (FR-RD-01 … FR-RD-04).
//!
//! Renders through a multi-pass pipeline on `wgpu` 30:
//! 1. opaque PBR-ish mesh pass (HDR color + view-space normal MRT),
//! 2. feature-edge line pass (slope-scaled depth bias),
//! 3. sorted transparency pass (depth-peeling: Phase-2 roadmap),
//! 4. on-demand GPU picking pass (color-ID + 1×1 async readback),
//! 5. composite pass with screen-space edge detection (Sobel on
//!    depth/normal), tone mapping and background.
//!
//! The renderer is decoupled from the UI toolkit: `forge-app` drives it
//! through [`Renderer::render`] (recorded from `egui_wgpu`'s
//! `prepare` callback) and [`Renderer::composite`] (recorded into the UI
//! render pass).

pub mod camera;
pub mod renderer;
pub mod scene;
pub mod shaders;

pub use camera::{Camera, CameraUniform};
pub use renderer::{DisplayMode, PickResult, RenderOptions, Renderer, SectionPlane};
pub use scene::{BodyStyle, Scene, SceneBody};

/// Render crate error type.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// GPU request failure.
    #[error("wgpu error: {0}")]
    Wgpu(String),
    /// No suitable adapter.
    #[error("no suitable GPU adapter found")]
    NoAdapter,
}
