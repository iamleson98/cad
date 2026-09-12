//! # ForgeCAM — 2.5/3-axis CAM for ForgeCAD
//!
//! Integrated CNC toolpath generation, mirroring what Autodesk Inventor
//! bundles as Inventor CAM/HSM: strategies computed in-process from the
//! parametric model, visible toolpaths, and a G-code post — but fully
//! headless and CI-scriptable.
//!
//! Architecture:
//!
//! - [`tool`] — tool & material libraries (cutters, drills, feeds/speeds),
//! - [`setup`] — stock billet, safe heights,
//! - [`field`] — heightfield kernel: rasterize any [`forge_geometry::TriMesh`]
//!   into a top-surface grid, dilate by the tool footprint (flat or ball)
//!   to get the *cutter-location field*, and extract marching-squares
//!   waterline contours,
//! - [`strategy`] — 2.5D strategies: raster roughing, facing, waterline
//!   finishing, drilling (peck cycles), all with gouge-safe stay-down
//!   links verified on the CL field,
//! - [`path`] — the toolpath IR (rapid/feed/plunge/drill moves + stats),
//! - [`post`] — Fanuc-style 3-axis G-code post-processor.
//!
//! Design constraints honored throughout:
//! - **No UI dependencies** — the whole crate runs headless in `cargo test`
//!   and in CI,
//! - **Deterministic** — same input ⇒ byte-identical G-code,
//! - **wasm32-safe** — rayon is compiled out on wasm (serial fallback),
//! - **Gouge-free by construction** — every cut point and link move is
//!   validated against the dilated CL field.
//!
//! ```no_run
//! use forge_cam::{setup::{Setup, Stock}, tool::{Tool, ToolLibrary}, strategy::{self, RoughParams}, path::Feeds, post::{self, PostOptions}};
//! use forge_core::{Point3, Vector3};
//! use forge_geometry::primitives;
//!
//! // Part: a 6×6×4 boss.
//! let mesh = primitives::box_from_center_extents(
//!     Point3::new(0.0, 0.0, 2.0),
//!     Vector3::new(6.0, 6.0, 4.0),
//! );
//! let stock = Stock::around_mesh(&mesh, 3.0, 1.0).expect("non-empty");
//! let setup = Setup::from_stock(stock);
//! let tool = ToolLibrary::starter().get(0).expect("T1").clone();
//! let result = strategy::rough(
//!     &mesh,
//!     &setup,
//!     &tool,
//!     Feeds::default(),
//!     &RoughParams::default(),
//! );
//! let gcode = post::post(&[result.path], &PostOptions::default());
//! println!("{gcode}");
//! ```

pub mod field;
pub mod path;
pub mod post;
pub mod setup;
pub mod strategy;
pub mod tool;

pub use field::HeightField;
pub use path::{Feeds, Move, Toolpath};
pub use setup::{Setup, Stock};
pub use strategy::{CamResult, DrillParams, Hole, RoughParams, WaterlineParams};
pub use tool::{Material, Tool, ToolKind, ToolLibrary, ToolMaterial};
