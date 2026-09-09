//! The 3D viewport: input handling, the egui-wgpu paint callback and the
//! navigation cube.

use crate::app::ForgeApp;
use forge_render::{Camera, RenderOptions, Renderer};
use std::sync::{Arc, Mutex};

/// Paint callback bridging the renderer into egui's frame.
pub struct ViewportCallback {
    renderer: Arc<Mutex<Renderer>>,
    camera: Camera,
    options: RenderOptions,
    size_px: (u32, u32),
}

impl ViewportCallback {
    pub fn new(
        renderer: Arc<Mutex<Renderer>>,
        camera: Camera,
        options: RenderOptions,
        size_px: (u32, u32),
    ) -> Self {
        Self {
            renderer,
            camera,
            options,
            size_px,
        }
    }
}

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        _resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Ok(mut renderer) = self.renderer.lock() {
            renderer.render(encoder, self.size_px, &self.camera, &self.options);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        let Ok(mut renderer) = self.renderer.lock() else {
            return;
        };
        let vp = info.clip_rect_in_pixels();
        if vp.width_px > 0 && vp.height_px > 0 {
            render_pass.set_scissor_rect(
                vp.left_px.max(0) as u32,
                vp.top_px.max(0) as u32,
                vp.width_px.max(0) as u32,
                vp.height_px.max(0) as u32,
            );
        }
        renderer.composite(render_pass);
    }
}

/// The central 3D viewport widget: input, camera control, picking.
pub fn viewport_ui(ui: &mut egui::Ui, app: &mut ForgeApp) {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

    // E2E bridge: the viewport itself is a clickable region (selection,
    // measure picks, gizmo drags) — tests click at its rect.
    #[cfg(any(test, debug_assertions))]
    crate::bridge::record("viewport", "3D viewport", "viewport", rect, true);

    let pixels_per_point = ui.ctx().pixels_per_point();

    // ---- Camera input (FR-RD-04) ----
    let drag = response.drag_delta();
    let alt = ui.input(|i| i.modifiers.alt);
    let shift = ui.input(|i| i.modifiers.shift);
    let middle = response.dragged_by(egui::PointerButton::Middle);
    let left = response.dragged_by(egui::PointerButton::Primary);

    if (middle && !shift) || (left && alt && !shift) {
        // Orbit (screen dx -> yaw, dy -> pitch; scaled for a natural feel).
        let k = 0.008;
        app.camera.orbit((drag.x * k) as f64, (-drag.y * k) as f64);
    }
    if (middle && shift) || (left && alt && shift) {
        // Pan: convert point-space delta to world units.
        let scale = 2.0 / rect.height() as f64;
        app.camera
            .pan(-drag.x as f64 * scale, drag.y as f64 * scale);
    }
    if response.hovered() {
        // Zoom via wheel events (egui 0.36 reports wheel input as events).
        let scroll: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } => delta.y,
                    _ => 0.0,
                })
                .sum()
        });
        if scroll != 0.0 {
            let factor = (scroll as f64 * 0.008).exp();
            app.camera.zoom(factor);
        }
    }

    // ---- Gizmo interaction (W-01): before the pick block so a handle
    // press can own the click; drawing happens after the 3D callback. ----
    let _gizmo_dragging = crate::gizmo::gizmo_interaction(ui, app, rect, &response);

    // ---- Picking (FR-RD-02): click to select ----
    if let Some(click_pos) = response.interact_pointer_pos() {
        if response.clicked_by(egui::PointerButton::Primary)
            && !app.gizmo_press_on_handle
            && app.gizmo_drag.is_none()
        {
            let local = click_pos - rect.min;
            if app.measure_mode {
                // W-08: measurement picks use CPU raycasts (exact hit
                // points), not the GPU id buffer.
                let nx = (local.x / rect.width()) as f64 * 2.0 - 1.0;
                let ny = 1.0 - (local.y / rect.height()) as f64 * 2.0;
                let aspect = (rect.width() / rect.height()) as f64;
                app.measure_click((nx, ny), aspect);
            } else if app.pick_mode != crate::picking::PickMode::Bodies {
                // W-04: sub-body picks via CPU raycast + topology queries.
                let nx = (local.x / rect.width()) as f64 * 2.0 - 1.0;
                let ny = 1.0 - (local.y / rect.height()) as f64 * 2.0;
                let aspect = (rect.width() / rect.height()) as f64;
                let click = egui::pos2(local.x, local.y);
                match crate::picking::sub_pick(app, (nx, ny), aspect, rect, click) {
                    Some(pick) => {
                        if app.selection.items.contains(&pick.item) {
                            app.selection.toggle(pick.item);
                        } else {
                            app.selection.select(pick.item);
                        }
                        app.set_status(pick.status);
                    }
                    None => {
                        app.selection.clear();
                        app.set_status("Selection cleared");
                    }
                }
            } else {
                let px = (
                    (local.x * pixels_per_point) as u32,
                    (local.y * pixels_per_point) as u32,
                );
                if let Some(renderer) = app.renderer() {
                    // Poison-tolerant lock: a panic that happened while a
                    // frame held the renderer must not cascade into a
                    // second panic on the next pick click (the app would
                    // die on *every* subsequent click — observed as
                    // "click a button and it quits").
                    if let Ok(mut r) = renderer.lock() {
                        r.schedule_pick(px);
                        app.pick_requested = true;
                    }
                }
            }
        }
    }

    // ---- Paint callback ----
    if let Some(renderer) = app.renderer() {
        // Upload the scene (no-op when the version is unchanged).
        // W-10 regression fix: this call was lost in the PR-08 UI
        // refactor — the renderer drew an empty scene forever.
        if let Ok(mut r) = renderer.lock() {
            r.update_scene(&app.scene);
        }
        let callback = ViewportCallback::new(
            renderer.clone(),
            app.camera,
            app.render_options,
            (
                (rect.width() * pixels_per_point) as u32,
                (rect.height() * pixels_per_point) as u32,
            ),
        );
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
    }

    // ---- Measurement overlay (W-08) ----
    if app.measure_mode && !app.measure_picks.is_empty() {
        draw_measurement(ui, app, rect);
    }

    // ---- Gizmo overlay (W-01): after the 3D paint callback (painter
    // shapes render in submission order) and above the grid. ----
    crate::gizmo::gizmo_draw(ui, app, rect);

    // ---- Sub-body selection highlight (W-04) ----
    crate::picking::draw_sub_selection(ui, app, rect);

    // ---- Navigation cube (FR-RD-04) ----
    navigation_cube(ui, &mut app.camera, rect);
}

/// Draw the active measurement: pick dots, the span line and the result
/// label at the projected midpoint (W-08).
fn draw_measurement(ui: &mut egui::Ui, app: &ForgeApp, rect: egui::Rect) {
    let aspect = (rect.width() / rect.height()) as f64;
    // Column-major glam -> nalgebra (f32 GPU precision is fine for labels).
    let cols = app.camera.view_proj(aspect).to_cols_array();
    let mut vp = nalgebra::Matrix4::<f32>::identity();
    for c in 0..4 {
        for r in 0..4 {
            vp[(r, c)] = cols[c * 4 + r];
        }
    }
    let project = |p: forge_core::Point3| -> Option<egui::Pos2> {
        let clip = vp * nalgebra::Point4::new(p.x as f32, p.y as f32, p.z as f32, 1.0);
        if clip.w <= 1e-6 {
            return None; // behind the camera
        }
        let ndc = clip.coords / clip.w;
        Some(egui::pos2(
            rect.min.x + (ndc.x + 1.0) * 0.5 * rect.width(),
            rect.min.y + (1.0 - ndc.y) * 0.5 * rect.height(),
        ))
    };

    let picks: Vec<(egui::Pos2, forge_core::Point3)> = app
        .measure_picks
        .iter()
        .filter_map(|m| project(m.point).map(|p| (p, m.point)))
        .collect();
    if picks.is_empty() {
        return;
    }

    let painter = ui.painter().clone();
    let accent = egui::Color32::from_rgb(255, 176, 32);
    for (screen, _) in &picks {
        painter.circle_filled(*screen, 4.0, accent);
        painter.circle_stroke(*screen, 4.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
    }

    if picks.len() == 2 {
        let (a, b) = (picks[0].0, picks[1].0);
        painter.line_segment([a, b], egui::Stroke::new(2.0, accent));
        if let Some(label) = &app.measure_label {
            let mid = egui::pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            // Offset the label above the line with a small backing panel
            // so it stays readable over geometry.
            let galley = painter.layout(
                label.clone(),
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
                200.0,
            );
            let size = galley.size();
            let pos = egui::pos2(mid.x - size.x * 0.5, mid.y - size.y - 10.0);
            painter.rect_filled(
                egui::Rect::from_min_size(pos - egui::vec2(4.0, 3.0), size + egui::vec2(8.0, 6.0)),
                4.0,
                egui::Color32::from_black_alpha(200),
            );
            painter.galley(pos, galley, egui::Color32::WHITE);
        }
    }
}

/// A small clickable orientation cube in the top-right corner.
fn navigation_cube(ui: &mut egui::Ui, camera: &mut Camera, viewport: egui::Rect) {
    let size = 74.0;
    let margin = 12.0;
    let center = egui::pos2(
        viewport.right_top().x - margin - size * 0.5,
        viewport.right_top().y + margin + size * 0.5,
    );
    let half = size * 0.5;

    // Camera basis for projection (screen space).
    let forward = camera.forward();
    let right = camera.right();
    let up = camera.up();

    // Cube face definitions: label, outward normal, target yaw/pitch.
    let faces: [(&str, [f64; 3], f64, f64); 6] = [
        ("Top", [0.0, 0.0, 1.0], 0.0, 1.5),
        ("Bottom", [0.0, 0.0, -1.0], 0.0, -1.5),
        ("Right", [1.0, 0.0, 0.0], 0.0, 0.0),
        ("Left", [-1.0, 0.0, 0.0], std::f64::consts::PI, 0.0),
        ("Front", [0.0, 1.0, 0.0], std::f64::consts::FRAC_PI_2, 0.0),
        ("Back", [0.0, -1.0, 0.0], -std::f64::consts::FRAC_PI_2, 0.0),
    ];

    let painter = ui.painter().clone();
    let cube_corners: [(f64, f64, f64); 8] = [
        (-1.0, -1.0, -1.0),
        (1.0, -1.0, -1.0),
        (1.0, 1.0, -1.0),
        (-1.0, 1.0, -1.0),
        (-1.0, -1.0, 1.0),
        (1.0, -1.0, 1.0),
        (1.0, 1.0, 1.0),
        (-1.0, 1.0, 1.0),
    ];
    let project = |p: (f64, f64, f64)| -> egui::Pos2 {
        let (x, y, z) = p;
        let sx = x * right.x + y * right.y + z * right.z;
        let sy = x * up.x + y * up.y + z * up.z;
        let sz = x * forward.x + y * forward.y + z * forward.z;
        // Slight perspective-ish scale by depth.
        let persp = 1.0 / (1.0 + sz * 0.18);
        let _ = sz;
        egui::pos2(
            center.x + (sx * persp) as f32 * half,
            center.y - (sy * persp) as f32 * half,
        )
    };

    // Face quads (corner indices) with back-face ordering.
    let quads: [(usize, usize, usize, usize); 6] = [
        (4, 5, 6, 7), // +z
        (1, 0, 3, 2), // -z
        (5, 1, 2, 6), // +x
        (0, 4, 7, 3), // -x
        (7, 6, 2, 3), // +y
        (0, 1, 5, 4), // -y
    ];

    // Interactivity: an invisible hit area.
    let hit_rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    let response = ui.interact(hit_rect, ui.id().with("navcube"), egui::Sense::click());
    let hovered = response.hovered();

    // Draw back-to-front: compute view-space z of each face center.
    let mut order: Vec<usize> = (0..6).collect();
    order.sort_by_key(|qi| {
        let (a, b, c, d) = quads[*qi];
        let cx =
            (cube_corners[a].0 + cube_corners[b].0 + cube_corners[c].0 + cube_corners[d].0) * 0.25;
        let cy =
            (cube_corners[a].1 + cube_corners[b].1 + cube_corners[c].1 + cube_corners[d].1) * 0.25;
        let cz =
            (cube_corners[a].2 + cube_corners[b].2 + cube_corners[c].2 + cube_corners[d].2) * 0.25;
        let vz = cx * forward.x + cy * forward.y + cz * forward.z;
        (vz * 1000.0) as i64
    });

    for qi in order.iter().copied() {
        let (a, b, c, d) = quads[qi];
        let pts = [
            project(cube_corners[a]),
            project(cube_corners[b]),
            project(cube_corners[c]),
            project(cube_corners[d]),
        ];
        let face = faces
            .iter()
            .find(|(_, n, _, _)| {
                let (a, b, c, d) = quads[qi];
                let cx =
                    (cube_corners[a].0 + cube_corners[b].0 + cube_corners[c].0 + cube_corners[d].0)
                        * 0.25;
                let cy =
                    (cube_corners[a].1 + cube_corners[b].1 + cube_corners[c].1 + cube_corners[d].1)
                        * 0.25;
                let cz =
                    (cube_corners[a].2 + cube_corners[b].2 + cube_corners[c].2 + cube_corners[d].2)
                        * 0.25;
                (cx - n[0]).abs() < 0.1 && (cy - n[1]).abs() < 0.1 && (cz - n[2]).abs() < 0.1
            })
            .map(|(label, _, yaw, pitch)| (*label, *yaw, *pitch));

        let fill = if hovered {
            egui::Color32::from_rgba_premultiplied(70, 82, 96, 230)
        } else {
            egui::Color32::from_rgba_premultiplied(56, 64, 74, 220)
        };
        painter.add(egui::Shape::convex_polygon(
            pts.to_vec(),
            fill,
            egui::Stroke::new(1.0, egui::Color32::from_gray(140)),
        ));
        if let Some((label, _, _)) = face {
            let centroid = {
                let sum = pts
                    .iter()
                    .fold(egui::Vec2::ZERO, |acc, p| acc + p.to_vec2());
                egui::pos2(sum.x / pts.len() as f32, sum.y / pts.len() as f32)
            };
            painter.text(
                centroid,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(225),
            );
        }
    }

    // Click handling: snap to the face whose projected quad contains the
    // click, testing front-to-back.
    if let Some(click) = response.interact_pointer_pos() {
        for qi in order.iter().rev() {
            let (a, b, c, d) = quads[*qi];
            let pts = [
                project(cube_corners[a]),
                project(cube_corners[b]),
                project(cube_corners[c]),
                project(cube_corners[d]),
            ];
            if !point_in_quad(click, &pts) {
                continue;
            }
            let face_center = (
                (cube_corners[a].0 + cube_corners[b].0 + cube_corners[c].0 + cube_corners[d].0)
                    * 0.25,
                (cube_corners[a].1 + cube_corners[b].1 + cube_corners[c].1 + cube_corners[d].1)
                    * 0.25,
                (cube_corners[a].2 + cube_corners[b].2 + cube_corners[c].2 + cube_corners[d].2)
                    * 0.25,
            );
            if let Some(f) = faces.iter().find(|(_, n, _, _)| {
                (face_center.0 - n[0]).abs() < 0.1
                    && (face_center.1 - n[1]).abs() < 0.1
                    && (face_center.2 - n[2]).abs() < 0.1
            }) {
                camera.yaw = f.2;
                camera.pitch = f.3.clamp(-1.5, 1.5);
            }
            break;
        }
    }
}

fn point_in_quad(p: egui::Pos2, pts: &[egui::Pos2; 4]) -> bool {
    // Winding test against all four edges.
    let mut inside = true;
    for i in 0..4 {
        let a = pts[i];
        let b = pts[(i + 1) % 4];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross < 0.0 {
            inside = false;
        }
    }
    inside
}
