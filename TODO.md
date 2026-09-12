# ForgeCAD — Road to World-Class: TODO

Gap analysis derived from a feature-matrix comparison against production 3D CAD
systems (**Autodesk Inventor — the reference target: part modeling, assembly,
drawings, sheet metal, and integrated CAM**; also SolidWorks, Fusion 360,
Onshape, FreeCAD, Shapr3D) and open-source Rust CAD research (Fornjot, truck,
opencascade-rs, KittyCAD solver experiments). Items are grouped by subsystem, priority-ranked (P0 = blocks
daily use, P1 = expected by any professional user, P2 = competitive parity,
P3 = differentiators), and ordered into implementation waves.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `(S)` small ≤ 1 day ·
`(M)` medium 2–5 days · `(L)` large > 1 week.

---

## Wave 1 — Production hygiene & core parametric features (current sprint)

- [x] **T-01 CI pipeline** `(S)` `P0`
      GitHub Actions: `cargo fmt --check`, `cargo clippy --workspace -- -D
      warnings`, `cargo test --workspace`, cached builds, on every push/PR.
      *Done: `.github/workflows/ci.yml` — now a full 3-OS matrix
      (ubuntu/windows/macos) on every push, with fmt as a fast separate
      gate job, per-OS rust-cache, fail-fast disabled, a weekly cron
      to keep caches warm, and a dedicated wasm32 job (W-10: check +
      clippy + trunk bundle). First 3-platform run green.*

- [x] **F-01 Mirror feature** `(S)` `P0`
      Reflect a body across a datum plane (point + normal); winding order
      corrected for the handedness flip; Join/Cut/New operations.
      *Done: `Feature::Mirror` + `TriMesh::mirrored` + tests (volume
      symmetry, orientation preserved).*

- [x] **F-02 Linear pattern feature** `(M)` `P0`
      N instances of a seed body along a 3D direction with spacing,
      optional symmetric distribution; instances combined by union or
      cut/joined against a target (SolidWorks-equivalent semantics).
      *Done: `Feature::LinearPattern` + tests (volume scales with count,
      cut semantics verified).*

- [x] **F-03 Circular pattern feature** `(M)` `P0`
      N instances rotated about an arbitrary axis (point + direction) over
      an angle span, equal or custom angular pitch.
      *Done: `Feature::CircularPattern` + tests (6 instances at 60°,
      axis off-origin).*

- [x] **S-01 Slot sketch macro** `(S)` `P1`
      Straight slot: two arc endpoints + two tangent line sides as
      constrained entities (exactly how Onshape/Fusion implement the slot
      tool). Center-to-center distance and radius remain parametric.
      *Done: `Sketch::add_slot` + tangency/parallel/coincidence constraints
      + chain test.*

- [x] **S-02 Regular polygon sketch macro** `(S)` `P1`
      N-sided polygon (3–64) from center + circumscribed radius, as line
      entities with equal-length sides and angle constraints.
      *Done: `Sketch::add_polygon` + tests (area ≈ ½·n·r²·sin(2π/n)).*

## Wave 2 — Sketch depth & modeling completeness

- [x] **S-03 Ellipse entity (native)** `(M)` `P1`
      Center-ellipse with semi-axes + tilt: solver DOFs `[cx, cy, rx, ry,
      θ]`, ellipse-arc variant, contour sampling, point-on-ellipse
      constraint. Requires solver packing extension.
      *Done: `SketchEntity::Ellipse` (5 DOF: `[cx, cy, rx, ry, tilt]`) +
      `EllipseArc` (7 DOF: + `a0, a1`, CCW in the tilted frame —
      parametric point `c + R(θ)·(rx cos a, ry sin a)`). Solver:
      `PointOnEllipse` (1 eq) on the implicit curve
      `(u/rx)² + (v/ry)² − 1` with the full analytic Jacobian
      (dF/dP, dF/dC, −2u²/rx³, −2v²/ry³, 2uv(1/rx²−1/ry²)); Center and
      Start/End roles carry exact Jacobians for arcs. Sampling: full
      ellipses are standalone closed contours (tessellated sweep, mean
      radius drives step count); ellipse arcs chain with lines/arcs.
      Extrusion works end-to-end (volume ≈ π·rx·ry·h test). Mirror
      (S-08 extension): tilt → 2φ−θ, arc params a → −a with role swap;
      exact reflection verified per sampled point; constraint linkage
      is center-symmetric (+ swapped endpoints for arcs) — a full
      shape-linkage constraint kind is deferred (tilt wrap-around
      equivalence needs dedicated handling). UI: palette "New Sketch:
      ellipse on XY" (15×8, 30° tilt). 7 solver/geometry tests +
      1 end-to-end extrude test.*
- [x] **S-04 Remaining sketch constraints** `(S)` `P1`
      Symmetric (about line/point), midpoint-on, point-on-curve (arc/
      spline/ellipse), equal-radius already ok → add "equal length"
      pairwise, tangent-arc-arc at point.
      *Done: `Symmetric` (midpoint-on-line + perpendicular, 2 eq),
      `MidpointOn`, `PointOnCircle` (circles and arcs), all with analytic
      Jacobians + convergence tests. Remaining sub-items: symmetric about
      a **point**, point-on-spline, tangent-arc-arc at a shared point —
      folded into S-03's solver-packing extension.*
- [x] **S-05 Sketch diagnostics UI** `(S)` `P1`
      Live DOF readout ("3 DOF remaining"), over-constrained /
      conflicting-constraint highlighting in tree + viewport badges.
      *Done: `Evaluation::sketch_reports` carries per-sketch solve
      reports (DOF, equations, residual); the feature tree shows a
      "N DOF / over-constrained / fully constrained" badge, the status
      bar shows live DOF of the selected sketch, the inspector shows the
      full solver report. Viewport badges land with W-04 (face/edge
      selection overlay).*
- [ ] **S-06 Offset entities** `(M)` `P2`
      Offset a contour chain by distance (creates new constrained
      geometry, handles chain-corner intersections).
- [ ] **S-07 Trim / extend** `(M)` `P2`
      Trim-to-intersection on line/arc chains; extend to head.
- [x] **S-08 Sketch mirror tool** `(S)` `P2`
      Mirror selected entities about a line, creating symmetric constraints.
      *Done: `Sketch::mirror_entities` — reflection about any line entity
      (point/line/circle/arc/spline; arc angle pairs map as θ → 2φ−θ with
      start/end roles swapped). Constraint sets are exactly rank-complete
      per kind (point 2 eq, line 4, circle 3 + EqualRadius, arc 5 with
      role-swapped Symmetric + EqualRadius) — no redundancy, DOF balance
      unchanged. UI: inspector mirror section (combo to pick the symmetry
      line, one-click mirror of the other entities, solve + undo-able
      edit) + palette command. Tests: reflection exactness, circle/arc
      geometry + role swap, perturb-and-resolve restores symmetry, bad
      input rejection.*
- [x] **F-04 Hole feature (compound)** `(M)` `P1`
      Standard holes: simple / counterbore / countersink, diameter, depth,
      drill-point angle, placed at sketch points; evaluated as boolean cut
      stack on cylinders/cones.
      *Done: `Feature::Hole` builds each hole as a **single solid of
      revolution** (cylinder + counterbore/countersink frustum + drill
      point from one revolve profile — no internal boolean interfaces),
      rigidly oriented along the sketch plane normal (Positive/Negative/
      Both), 1 mm entry overshoot avoids coplanar-face degeneracy.
      Volume-exact tests incl. counterbore/countersink/drill point and
      multi-placement; hole consumes its target; inspector hole wizard;
      palette command.*
- [ ] **F-05 Shell / hollow** `(L)` `P2` — needs face-level selection (see
      W-04) or a B-Rep kernel; documented in `forge-geometry/src/detail.rs`.
- [x] **F-06 Draft / taper on extrude** `(M)` `P2`
      Extrude with per-side slope (loft trick: section pair, one offset-
      scaled). Reuses loft path.
      *Done: `ExtrudeParams.draft_angle` (radians, serde-default 0 — old
      files unchanged). The far cap is scaled toward the section centroid
      by k = 1 − tan(draft)·h_wall/r_mean (clamped ≥ 0.02 so extreme
      angles degenerate, never flip; |draft| < 80° validated). Scaling is
      an affine map, so the same triangulation + contour correspondence
      stay valid — walls become ruled surfaces (the scaled-section loft
      trick). Symmetric + draft evaluates as two mirrored frusta sharing
      the sketch-plane section (a single both-caps-scaled mesh would lose
      the waist). Inspector draft slider (±30°, live), feature label
      shows ≈N°. Frustum volumes exact in tests; RON backward-compat
      tested.*
- [ ] **F-07 Thin-wall / rib extrude mode** `(M)` `P2`
      Offset open profiles by thickness and cap ends (sheet-metal basics).
- [x] **P-01 User parameter table + expressions** `(M)` `P1`
      Named parameters (mm/deg), expression evaluator (`width = 2*th + 1`),
      dimensions reference parameters; UI table; document serialization.
      Foundation for all later parametric depth.
      *Done: recursive-descent expression evaluator (precedence, `^`,
      functions, `deg()`; cycles and domain errors reported by name);
      `Param::expression` + `Document::resolve_params` (fixpoint
      dependency order); `DimBinding` side-table drives extrude distance,
      revolve angle, hole diameter/depth, pattern spacing, mirror offset,
      primitive dim A; undoable `SetParam`/`SetBinding` commands with
      dirty-marking so cached features re-evaluate; parameters panel in
      the left panel; binding fields in the inspector; RON roundtrip
      (serde defaults keep old files loadable).*
- [x] **D-01 Datum planes & axes beyond the three datums** `(S)` `P1`
      Offset-from-face, at-angle, through-edge/through-two-points planes;
      selectable as sketch carriers and mirror/pattern references.
      *Done: `Feature::Datum` (Offset and Angle kinds) as first-class
      construction features; `SketchPlane::DatumRef` resolves through the
      tree at evaluation time (suppressed/missing datum → clear feature
      error); datum edits dirty dependent sketches; palette commands for
      datum creation, sketch-on-datum, mirror-across-datum. Remaining
      sub-items: through-edge / through-two-points / offset-from-face
      datums need W-04 face-edge selection.*

## Wave 3 — Viewport interaction & rendering parity

- [x] **W-01 3D drag manipulator (gizmo)** `(M)` `P0`
      Translate/rotate screen-space handles with axis picking, drag plane
      raycast, snapping (1 mm / 5°), replacing inspector sliders for
      transforms. Selected-body + feature-instance drag.
      *Done: `gizmo.rs` — translate arrows + plane handles (ray-line
      closest-point / ray-plane intersection, 1 mm snap, Shift = off) and
      rotation rings (signed in-plane angle about world axes, 5° snap).
      Drives `Feature::TransformBody`, which now *consumes* its source
      (move semantics) and gained a `pivot` (serde-default origin, old
      files load unchanged): rotation = `pivot + R·(p−pivot) + t`, so
      rings spin the body about the gizmo anchor (pivot rebased at grab,
      world-preserving via `T + (I−R)(p−p')`). Bodies without a transform
      get a wrapper feature; exactly ONE undo command per drag (Add or
      Edit, zero-delta drags discarded). Keyboard T/R, toolbar Move/
      Rotate toggles, palette commands, constant screen size, hover
      highlight, away-axis dimming, pick-click suppression while a
      handle owns the press. Multi-body drag lands with A-01.*
- [x] **W-02 Section view (clipping)** `(S)` `P1`
      Arbitrary clipping plane per viewport: GPU-side clip distance in WGSL
      + cap-plane rendering (stencil technique) or exposed cutaway
      (simplest: clip only). Toolbar toggle + plane manipulator.
      *Done: `SectionPlane` in every pass — mesh/transparent fragments,
      feature-edge lines (separate clipped/unclipped line pipelines so the
      ground grid survives the cut), and picking (clipped geometry can
      not be selected). Toolbar: toggle + axis (X/Y/Z) + offset slider +
      flip. Cap-plane rendering deferred to W-04 face selection.*
- [x] **W-03 Depth-peeled transparency** `(M)` `P1`
      Dual depth peeling (8–16 layers) replacing sorted blending; correct
      interpenetrating transparent geometry (documented in render TODO).
      *Done: **front depth peeling** — `peel_layers` iterations (default
      8, `RenderOptions`) of a depth-only peel pass + a per-layer
      under-blend pass, then one residual pass for everything behind
      the last layer. Peel pass: survivors strictly behind the previous
      layer write depth into a ping-pong `Depth32Float` attachment
      (Less test min-accumulates → the next front layer); the blend
      pass keeps fragments within `LAYER_EPS` of that layer and
      premultiplied-under-blends into an `Rgba16Float` accumulation
      (front-to-back, no CPU sorting — `GpuBody.centroid` removed).
      Residual = one unsorted pass for K+1-nested and farther layers.
      The composite pass composites `accum.rgb + (1-a)·scene` before
      edge-detect/tone-map (background included; wireframe mode keeps
      its pre-peel look). Section clip + X-ray alpha apply in every
      peel fragment; blend passes depth-test against opaque depth only.
      Bounds live in depth attachments (no 32F color blending —
      Metal-safe). Tests: every WGSL source now parses + validates
      headless via `wgpu::naga` (the same validator wgpu runs), which
      immediately caught a latent composite-shader type bug
      (`textureLoad` returned `vec4` into an `f32`). Dual peeling
      (front+back per iteration, half the passes) remains an
      optimization option if 8 layers ever show.*
- [x] **W-04 Face/edge/vertex selection model** `(M)` `P1`
      Picking granularity beyond bodies: ray→BVH→triangle → face cluster
      (coplanar/normal-threshold flood), edge chains (sharp edge sets),
      vertex snap. Prerequisite for fillet/chamfer/shell/draft UI.
      *Done: `TriMesh::face_cluster` (BFS flood vs the seed normal, 1°
      tol — flat patches; curved faces select as narrow bands until the
      B-Rep kernel lands), `cluster_boundary_edges`, and
      `sharp_edge_chains` (sharp edges grouped by *collinear
      continuation* through shared vertices — |dot| tangency: a cube
      yields 12 single-edge chains, a cylinder rim one closed loop; the
      fillet-run grouping). App side: `picking.rs` — a Bodies/Faces/
      Edges/Vertices granularity combo in the toolbar + 4 palette
      commands; sub-picks raycast via the shared per-body BVH
      (`raycast_bodies`, refactored out of the measure tool). Ids are
      canonical and deterministic (face = cluster's min triangle index,
      edge chain = min vertex index, vertex = position index).
      Highlights: translucent face fill + boundary outline, chain
      polylines, vertex dots (egui overlay). `New sketch on selected
      face` creates a `SketchPlane::Face` sketch (centroid + outward
      normal, rectangle sized to 40% of the cluster bbox) — extrude/cut
      directly on faces. Deferred: fillet/chamfer/shell UI (K-03/F-05),
      cap-plane section rendering, edge/edge + face/face measure modes.*
- [x] **W-05 Display modes** `(S)` `P2`
      Wireframe / hidden-line (depth-precision lines) / shaded / shaded+
      edges / x-ray (opacity slider) / section — viewport toolbar presets.
      *Done: toolbar combo — Shaded / Wireframe (hidden-line: flat ghost
      surfaces + screen-space edge-detect line art + all feature edges,
      0° threshold) / X-Ray (alpha override routes every body through the
      sorted transparency pass). Section view = W-02. Wireframe-of-curved-
      surfaces shows feature edges, not tessellation diagonals (by
      design: coplanar edges are excluded). Opacity slider folded into
      xray_alpha; per-body material editor = W-07.*
- [ ] **W-06 Ambient occlusion (SSAO/GTAO)** `(M)` `P3`
      Half-res depth+normal GTAO pass, bilateral upsample; brings parity
      with Fusion/Onshape viewport quality.
- [ ] **W-07 Materials & studio lighting** `(M)` `P3`
      PBR material editor (metal/rough workflow), env-map (split-sum or
      prefiltered), 3-point studio presets, scene background options.
- [x] **W-08 Measurement tool** `(S)` `P2`
      Two-pick measure (point-point, edge-edge, face-face distances,
      angles) with persistent viewport labels.
      *Done: measure mode (palette + toolbar) picks exact surface points
      via CPU ray→BVH→Möller–Trumbore (rebuilt per evaluation), shows
      the span line + persistent viewport label with distance and
      hit-face normal angle; empty-space click restarts; third click
      starts a fresh measurement. Edge/edge + face/face distance modes
      land with W-04 sub-body selection.*
- [ ] **W-09 Infinite ground + shadows** `(S)` `P3`
      Shadow-only ground plane (shadow-mapped), grid fade, horizon.

- [x] **W-10 WASM browser build** `(M)` `P2`
      Same Rust code, compiled to `wasm32-unknown-unknown`, running in
      the browser via WebGPU/WebGL2 + egui-wgpu: instant-shareable CAD
      with no install.
      *Done: `crates/forge-app` is now dual-target. `main.rs` runs the
      `eframe::WebRunner` on `#forgecad_canvas` (`index.html` + trunk
      pipeline, `Trunk.toml`); wasm-only `web.rs` turns exports into
      browser downloads (blob URLs). Threading: the eval worker runs
      synchronously on wasm (`background::EvalWorker`, no threads),
      tokio/env_logger stay native-only via `cfg` gates. Timing:
      `forge_core::time` re-exports `web-time` (`std::time::Instant`
      panics on wasm32). WebGL2 renderer port, verified in-browser by
      an autonomous agent (agent-browser + VLM screenshots): depth
      peeling and the composite's depth read moved off depth-texture
      sampling (`textureLoad` on depth textures is a WebGL2 no-go) —
      peel bounds and a readable depth copy now live in R32Float color
      targets; stencil ops dropped on Depth32Float attachments; line
      depth bias replaced by a clip-space nudge in the shader; grid now
      composites over the background (premultiplied `t_color`). Fixed
      a lost-in-refactor regression: `Renderer::update_scene` was never
      called, so nothing rendered at all (both targets). Exports
      download; mesh import works via drag-and-drop. CI gained a
      dedicated wasm job (check + clippy + trunk bundle).*

- [x] **W-11 Intensive E2E test harness (native + browser)** `(M)` `P0`
      Every button, menu, tool and feature exercised by real input
      events: a headless Rust harness in `cargo test` (all 3 OS) plus
      Playwright + headless Chromium driving the actual wasm app in
      CI. The regression net for a fast-evolving app.
      *Done — two layers, one code path:
      (1) **In-app E2E bridge** (`forge-app/src/bridge.rs`, compiled
      into debug/test builds only, zero overhead in release): a
      per-frame widget registry (id/label/rect/enabled of every
      interactive widget), an app-state JSON snapshot, a frame
      heartbeat, a panic/error registry and an action queue
      (`window.__forgecad.action(...)`) that drives features needing
      file bytes (imports) without a file dialog.
      (2) **Native harness** (`harness.rs` + `ui_tests.rs`): the REAL
      `ui_body` (the exact code the desktop app runs) against a bare
      `egui::Context`, driven by synthetic pointer/keyboard events at
      real widget rects — 43 tests: boot/layout, every toolbar button
      incl. empty-scene edge cases, every Solid primitive, tree
      select/suppress/delete, inspector apply, sketch→solve→extrude,
      palette (keyboard + click), booleans, measure (2 picks, empty
      click), section toggle/flip, camera orbit/zoom/nav-cube, gizmo
      drags, params, undo/redo cycles, save, import (bridge action),
      rapid-interaction soak. Runs in ~1.3 s.
      (3) **Browser suite** (`e2e/`, Playwright 1.62 + headless
      Chromium): 22 tests mirroring the native flows against the trunk
      DEBUG wasm bundle (bridge active via `debug_assertions`),
      plus browser-specific coverage: export/save DOWNLOADS, bridge
      import, garbage-import robustness, screenshots for visual
      inspection. WebGL2 is forced in debug builds (Dawn software
      WebGPU loses the device under the multi-pass renderer;
      SwiftShader GL is stable) — production keeps WebGPU.
      CI: new `e2e` job (debug trunk build — no wasm-opt, fast —
      cached rust artifacts + cached Playwright browsers + apt deps;
      artifacts uploaded on failure).
      **Bugs the harness caught and fixed on day one:**
      - gizmo.rs u8 color-blend overflow — panicked (app quit) every
        time a selection showed the gizmo plane handles;
      - T/R shortcut self-deadlock — `ctx.egui_wants_keyboard_input`
        (read lock) called inside `ctx.input` (write lock) froze and
        killed the app;
      - params panel pushed off-screen by the tree's full-height
        ScrollArea (P-01 unreachable on default windows);
      - renderer-mutex poison cascade on viewport picks (a first
        panic turned every later click fatal);
      - STL triangle-count multiplication overflow on wasm32 (32-bit
        usize) crashing browser imports;
      - key-chord coalescing race — fast Ctrl+Shift+P could lose the
        modifiers and silently not open the palette (now matched per
        event).*


## Wave 4 — Interoperability

- [x] **I-01 STL/OBJ import as mesh bodies** `(S)` `P1`
      Import → weld → normal-consistency repair → mesh body feature;
      enables boolean workflow on imported meshes. (Export already done.)
      *Done: `forge_io::import_mesh` + OBJ reader (quad fan-triangulation,
      a/t/b face entries, negative indices) + `TriMesh::repair_orientation`
      (BFS manifold-edge propagation + volume-sign outward fix) —
      `Feature::ImportedMesh` embeds the repaired mesh in the document
      (RON round-trip tested), booleans/patterns/mirrors work on imported
      bodies (volume-exact cut test). UX: drag-and-drop .stl/.obj onto the
      window + palette import command. 3MF still open (I-02).*
- [x] **I-02 3MF export/import** `(M)` `P2`
      ZIP container + XML mesh (with units + optional color), production
      3D-print format; import for mesh bodies.
      *Done: `forge_io::threemf` — hand-rolled OPC/ZIP writer
      (deflate via pure-Rust `miniz_oxide`) + reader (central directory,
      stored AND deflate entries, CRC-verified, Zip64 rejected with a
      clear error). Minimal streaming XML scanner (quote-aware `>` in
      attributes, namespace stripping, entity escape/unescape, comments/
      CDATA skipped). Import: model part discovered via package
      relationships (extension fallback), `<model unit>` scaling (micron
      → meter), multi-object packages → one body per object,
      `<build><item>` 4×3 row-vector transforms, recursive `<components>`
      expansion with cycle guard, then the standard weld/repair pipeline.
      Export: one object per body + build items; names XML-escaped.
      App: Export 3MF palette command, .3mf drag-and-drop + import
      command (multi-object imports as `file:object` bodies). Not
      supported: materials/colors + extension namespaces (ignored on
      read). Tests: multi-body roundtrip (incl. escaped names), stored-
      entry ZIP fixture with transforms, unit scaling, components
      expansion, malformed + CRC-corruption rejection, XML scanner edge
      cases.*
- [ ] **I-03 STEP AP242 export** `(L→Phase 6)` `P1`
      Via `opencascade-rs` optional feature: tessellated→B-Rep (sewn
      shells) → STEP; exact-geometry path when Wave 6 kernel lands.
      The `StepExchange` trait already defines the seam.
- [ ] **I-04 DXF/DWG sketch import** `(M)` `P2`
      2D sketch exchange (ezdxf-rs or hand-rolled DXF subset), profiles
      become constrained entities.
- [ ] **I-05 glTF import + USD glTF-level parity** `(M)` `P3`
- [x] **I-06 Native format versioning/migration** `(S)` `P2`
      RON schema version field + migration tests (forward one version).
      *Done: `format_version` now serde-defaults to 0 (pre-versioning
      legacy files deserialize); `forge_model::migrate_document` walks a
      stepwise v0→v1→… pipeline (v0→v1 = allocator hygiene: reserve the
      id counter above every tree feature id so post-load additions
      never collide). `forge_io::load_document` runs migrations after the
      newer-version rejection check. Tests: allocator-collision repair,
      idempotence, unknown-version path, hand-written legacy v0 fixture
      (missing field) loads + migrates + re-saves at current, future
      version rejected. Contributor rules documented in `migrate.rs`.*

## Wave 8 — Inventor parity: integrated CAM subsystem (the "beat Inventor" wave)

Inventor ships Inventor CAM/HSM in-process: feature recognition, 2.5D/3-axis
strategies, stock simulation, G-code post. ForgeCAD's mesh kernel is actually
well-suited to CAM (waterline slicing of a mesh is the standard 3-axis CAM
input); this wave builds a full CAM module as a new `forge-cam` crate plus app
integration. Naming: `C-xx`.

- [x] **C-01 CAM core crate (`forge-cam`)** `(M)` `P0`
      Tool library (flat/bull-nose/ball end mills + drills, feeds & speeds
      defaults, units); machine setup (stock box / from-body bounds, WCS
      origin, safe Z, clearance); heightfield CAM kernel (top-surface
      rasterization, tool-footprint dilation → cutter-location field,
      marching-squares waterlines) on any `TriMesh`; 2.5D strategies —
      facing, raster roughing with stepover/stepdown and stay-down links,
      waterline contour finishing, drilling (hole centers + peck cycles);
      time & path-length estimates. Fully headless + unit-tested (no UI
      deps).
      *Done: new `forge-cam` crate (7th workspace member). Heightfield
      kernel: sparse-batch parallel rasterization (O(covered) memory),
      flat/ball tool dilation → CL field, marching squares with saddle
      disambiguation + hash stitching. Strategies: raster roughing with
      collision-checked stay-down links (every link verified against the
      CL field — gouge-free by construction), facing, waterline finishing
      with climb-milling-consistent loop orientation, peck drilling with
      nearest-neighbor ordering + tool-vs-hole fit warnings. 29 unit
      tests incl. determinism, floor-z, gouge guards.*
- [x] **C-02 G-code post-processor** `(S)` `P1`
      Generic Fanuc-style 3-axis post: G0/G1 rapids & feeds, G81/G83 peck
      drilling with R-plane, tool changes (T/M6), spindle (S/M3), program
      header/footer + percent markers, modal feed formatting; line
      numbering option; fixture offset G54. Regression: golden-file tests
      for each strategy on a fixed part.
      *Done: `forge-cam::post` — Fanuc-style 3-axis post with modal
      motion+feed suppression, G81/G83 drill cycles (R-plane, Q peck,
      G80 cancel), tool changes (T/M6, G43 H), spindle, header/footer
      with `%` markers, per-op comments with stats. Ops without cutting
      moves are skipped. Golden assertions per strategy; byte-identical
      determinism test.*
- [x] **C-03 CAM UI: setup + strategy panel** `(M)` `P1`
      New CAM mode/tab in the app: stock preview (ghost box), tool pick
      from library (table + edit), strategy selection, per-strategy
      parameters (depth of cut, stepover, stock to leave, feed rates,
      spindle), compute button with progress, operation list with
      per-op suppress/delete (like the feature tree), unit-aware.
      *Done: `ForgeApp.cam` + `cam_panel` bottom dock (toggle from the
      toolbar DRILL button): op list with suppress-eye/delete, per-strategy
      param editor (tool picker, DOC, stepover, leave, floor Z, peck),
      display toggles, stock margins, Compute + Export G-code, status
      line with cut length / est. time / hole count. CAM results auto-
      invalidate on re-evaluation. 6 new native E2E tests + 8 unit tests
      (incl. the harness scroll helper for below-fold widgets).*
- [x] **C-04 Toolpath visualization** `(M)` `P1`
      Viewport overlay: polylines per op, rapid vs feed moves styled
      differently, tool-position animation (play/step), collision-free
      depth-coloring; toggle visibility per operation. Rendered via the
      existing line pipeline (forge-render feature edges reuse).
      *Done: `Scene.overlays: Vec<OverlayLines>` — new generic line-
      overlay path in the renderer (alpha-blended line pipeline, never
      pickable). CAM feed moves render cyan, rapids amber, stock ghost
      as a 12-edge box; per-op visibility = suppress; version bump only
      on change. Tool-position play/step animation folded into C-05
      (simulation will drive it).*
- [x] **C-05 Stock & material removal simulation** `(M)` `P2`
      Heightfield (2.5D) material grid updated per move → voxel-ish color
      map of remaining stock, "in-process" part comparison, cut-vs-gouge
      report. This is what makes CAM feel trustworthy.
      *Done: `forge-cam::sim` — `MaterialSim` height grid initialized to
      the stock top, per-move tool stamps (flat disc / ball sphere along
      swept segments incl. ramps & plunges; drill columns; rapids cut
      nothing and correctly sever cut-link state), volume + percent
      removed, gouge detection against the raw part field with sub-cell
      tolerance, remaining-stock mesh export. App: Simulate button +
      report badge (green/amber) + translucent ghost body in the viewport
      ("stock sim" toggle). Two solver bugs found by the simulator:
      (1) CL-field dilation under-covered the true tool disc by half a
      cell → now dilates conservatively by radius + cell/2; (2) the sim
      itself swept phantom cuts across parts after rapids (prev-point
      not reset). Both fixed + regression tests. 8 sim unit tests + 2
      E2E.*
- [x] **C-06 Hole feature recognition → drill ops** `(S)` `P2`
      Detect `Feature::Hole` placements (already parametric!) and
      auto-generate peck-drill operations with cycle depths from the
      feature (Inventor's AFR-lite for holes).
      *Done: `forge_model::hole_placements` resolves every Hole feature's
      sketch (points/circle centers) through its datum plane to world
      XYZ + entry/depth; `CamState::holes_from_doc` feeds the Drill
      strategy directly. Recognition fixture test with point + circle
      placements.*
- [ ] **C-07 Rest machining / 3D strategies** `(L)` `P3`
      Steep+shallow finishing, pencil passes, 3D roughing with Z-level
      rest detection (needs C-05's material grid).
- [ ] **C-08 Speeds & feeds advisor** `(S)` `P3`
      Material + tool lookup table (chipload per material), surface-speed
      → RPM/feed calculation, warnings on unrealistic values.
- [ ] **C-09 CLSF / machine simulator import** `(P3)` — deferred.
      Verifying third-party G-code round-trip is nice-to-have.

## Wave 9 — Sheet metal (Inventor parity)

- [ ] **SM-01 Sheet-metal rules + base face** `(M)` `P2`
      Thickness, K-factor per bend, bend radius default; base face from
      sketch profile extruded to thickness.
- [ ] **SM-02 Flange + bend authoring** `(M)` `P2`
      Pick model edge → flange with angle/bend; bend unroll math (K-factor
      bend allowance); corner reliefs.
- [ ] **SM-03 Flat pattern + DXF export** `(S)` `P2`
      Flatten the flange graph to a 2D outline (bend lines marked) and
      export DXF for laser/waterjet cutting — pairs with CAM (C-xx) as
      the classic laser workflow.
- [ ] **SM-04 Punch/emboss features** `(P3)` — deferred.
- [ ] **SM-05 Unfold/refold state toggle** `(M)` `P3`.

## Wave 10 — Multi-body → assembly ramp (explicitly re-sequenced)

A-01 is the foundation both for assembly (occurrences reference bodies) and
for CAM (stock = body, ops target bodies). Do it before A-02.

- [ ] **A-01 Multi-body part documents** `(M)` `P1` *(first)*
      Bodies list (independent solids per document with per-body
      visibility/material/name); foundation for assemblies and patterns
      producing multiple bodies. Today: extrude-New/imports/booleans
      already make multiple `EvalBody`s — what's missing is body-level
      identity across re-evals (stable ids), per-body visibility, a
      bodies panel, and body-to-body boolean join/cut by selection.
- [ ] **A-02 Assembly documents + occurrences** `(L)` `P2`
      Instance graphs referencing part documents, rigid transforms per
      occurrence, per-occurrence override color/suppress.
- [ ] **A-03 Standard mates** `(L)` `P2`
      Coincident/axis-align/distance-angle/planar contacts with solver
      reuse (the 2D LM solver generalizes; 6-DOF per occurrence).
- [ ] **A-04 Interference detection** `(M)` `P3`
      Pairwise BSP intersection volume, report table.
- [ ] **A-05 BOM generation** `(S)` `P3`
      Auto table (part name, qty, custom columns) export CSV.
- [ ] **DR-01 Drawing sheets** `(L)` `P3`
      Orthographic projections (first/third angle), sections from W-02,
      dimensioning, title blocks, PDF/SVG export. (Breaks into DR-01a view
      engine / DR-01b dimensioning / DR-01c title block + export when
      started.)
- [ ] **A-02 Assembly documents + occurrences** `(L)` `P2`
      Instance graphs referencing part documents, rigid transforms per
      occurrence, per-occurrence override color/suppress.
- [ ] **A-03 Standard mates** `(L)` `P2`
      Coincident/axis-align/distance-angle/planar contacts with solver
      reuse (the 2D LM solver generalizes; 6-DOF per occurrence).
- [ ] **A-04 Interference detection** `(M)` `P3`
      Pairwise BSP intersection volume, report table.
- [ ] **A-05 BOM generation** `(S)` `P3`
      Auto table (part name, qty, custom columns) export CSV.
- [ ] **DR-01 Drawing sheets** `(L)` `P3`
      Orthographic projections (first/third angle), sections from W-02,
      dimensioning, title blocks, PDF/SVG export.

## Wave 6 — Geometry kernel maturity

- [ ] **K-01 Robust CSG tolerancing** `(L)` `P1`
      BSP epsilon policy (per-predicate tolerances), near-degenerate plane
      classification fallbacks, fuzz corpus (random primitive soup,
      property-test in CI).
- [ ] **K-02 T-junction healing on booleans** `(M)` `P2`
      Post-boolean edge matching: insert T-vertices to restore exact
      watertight topology (currently volume-exact, topologically open).
- [x] **K-03 Fillet/chamfer (mesh approximate)** `(M)` `P2`
      Edge-chain discovery (dihedral threshold) → rolling-ball surface
      replacement on the triangle mesh; exact fillets deferred to K-06.
      *Done: boolean-cutter approach in `forge-geometry::detail` —
      `EdgeSpec` (positions, survives re-tessellation) → resolved frame
      (area-weighted face clustering survives T-junction bridge slivers
      and split faces; convexity via cross-face normal test) → cross-
      section polygon → production extrude prism → Difference (convex
      ridges) / Union (concave valleys). Fillets = tangent-polygon arc
      (r²(1−π/4)·L cross-section, the standard fillet-weld area; arc-side
      selection by midpoint distance; the atan2-arg-order bug class
      documented). PEN overshoot breaks coplanar boolean faces. Features
      `Chamfer`/`Fillet` in the tree (consume their target, parametric
      distance/radius via DimField bindings), palette actions + Modify
      toolbar menu from edge-chain selections, inspector sliders, 3 E2E
      tests (apply + exact volume + undo, no-selection guard, menu
      reachability) + 8 geometry unit tests.*
- [ ] **K-04 Incremental tessellation cache** `(M)` `P2`
      Per-feature mesh caching with invalidation only on param change
      (already fingerprinted) + LOD per zoom; keeps 10k-feature docs at
      60 fps.
- [x] **K-05 Frustum culling + draw batching** `(S)` `P2`
      Per-body AABB culling on CPU; instance-buffer rendering for patterns.
      *Done: `forge_render::cull` — Gribb–Hartmann plane extraction from
      the view-projection (normalized) + positive-vertex AABB test, with
      degenerate bounds always visible. Applied to every body pass:
      opaque, feature-edge lines, transparency (pre-sort) and GPU picking
      (a culled body cannot be picked). Tests: identity-matrix ground
      truth, behind-eye / beyond-far / far-off-axis culling, degenerate
      AABBs. Instance batching is moot for patterns: they evaluate into
      single merged meshes (one draw call already); revisit with K-04
      LOD instancing.*
- [ ] **K-06 B-Rep kernel integration (truck)** `(L→Phase 4)` `P1`
      Exact-geometry fillet/chamfer/shell/offset on B-Rep with mesh output
      for rendering; feature tree maps 1:1 to kernel operations. The
      `detail.rs` stubs define the API.
- [ ] **K-07 NURBS curves/surfaces in sketches & lofts** `(M)` `P2`
      Weighted control points, knot editing, G1/G2 loft continuity.

## Wave 7 — Production hardening

- [x] **PR-08 Modern UI + icon system** `(M)` `P1`
      Cohesive visual identity: an icon font on every button/row (no more
      ad-hoc unicode glyphs), refined dark theme, real typography, smooth
      hover animation.
      *Done: Lucide icon font (ISC) embedded as the `icons` family —
      `theme.rs` installs fonts (Inter regular + semibold for text,
      Lucide for glyphs) and a forge-ember dark theme (charcoal
      surfaces, 6 px rounding, roomier spacing, accent matching the
      viewport selection tint). `icons.rs` is generated from
      `assets/lucide.css` (`scripts/gen_icons.py`, 70 curated glyphs,
      fails loudly on unknown names); regression tests pin constants to
      the shipped CSS and font coverage. Toolbar: grouped animated icon
      buttons (`theme::tool_button` — 0.12 s hover fade, pinned accent
      state, tooltips with shortcut hints) + 3MF added to the export
      menu. Feature tree: per-feature-kind icons, DOF/err badges, eye
      suppress toggle, right-click + ⋮ context menu. Status bar: icon
      stat chips (bodies/tris/fps/eval). Command palette: icons,
      ↑/↓/↵ keyboard navigation, hover sync, search focus. Inspector
      header shows the feature icon; panel section headers with icons.*

- [x] **PR-01 Crash reporter + telemetry opt-in** `(S)` `P2`
      Panic hook → local report file with document snapshot; opt-in
      anonymous usage stats.
      *Done (local half): `forge_app::crash` — panic hook chained after
      the default one writes a report (version, timestamp, panic +
      location, backtrace) and a document snapshot to
      `<temp>/forgecad_crash/`; the snapshot refreshes after *every*
      mutation (rides the evaluation request — parametric data only,
      cheap) so recovery is fresher than the 2-minute autosave; startup
      prefers the newest of {crash snapshot, autosave} and says which.
      Never panics in the panic path (lock poisoning recovered, writes
      best-effort). Telemetry: deliberately NOT implemented — reports
      stay on disk, nothing is sent anywhere; any future opt-in is an
      explicit user decision, never silent.*
- [ ] **PR-02 Plugin/scripting API** `(L)` `P3`
      Rust-ABI stable trait surface + Rhai or WASM scripting for user
      commands (Onshape FeatureScript equivalent).
- [ ] **PR-03 Packaging & auto-update** `(M)` `P2`
      GitHub Release artifacts (Windows MSI, macOS DMG, Linux AppImage),
      signed, with delta updates.
- [ ] **PR-04 i18n + HiDPI + accessibility** `(M)` `P3`
      fluent-rs catalogs, scale-aware egui, keyboard-only modeling.
- [x] **PR-05 Performance dashboard** `(S)` `P2`
      Frame-time, eval-time, memory stats in status bar (debug builds) +
      regression benchmark in CI (criterion).
      *Done (dashboard half): status bar now shows bodies, triangles,
      rolling-average fps + frame ms, and the wall duration of the last
      evaluation (measured inside the eval worker, reported with the
      result). Shown in all builds — it is one cheap label, and eval >
      frame time is expected (geometry runs on the background worker).
      Criterion CI benchmarks remain open (fold into K-04's tessellation
      cache work where the regression risk actually lives).*
- [ ] **PR-06 User docs** `(M)` `P2`
      mdBook guide: quickstart, every tool, troubleshooting; in-app help
      (F1) reusing the same source.
- [x] **PR-07 Fuzz + property CI job** `(S)` `P2`
      cargo-fuzz on RON parser, boolean corpus, solver random sketches.
      *Done (boolean corpus): `forge-geometry/tests/fuzz_csg.rs` — 48
      deterministic random primitive pairs × 3 ops per run, asserting
      structural sanity (indices, NaN), volume monotonicity per op,
      outward orientation, and error discipline (only empty results may
      fail). Runs in every `cargo test` on all 3 platforms (~2 s).
      `FORGE_FUZZ_SEED` env re-seeds; verbose mode tracks the
      T-junction closedness statistic (38/105 strictly closed today —
      K-02's backlog). Remaining: cargo-fuzz on the RON parser and
      random-sketch solver corpus (fold into K-01).*

---

## Definition of "world-class, production-ready" (checklist)

1. Every feature above P0/P1 closed, or explicitly roadmap-gated with a
   documented stub (no silent gaps).
2. CI green on three platforms (Linux/Windows/macOS) with clippy `-D
   warnings` and fuzz corpus.
3. Performance budget: < 16 ms frame at 1 M triangles; < 200 ms full
   re-eval of a 200-feature document; autosave never blocks > 50 ms.
4. Interop round-trips: STEP AP242 out, STL/OBJ/3MF in/out, native format
   versioned.
5. Crash-safe: any panic recoverable to last autosave (already true) +
   crash reporter (PR-01).
6. Docs shipped; every UI control reachable by keyboard.
7. **Inventor-parity bar (the "beat Inventor" definition):** parametric
   part features incl. fillet/chamfer/shell; multi-body; assemblies with
   mates; drawings out; sheet-metal flat pattern to DXF; and the differentiator
   — integrated CAM: 2.5D strategies with visible toolpaths, stock
   simulation, and verified G-code post, all in-process and scriptable in
   CI (C-01 … C-06).
