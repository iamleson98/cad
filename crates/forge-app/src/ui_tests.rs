//! Intensive E2E UI suite (native headless): every button, every menu,
//! every feature — including the empty-scene edge cases that a user
//! clicking around a fresh install hits. Panics anywhere in a click path
//! fail the test here (this is the suite that hunts "I clicked a button
//! and the app quit" bugs).
//!
//! The browser layer (`e2e/`) mirrors these flows through the wasm
//! bundle; keep the test names aligned for cross-referencing failures.

use crate::harness::{serial, Harness};
use egui::{pos2, Key, Modifiers};
use forge_model::PrimitiveKind;

/// Bridge error registry must stay empty across every test.
fn assert_no_bridge_errors() {
    let errors = crate::bridge::errors();
    assert!(errors.is_empty(), "bridge captured errors: {errors:#?}");
}

fn body_count(h: &Harness) -> usize {
    h.app
        .last_evaluation
        .as_ref()
        .map(|ev| ev.bodies.len())
        .unwrap_or(0)
}

/// Open the Solid menu and click one primitive.
fn add_solid(h: &mut Harness, kind: PrimitiveKind) {
    h.click("menu:Solid");
    h.frames(2);
    let id = format!("solid:{kind}");
    h.click(&id);
    h.frames(2);
}

// ---------------------------------------------------------------------------
// Boot + layout
// ---------------------------------------------------------------------------

#[test]
fn boot_layout_registers_core_toolbar() {
    let _s = serial();
    let h = Harness::new();

    // Every toolbar widget the first frame should expose.
    let expected = [
        "tool:undo",
        "tool:redo",
        "tool:sketch-xy",
        "tool:extrude",
        "tool:drill",
        "tool:projection",
        "tool:fit",
        "tool:grid",
        "tool:edges",
        "tool:section",
        "tool:measure",
        "tool:gizmo-move",
        "tool:gizmo-rotate",
        "tool:save",
        "tool:palette",
        "menu:Solid",
        "menu:Export",
        "viewport",
    ];
    for id in expected {
        assert!(h.has_widget(id), "missing widget {id:?} at boot");
    }
    // An empty scene evaluates cleanly.
    assert!(h.app.eval_done_count >= 1);
    assert_eq!(h.app.doc.tree.len(), 0);
    assert_no_bridge_errors();
}

#[test]
fn boot_state_json_is_parseable_and_sane() {
    let _s = serial();
    let mut h = Harness::new();
    let json = crate::bridge::state();
    assert!(json.starts_with('{') && json.ends_with('}'), "{json}");
    assert!(json.contains("\"frame\":"));
    assert!(json.contains("\"doc\":\"untitled\""));
    assert!(json.contains("\"features\":0"));
    assert!(json.contains("\"status\":\""));
    // Heartbeat advances with frames.
    let f1 = crate::bridge::frame();
    h.frames(3);
    assert!(crate::bridge::frame() >= f1 + 3);
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Empty-scene edge cases — the "fresh install, click everything" suite
// (this is the class of bugs the user hit: app quits on button clicks).
// ---------------------------------------------------------------------------

#[test]
fn empty_scene_every_toolbar_button_survives() {
    let _s = serial();
    let mut h = Harness::new();

    // Undo / redo on an empty stack: disabled widgets, clicks no-op —
    // and clicking them must never panic.
    h.click_anyway("tool:undo");
    h.click_anyway("tool:redo");

    // Features with no prerequisites: no sketch, no body, no datum.
    h.click("tool:extrude"); // no sketch yet
    h.click("tool:drill"); // no body yet
    h.click("tool:projection"); // empty scene

    // View toggles.
    h.click("tool:fit");
    h.click("tool:grid");
    h.click("tool:edges");

    // Tools on an empty scene.
    h.click("tool:section");
    h.click("tool:section"); // toggle back off
    h.click("tool:measure");
    h.click("tool:measure"); // off again
    h.click("tool:gizmo-move");
    h.click("tool:gizmo-rotate");

    // Viewport clicks on empty space: selection clear + measure restart.
    h.click_pos(h.viewport_center());
    h.click_pos(h.viewport_empty_corner());

    // Undo/redo again after all that.
    h.click_anyway("tool:undo");
    h.click_anyway("tool:redo");

    assert_eq!(h.app.doc.tree.len(), 0, "no features should be added");
    assert_no_bridge_errors();
}

#[test]
fn empty_scene_export_menu_every_format_survives() {
    let _s = serial();
    let mut h = Harness::new();

    // Export with zero bodies: every format must degrade to a status
    // message, never a panic (empty-mesh export is the classic crash).
    for fmt in ["STL (binary)", "OBJ", "glTF 2.0", "3MF (3D print package)"] {
        h.click("menu:Export");
        h.frames(2);
        let w = h.widget_label(fmt);
        let (x, y) = w.center();
        h.click_pos(pos2(x, y));
        h.frames(2);
        assert!(
            h.app.status.contains("Nothing to export")
                || h.app.status.contains("No bodies")
                || h.app.status.contains("Export"),
            "status after exporting {fmt:?} on empty scene: {}",
            h.app.status
        );
    }
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Solid primitives — the Create menu
// ---------------------------------------------------------------------------

#[test]
fn solid_menu_creates_every_primitive_then_undo_all() {
    let _s = serial();
    let mut h = Harness::new();

    for kind in [
        PrimitiveKind::Box,
        PrimitiveKind::Sphere,
        PrimitiveKind::Cylinder,
        PrimitiveKind::Cone,
        PrimitiveKind::Torus,
    ] {
        add_solid(&mut h, kind);
    }
    assert!(h.wait_for_eval(2500));
    assert_eq!(h.app.doc.tree.len(), 5);
    assert_eq!(body_count(&h), 5, "all primitives evaluate to bodies");
    assert!(h.app.status.contains("Selected") || !h.app.status.is_empty());

    // Undo the whole stack back to empty.
    for _ in 0..5 {
        h.click("tool:undo");
        h.frames(1);
    }
    assert_eq!(h.app.doc.tree.len(), 0);

    // Redo one back.
    h.click("tool:redo");
    h.frames(1);
    assert_eq!(h.app.doc.tree.len(), 1);
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Tree + inspector
// ---------------------------------------------------------------------------

#[test]
fn tree_select_opens_inspector_and_apply_edits() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));

    // Select the body through the REAL tree row.
    let row = h.widget_prefix("tree:");
    let (x, y) = row.center();
    h.click_pos(pos2(x, y));
    h.frames(2);
    assert_eq!(h.app.selection.items.len(), 1, "tree row selects the body");

    // Inspector shows the primitive editor with an Apply button.
    assert!(h.has_widget("btn:Apply"));
    h.click("btn:Apply");
    h.frames(2);
    assert!(h.wait_for_eval(500));
    // The edit went through the command stack (one EditFeature).
    assert!(h.app.commands.can_undo());
    assert_no_bridge_errors();
}

#[test]
fn tree_suppress_eye_toggles_body_visibility() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));
    assert_eq!(body_count(&h), 1);

    // Click the eye toggle on the feature row.
    h.click_prefix("tree:");
    h.frames(2); // select first
    let eye = h
        .widgets()
        .into_iter()
        .find(|w| w.id.ends_with(":suppress"))
        .unwrap_or_else(|| panic!("suppress eye not found"));
    let (x, y) = eye.center();
    h.click_pos(pos2(x, y));
    h.frames(2);
    assert!(h.wait_for_eval(500));
    assert_eq!(body_count(&h), 0, "suppressed feature hides the body");

    // And back on.
    let eye = h
        .widgets()
        .into_iter()
        .find(|w| w.id.ends_with(":suppress"))
        .expect("suppress eye still present");
    let (x, y) = eye.center();
    h.click_pos(pos2(x, y));
    h.frames(2);
    assert!(h.wait_for_eval(500));
    assert_eq!(body_count(&h), 1);
    assert_no_bridge_errors();
}

#[test]
fn tree_row_menu_delete_removes_feature() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));

    // Open the ⋮ row menu and click Delete. The row menu button is an
    // icon-only menu at the row's right side: open via the row's menu
    // widget (recorded when the menu renders).
    // The ⋮ menu trigger is not bridge-recorded (icon-only menu_button
    // in a nested layout), so drive the same menu through the
    // right-click path: context menu shares feature_row_menu.
    let row = h.widget_prefix("tree:");
    let (x, y) = row.center();
    // Right-click: press Secondary at the row.
    h.frame(vec![egui::Event::PointerButton {
        pos: pos2(x, y),
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::default(),
    }]);
    h.frame(vec![egui::Event::PointerButton {
        pos: pos2(x, y),
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::default(),
    }]);
    h.frames(2);
    // Context menu items are recorded as rowmenu:*.
    h.click("rowmenu:delete");
    h.frames(2);
    assert!(h.wait_for_eval(500));
    assert_eq!(h.app.doc.tree.len(), 0, "delete removes the feature");
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Sketch → extrude flow
// ---------------------------------------------------------------------------

#[test]
fn sketch_then_extrude_creates_solid() {
    let _s = serial();
    let mut h = Harness::new();

    // 1. New sketch on XY (toolbar button).
    h.click("tool:sketch-xy");
    assert!(h.wait_for_eval(500));
    assert_eq!(h.app.doc.tree.len(), 1, "sketch feature");
    assert_eq!(body_count(&h), 0, "a sketch alone renders no body");

    // 2. Select the sketch through the tree, solve constraints.
    h.click_prefix("tree:");
    h.frames(2);
    assert!(h.has_widget("btn:Solve constraints"));
    h.click("btn:Solve constraints");
    h.frames(2);
    assert!(h.wait_for_eval(500));

    // 3. Extrude the latest sketch (toolbar).
    h.click("tool:extrude");
    assert!(h.wait_for_eval(2500));
    assert_eq!(h.app.doc.tree.len(), 2, "sketch + extrude");
    assert_eq!(body_count(&h), 1, "extruded solid exists");
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Command palette — every entry is reachable through keyboard + click
// ---------------------------------------------------------------------------

#[test]
fn palette_keyboard_opens_types_and_runs_action() {
    let _s = serial();
    let mut h = Harness::new();

    h.ctrl_shift(Key::P);
    h.frames(2);
    assert!(h.app.palette_open, "Ctrl+Shift+P opens the palette");

    // Type a query and run the first match.
    h.type_text("box");
    h.frames(2);
    h.enter();
    h.frames(4);
    assert!(!h.app.palette_open, "running an action closes the palette");
    assert!(h.wait_for_eval(500));
    assert_eq!(h.app.doc.tree.len(), 1, "Add Box ran from the palette");
    assert_no_bridge_errors();
}

#[test]
fn palette_button_click_and_row_click_run_actions() {
    let _s = serial();
    let mut h = Harness::new();

    // Open via the toolbar button.
    h.click("tool:palette");
    h.frames(2);
    assert!(h.app.palette_open);

    // Click the "Add Sphere" row.
    h.click_prefix("palette:Add Sphere");
    h.frames(4);
    assert!(h.wait_for_eval(500));
    assert_eq!(h.app.doc.tree.len(), 1, "Add Sphere ran");

    // Escape closes the palette.
    h.click("tool:palette");
    h.frames(2);
    assert!(h.app.palette_open);
    h.key(Key::Escape, Modifiers::default());
    h.frames(2);
    assert!(!h.app.palette_open);
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Measure tool (W-08)
// ---------------------------------------------------------------------------

#[test]
fn measure_two_surface_picks_produce_a_label() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));

    h.click("tool:measure");
    h.frames(2);
    assert!(h.app.measure_mode);

    // Two picks on the body (center of the viewport hits the box).
    let c = h.viewport_center();
    h.click_pos(pos2(c.x - 20.0, c.y));
    h.frames(2);
    assert_eq!(h.app.measure_picks.len(), 1, "first pick recorded");
    h.click_pos(pos2(c.x + 20.0, c.y));
    h.frames(2);
    assert_eq!(h.app.measure_picks.len(), 2, "second pick recorded");
    assert!(
        h.app.measure_label.is_some(),
        "distance + angle label produced"
    );
    assert!(h.app.status.contains("Measure"));

    // Third click restarts the measurement.
    h.click_pos(pos2(c.x, c.y - 20.0));
    h.frames(2);
    assert_eq!(h.app.measure_picks.len(), 1, "third click restarts");
    assert_no_bridge_errors();
}

#[test]
fn measure_empty_scene_click_is_safe() {
    let _s = serial();
    let mut h = Harness::new();
    h.click("tool:measure");
    h.frames(2);
    // Click empty space — restart message, no panic.
    h.click_pos(h.viewport_center());
    h.click_pos(h.viewport_empty_corner());
    assert_eq!(h.app.measure_picks.len(), 0);
    assert!(h.app.status.contains("Measure"));
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Section view (W-02)
// ---------------------------------------------------------------------------

#[test]
fn section_toggle_on_off_and_flip() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));

    // Empty scene first: toggling on an empty scene must not panic.
    h.click("tool:section");
    h.frames(2);
    assert!(h.app.render_options.section.is_some());

    // The section mini-panel exposes a flip button.
    assert!(h.has_widget("tool:section-flip"));
    let normal_before = h.app.render_options.section.map(|s| s.normal);
    h.click("tool:section-flip");
    h.frames(2);
    let normal_after = h.app.render_options.section.map(|s| s.normal);
    assert_ne!(normal_before, normal_after, "flip inverts the normal");

    // Toggle off.
    h.click("tool:section");
    h.frames(2);
    assert!(h.app.render_options.section.is_none());
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Camera + viewport interaction
// ---------------------------------------------------------------------------

#[test]
fn viewport_drag_orbits_camera() {
    let _s = serial();
    let mut h = Harness::new();
    let (yaw0, pitch0) = (h.app.camera.yaw, h.app.camera.pitch);
    let c = h.viewport_center();
    // Left+Alt drag = orbit.
    h.drag(
        pos2(c.x, c.y),
        pos2(c.x - 120.0, c.y - 90.0),
        Modifiers::ALT,
    );
    assert!(
        (h.app.camera.yaw - yaw0).abs() > 1e-6 || (h.app.camera.pitch - pitch0).abs() > 1e-6,
        "orbit changed the camera"
    );
    assert_no_bridge_errors();
}

#[test]
fn viewport_wheel_zooms_camera() {
    let _s = serial();
    let mut h = Harness::new();
    let d0 = h.app.camera.distance;
    h.scroll(5.0, h.viewport_center());
    assert!(h.app.camera.distance != d0, "wheel zoom changed distance");
    assert_no_bridge_errors();
}

#[test]
fn navigation_cube_click_snaps_camera() {
    let _s = serial();
    let mut h = Harness::new();
    let (yaw0, pitch0) = (h.app.camera.yaw, h.app.camera.pitch);
    // The nav cube sits at the viewport's top-right.
    let vp = h.widget("viewport");
    let cube = pos2(vp.rect[2] - 49.0, vp.rect[1] + 49.0);
    h.click_pos(cube);
    h.frames(2);
    assert!(
        (h.app.camera.yaw - yaw0).abs() > 1e-6 || (h.app.camera.pitch - pitch0).abs() > 1e-6,
        "nav cube click snapped the camera"
    );
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

#[test]
fn keyboard_shortcuts_toggle_view_state() {
    let _s = serial();
    let mut h = Harness::new();

    let grid0 = h.app.render_options.show_grid;
    h.key(Key::G, Modifiers::default());
    assert_ne!(h.app.render_options.show_grid, grid0, "G toggles grid");

    let edges0 = h.app.render_options.show_edges;
    h.key(Key::E, Modifiers::default());
    assert_ne!(h.app.render_options.show_edges, edges0, "E toggles edges");

    let ortho0 = h.app.camera.orthographic;
    h.key(Key::P, Modifiers::default());
    assert_ne!(h.app.camera.orthographic, ortho0, "P toggles ortho");

    h.key(Key::T, Modifiers::default());
    assert!(matches!(
        h.app.gizmo_mode,
        crate::gizmo::GizmoMode::Translate
    ));
    h.key(Key::R, Modifiers::default());
    assert!(matches!(h.app.gizmo_mode, crate::gizmo::GizmoMode::Rotate));
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Parameters (P-01)
// ---------------------------------------------------------------------------

#[test]
fn params_add_parameter_button() {
    let _s = serial();
    let mut h = Harness::new();
    let params_before = h.app.doc.params.len();
    h.click_prefix("btn:\u{ff0b} Add parameter");
    h.frames(2);
    assert_eq!(
        h.app.doc.params.len(),
        params_before + 1,
        "param added (status: {:?})",
        h.app.status
    );
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Boolean features via palette actions (exercised through the REAL UI)
// ---------------------------------------------------------------------------

#[test]
fn palette_union_two_bodies() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    add_solid(&mut h, PrimitiveKind::Sphere);
    assert!(h.wait_for_eval(2500));
    assert_eq!(body_count(&h), 2);

    h.ctrl_shift(Key::P);
    h.frames(2);
    h.type_text("union");
    h.frames(2);
    h.enter();
    h.frames(4);
    assert!(h.wait_for_eval(2500));
    // Union combines the two latest bodies into one.
    assert_eq!(body_count(&h), 1, "union merges two bodies into one");
    assert_no_bridge_errors();
}

#[test]
fn palette_cut_with_cylinder_on_empty_scene_is_safe() {
    let _s = serial();
    let mut h = Harness::new();
    h.ctrl_shift(Key::P);
    h.frames(2);
    h.type_text("cut");
    h.frames(2);
    h.enter();
    h.frames(4);
    // No body to cut: graceful status, no panic.
    assert!(!h.app.status.is_empty());
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Bridge action queue (the channel the browser E2E uses for imports)
// ---------------------------------------------------------------------------

#[test]
fn bridge_action_queue_round_trips() {
    let _s = serial();
    let mut h = Harness::new();
    crate::bridge::queue_action("status", "ping");
    h.frames(2);
    assert_eq!(h.app.status, "action:ping", "queued action ran next frame");
    assert_no_bridge_errors();
}

#[test]
fn bridge_import_stl_action_adds_feature() {
    let _s = serial();
    let mut h = Harness::new();
    // ASCII STL (binary bytes would be corrupted by the string action
    // channel — the browser E2E uses the same text payloads). Real
    // newlines: the parser is line-based (no `\`-continuations!).
    let stl = concat!(
        "solid test\n",
        "  facet normal 0 0 1\n",
        "    outer loop\n",
        "      vertex 0 0 0\n",
        "      vertex 10 0 0\n",
        "      vertex 0 10 0\n",
        "    endloop\n",
        "  endfacet\n",
        "endsolid test\n",
    );
    let payload = format!("stl:{stl}");
    crate::bridge::queue_action("import", &payload);
    h.frames(6);
    assert!(h.wait_for_eval(500));
    assert_eq!(
        h.app.doc.tree.len(),
        1,
        "imported mesh added as a feature (status: {:?})",
        h.app.status
    );
    assert_eq!(body_count(&h), 1);
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Save (native file write)
// ---------------------------------------------------------------------------

#[test]
fn save_button_writes_document() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));

    h.click("tool:save");
    h.frames(2);
    assert!(
        h.app.status.contains("Saved"),
        "save reports success: {}",
        h.app.status
    );
    assert!(!h.app.doc.modified, "save clears the modified flag");
    // Clean up the artifact written to the crate dir.
    let _ = std::fs::remove_file("untitled.forgecad");
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Gizmo (W-01) — loose assertions: the primary contract is NO PANIC on
// the drag path; exact handle hit-testing is covered by gizmo unit tests.
// ---------------------------------------------------------------------------

#[test]
fn gizmo_move_mode_with_selection_survives_drag() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));
    h.click_prefix("tree:");
    h.frames(2);
    h.click("tool:gizmo-move");
    h.frames(2);

    // Drag near the body center where the gizmo handles live.
    let c = h.viewport_center();
    h.drag(pos2(c.x, c.y), pos2(c.x + 60.0, c.y), Modifiers::default());
    h.frames(4);
    // Either a transform feature was created (handle drag) or the
    // click fell through to selection/orbit — both must be panic-free.
    assert!(h.wait_for_eval(500));
    assert_no_bridge_errors();
}

#[test]
fn gizmo_rotate_mode_with_selection_survives_drag() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    assert!(h.wait_for_eval(500));
    h.click_prefix("tree:");
    h.frames(2);
    h.click("tool:gizmo-rotate");
    h.frames(2);
    let c = h.viewport_center();
    h.drag(
        pos2(c.x, c.y - 40.0),
        pos2(c.x + 40.0, c.y),
        Modifiers::default(),
    );
    h.frames(4);
    assert!(h.wait_for_eval(500));
    assert_no_bridge_errors();
}

// ---------------------------------------------------------------------------
// Long interaction chains — soak the state machine
// ---------------------------------------------------------------------------

#[test]
fn rapid_mixed_interactions_soak() {
    let _s = serial();
    let mut h = Harness::new();
    add_solid(&mut h, PrimitiveKind::Box);
    h.click("tool:sketch-xy");
    h.click("tool:extrude");
    h.click("tool:measure");
    let c = h.viewport_center();
    h.click_pos(c);
    h.click_pos(pos2(c.x + 10.0, c.y));
    h.click("tool:measure");
    h.click("tool:section");
    h.click("tool:grid");
    h.click("tool:edges");
    h.click("tool:fit");
    h.ctrl(Key::Z); // undo
    h.ctrl_shift(Key::Z); // redo
    h.ctrl_shift(Key::P); // palette
    h.key(Key::Escape, Modifiers::default());
    h.frames(8);
    assert!(h.wait_for_eval(500));
    assert_no_bridge_errors();
}
