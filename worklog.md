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
