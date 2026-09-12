//! Detailing operations: fillet, chamfer (mesh-approximate, K-03).
//!
//! **Approach:** boolean cutters. Each edge to detail is captured
//! geometrically ([`EdgeSpec`]); at apply time the two adjacent face
//! normals are resolved from the mesh, the edge is classified convex
//! (ridge — material corner) or concave (valley — void corner), and a
//! prismatic cutter is built from the cross-section polygon
//!
//! ```text
//!   convex edge (chamfer d):   A —— the cutter wedge removes the corner
//!        u2                     [A, A+d·u1, A+d·u2]  → Difference
//!      ↗   A —— u1
//!   ────────────── face 1
//! ```
//!
//! - **chamfer**: the wedge triangle above (equal-distance semantics),
//! - **fillet**: the same polygon with the corner arc discretized to
//!   `segments` points (tangent distance `r/tan(ψ/2)`, arc center on the
//!   bisector),
//! - **concave edges** Union the mirrored region instead (filling the
//!   valley corner).
//!
//! This is the classic mesh-approximate detail (exact rolling-ball
//! fillets remain gated on the Phase-4 B-Rep kernel; see the stubs at
//! the bottom of this file). Undercut-free and watertight by
//! construction, because the boolean kernel guarantees both.
//!
//! [`EdgeSpec`] carries *positions*, not mesh indices: the spec survives
//! re-tessellation and re-evaluation (normals are re-resolved against
//! the current mesh at apply time).

use crate::boolean::{boolean, CsgOp};
use crate::error::{GeometryError, Result};
use crate::mesh::TriMesh;
use forge_core::{EdgeId, FaceId, Point3, Vector3};

/// Snap tolerance for matching an [`EdgeSpec`] to a mesh edge (mm).
const EDGE_MATCH_TOL: f64 = 1e-4;

/// Cutter overshoot (mm): extends cutters past exact-coplanar boolean
/// faces (the same LEAD trick the hole tool uses). End extensions run
/// into air; concave leg penetrations run into existing material —
/// neither changes the result volume.
const PEN: f64 = 1e-3;

/// A chamfer/fillet target edge, captured geometrically.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EdgeSpec {
    /// Edge start (world/model space).
    pub a: Point3,
    /// Edge end.
    pub b: Point3,
}

impl EdgeSpec {
    /// Edge direction (unit).
    fn dir(&self) -> Vector3 {
        let mut t = self.b - self.a;
        let len = t.norm();
        if len < 1e-12 {
            Vector3::z()
        } else {
            t /= len;
            t
        }
    }

    /// Length (mm).
    #[allow(dead_code)]
    fn length(&self) -> f64 {
        (self.b - self.a).norm()
    }
}

/// Resolved local frame of an edge inside a mesh.
struct EdgeFrame {
    /// A point on the edge (the mesh edge midpoint).
    mid: Point3,
    /// Face 1 outward normal (area-weighted cluster mean).
    n1: Vector3,
    /// Face 2 outward normal.
    n2: Vector3,
    /// Unit edge direction.
    t: Vector3,
    /// Direction from the edge into face 1 (⊥ t).
    u1: Vector3,
    /// Direction from the edge into face 2 (⊥ t).
    u2: Vector3,
    /// True when the edge is a convex ridge (material corner).
    convex: bool,
    /// Edge start (the t-most-negative endpoint).
    start: Point3,
    /// Edge length (mm).
    length: f64,
}

impl EdgeFrame {
    /// Extrusion extent: the start point and length along `t`.
    fn extent(&self) -> (Point3, f64) {
        (self.start, self.length)
    }
}

/// Resolve the edge frame: find the mesh edge matching `spec` (endpoints
/// within tolerance), take its two adjacent triangles, and derive the
/// in-face directions + convexity.
fn resolve_frame(mesh: &TriMesh, spec: &EdgeSpec) -> Result<EdgeFrame> {
    // Build the edge → triangles map once. Booleans split edges with
    // T-junctions and can duplicate endpoints: match *every* mesh edge
    // that lies on the spec segment (both endpoints on the a→b line) and
    // pool their adjacent triangles.
    let edge_tris = mesh.edge_triangle_map();
    let seg = spec.b - spec.a;
    let seg_len = seg.norm();
    let seg_dir = if seg_len > 1e-12 {
        seg / seg_len
    } else {
        Vector3::z()
    };
    let dist_to_seg = |p: &Point3| -> f64 {
        let d = *p - spec.a;
        let t = (d.dot(&seg_dir) / seg_len).clamp(0.0, 1.0);
        (*p - (spec.a + seg_dir * (t * seg_len))).norm()
    };
    let want = |p: &Point3, q: &Point3| -> bool {
        dist_to_seg(p) < EDGE_MATCH_TOL
            && dist_to_seg(q) < EDGE_MATCH_TOL
            && ((q - p).norm() > 1e-9)
            && ((q - p).normalize().dot(&seg_dir).abs() > 0.99)
    };
    let mut tris: Vec<usize> = Vec::new();
    let mut found_any = false;
    for ((a, b), ts) in &edge_tris {
        let pa = mesh.positions[*a as usize];
        let pb = mesh.positions[*b as usize];
        if want(&pa, &pb) {
            found_any = true;
            tris.extend(ts.iter().copied());
        }
    }
    if !found_any {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            format!("chamfer/fillet edge {spec:?} not found in the mesh (±{EDGE_MATCH_TOL} mm)"),
        )));
    }
    // Booleans split faces and T-junction bridge slivers can touch an
    // edge: cluster adjacent triangles by *area-weighted* normal (the
    // slivers carry ~zero weight) and keep the largest representative
    // per side; we need exactly two dominant sides.
    let mut sides: Vec<(Vector3, f64, usize)> = Vec::new(); // (mean normal, area, best tri)
    for &t in &tris {
        let Some(n) = mesh.triangle_normal(t) else {
            continue;
        };
        let area = 0.5 * mesh.triangle_normal_raw(t).norm();
        match sides
            .iter_mut()
            .find(|(sn, _, _)| sn.dot(&n) > (30.0_f64).to_radians().cos())
        {
            Some(slot) => {
                let total = slot.1 + area;
                let w_old = slot.1 / total;
                let w_new = area / total;
                let mean = slot.0 * w_old + n * w_new;
                slot.0 = mean.normalize();
                if area > 0.5 * mesh.triangle_normal_raw(slot.2).norm() {
                    slot.2 = t;
                }
                slot.1 = total;
            }
            None => sides.push((n, area, t)),
        }
    }
    if sides.len() != 2 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            format!(
                "edge {spec:?} has {} distinct adjacent faces (need exactly 2)",
                sides.len()
            ),
        )));
    }
    let tris = [sides[0].2, sides[1].2];
    // The spec *is* the full edge (within EDGE_MATCH_TOL): derive the
    // frame geometry from it — pooled sub-edges have arbitrary split
    // points (and HashMap iteration order is nondeterministic).
    let mid = spec.a + (spec.b - spec.a) * 0.5;
    // The edge direction is the spec's own direction (the mesh sub-edges
    // may be split arbitrarily; the spec spans the full edge).
    let t = spec.dir();
    let into_face = |tri: usize, mid: &Point3, t: &Vector3| -> Vector3 {
        let [a, b, c] = mesh.triangle(tri);
        let centroid = a + (b - a) * 0.5 + (c - a) * (1.0 / 3.0);
        let mut u = centroid - *mid;
        u -= t * u.dot(t); // project off the edge direction
        let len = u.norm();
        if len < 1e-12 {
            // Degenerate: any in-plane direction ⊥ t ⊥ n.
            let n = mesh.triangle_normal(tri).unwrap_or(Vector3::z());
            t.cross(&n)
        } else {
            u / len
        }
    };
    let u1 = into_face(tris[0], &mid, &t);
    let u2 = into_face(tris[1], &mid, &t);
    // Convexity: face 2's interior pointing against face 1's outward
    // normal ⇒ the faces fold toward each other (ridge).
    let convex = sides[0].0.dot(&u2) < 0.0;
    // Extent along +t (spec endpoints, ordered).
    let (start, end) = if (spec.b - spec.a).dot(&t) >= 0.0 {
        (spec.a, spec.b)
    } else {
        (spec.b, spec.a)
    };
    let length = (end - start).norm();
    Ok(EdgeFrame {
        mid,
        n1: sides[0].0,
        n2: sides[1].0,
        t,
        u1,
        u2,
        convex,
        start,
        length,
    })
}

/// Cross-section polygon for a chamfer: `[A, A+d·u1, A+d·u2]`.
///
/// Concave edges: the legs lie exactly on the face planes (coplanar with
/// the boolean partner) — penetrate them by `PEN` into the material,
/// which is already solid, so the Union volume is unchanged.
#[allow(clippy::too_many_arguments)]
fn chamfer_polygon(
    mid: &Point3,
    u1: &Vector3,
    u2: &Vector3,
    d: f64,
    concave: bool,
    n1: &Vector3,
    n2: &Vector3,
) -> Vec<Point3> {
    let pen = if concave { PEN } else { 0.0 };
    vec![*mid, *mid + *u1 * d - *n1 * pen, *mid + *u2 * d - *n2 * pen]
}

/// Cross-section polygon for a fillet: `[corner, V1, arc…, V2]`.
///
/// ψ = the small angle between the in-face directions. The tangent
/// distance is `r/tan(ψ/2)`; the arc center sits on the small-wedge
/// bisector at `r/sin(ψ/2)` and the arc passes through the apex
/// `center − bis·r` (the point closest to the corner). The region
/// between the corner and the arc is *removed* for convex edges and
/// *added* for concave ones — the polygon itself is identical.
#[allow(clippy::too_many_arguments)]
fn fillet_polygon(
    mid: &Point3,
    u1: &Vector3,
    u2: &Vector3,
    r: f64,
    segments: usize,
    concave: bool,
    n1: &Vector3,
    n2: &Vector3,
) -> Vec<Point3> {
    let segments = segments.clamp(2, 64);
    let cos_psi = u1.dot(u2).clamp(-1.0, 1.0);
    let psi = cos_psi
        .acos()
        .min((178.0_f64).to_radians())
        .max((2.0_f64).to_radians());
    let half = psi * 0.5;
    let tan_len = r / half.tan();
    let bis = (*u1 + *u2).normalize();
    let center = *mid + bis * (r / half.sin());
    // Concave (Union): penetrate the tangent legs by PEN into the
    // material so the prism walls are not coplanar with the faces (the
    // overlap is already solid — the added volume is unchanged).
    let pen = if concave { PEN } else { 0.0 };
    let v1 = *mid + *u1 * tan_len - *n1 * pen;
    let v2 = *mid + *u2 * tan_len - *n2 * pen;
    let apex = center - bis * r;
    // Circle basis.
    let e0 = (v1 - center).normalize();
    let plane_n = u1.cross(u2).normalize();
    let e1 = plane_n.cross(&e0).normalize();
    let ang_of = |p: &Point3| -> f64 {
        let d = *p - center;
        // atan2(y, x): y along e1, x along e0.
        d.dot(&e1).atan2(d.dot(&e0))
    };
    let a1 = ang_of(&v1);
    let a2 = ang_of(&v2);
    // Sweep direction: whichever arc midpoint lands closer to the apex.
    let two_pi = 2.0 * std::f64::consts::PI;
    let norm_angle = |x: f64| -> f64 {
        let mut v = x % two_pi;
        if v < 0.0 {
            v += two_pi;
        }
        v
    };
    let ccw = norm_angle(a2 - a1);
    let cw = norm_angle(a1 - a2);
    let point_at = |ang: f64| -> Point3 { center + e0 * (r * ang.cos()) + e1 * (r * ang.sin()) };
    let mid_ccw = point_at(a1 + ccw * 0.5);
    let mid_cw = point_at(a1 - cw * 0.5);
    let sweep = if (mid_ccw - apex).norm() <= (mid_cw - apex).norm() {
        ccw
    } else {
        -cw
    };
    let mut pts = vec![v1];
    for i in 1..segments {
        let t = i as f64 / segments as f64;
        pts.push(point_at(a1 + sweep * t));
    }
    pts.push(v2);
    let mut poly = vec![*mid];
    poly.extend(pts);
    poly
}

/// Sweep the cross-section polygon (all points lie in the plane ⊥ `t`
/// through the edge start) along `t` by `length`, via the production
/// extrude pipeline (correct winding, caps, watertightness).
fn build_prism(poly: &[Point3], start: &Point3, t: &Vector3, length: f64) -> Result<TriMesh> {
    // Overshoot both ends into air: avoids coplanar caps at the body's
    // end faces without changing the cut volume.
    let start = *start - *t * PEN;
    let plane = forge_core::Plane::new(start, *t).ok_or_else(|| {
        GeometryError::Core(forge_core::CoreError::Invalid(
            "degenerate chamfer plane".into(),
        ))
    })?;
    let outer: Vec<forge_core::Point2> = poly.iter().map(|p| plane.to_local(*p)).collect();
    let profile = crate::sweep::Profile2D::new(outer, Vec::new())?;
    crate::sweep::extrude(
        &profile,
        &plane,
        &crate::sweep::ExtrudeParams {
            distance: length + 2.0 * PEN,
            direction: crate::ExtrudeDirection::Positive,
            draft_angle: 0.0,
        },
    )
}

/// Apply chamfers to the listed edges (K-03, mesh-approximate).
///
/// Each edge is processed sequentially against the evolving mesh; convex
/// edges subtract the wedge, concave edges union it.
pub fn chamfer_edges(mesh: &TriMesh, edges: &[EdgeSpec], distance: f64) -> Result<TriMesh> {
    if distance <= 0.0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "chamfer distance must be positive".into(),
        )));
    }
    let mut body = mesh.clone();
    for spec in edges {
        let frame = resolve_frame(&body, spec)?;
        let (start, length) = frame.extent();
        let poly = chamfer_polygon(
            &frame.mid,
            &frame.u1,
            &frame.u2,
            distance,
            !frame.convex,
            &frame.n1,
            &frame.n2,
        );
        let cutter = build_prism(&poly, &start, &frame.t, length)?;
        let op = if frame.convex {
            CsgOp::Difference
        } else {
            CsgOp::Union
        };
        body = boolean(&body, &cutter, op)?;
    }
    Ok(body)
}

/// Apply constant-radius fillets (polygonal approximation, K-03).
pub fn fillet_edges(
    mesh: &TriMesh,
    edges: &[EdgeSpec],
    radius: f64,
    segments: usize,
) -> Result<TriMesh> {
    if radius <= 0.0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "fillet radius must be positive".into(),
        )));
    }
    let mut body = mesh.clone();
    for spec in edges {
        let frame = resolve_frame(&body, spec)?;
        let (start, length) = frame.extent();
        let poly = fillet_polygon(
            &frame.mid,
            &frame.u1,
            &frame.u2,
            radius,
            segments,
            !frame.convex,
            &frame.n1,
            &frame.n2,
        );
        let cutter = build_prism(&poly, &start, &frame.t, length)?;
        let op = if frame.convex {
            CsgOp::Difference
        } else {
            CsgOp::Union
        };
        body = boolean(&body, &cutter, op)?;
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// B-Rep-gated API (Phase 4): unchanged stubs, kept for API compatibility.
// ---------------------------------------------------------------------------

/// Apply a constant-radius fillet to topological edges (requires the
/// B-Rep kernel; use [`fillet_edges`] for the mesh approximation).
pub fn fillet(_mesh: &TriMesh, _edges: &[EdgeId], _radius: f64) -> Result<TriMesh> {
    not_implemented("topological fillet")
}

/// Apply a chamfer to topological edges (B-Rep gated; use
/// [`chamfer_edges`] for the mesh approximation).
pub fn chamfer(_mesh: &TriMesh, _edges: &[EdgeId], _distance: f64) -> Result<TriMesh> {
    not_implemented("topological chamfer")
}

/// Hollow the solid, keeping walls of `thickness` and opening the listed
/// faces.
pub fn shell(_mesh: &TriMesh, _thickness: f64, _open_faces: &[FaceId]) -> Result<TriMesh> {
    not_implemented("shell")
}

/// Offset a body by `distance` (positive grows outward).
pub fn offset(_mesh: &TriMesh, _distance: f64) -> Result<TriMesh> {
    not_implemented("offset body")
}

fn not_implemented(what: &str) -> Result<TriMesh> {
    Err(crate::error::GeometryError::Core(
        forge_core::CoreError::NotImplemented(format!(
            "{what} requires the B-Rep kernel (roadmap Phase 4)"
        )),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;
    use forge_core::Vector3;

    fn boxmesh(w: f64, d: f64, h: f64) -> TriMesh {
        primitives::box_from_center_extents(Point3::new(0.0, 0.0, h / 2.0), Vector3::new(w, d, h))
    }

    /// The top front edge of a box: (-w/2..w/2, +d/2, h).
    fn top_front_edge(m: &TriMesh, w: f64, d: f64, h: f64) -> EdgeSpec {
        let _ = m;
        EdgeSpec {
            a: Point3::new(-w / 2.0, d / 2.0, h),
            b: Point3::new(w / 2.0, d / 2.0, h),
        }
    }

    #[test]
    fn chamfer_one_box_edge_removes_exact_wedge() {
        // Box 10×10×10 = 1000 mm³; chamfer d=2 on one vertical-ish edge
        // removes a triangular prism: (d²/2)·length.
        let m = boxmesh(10.0, 10.0, 10.0);
        let e = top_front_edge(&m, 10.0, 10.0, 10.0);
        let out = chamfer_edges(&m, &[e], 2.0).unwrap();
        let v = out.volume_signed();
        let want = 1000.0 - 0.5 * 2.0 * 2.0 * 10.0;
        assert!((v - want).abs() < 1e-6, "volume {v} vs {want}");
    }

    #[test]
    fn chamfer_concave_edge_adds_material() {
        // L-shaped pocket: the inner corner is concave — chamfering adds
        // the wedge. Build an L from two boxes via union.
        let a = primitives::box_from_center_extents(
            Point3::new(0.0, 0.0, 2.5),
            Vector3::new(10.0, 10.0, 5.0),
        );
        // The riser overlaps the base volume (extends to z=2.5, inside
        // the base): a proper L, not two edge-kissing boxes.
        let b = primitives::box_from_center_extents(
            Point3::new(7.5, 0.0, 6.25),
            Vector3::new(5.0, 10.0, 7.5),
        );
        let l = boolean(&a, &b, CsgOp::Union).unwrap();
        let v0 = l.volume_signed();
        // Concave edge: where the step meets the base top: x=5, z=5.
        let e = EdgeSpec {
            a: Point3::new(5.0, -5.0, 5.0),
            b: Point3::new(5.0, 5.0, 5.0),
        };
        let out = chamfer_edges(&l, &[e], 2.0).unwrap();
        let v1 = out.volume_signed();
        assert!(v1 > v0, "concave chamfer must add material: {v0} → {v1}");
        // Added wedge ≈ d²/2 × length (10).
        assert!(
            (v1 - v0 - 0.5 * 4.0 * 10.0).abs() < 1.0,
            "delta {}",
            v1 - v0
        );
    }

    #[test]
    fn fillet_one_box_edge_removes_corner_sliver() {
        let m = boxmesh(10.0, 10.0, 10.0);
        let e = top_front_edge(&m, 10.0, 10.0, 10.0);
        let out = fillet_edges(&m, &[e], 2.0, 16).unwrap();
        let v = out.volume_signed();
        // Removed = (r² − πr²/4)·L for a 90° edge (corner area minus the
        // quarter disc), r=2, L=10.
        let want = 1000.0 - (4.0 - std::f64::consts::PI) * 4.0 / 4.0 * 10.0;
        assert!((v - want).abs() < 0.6, "volume {v} vs {want}");
    }

    #[test]
    fn fillet_concave_edge_adds_quarter_cylinder() {
        // The L corner: a fillet adds roughly a quarter-cylinder of
        // material (πr²/4·L) minus nothing — for a 90° valley the fill
        // is the quarter disc area × length.
        let a = primitives::box_from_center_extents(
            Point3::new(0.0, 0.0, 2.5),
            Vector3::new(10.0, 10.0, 5.0),
        );
        let b = primitives::box_from_center_extents(
            Point3::new(7.5, 0.0, 6.25),
            Vector3::new(5.0, 10.0, 7.5),
        );
        let l = boolean(&a, &b, CsgOp::Union).unwrap();
        let v0 = l.volume_signed();
        let e = EdgeSpec {
            a: Point3::new(5.0, -5.0, 5.0),
            b: Point3::new(5.0, 5.0, 5.0),
        };
        let out = fillet_edges(&l, &[e], 2.0, 24).unwrap();
        let v1 = out.volume_signed();
        // A concave (valley) fillet adds the wedge-minus-quarter-disc —
        // the standard fillet-weld cross-section: r²(1 − π/4)·L.
        let want = (1.0 - std::f64::consts::PI / 4.0) * 2.0_f64.powi(2) * 10.0;
        assert!(v1 > v0, "concave fillet must add material");
        assert!(
            (v1 - v0 - want).abs() < 0.6,
            "delta {} vs fillet-wedge {}",
            v1 - v0,
            want
        );
    }

    #[test]
    fn fillet_chamfer_reject_bad_input() {
        let m = boxmesh(10.0, 10.0, 10.0);
        let e = top_front_edge(&m, 10.0, 10.0, 10.0);
        assert!(chamfer_edges(&m, &[e], 0.0).is_err());
        assert!(chamfer_edges(&m, &[e], -1.0).is_err());
        assert!(fillet_edges(&m, &[e], 0.0, 8).is_err());
        // Unknown edge → clean error, not a panic.
        let ghost = EdgeSpec {
            a: Point3::new(100.0, 0.0, 0.0),
            b: Point3::new(100.0, 1.0, 0.0),
        };
        let err = chamfer_edges(&m, &[ghost], 1.0).unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[test]
    fn multiple_edges_chain_sequentially() {
        let m = boxmesh(10.0, 10.0, 10.0);
        // Two opposite top edges.
        let e1 = EdgeSpec {
            a: Point3::new(-5.0, 5.0, 10.0),
            b: Point3::new(5.0, 5.0, 10.0),
        };
        let e2 = EdgeSpec {
            a: Point3::new(-5.0, -5.0, 10.0),
            b: Point3::new(5.0, -5.0, 10.0),
        };
        let out = chamfer_edges(&m, &[e1, e2], 1.5).unwrap();
        let v = out.volume_signed();
        let want = 1000.0 - 2.0 * 0.5 * 1.5 * 1.5 * 10.0;
        assert!((v - want).abs() < 1e-6, "volume {v} vs {want}");
    }

    #[test]
    fn fillet_reports_not_implemented() {
        let err = fillet(&TriMesh::default(), &[], 2.0).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("B-Rep"));
    }
}
