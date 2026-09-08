//! W-04: sub-body selection — faces (coplanar clusters), edges (tangent
//! sharp-edge chains) and vertices — plus the viewport highlight overlay.
//!
//! Face ids are the cluster's smallest triangle index (deterministic under
//! re-evaluation of an unchanged mesh); edge ids the chain's smallest
//! vertex index; vertex ids the position index. Selections are transient
//! UI state, so tessellation changes simply invalidate them.

use crate::app::ForgeApp;
use crate::gizmo::{dist_to_segment, Projector};
use forge_core::{EdgeId, FaceId, VertexId};
use forge_model::SelectionItem;

/// Picking granularity (toolbar combo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickMode {
    /// Whole bodies (GPU id-buffer pick — the original behavior).
    #[default]
    Bodies,
    /// Coplanar triangle clusters under the cursor.
    Faces,
    /// Sharp-edge tangent chains near the cursor.
    Edges,
    /// Vertices near the cursor.
    Vertices,
}

impl PickMode {
    /// Toolbar label.
    pub fn label(self) -> &'static str {
        match self {
            PickMode::Bodies => "Bodies",
            PickMode::Faces => "Faces",
            PickMode::Edges => "Edges",
            PickMode::Vertices => "Vertices",
        }
    }
}

/// Screen-space pick radius (logical px).
const PICK_RADIUS: f32 = 10.0;
/// Coplanar clustering tolerance (radians).
const FACE_TOL: f64 = 1.0_f64.to_radians();
/// Dihedral threshold for sharp edges (radians).
const EDGE_TOL: f64 = 45.0_f64.to_radians();
/// Tangent chaining tolerance (radians).
const CHAIN_TOL: f64 = 30.0_f64.to_radians();

/// Canonical id of an edge chain: its smallest vertex index.
fn chain_id(chain: &[[u32; 2]]) -> u64 {
    chain
        .iter()
        .flat_map(|e| e.iter())
        .copied()
        .map(u64::from)
        .min()
        .unwrap_or(u64::MAX)
}

/// The result of a sub-body pick: the item plus a status-line message.
pub struct SubPick {
    pub item: SelectionItem,
    pub status: String,
}

/// One sub-body pick at viewport-local `click` (logical px).
/// `None` = nothing under the cursor (empty-space click).
pub fn sub_pick(
    app: &mut ForgeApp,
    ndc: (f64, f64),
    aspect: f64,
    rect: egui::Rect,
    click: egui::Pos2,
) -> Option<SubPick> {
    let mode = app.pick_mode;
    if mode == PickMode::Bodies {
        return None; // bodies go through the GPU id-buffer pick
    }
    let (body_idx, _t, tri) = app.raycast_bodies(ndc, aspect)?;
    let body = app
        .last_evaluation
        .as_ref()
        .and_then(|ev| ev.bodies.get(body_idx))?
        .id;
    let mesh = &app
        .last_evaluation
        .as_ref()
        .and_then(|ev| ev.bodies.get(body_idx))?
        .mesh;
    let body_name = app
        .last_evaluation
        .as_ref()
        .and_then(|ev| ev.bodies.get(body_idx))?
        .name
        .clone();

    match mode {
        PickMode::Faces => {
            let cluster = mesh.face_cluster(tri, FACE_TOL);
            let min_tri = *cluster.first()? as u64;
            Some(SubPick {
                item: SelectionItem::Face {
                    body,
                    face: FaceId::new(min_tri),
                },
                status: format!(
                    "Selected face ({cluster_len} triangles) of {body_name}",
                    cluster_len = cluster.len()
                ),
            })
        }
        PickMode::Edges => {
            let proj = Projector::new(&app.camera, aspect, rect);
            let chains = mesh.sharp_edge_chains(EDGE_TOL, CHAIN_TOL);
            let mut best: Option<(f32, u64, usize)> = None; // (dist, chain id, edge count)
            for chain in &chains {
                'seg: for [a, b] in chain {
                    let (Some(pa), Some(pb)) = (
                        mesh.positions.get(*a as usize),
                        mesh.positions.get(*b as usize),
                    ) else {
                        continue 'seg;
                    };
                    let (Some(sa), Some(sb)) = (proj.project(pa), proj.project(pb)) else {
                        continue 'seg;
                    };
                    let d = dist_to_segment(click, sa, sb);
                    if d <= PICK_RADIUS && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                        best = Some((d, chain_id(chain), chain.len()));
                    }
                }
            }
            best.map(|(_, id, count)| SubPick {
                item: SelectionItem::Edge {
                    body,
                    edge: EdgeId::new(id),
                },
                status: format!("Selected edge chain ({count} segments) of {body_name}"),
            })
        }
        PickMode::Vertices => {
            let proj = Projector::new(&app.camera, aspect, rect);
            let mut best: Option<(f32, usize)> = None;
            for (i, p) in mesh.positions.iter().enumerate() {
                if let Some(sp) = proj.project(p) {
                    let d = click.distance(sp);
                    if d <= PICK_RADIUS && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                        best = Some((d, i));
                    }
                }
            }
            best.map(|(_, i)| SubPick {
                item: SelectionItem::Vertex {
                    body,
                    vertex: VertexId::new(i as u64),
                },
                status: format!("Selected vertex {i} of {body_name}"),
            })
        }
        PickMode::Bodies => None,
    }
}

/// Highlight overlay for selected faces / edges / vertices (drawn after
/// the 3D paint callback): translucent face fill + boundary outline,
/// chain polylines, vertex dots.
pub fn draw_sub_selection(ui: &mut egui::Ui, app: &ForgeApp, rect: egui::Rect) {
    let Some(ev) = &app.last_evaluation else {
        return;
    };
    let has_sub = app.selection.items.iter().any(|i| {
        matches!(
            i,
            SelectionItem::Face { .. } | SelectionItem::Edge { .. } | SelectionItem::Vertex { .. }
        )
    });
    if !has_sub {
        return;
    }

    let aspect = (rect.width() / rect.height()) as f64;
    let proj = Projector::new(&app.camera, aspect, rect);
    let painter = ui.painter().clone();
    let accent = egui::Color32::from_rgb(255, 176, 32);
    let fill = egui::Color32::from_rgba_unmultiplied(255, 176, 32, 56);

    for item in &app.selection.items {
        let body_id = item.body();
        let Some(b) = ev.bodies.iter().find(|b| b.id == body_id) else {
            continue;
        };
        match item {
            SelectionItem::Face { face, .. } => {
                let cluster = b.mesh.face_cluster(face.raw() as usize, FACE_TOL);
                if cluster.is_empty() {
                    continue;
                }
                // Translucent fill of every cluster triangle.
                for t in &cluster {
                    let [a, c, d] = b.mesh.triangle_idx(*t);
                    let pts = [a, c, d]
                        .iter()
                        .filter_map(|v| b.mesh.positions.get(*v as usize))
                        .filter_map(|p| proj.project(p))
                        .collect::<Vec<_>>();
                    if pts.len() == 3 {
                        painter.add(egui::Shape::convex_polygon(pts, fill, egui::Stroke::NONE));
                    }
                }
                // Boundary outline.
                for [a, c] in b.mesh.cluster_boundary_edges(&cluster) {
                    if let (Some(pa), Some(pc)) = (
                        b.mesh
                            .positions
                            .get(a as usize)
                            .and_then(|p| proj.project(p)),
                        b.mesh
                            .positions
                            .get(c as usize)
                            .and_then(|p| proj.project(p)),
                    ) {
                        painter.line_segment([pa, pc], egui::Stroke::new(2.0, accent));
                    }
                }
            }
            SelectionItem::Edge { edge, .. } => {
                let chains = b.mesh.sharp_edge_chains(EDGE_TOL, CHAIN_TOL);
                let Some(chain) = chains
                    .iter()
                    .find(|c| chain_id(c) == edge.raw())
                    .or_else(|| {
                        chains
                            .iter()
                            .find(|c| c.iter().any(|e| e.contains(&(edge.raw() as u32))))
                    })
                else {
                    continue;
                };
                for [a, c] in chain {
                    if let (Some(pa), Some(pc)) = (
                        b.mesh
                            .positions
                            .get(*a as usize)
                            .and_then(|p| proj.project(p)),
                        b.mesh
                            .positions
                            .get(*c as usize)
                            .and_then(|p| proj.project(p)),
                    ) {
                        painter.line_segment([pa, pc], egui::Stroke::new(3.0, accent));
                    }
                }
            }
            SelectionItem::Vertex { vertex, .. } => {
                if let Some(p) = b
                    .mesh
                    .positions
                    .get(vertex.raw() as usize)
                    .and_then(|p| proj.project(p))
                {
                    painter.circle_filled(p, 5.0, accent);
                    painter.circle_stroke(p, 5.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
                }
            }
            SelectionItem::Body(_) => {}
        }
    }
}
