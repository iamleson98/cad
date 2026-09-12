# ForgeCAD Worklog

---
Task ID: 1
Agent: main (Super Z)
Task: Design and build the intensive E2E test harness (desktop-first, wasm inherits the desktop app for headless-browser testing), find and fix the button-click crashes, wire the suite into CI/CD with fast compile caching.

Work Log:
- Restored environment after sandbox reset: rustup (stable 1.98.1), wasm32-unknown-unknown target, trunk 0.21.14, repo clone (github.com/iamleson98/cad @ eee5f2b), git credentials + PAT.
- Audited the app crate (app.rs ui flows, ui.rs widget call sites, gizmo, viewport, palette actions, background eval worker) for panic-prone paths and enumerated every interactive widget.
- Implemented the in-app E2E bridge (`crates/forge-app/src/bridge.rs`), cfg-gated to `any(test, debug_assertions)` so release builds carry zero overhead: per-frame widget registry (id/label/rect/enabled), app-state JSON snapshot, frame heartbeat, panic/error registry, action queue; wasm JS exposure `window.__forgecad` (frame/widgets/state/errors/clearErrors/action) + panic hook.
- Refactored `ForgeApp`: `new_headless()` (no eframe CC, no GPU renderer, no crash/autosave recovery — deterministic tests), `assemble()` shared constructor, extracted `ui_body()` from the eframe::App impl (one code path driven two ways), added eval sent/done counters.
- Wired widget recording into theme::tool_button, ui.rs (buttons, menus, tree rows, palette rows, checkboxes), viewport.rs (viewport rect).
- Built the native headless harness (`harness.rs`): synthetic egui pointer/keyboard input at real widget rects (hover/press/release 3-frame clicks, drags with ModifiersChanged, wheel, key chords), wall-clock `wait_for_eval` (sent==done counters — the boolean eval_pending misreports with overlapping requests).
- Wrote the intensive native suite (`ui_tests.rs`): 43 tests — boot/layout, every toolbar button incl. empty-scene edge cases, all 5 primitives + undo/redo, tree select/suppress/delete (context menu), inspector apply, sketch→solve→extrude, palette keyboard+click, union, measure 2-pick + empty click, section toggle/flip, camera orbit/zoom/nav-cube, gizmo drags, params, save, bridge import (ASCII STL), rapid soak. Serialized via a global lock (bridge statics are process-global).
- Fixed 6 real bugs the harness surfaced:
  1. gizmo.rs `(a.r() + b.r()) / 2` u8 overflow — panic on every selection (gizmo plane handles) → THE user-visible "click and the app quits" crash;
  2. T/R shortcut self-deadlock: `ctx.egui_wants_keyboard_input()` (read lock) inside `ctx.input()` (write lock) → 10s deadlock panic;
  3. left panel: tree's full-height ScrollArea pushed the params panel off-screen (y=972 on a 900px window) — P-01 unreachable; one shared ScrollArea now;
  4. viewport pick `renderer.lock().unwrap()` — poison cascade (any first panic makes every later pick fatal); now poison-tolerant;
  5. stl.rs `count * 50` overflows 32-bit usize on wasm (ASCII file's header parsed as ~1.8e9 triangles) → import panic in the browser build; 64-bit math now;
  6. key-chord coalescing race: shortcuts read end-of-frame `i.modifiers` state; a fast chord (all events in one frame) silently loses Ctrl+Shift — now matched per-event.
- Browser layer: `e2e/` (Playwright 1.62 + cached chromium-1234), zero-dep static server (`serve.mjs`, correct wasm MIME + COOP/COEP), bridge client (`helpers/forge.ts`), 22 specs (boot, toolbar/empty-export, solids/tree, sketch/palette/union/params, measure/section/keyboard/camera, downloads/import/gizmo). Frame wiggles force on-demand repaints (egui idle = no frames by design); waitForEval tolerates wasm sync-eval page freezes.
- Found + fixed the browser-only crash: Dawn software WebGPU loses the device (wgpu createBuffer RangeError) — debug builds now force the WebGL2 backend (`WebOptions` → `Backends::GL`); production keeps WebGPU.
- CI: new `e2e` job (fmt gate → parallel): debug trunk build (no wasm-opt = fast), own rust-cache lineage, cached Playwright browsers, apt deps, artifacts on failure. Existing fmt / 3-OS build-test / wasm jobs unchanged.
- Local verification: fmt clean, clippy native all-targets + wasm clean, 207 workspace tests green (43 new E2E), 22/22 browser tests green (~4.7 min under SwiftShader).

Stage Summary:
- Deliverables: two-layer E2E harness (native 43 tests + browser 22 tests) sharing one app code path through the debug-only in-app bridge; 6 real crash/UX bugs fixed; CI runs the browser suite headlessly on every push.
- Key insight recorded: the user's desktop crash was the gizmo u8 overflow (selection → panic); the desktop app and browser build share it, so the browser E2E also guards the desktop.
- Next: push, watch the first CI e2e run (cold cache ~15 min, warm ~5), then continue TODO items (W-02 section polish, W-05 display modes, W-08 measurement modes, W-01 gizmo refinement) with the harness as the regression net.

---
Task ID: 2
Agent: main (Super Z)
Task: Fix the CI wasm release-build failure, land the full pipeline green.

Work Log:
- The W-11 push failed the wasm job: `trunk build --release` hit 13 E0433 "cannot find bridge in crate" errors — the bridge module was cfg-gated out of release builds while its call sites (ui.rs/theme.rs/viewport.rs, bridge::button/checkbox wrappers) compiled unconditionally. Root cause: local verification only ran dev-profile checks; release profiles compile a different cfg matrix.
- Restructured the bridge: `src/bridge.rs` (shell: WidgetInfo + always-compiled wrappers using `record`), `src/bridge/imp.rs` (real machinery, cfg any(test, debug_assertions)), `src/bridge/noop.rs` (zero-cost stubs, release). Call sites dropped their cfg gates; apply_test_action always compiles.
- Verified the full matrix locally: dev + release x native + wasm32 all compile clean; 43 native E2E + 22 browser E2E still green.
- CI hardening: added a native release-profile check step (ubuntu job) so `cargo build --release` breakage (what desktop users ship) is caught on every push, not only via the wasm release bundle.
- Pushed 7d3ce8f; run 34314849991: ALL SIX JOBS GREEN — rustfmt, 3-OS check+clippy+test, wasm32 (release trunk bundle), e2e browser suite (22 tests, 4 min warm).

Stage Summary:
- The complete fast-iteration loop the user asked for now exists: change code → cargo test -p forge-app (1.3 s, 43 UI tests) → npx playwright test (~5 min, 22 browser tests) → push → CI runs everything headlessly in ~4-8 min warm.
- Lesson recorded: ALWAYS verify release profiles when using cfg-gated modules (dev checks hide it); CI now enforces it on both targets.

---
Task ID: 3
Agent: main (Super Z)
Task: Deep research Inventor-parity gaps, update the TODO with CAM/sheet-metal/assembly waves, and build the CAM subsystem (forge-cam crate + app integration + E2E + CI).

Work Log:
- Deep research (web): Inventor feature matrix (part/assembly/drawings/sheet metal), InventorCAM/HSM strategies (adaptive clearing, waterline, peck drilling, AFR), Rust CAM landscape (heightfield/marching-squares is the standard 3-axis mesh approach).
- TODO.md: inventor-parity re-anchor (header + "beat Inventor" checklist bar), new Wave 8 (C-01..C-09 CAM), Wave 9 (SM-01..SM-05 sheet metal), Wave 10 (A-01 re-sequenced as the assembly/CAM foundation).
- Sandbox reset mid-session: re-provisioned rustup 1.98.1 + clippy/rustfmt, restored PAT credentials, re-cloned at 3a7bf24, re-applied the TODO + crate work.
- New crate `forge-cam` (7th workspace member): tool.rs (Tool/ToolKind/ToolMaterial/ToolLibrary + Material speeds&feeds advisor data), field.rs (HeightField: sparse-batch parallel rasterization — memory O(covered), flat/ball dilation → CL field, marching squares with saddle disambiguation + hash-grid stitching), path.rs (Move/Feeds/Toolpath + length/time/segment stats), strategy.rs (rough/face/waterline/drill, segment_clear collision-checked stay-down links, climb-consistent loop orientation, nearest-neighbor drilling), setup.rs (Stock/Setup/safe_z), post.rs (Fanuc-style post: modal G0/G1, G81/G83 peck, T/M6, G43, % markers, deterministic).
- forge-model: public `hole_placements` (hole feature → world centers/diameters/top/bottom through the sketch's datum plane).
- forge-render: generic `OverlayLines` scene overlays (alpha-blended line pipeline, un-pickable, version bump only on change) + CAM_STOCK/CAM_FEED/CAM_RAPID colors.
- forge-app: `cam.rs` (CamState: library/ops/stock/gcode/overlays; compute against the merged body mesh — concatenation is exact for the heightfield kernel; auto-invalidation on re-eval), `cam_panel` bottom dock (op list w/ suppress+delete, strategy params, tool picker, display toggles, stock margins, Compute, Export G-code native `.nc` + wasm download), toolbar CAM toggle, G-code export plumbing.
- 6 native E2E CAM tests (panel toggle, add-op+compute+overlays+gcode, all four strategies, suppress/delete, re-eval invalidation, display toggles) + harness `scroll_at`/`scroll_to_widget` helpers; 8 CAM unit tests; 29 forge-cam crate tests.
- Bugs found & fixed while testing: part_mesh index-offset-after-extend panic (out-of-bounds triangles), CAM eye/checkbox clicks landing below the panel's scroll fold (reordered panel so ops render first), checkbox label colliding with "Compute toolpaths" (renamed), overlays not rebuilding when all display toggles off (unconditional rebuild, change-detected version bump).
- Verified: cargo fmt clean; clippy -D warnings clean (native dev+release, wasm32); 250 workspace tests green (18 suites); native release build (the shipped profile) compiles.

Stage Summary:
- Deliverables: full CAM core (C-01), G-code post (C-02), CAM UI dock (C-03), toolpath visualization overlays (C-04), hole recognition (C-06) — the "beat Inventor CAD/CAM" differentiator now exists end-to-end in-process: model → strategies → visible toolpaths → .nc program, all CI-scriptable.
- Key insight: the mesh kernel is a CAM asset, not a liability — heightfield CAM over concatenated body meshes is exact, robust, and needs no B-Rep.
- Next: C-05 material-removal simulation (drives tool animation), then K-03 fillet/chamfer + F-05 shell (part-modeling gaps), then push CI watch loop.

---
Task ID: 4
Agent: main (Super Z)
Task: C-05 material-removal simulation: stock grid, per-move removal, gouge detection, viewport ghost, E2E; verify CI green for the CAM push.

Work Log:
- forge-cam/src/sim.rs: MaterialSim (height grid @ stock top; flat-disc/ball-sphere stamps swept along segments with Z interpolation — ramps and plunges exact; drill cycles stamp columns; rapids remove nothing AND reset the prev-point so plunges never sweep phantom cuts), SimReport (volume/percent/gouges with grid-resampled part comparison + sub-cell tolerance), stepped stock mesh export.
- Simulator found 2 real bugs (exactly its purpose): CL-field dilation under-covered the true tool footprint by up to half a grid cell → strategy cl_field now dilates by radius + cell/2 (conservative); the sim's own apply() swept from the last cut point after rapids — fixed with prev=None on Rapid.
- App: cam_simulate(), remaining-stock ghost body (sentinel SIM_BODY_ID, translucent GLASS_SIM) in rebuild_scene, "Simulate" button, "stock sim" toggle (rebuilds the scene on change), sim report badge (green 0 gouges / amber N).
- E2E: cam_simulate_reports_and_renders_stock (report + ghost body + toggle), cam_simulate_without_compute_is_a_clean_noop.
- Full verification: fmt clean; clippy -D warnings clean native (dev, release, all-targets) + wasm32; 259 workspace tests / 18 suites green.
- CI for the CAM push 2c32650: ALL 6 JOBS GREEN (rustfmt, 3-OS test matrix, wasm32 trunk bundle, browser e2e 22 specs).

Stage Summary:
- The CAM loop is now closed end-to-end: model → strategies → toolpaths (visible) → simulation (verified gouge-free, % removed) → G-code (.nc), with the browser E2E guarding every button.
- The simulator doubles as a toolpath verifier in CI (zero-gouge assertion on roughing) — the beginning of the "trustworthy CAM" story.
- Next: K-03 fillet/chamfer + F-05 shell (part modeling gaps Inventor users expect), then browser-side e2e specs for CAM flows, then A-01 multi-body.
