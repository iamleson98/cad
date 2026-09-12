//! Scene description: what the renderer draws.

use forge_core::BodyId;
use forge_geometry::TriMesh;

/// Display style of a body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyStyle {
    /// Base color (linear RGB, 0..1) and alpha.
    pub color: [f32; 4],
    /// Treat as transparent (sorted blending pass).
    pub transparent: bool,
}

impl Default for BodyStyle {
    fn default() -> Self {
        Self {
            color: [0.72, 0.74, 0.78, 1.0],
            transparent: false,
        }
    }
}

/// One drawable body.
#[derive(Debug, Clone)]
pub struct SceneBody {
    /// Body id (picking identity).
    pub id: BodyId,
    /// Display name.
    pub name: String,
    /// Tessellated mesh (owned copy; rebuilt by the evaluator).
    pub mesh: TriMesh,
    /// Display style.
    pub style: BodyStyle,
}

/// Analysis line overlay drawn with the line pipeline (world-space
/// segments, constant color — C-04 CAM toolpaths, future sketch/debug
/// overlays). Overlays are never pickable.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLines {
    /// World-space segment list.
    pub segments: Vec<([f32; 3], [f32; 3])>,
    /// Linear RGBA color.
    pub color: [f32; 4],
}

/// The whole scene, versioned for GPU upload tracking.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Drawable bodies.
    pub bodies: Vec<SceneBody>,
    /// Line overlays (C-04: CAM toolpaths; unlit, un-pickable).
    pub overlays: Vec<OverlayLines>,
    /// Show sharp-feature edge lines (FR-RD-03).
    pub show_edges: bool,
    /// Dihedral threshold (degrees) for what counts as a sharp edge.
    pub edge_angle_deg: f64,
    /// Draw a ground reference grid.
    pub show_grid: bool,
    /// Background color (linear RGB).
    pub background: [f32; 4],
    /// Bumped by the app whenever the scene content changes.
    pub version: u64,
}

impl Scene {
    /// Empty scene.
    pub fn new() -> Self {
        Self {
            bodies: Vec::new(),
            overlays: Vec::new(),
            show_edges: true,
            edge_angle_deg: 40.0,
            show_grid: true,
            background: [0.118, 0.121, 0.133, 1.0],
            version: 0,
        }
    }

    /// Replace the body list and bump the version.
    pub fn set_bodies(&mut self, bodies: Vec<SceneBody>) {
        self.bodies = bodies;
        self.version += 1;
    }

    /// Force a GPU buffer rebuild on the next frame (e.g. after changing
    /// `show_edges` or `edge_angle_deg` without new bodies).
    pub fn touch(&mut self) {
        self.version += 1;
    }

    /// Highlight color for selected bodies.
    pub const SELECTED: [f32; 4] = [0.98, 0.66, 0.16, 1.0];
    /// Default body color.
    pub const DEFAULT: [f32; 4] = [0.72, 0.74, 0.78, 1.0];
    /// Transparent body color.
    pub const GLASS: [f32; 4] = [0.65, 0.78, 0.92, 0.35];
    /// CAM feed-move color (cyan).
    pub const CAM_FEED: [f32; 4] = [0.16, 0.92, 0.86, 1.0];
    /// CAM rapid-move color (amber, thin).
    pub const CAM_RAPID: [f32; 4] = [0.95, 0.66, 0.19, 0.55];
    /// CAM stock-ghost color.
    pub const CAM_STOCK: [f32; 4] = [0.85, 0.88, 0.95, 0.12];
}
