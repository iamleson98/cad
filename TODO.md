# ForgeCAD — Road to World-Class: TODO

Gap analysis derived from a feature-matrix comparison against production 3D CAD
systems (SolidWorks, Fusion 360, Onshape, FreeCAD, Shapr3D) and open-source
Rust CAD research (Fornjot, truck, opencascade-rs, KittyCAD solver
experiments). Items are grouped by subsystem, priority-ranked (P0 = blocks
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
      gate job, per-OS rust-cache, fail-fast disabled, and a weekly cron
      to keep caches warm. First 3-platform run green.*

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

- [ ] **S-03 Ellipse entity (native)** `(M)` `P1`
      Center-ellipse with semi-axes + tilt: solver DOFs `[cx, cy, rx, ry,
      θ]`, ellipse-arc variant, contour sampling, point-on-ellipse
      constraint. Requires solver packing extension.
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
- [ ] **W-03 Depth-peeled transparency** `(M)` `P1`
      Dual depth peeling (8–16 layers) replacing sorted blending; correct
      interpenetrating transparent geometry (documented in render TODO).
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
- [ ] **I-02 3MF export/import** `(M)` `P2`
      ZIP container + XML mesh (with units + optional color), production
      3D-print format; import for mesh bodies.
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

## Wave 5 — Assembly & drawing subsystems

- [ ] **A-01 Multi-body part documents** `(M)` `P1`
      Bodies list (independent solids per document with per-body
      visibility/material/name); foundation for assemblies and patterns
      producing multiple bodies.
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
- [ ] **K-03 Fillet/chamfer (mesh approximate)** `(M)` `P2`
      Edge-chain discovery (dihedral threshold) → rolling-ball surface
      replacement on the triangle mesh; exact fillets deferred to K-06.
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

- [ ] **PR-01 Crash reporter + telemetry opt-in** `(S)` `P2`
      Panic hook → local report file with document snapshot; opt-in
      anonymous usage stats.
- [ ] **PR-02 Plugin/scripting API** `(L)` `P3`
      Rust-ABI stable trait surface + Rhai or WASM scripting for user
      commands (Onshape FeatureScript equivalent).
- [ ] **PR-03 Packaging & auto-update** `(M)` `P2`
      GitHub Release artifacts (Windows MSI, macOS DMG, Linux AppImage),
      signed, with delta updates.
- [ ] **PR-04 i18n + HiDPI + accessibility** `(M)` `P3`
      fluent-rs catalogs, scale-aware egui, keyboard-only modeling.
- [ ] **PR-05 Performance dashboard** `(S)` `P2`
      Frame-time, eval-time, memory stats in status bar (debug builds) +
      regression benchmark in CI (criterion).
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
