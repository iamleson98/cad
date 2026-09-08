# ForgeCAD

A modern, high-performance desktop **3D CAD application in Rust**, rendering through
**WebGPU** ([`wgpu`](https://wgpu.rs) 30 → Vulkan / Metal / DirectX 12) with an
**egui** interface running directly on the GPU canvas.

This repository is the **v0.1 milestone** of the phased roadmap in
`docs/ForgeCAD-SRS-and-Architecture.pdf`: a complete, compiling, tested
foundation covering the 2D parametric sketcher, the solid modeling kernel
(extrude / revolve / loft / sweep / CSG booleans), the multi-pass WebGPU
viewport, the undo/redo command stack, file I/O and the desktop shell.

## Quick start

```bash
cargo run --release -p forge-app        # launches the forgecad desktop app
cargo test --workspace                  # 70 unit/integration tests
```

Linux requires the usual GUI stack (X11 or Wayland; `libxkbcommon`); macOS and
Windows work out of the box. A Vulkan/Metal/D3D12-capable GPU or a software
fallback adapter (llvmpipe / lavapipe) is needed at runtime.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `forge-core` | `f64` math (nalgebra), typed ids, units, tessellation settings |
| `forge-geometry` | Mesh kernel: triangulation with hole bridging, primitives, extrude/revolve/loft/sweep, BSP CSG booleans, BVH raycasting |
| `forge-sketch` | 2D parametric sketcher: entities, geometric/dimensional constraints, Levenberg–Marquardt solver, contour chaining |
| `forge-model` | Document: feature tree DAG, evaluation with dirty propagation + `catch_unwind`, undo/redo command stack |
| `forge-io` | STL (binary/ASCII), OBJ, glTF 2.0, versioned native RON format, STEP interface (Phase 6) |
| `forge-render` | WebGPU multi-pass renderer: PBR-ish shading, edge lines, transparency, GPU picking, screen-space edge detection |
| `forge-app` | eframe/egui shell: viewport, feature tree, inspector, command palette, autosave, background evaluation |

## Threading model

- **UI thread** – egui panels, camera, selection, document ownership.
- **Eval worker** – owns the geometry evaluation cache; kernel math is
  `rayon`-parallel; the UI never blocks on geometry (NFR-PER-01).
- **Tokio runtime** – background file I/O for exports.
- **GPU** – multi-pass rendering driven through `egui_wgpu` callbacks.

## Using the app

- `Ctrl+Shift+P` – command palette (add solids, sketches, extrude, booleans,
  view commands, exports).
- Orbit: middle-drag (or `Alt`+left-drag) · Pan: `Shift`+middle · Zoom: wheel.
- Click bodies to select (GPU picking); the inspector edits parameters with
  live preview.
- The navigation cube (top-right) snaps standard views.
- `Ctrl+Z` / `Ctrl+Y` – unbounded undo/redo; `Ctrl+S` – save.
- Autosave every 2 minutes with crash recovery on next launch.

## Precision & performance notes

- All CAD math is `f64`; only GPU vertex buffers are `f32` (NFR-PREC-01).
- The CSG kernel is an exact-volume BSP implementation; seam T-junctions are
  a documented v0.1 limitation (geometrically sealed, rendering-correct),
  resolved by the Phase-4 B-Rep integration.
- CSG boolean results integrate to machine precision (see
  `forge-geometry/src/boolean.rs` tests).

## License

MIT OR Apache-2.0.
