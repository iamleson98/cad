//! W-01: the 3D drag manipulator (gizmo).
//!
//! Screen-space overlay (egui painter) with world-space drag math:
//! - **Translate**: axis arrows + plane handles, ray-line closest-point /
//!   ray-plane intersection, 1 mm snap (Shift disables).
//! - **Rotate**: rings about the world axes, signed in-plane angle with
//!   5° snap (Shift disables).
//!
//! Drags drive a `Feature::TransformBody` (creating a wrapper when the
//! selected body has no transform yet) and commit exactly **one** undo
//! command at drag end. The pivot is rebased to the body's bbox center at
//! grab time (world-position preserving), so rings spin the body about
//! the gizmo anchor.

use crate::app::ForgeApp;
use forge_core::{BodyId, FeatureId, Point3, Ray3, Vector3};
use forge_model::{Command, Feature, SelectionItem};
use forge_render::Camera;

/// Translation snap step (mm).
const SNAP_TRANSLATE: f64 = 1.0;
/// Rotation snap step (radians).
const SNAP_ROTATE: f64 = 5.0_f64.to_radians();
/// Screen-space pick radius (logical px).
const PICK_RADIUS: f32 = 8.0;
/// Gizmo arrow length (logical px, converted to world units per frame).
const GIZMO_LEN_PX: f64 = 96.0;
/// Ring radius as a fraction of the arrow length.
const RING_SCALE: f64 = 0.85;

/// Gizmo interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoMode {
    /// Axis arrows + plane handles.
    #[default]
    Translate,
    /// Rotation rings.
    Rotate,
}

/// One draggable handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GizmoHandle {
    /// Translation along world axis 0/1/2 (X/Y/Z).
    Axis(usize),
    /// Translation in the plane spanned by two world axes.
    Plane(usize, usize),
    /// Rotation about world axis 0/1/2.
    Ring(usize),
}

impl GizmoHandle {
    /// Short description for the status line.
    fn describe(self) -> &'static str {
        match self {
            GizmoHandle::Axis(0) => "X",
            GizmoHandle::Axis(1) => "Y",
            GizmoHandle::Axis(2) => "Z",
            GizmoHandle::Plane(0, 1) | GizmoHandle::Plane(1, 0) => "XY plane",
            GizmoHandle::Plane(1, 2) | GizmoHandle::Plane(2, 1) => "YZ plane",
            GizmoHandle::Plane(2, 0) | GizmoHandle::Plane(0, 2) => "ZX plane",
            GizmoHandle::Ring(0) => "about X",
            GizmoHandle::Ring(1) => "about Y",
            GizmoHandle::Ring(2) => "about Z",
            _ => "handle",
        }
    }
}

/// The drag reference frame captured at grab time. All drag math is
/// parameterized against these *fixed* start references, so the mapping
/// from pointer to motion stays stable while the body (and the rendered
/// gizmo) moves under it.
#[derive(Debug, Clone, Copy)]
enum DragRef {
    /// Parameter along the axis line through the anchor.
    AxisT(f64),
    /// Ray-plane intersection point at grab.
    PlanePt(Point3),
    /// Normalized in-plane reference direction for ring angle measurement.
    RingRef(Vector3),
}

/// Live drag state (one active drag at a time).
#[derive(Debug, Clone)]
pub struct GizmoDrag {
    /// The `TransformBody` feature being dragged.
    target: FeatureId,
    /// Its source body feature.
    source: FeatureId,
    /// `true` when the drag created the TransformBody wrapper (undo =
    /// remove the feature instead of reverting an edit).
    created: bool,
    /// Feature before the drag (for the single undo command).
    before: Box<Feature>,
    handle: GizmoHandle,
    /// World anchor == rotation pivot (body bbox center at grab).
    anchor: Point3,
    start_translation: Vector3,
    start_rotation: Vector3,
    reference: DragRef,
}

/// World axis unit vector (Z-up CAD convention).
fn axis(i: usize) -> Vector3 {
    match i {
        0 => Vector3::x(),
        1 => Vector3::y(),
        2 => Vector3::z(),
        _ => Vector3::x(),
    }
}

/// Axis display color (X red / Y green / Z blue, CAD convention).
fn axis_color(i: usize) -> egui::Color32 {
    match i {
        0 => egui::Color32::from_rgb(228, 76, 82),
        1 => egui::Color32::from_rgb(84, 167, 92),
        2 => egui::Color32::from_rgb(66, 140, 224),
        _ => egui::Color32::WHITE,
    }
}

/// Blend two colors (for plane handles: their two axes), semi-transparent.
fn blend(a: egui::Color32, b: egui::Color32) -> egui::Color32 {
    // u8 + u8 overflows for channels > 127 (X-red is 228): the panic
    // killed the app every time a selection showed the gizmo plane
    // handles (the "click something and it quits" bug). Widen first.
    let ch = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
    egui::Color32::from_rgba_unmultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), 150)
}

/// Snap `v` to a multiple of `step`.
fn snap(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// Closest parameter `t` along the line `anchor + t * axis_dir` to `ray`
/// (line-line closest point). `None` when the ray is parallel to the line.
fn axis_param(ray: &Ray3, anchor: &Point3, axis_dir: &Vector3) -> Option<f64> {
    let d = ray.origin - anchor;
    let u = axis_dir;
    let v = ray.direction.as_ref();
    let uv = u.dot(v);
    let denom = 1.0 - uv * uv;
    if denom.abs() < 1e-9 {
        return None;
    }
    Some((d.dot(u) - d.dot(v) * uv) / denom)
}

/// Ray-plane intersection with the plane through `anchor` with `normal`.
/// `None` when the ray is parallel to the plane or the plane is behind.
fn plane_hit(ray: &Ray3, anchor: &Point3, normal: &Vector3) -> Option<Point3> {
    let denom = ray.direction.dot(normal);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = (anchor - ray.origin).dot(normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some(ray.at(t))
}

/// Signed angle from `reference` to the in-plane direction of `ray`'s hit,
/// measured about `+normal` (right-hand rule).
fn ring_angle(ray: &Ray3, anchor: &Point3, normal: &Vector3, reference: &Vector3) -> Option<f64> {
    let hit = plane_hit(ray, anchor, normal)?;
    let v = hit - anchor;
    if v.norm() < 1e-9 {
        return None;
    }
    let v = v.normalize();
    let cross = reference.cross(&v);
    Some(cross.dot(normal).atan2(reference.dot(&v)))
}

/// Compose a world-axis delta rotation onto Euler XYZ angles
/// (left-multiplication: the delta acts in the world frame).
fn compose_rotation(euler: &Vector3, world_axis: &Vector3, angle: f64) -> Vector3 {
    let q_start = nalgebra::UnitQuaternion::from_euler_angles(euler.x, euler.y, euler.z);
    let q_delta = nalgebra::UnitQuaternion::from_axis_angle(
        &nalgebra::Unit::new_normalize(*world_axis),
        angle,
    );
    let q = q_delta * q_start;
    let (x, y, z) = q.euler_angles();
    Vector3::new(x, y, z)
}

/// Move the rotation pivot while preserving every world position:
/// `p' = pivot + R·(p − pivot) + T` must be invariant, so the translation
/// adjusts by `T' = T + (I − R)·(pivot_old − pivot_new)`.
fn rebase_pivot(
    translation: &Vector3,
    euler: &Vector3,
    old_pivot: &Point3,
    new_pivot: &Point3,
) -> Vector3 {
    let r = nalgebra::UnitQuaternion::from_euler_angles(euler.x, euler.y, euler.z);
    let delta = old_pivot - new_pivot;
    translation + delta - r.transform_vector(&delta)
}

/// World units per logical screen pixel at `anchor` (gizmo stays a
/// constant screen size regardless of zoom).
fn world_per_pixel(camera: &Camera, rect_h_px: f64, anchor: &Point3) -> f64 {
    let world_h = if camera.orthographic {
        2.0 * camera.ortho_half_height()
    } else {
        // Distance from the eye to the anchor along the view ray.
        let d = (camera.eye() - anchor).norm();
        2.0 * d * (camera.fov_deg.to_radians() * 0.5).tan()
    };
    world_h / rect_h_px.max(1.0)
}

/// World point → screen position (logical px, viewport-rect local).
/// Shared by the gizmo and the sub-body selection overlay (W-04).
pub(crate) struct Projector {
    vp: nalgebra::Matrix4<f32>,
    rect: egui::Rect,
}

impl Projector {
    pub(crate) fn new(camera: &Camera, aspect: f64, rect: egui::Rect) -> Self {
        let cols = camera.view_proj(aspect).to_cols_array();
        let mut vp = nalgebra::Matrix4::<f32>::identity();
        for c in 0..4 {
            for r in 0..4 {
                vp[(r, c)] = cols[c * 4 + r];
            }
        }
        Self { vp, rect }
    }

    pub(crate) fn project(&self, p: &Point3) -> Option<egui::Pos2> {
        let clip = self.vp * nalgebra::Point4::new(p.x as f32, p.y as f32, p.z as f32, 1.0);
        if clip.w <= 1e-6 {
            return None; // behind the camera
        }
        let ndc = clip.coords / clip.w;
        Some(egui::pos2(
            self.rect.min.x + (ndc.x + 1.0) * 0.5 * self.rect.width(),
            self.rect.min.y + (1.0 - ndc.y) * 0.5 * self.rect.height(),
        ))
    }
}

/// Screen-space distance from a point to a line segment.
pub(crate) fn dist_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Screen-space distance from a point to a polyline.
fn dist_to_polyline(p: egui::Pos2, pts: &[egui::Pos2]) -> f32 {
    pts.windows(2)
        .map(|w| dist_to_segment(p, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

/// Point-in-convex-quad test, winding-agnostic (projection can flip
/// the orientation).
fn point_in_quad(p: egui::Pos2, pts: &[egui::Pos2; 4]) -> bool {
    let mut sign = 0.0_f32;
    for i in 0..4 {
        let (a, b) = (pts[i], pts[(i + 1) % 4]);
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() > 1e-9 {
            let s = cross.signum();
            if sign == 0.0 {
                sign = s;
            } else if s != sign {
                return false;
            }
        }
    }
    true
}

/// A hit-test candidate: (handle, screen distance).
struct Hit {
    handle: GizmoHandle,
    dist: f32,
}

/// All handle geometry projected to screen space, shared by hit testing
/// and drawing.
struct GizmoGeometry {
    /// Arrow segments per axis: (root, tip).
    arrows: [(egui::Pos2, egui::Pos2); 3],
    /// Plane quads: axis pair + 4 corners.
    planes: [([usize; 2], [egui::Pos2; 4]); 3],
    /// Ring polylines per axis.
    rings: [Vec<egui::Pos2>; 3],
}

impl GizmoGeometry {
    fn build(proj: &Projector, anchor: &Point3, len: f64, mode: GizmoMode) -> Self {
        let mut arrows = [(egui::Pos2::ZERO, egui::Pos2::ZERO); 3];
        let mut planes: [([usize; 2], [egui::Pos2; 4]); 3] = [
            ([0, 1], [egui::Pos2::ZERO; 4]),
            ([1, 2], [egui::Pos2::ZERO; 4]),
            ([2, 0], [egui::Pos2::ZERO; 4]),
        ];
        let mut rings: [Vec<egui::Pos2>; 3] = [Vec::new(), Vec::new(), Vec::new()];

        if mode == GizmoMode::Translate {
            for (i, arrow) in arrows.iter_mut().enumerate() {
                if let (Some(a), Some(b)) = (
                    proj.project(anchor),
                    proj.project(&(anchor + axis(i) * len)),
                ) {
                    *arrow = (a, b);
                }
            }
            // Plane handles between axis pairs, offset diagonally from the
            // anchor, sized as a fraction of the arrow length.
            let off = len * 0.60;
            let hs = len * 0.16;
            for (k, (i, j)) in [(0, 1), (1, 2), (2, 0)].into_iter().enumerate() {
                let center = anchor + axis(i) * off + axis(j) * off;
                let corners = [
                    center - axis(i) * hs - axis(j) * hs,
                    center + axis(i) * hs - axis(j) * hs,
                    center + axis(i) * hs + axis(j) * hs,
                    center - axis(i) * hs + axis(j) * hs,
                ];
                let mut pts = [egui::Pos2::ZERO; 4];
                for (c, corner) in corners.iter().enumerate() {
                    pts[c] = proj.project(corner).unwrap_or(egui::Pos2::ZERO);
                }
                planes[k] = ([i, j], pts);
            }
        } else {
            let radius = len * RING_SCALE;
            const SEGMENTS: usize = 48;
            for (i, ring) in rings.iter_mut().enumerate() {
                // Ring for axis i: circle in the plane through the anchor
                // perpendicular to axis i.
                let (e1, e2) = match i {
                    0 => (axis(1), axis(2)),
                    1 => (axis(2), axis(0)),
                    _ => (axis(0), axis(1)),
                };
                let mut pts = Vec::with_capacity(SEGMENTS + 1);
                for s in 0..=SEGMENTS {
                    let theta = s as f64 * std::f64::consts::TAU / SEGMENTS as f64;
                    let p = anchor + e1 * (radius * theta.cos()) + e2 * (radius * theta.sin());
                    if let Some(sp) = proj.project(&p) {
                        pts.push(sp);
                    }
                }
                *ring = pts;
            }
        }
        Self {
            arrows,
            planes,
            rings,
        }
    }

    /// Closest handle under the pointer (within the pick radius).
    fn hit(&self, mouse: egui::Pos2, mode: GizmoMode) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let better = |hit: Hit, best: &mut Option<Hit>| {
            if best.as_ref().map(|b| hit.dist < b.dist).unwrap_or(true) {
                *best = Some(hit);
            }
        };
        if mode == GizmoMode::Translate {
            // Plane handles first (they sit below the arrows): same
            // distance is won by the plane.
            for (pair, pts) in &self.planes {
                if point_in_quad(mouse, pts) {
                    better(
                        Hit {
                            handle: GizmoHandle::Plane(pair[0], pair[1]),
                            dist: 0.0,
                        },
                        &mut best,
                    );
                }
            }
            for (i, (a, b)) in self.arrows.iter().enumerate() {
                let d = dist_to_segment(mouse, *a, *b);
                if d <= PICK_RADIUS * 1.5 {
                    // Slightly wider pick for the thin arrows.
                    better(
                        Hit {
                            handle: GizmoHandle::Axis(i),
                            dist: d - 0.5,
                        },
                        &mut best,
                    );
                }
            }
        } else {
            for (i, pts) in self.rings.iter().enumerate() {
                if pts.len() < 2 {
                    continue;
                }
                let d = dist_to_polyline(mouse, pts);
                if d <= PICK_RADIUS {
                    better(
                        Hit {
                            handle: GizmoHandle::Ring(i),
                            dist: d,
                        },
                        &mut best,
                    );
                }
            }
        }
        best
    }
}

/// The body the gizmo manipulates: primary selected body that is visible
/// in the last evaluation (anchor = its bbox center).
fn gizmo_target(app: &ForgeApp) -> Option<(FeatureId, Point3)> {
    if app.measure_mode || app.gizmo_drag.is_some() {
        return None;
    }
    let body = app.selection.primary_body()?;
    let ev = app.last_evaluation.as_ref()?;
    for b in &ev.bodies {
        if b.id == body {
            return Some((FeatureId::new(body.raw()), b.mesh.bbox().center()));
        }
    }
    None
}

/// The world anchor to render at while dragging (anchor + live
/// translation) or the live bbox center otherwise.
fn render_anchor(app: &ForgeApp, live: Option<(FeatureId, Point3)>) -> Option<Point3> {
    if let Some(drag) = &app.gizmo_drag {
        let t = current_translation(app).unwrap_or(drag.start_translation);
        return Some(drag.anchor + t);
    }
    live.map(|(_, anchor)| anchor)
}

/// Current translation of the drag target feature (live state).
fn current_translation(app: &ForgeApp) -> Option<Vector3> {
    let drag = app.gizmo_drag.as_ref()?;
    match app.doc.feature(drag.target) {
        Some(Feature::TransformBody { translation, .. }) => Some(*translation),
        _ => None,
    }
}

/// Start a drag on `handle`.
fn start_drag(app: &mut ForgeApp, handle: GizmoHandle, anchor: Point3, ray: &Ray3) {
    let Some(fid) = app.selection.primary_feature() else {
        return;
    };
    let feature = app.doc.feature(fid).cloned();

    let (target, created, before, start_translation, start_rotation) = match feature {
        Some(
            f @ Feature::TransformBody {
                source,
                translation,
                rotation,
                pivot,
            },
        ) => {
            // Rebase the pivot to the grab anchor (world-preserving) so the
            // rings spin the body about the gizmo. The rebase is part of
            // the drag: `before` keeps the original for undo.
            let t = rebase_pivot(&translation, &rotation, &pivot, &anchor);
            let rebased = Feature::TransformBody {
                source,
                translation: t,
                rotation,
                pivot: anchor,
            };
            let _ = app.doc.edit_feature(fid, rebased);
            (fid, false, Box::new(f), t, rotation)
        }
        Some(other) => {
            // Wrap the body in a fresh TransformBody appended to the tree.
            let wrapper = Feature::TransformBody {
                source: fid,
                translation: Vector3::zeros(),
                rotation: Vector3::zeros(),
                pivot: anchor,
            };
            match app.doc.add_feature(wrapper.clone()) {
                Ok(id) => {
                    app.selection
                        .select(SelectionItem::Body(BodyId::new(id.raw())));
                    (
                        id,
                        true,
                        Box::new(other),
                        Vector3::zeros(),
                        Vector3::zeros(),
                    )
                }
                Err(_) => return,
            }
        }
        None => return,
    };

    // Capture the drag reference frame.
    let reference = match handle {
        GizmoHandle::Axis(i) => DragRef::AxisT(axis_param(ray, &anchor, &axis(i)).unwrap_or(0.0)),
        GizmoHandle::Plane(_, _) => {
            // The plane normal is the remaining axis.
            let normal = plane_normal(handle);
            DragRef::PlanePt(plane_hit(ray, &anchor, &normal).unwrap_or(anchor))
        }
        GizmoHandle::Ring(i) => {
            let n = axis(i);
            let mut r = plane_hit(ray, &anchor, &n)
                .map(|p| (p - anchor).normalize())
                .unwrap_or_else(|| {
                    // Ray nearly parallel: fall back to the screen-right
                    // in-plane direction.
                    let cam_right = app.camera.right();
                    cam_right - n * cam_right.dot(&n)
                });
            if r.norm() < 1e-9 {
                r = Vector3::y();
            }
            DragRef::RingRef(r.normalize())
        }
    };

    app.gizmo_drag = Some(GizmoDrag {
        target,
        source: match &*before {
            Feature::TransformBody { source, .. } => *source,
            _ => fid,
        },
        created,
        before,
        handle,
        anchor,
        start_translation,
        start_rotation,
        reference,
    });
    app.set_status(format!(
        "Gizmo: drag {} (snap {} / Shift = off)",
        handle.describe(),
        if matches!(handle, GizmoHandle::Ring(_)) {
            format!("{}", SNAP_ROTATE.to_degrees())
        } else {
            format!("{SNAP_TRANSLATE:.0} mm")
        }
    ));
    app.request_evaluation();
}

/// The normal of a plane handle (the axis spanning neither of its axes).
fn plane_normal(handle: GizmoHandle) -> Vector3 {
    match handle {
        GizmoHandle::Plane(i, j) => axis(3 - i - j),
        _ => axis(2),
    }
}

/// Pure drag math: the (translation, rotation) the feature should hold
/// for the current pointer ray. `None` leaves the live state untouched
/// (degenerate frame, e.g. ray parallel to the drag plane).
fn drag_values(drag: &GizmoDrag, ray: &Ray3, snap_on: bool) -> Option<(Vector3, Vector3)> {
    let t0 = drag.start_translation;
    let r0 = drag.start_rotation;
    match (drag.handle, &drag.reference) {
        (GizmoHandle::Axis(i), DragRef::AxisT(t_start)) => {
            let t = axis_param(ray, &drag.anchor, &axis(i))?;
            let mut delta = t - t_start;
            if snap_on {
                delta = snap(delta, SNAP_TRANSLATE);
            }
            Some((t0 + axis(i) * delta, r0))
        }
        (GizmoHandle::Plane(_, _), DragRef::PlanePt(p_start)) => {
            let normal = plane_normal(drag.handle);
            let hit = plane_hit(ray, &drag.anchor, &normal)?;
            let mut delta = hit - *p_start;
            if snap_on {
                delta.x = snap(delta.x, SNAP_TRANSLATE);
                delta.y = snap(delta.y, SNAP_TRANSLATE);
                delta.z = snap(delta.z, SNAP_TRANSLATE);
            }
            Some((t0 + delta, r0))
        }
        (GizmoHandle::Ring(i), DragRef::RingRef(reference)) => {
            let n = axis(i);
            let mut angle = ring_angle(ray, &drag.anchor, &n, reference)?;
            if snap_on {
                angle = snap(angle, SNAP_ROTATE);
            }
            Some((t0, compose_rotation(&r0, &n, angle)))
        }
        _ => None,
    }
}

/// Update the live feature from the current pointer position.
fn update_drag(app: &mut ForgeApp, ray: &Ray3, snap_on: bool) {
    let Some(drag) = app.gizmo_drag.as_ref().cloned() else {
        return;
    };
    let Some((translation, rotation)) = drag_values(&drag, ray, snap_on) else {
        return;
    };
    let feature = Feature::TransformBody {
        source: drag.source,
        translation,
        rotation,
        pivot: drag.anchor,
    };
    if app.doc.edit_feature(drag.target, feature).is_ok() {
        app.request_evaluation();
    }
}

/// Finish the drag and commit exactly one undo command.
fn finish_drag(app: &mut ForgeApp) {
    let Some(drag) = app.gizmo_drag.take() else {
        return;
    };
    let after = app.doc.feature(drag.target).cloned();

    if drag.created {
        // Wrapper case: the live feature is already in the tree. Remove
        // it silently, then execute `AddFeature` for a single clean undo
        // step. A zero-delta drag is discarded entirely.
        let node = app.doc.tree.get(drag.target).cloned();
        if let Some(node) = node {
            let is_noop = match &node.feature {
                Feature::TransformBody {
                    translation,
                    rotation,
                    ..
                } => translation.norm() < 1e-9 && rotation.norm() < 1e-9,
                _ => false,
            };
            app.doc.tree.remove(node.id);
            if is_noop {
                // Revert the selection to the source body.
                app.selection
                    .select(SelectionItem::Body(BodyId::new(drag.source.raw())));
                app.set_status("Gizmo: no movement, nothing changed");
            } else {
                if let Err(e) = app
                    .commands
                    .execute(Command::AddFeature { node }, &mut app.doc)
                {
                    app.set_status(format!("{e}"));
                } else {
                    app.set_status(format!(
                        "Moved {} via gizmo (one undo step)",
                        drag.before.label()
                    ));
                }
            }
            app.request_evaluation();
        }
        return;
    }

    if let Some(after) = after {
        let label = after.label();
        let _ = app.commands.execute(
            Command::EditFeature {
                id: drag.target,
                before: drag.before,
                after: Box::new(after),
            },
            &mut app.doc,
        );
        app.set_status(format!("Transformed {label} via gizmo (one undo step)"));
        app.request_evaluation();
    }
}

/// Draw the gizmo (screen-space overlay, after the 3D paint callback).
fn draw_gizmo(
    ui: &mut egui::Ui,
    app: &ForgeApp,
    proj: &Projector,
    geom: &GizmoGeometry,
    anchor: Point3,
    hover: Option<GizmoHandle>,
) {
    let painter = ui.painter().clone();
    let mode = app.gizmo_mode;
    let active = app.gizmo_drag.as_ref().map(|d| d.handle);

    // Axis away-dimming: an axis pointing into the screen renders weaker.
    let dim = |i: usize| {
        let forward = app.camera.forward();
        let d = axis(i).dot(&forward);
        if d > 0.3 {
            0.45
        } else {
            1.0
        }
    };
    let color = |i: usize, hot: bool| {
        let base = axis_color(i);
        let a = dim(i);
        if hot {
            egui::Color32::from_rgb(
                ((base.r() as f32 * 0.55 + 255.0 * 0.45) * a) as u8,
                ((base.g() as f32 * 0.55 + 255.0 * 0.45) * a) as u8,
                ((base.b() as f32 * 0.55 + 255.0 * 0.45) * a) as u8,
            )
        } else {
            egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (255.0 * a) as u8)
        }
    };

    if mode == GizmoMode::Translate {
        // Plane handles first (under the arrows).
        for (pair, pts) in &geom.planes {
            let hot = matches!(hover, Some(h) if h == GizmoHandle::Plane(pair[0], pair[1]))
                || matches!(active, Some(h) if h == GizmoHandle::Plane(pair[0], pair[1]));
            let c = blend(axis_color(pair[0]), axis_color(pair[1]));
            let fill = if hot {
                c
            } else {
                egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 80)
            };
            let stroke = if hot {
                egui::Stroke::new(1.5, fill)
            } else {
                egui::Stroke::new(1.0, c)
            };
            painter.add(egui::Shape::convex_polygon(pts.to_vec(), fill, stroke));
        }
        // Arrows.
        for (i, (a, b)) in geom.arrows.iter().enumerate() {
            let hot = matches!(hover, Some(GizmoHandle::Axis(k)) if k == i)
                || matches!(active, Some(GizmoHandle::Axis(k)) if k == i);
            let col = color(i, hot);
            let width = if hot { 5.0 } else { 3.5 };
            let dir = (*b - *a).normalized();
            let head_len = 13.0_f32;
            let tip = *b;
            let base = *b - dir * head_len;
            let perp = egui::vec2(-dir.y, dir.x);
            // Shaft stops at the head base.
            painter.line_segment([*a, base], egui::Stroke::new(width, col));
            // Arrowhead triangle.
            let tri = [tip, base + perp * 6.0, base - perp * 6.0];
            painter.add(egui::Shape::convex_polygon(
                tri.to_vec(),
                col,
                egui::Stroke::NONE,
            ));
        }
    } else {
        for (i, pts) in geom.rings.iter().enumerate() {
            if pts.len() < 2 {
                continue;
            }
            let hot = matches!(hover, Some(GizmoHandle::Ring(k)) if k == i)
                || matches!(active, Some(GizmoHandle::Ring(k)) if k == i);
            let col = color(i, hot);
            let width = if hot { 5.0 } else { 3.0 };
            painter.add(egui::Shape::line(
                pts.clone(),
                egui::Stroke::new(width, col),
            ));
        }
        // Center dot marking the rotation pivot.
        if let Some(c) = proj.project(&anchor) {
            painter.circle_filled(c, 4.0, egui::Color32::from_gray(240));
            painter.circle_stroke(c, 4.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
        }
    }
}

/// Gizmo interaction phase: hover hit-testing, drag start/update/finish.
/// Runs *before* the viewport pick block so a gizmo press can suppress
/// pick-clicks. Returns `true` while a gizmo drag is active.
pub fn gizmo_interaction(
    ui: &mut egui::Ui,
    app: &mut ForgeApp,
    rect: egui::Rect,
    response: &egui::Response,
) -> bool {
    let target = gizmo_target(app);
    if target.is_none() && app.gizmo_drag.is_none() {
        app.gizmo_hover = None;
        return false;
    }

    let aspect = (rect.width() / rect.height()) as f64;
    let to_local = |p: egui::Pos2| egui::pos2(p.x - rect.min.x, p.y - rect.min.y);
    let to_ndc = |local: egui::Pos2| {
        (
            (local.x / rect.width()) as f64 * 2.0 - 1.0,
            1.0 - (local.y / rect.height()) as f64 * 2.0,
        )
    };

    // Live anchor: bbox center while idle, anchor + translation while
    // dragging (the gizmo rides the body).
    let anchor = render_anchor(app, target)
        .or_else(|| app.gizmo_drag.as_ref().map(|d| d.anchor))
        .unwrap_or_else(Point3::origin);
    let len = GIZMO_LEN_PX * world_per_pixel(&app.camera, rect.height() as f64, &anchor);

    let proj = Projector::new(&app.camera, aspect, rect);
    let geom = GizmoGeometry::build(&proj, &anchor, len, app.gizmo_mode);

    let mut dragging = app.gizmo_drag.is_some();
    let mut hover: Option<GizmoHandle> = None;
    let pointer = response.interact_pointer_pos().map(to_local);

    if let Some(mouse) = pointer {
        if dragging {
            hover = app.gizmo_drag.as_ref().map(|d| d.handle);
            if let Some(ray) = app.camera.ray_through_ndc(to_ndc(mouse), aspect) {
                let snap_off = ui.input(|i| i.modifiers.shift);
                update_drag(app, &ray, snap_off);
            }
        } else {
            hover = geom.hit(mouse, app.gizmo_mode).map(|h| h.handle);
        }
    }

    // A press on a handle (even one that never turns into a drag) owns
    // the click: the pick block must not re-select the body behind it.
    if response.is_pointer_button_down_on() && hover.is_some() {
        app.gizmo_press_on_handle = true;
    }

    if !dragging {
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let (Some(mouse), Some(hit)) = (pointer, hover) {
                if let Some(ray) = app.camera.ray_through_ndc(to_ndc(mouse), aspect) {
                    start_drag(app, hit, anchor, &ray);
                    dragging = true;
                }
            }
        }
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        finish_drag(app);
        dragging = false;
    }

    // Cursor feedback.
    if hover.is_some() || dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    app.gizmo_hover = hover;
    dragging
}

/// Gizmo drawing phase: screen-space overlay on top of the 3D paint
/// callback (call *after* the callback is registered — painter shapes
/// render in submission order).
pub fn gizmo_draw(ui: &mut egui::Ui, app: &ForgeApp, rect: egui::Rect) {
    let target = gizmo_target(app);
    if target.is_none() && app.gizmo_drag.is_none() {
        return;
    }
    let aspect = (rect.width() / rect.height()) as f64;
    let anchor = render_anchor(app, target)
        .or_else(|| app.gizmo_drag.as_ref().map(|d| d.anchor))
        .unwrap_or_else(Point3::origin);
    let len = GIZMO_LEN_PX * world_per_pixel(&app.camera, rect.height() as f64, &anchor);
    let proj = Projector::new(&app.camera, aspect, rect);
    let geom = GizmoGeometry::build(&proj, &anchor, len, app.gizmo_mode);
    draw_gizmo(ui, app, &proj, &geom, anchor, app.gizmo_hover);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(origin: Point3, dir: Vector3) -> Ray3 {
        Ray3::new(origin, dir).expect("unit-able")
    }

    #[test]
    fn snap_rounds_to_step() {
        assert!((snap(0.4, 1.0)).abs() < 1e-12);
        assert!((snap(0.7, 1.0) - 1.0).abs() < 1e-12);
        assert!((snap(1.49, 1.0) - 1.0).abs() < 1e-12);
        assert!((snap(-0.6, 1.0) + 1.0).abs() < 1e-12);
        assert!((snap(0.04, 0.1)).abs() < 1e-12);
        assert!((snap(0.06, 0.1) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn axis_param_finds_closest_line_point() {
        // Ray from the origin along +Y; axis line through (5, 0, 0)
        // along +X: the closest point on the line to the ray is the
        // origin itself, i.e. t = -5.
        let r = ray(Point3::origin(), Vector3::y());
        let t = axis_param(&r, &Point3::new(5.0, 0.0, 0.0), &Vector3::x());
        assert!((t.unwrap() - (-5.0)).abs() < 1e-9);

        // Perpendicular offset: ray along +Y starting at (0, 0, 3):
        // closest approach to the X line through the origin stays t = 0.
        let r = ray(Point3::new(0.0, 0.0, 3.0), Vector3::y());
        let t = axis_param(&r, &Point3::origin(), &Vector3::x());
        assert!(t.unwrap().abs() < 1e-9);
    }

    #[test]
    fn axis_param_parallel_is_none() {
        let r = ray(Point3::origin(), Vector3::x());
        assert!(axis_param(&r, &Point3::new(0.0, 5.0, 0.0), &Vector3::x()).is_none());
    }

    #[test]
    fn plane_hit_intersects() {
        let r = ray(Point3::new(0.0, 0.0, 5.0), -Vector3::z());
        let hit = plane_hit(&r, &Point3::new(2.0, 2.0, 0.0), &Vector3::z());
        let hit = hit.expect("hit");
        assert!(hit.coords.norm() < 1e-9, "{hit:?}");

        // Behind the ray origin: no hit.
        let r = ray(Point3::new(0.0, 0.0, 5.0), Vector3::z());
        assert!(plane_hit(&r, &Point3::origin(), &Vector3::z()).is_none());
    }

    #[test]
    fn ring_angle_signs() {
        let anchor = Point3::origin();
        let n = Vector3::z();
        let reference = Vector3::x();
        // Hit at +Y: quarter turn CCW about +Z.
        let r = ray(Point3::new(0.0, 10.0, 7.0), -Vector3::z());
        let a = ring_angle(&r, &anchor, &n, &reference).unwrap();
        assert!((a - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{a}");
        // Hit at -Y: -90°.
        let r = ray(Point3::new(0.0, -10.0, 7.0), -Vector3::z());
        let a = ring_angle(&r, &anchor, &n, &reference).unwrap();
        assert!((a + std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{a}");
    }

    #[test]
    fn compose_rotation_world_frame() {
        // Zero start + 90° about Z → pure Z euler.
        let e = compose_rotation(
            &Vector3::zeros(),
            &Vector3::z(),
            std::f64::consts::FRAC_PI_2,
        );
        assert!(e.x.abs() < 1e-9 && e.y.abs() < 1e-9);
        assert!((e.z - std::f64::consts::FRAC_PI_2).abs() < 1e-9);

        // Left-multiplication semantics: the delta acts in the world
        // frame, i.e. from_euler_angles(result) == q_delta * q_start.
        let start = Vector3::new(0.3, 0.2, 0.1);
        let q_start = nalgebra::UnitQuaternion::from_euler_angles(start.x, start.y, start.z);
        let q_delta = nalgebra::UnitQuaternion::from_axis_angle(
            &nalgebra::Unit::new_normalize(Vector3::y()),
            0.6,
        );
        let want = q_delta * q_start;
        let got = compose_rotation(&start, &Vector3::y(), 0.6);
        let q_got = nalgebra::UnitQuaternion::from_euler_angles(got.x, got.y, got.z);
        let diff = (want.inverse() * q_got).angle();
        assert!(diff.abs() < 1e-9, "rotational mismatch {diff}");
    }

    #[test]
    fn rebase_pivot_preserves_world_positions() {
        let euler = Vector3::new(0.4, -0.2, 0.9);
        let translation = Vector3::new(10.0, -3.0, 2.0);
        let old_pivot = Point3::origin();
        let new_pivot = Point3::new(4.0, 5.0, -1.0);

        let r = nalgebra::UnitQuaternion::from_euler_angles(euler.x, euler.y, euler.z);
        let map = |pivot: &Point3, t: &Vector3, p: &Point3| -> Point3 {
            pivot + r.transform_vector(&(p - pivot)) + t
        };

        let t_new = rebase_pivot(&translation, &euler, &old_pivot, &new_pivot);
        for p in [
            Point3::origin(),
            Point3::new(3.0, -7.0, 11.0),
            Point3::new(-20.0, 50.0, 0.5),
        ] {
            let before = map(&old_pivot, &translation, &p);
            let after = map(&new_pivot, &t_new, &p);
            assert!(
                (before - after).norm() < 1e-9,
                "p {p:?}: {before:?} vs {after:?}"
            );
        }

        // Identity rotation: the translation is unchanged.
        let t_id = rebase_pivot(&translation, &Vector3::zeros(), &old_pivot, &new_pivot);
        assert!((t_id - translation).norm() < 1e-12);
    }

    #[test]
    fn world_per_pixel_orthographic() {
        let cam = forge_render::Camera {
            orthographic: true,
            distance: 100.0,
            fov_deg: 45.0,
            ..Default::default()
        };
        // half-height = 100·tan(22.5°) ≈ 41.42; full = 82.84 world units
        // over 400 px → ~0.2071 per px.
        let wpp = world_per_pixel(&cam, 400.0, &Point3::origin());
        let expect = 2.0 * cam.ortho_half_height() / 400.0;
        assert!((wpp - expect).abs() < 1e-12);
    }

    #[test]
    fn projector_maps_center_to_rect_center() {
        let cam = Camera::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(600.0, 400.0));
        let aspect = 600.0 / 400.0;
        let proj = Projector::new(&cam, aspect, rect);
        let p = proj.project(&cam.target).expect("target in view");
        assert!(p.distance(rect.center()) < 0.5, "projected {p:?}");
    }

    #[test]
    fn geometry_hit_arrow_and_ring() {
        // Anchor at the camera target so the gizmo is centered on screen.
        let cam = Camera::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 400.0));
        let aspect = 600.0 / 400.0;
        let proj = Projector::new(&cam, aspect, rect);
        let anchor = cam.target;
        let len = 50.0 * world_per_pixel(&cam, 400.0, &anchor);

        let geom = GizmoGeometry::build(&proj, &anchor, len, GizmoMode::Translate);
        // Mouse near the middle of some arrow: must hit an axis handle.
        let (a, b) = geom.arrows[0];
        let mid = a + (b - a) * 0.5;
        assert!(matches!(
            geom.hit(mid, GizmoMode::Translate).map(|h| h.handle),
            Some(GizmoHandle::Axis(_))
        ));

        // A point far from all handles: no hit.
        assert!(geom
            .hit(egui::pos2(5.0, 5.0), GizmoMode::Translate)
            .is_none());

        let geom = GizmoGeometry::build(&proj, &anchor, len, GizmoMode::Rotate);
        // A point on the Z ring (screen-horizontal circle in the default
        // iso view): must hit Ring.
        let ring = &geom.rings[2];
        let p = ring[ring.len() / 4]; // quarter-way point
        assert!(matches!(
            geom.hit(p, GizmoMode::Rotate).map(|h| h.handle),
            Some(GizmoHandle::Ring(_))
        ));
    }

    #[test]
    fn drag_values_translate_and_rotate() {
        let drag = GizmoDrag {
            target: FeatureId::new(1),
            source: FeatureId::new(1),
            created: false,
            before: Box::new(Feature::Primitive(forge_model::PrimitiveParams {
                kind: forge_model::PrimitiveKind::Box,
                center: Point3::origin(),
                dims: Vector3::new(1.0, 1.0, 1.0),
            })),
            handle: GizmoHandle::Axis(0),
            anchor: Point3::origin(),
            start_translation: Vector3::zeros(),
            start_rotation: Vector3::zeros(),
            reference: DragRef::AxisT(0.0),
        };

        // Pointer ray passing near the point (7.2, 0, 0): its closest
        // point on the X axis is t = 7.2; with snapping the delta snaps
        // to 7 mm.
        let r = ray(Point3::new(7.2, 3.0, 5.0), -Vector3::z());
        let (t, rot) = drag_values(&drag, &r, true).expect("values");
        assert!((t.x - 7.0).abs() < 1e-9, "{t:?}");
        assert!(t.y.abs() < 1e-9 && t.z.abs() < 1e-9);
        assert!(rot.norm() < 1e-9);

        // Snapping off: raw 7.2.
        let (t, _) = drag_values(&drag, &r, false).expect("values");
        assert!((t.x - 7.2).abs() < 1e-9);

        // Ring drag: 30° about Z (snapped to 5° steps: 30° stays 30°).
        let mut drag = drag;
        drag.handle = GizmoHandle::Ring(2);
        drag.reference = DragRef::RingRef(Vector3::x());
        // Hit 30° CCW: point at (cos30, sin30) scaled.
        let (c, s) = (30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin());
        let r = ray(Point3::new(10.0 * c, 10.0 * s, 8.0), -Vector3::z());
        let (t, rot) = drag_values(&drag, &r, true).expect("values");
        assert!(t.norm() < 1e-9);
        assert!((rot.z - 30.0_f64.to_radians()).abs() < 1e-9, "{rot:?}");
    }
}
