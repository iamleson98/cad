//! Sweep-based solid creation: extrude, revolve, loft and profile sweep.
//!
//! All operations take profiles expressed as 2D contours on a sketch plane
//! ([`Plane`]) and produce closed, outward-oriented [`TriMesh`] solids.
//!
//! The profile convention is shared with [`crate::triangulate`]:
//! outer contour CCW, holes CW.

use crate::error::{GeometryError, Result};
use crate::mesh::TriMesh;
use crate::triangulate::{
    ensure_ccw, ensure_cw, resample_closed, signed_area, triangulate_with_holes,
};
use forge_core::{Plane, Point2, Point3, TessellationConfig, Vector3};

/// A closed 2D profile: one outer contour plus optional holes.
/// Orientation is normalized on construction.
#[derive(Debug, Clone, Default)]
pub struct Profile2D {
    /// Outer contour, CCW.
    pub outer: Vec<Point2>,
    /// Hole contours, CW.
    pub holes: Vec<Vec<Point2>>,
}

impl Profile2D {
    /// Construct and normalize orientation. Returns `Err` on degenerate
    /// outer contours.
    pub fn new(mut outer: Vec<Point2>, mut holes: Vec<Vec<Point2>>) -> Result<Self> {
        crate::triangulate::dedup(&mut outer);
        if outer.len() < 3 {
            return Err(GeometryError::EmptyProfile);
        }
        ensure_ccw(&mut outer);
        for h in &mut holes {
            crate::triangulate::dedup(h);
            ensure_cw(h);
        }
        holes.retain(|h| h.len() >= 3);
        Ok(Self { outer, holes })
    }

    /// Net area enclosed by the profile (outer minus holes).
    pub fn area(&self) -> f64 {
        signed_area(&self.outer) + self.holes.iter().map(|h| signed_area(h)).sum::<f64>()
    }

    /// All contours (outer first, then holes).
    pub fn contours(&self) -> impl Iterator<Item = &Vec<Point2>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

/// Direction of an extrusion relative to the sketch plane normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExtrudeDirection {
    /// Along the plane normal.
    Positive,
    /// Against the plane normal.
    Negative,
    /// Symmetric on both sides (half distance each way).
    Symmetric,
}

/// Parameters of an extrusion.
#[derive(Debug, Clone, Copy)]
pub struct ExtrudeParams {
    /// Extrusion distance (mm, positive).
    pub distance: f64,
    /// Which side to extrude towards.
    pub direction: ExtrudeDirection,
    /// Draft (taper) angle in radians, 0 = straight walls (F-06). The
    /// far cap is scaled toward the profile centroid so the walls
    /// slope inward (positive angle) or outward (negative). The angle
    /// is exact at the profile's mean radius from its centroid and an
    /// approximation elsewhere (the classic scaled-section loft trick).
    pub draft_angle: f64,
}

impl Default for ExtrudeParams {
    fn default() -> Self {
        Self {
            distance: 10.0,
            direction: ExtrudeDirection::Positive,
            draft_angle: 0.0,
        }
    }
}

/// (u, v, n) frame of a plane.
fn frame(plane: &Plane) -> (Vector3, Vector3, Vector3) {
    let (u, v) = plane.basis();
    (u, v, *plane.normal.as_ref())
}

/// Point at plane-local `(x, y)` and height `z` above the plane.
fn plane_point(
    frame: (Vector3, Vector3, Vector3),
    origin: &Point3,
    local: Point2,
    z: f64,
) -> Point3 {
    let (u, v, n) = frame;
    *origin + u * local.x + v * local.y + n * z
}

/// Extrude `profile` (lying on `plane`) into a solid, optionally with a
/// draft (taper) angle (F-06).
pub fn extrude(profile: &Profile2D, plane: &Plane, params: &ExtrudeParams) -> Result<TriMesh> {
    if params.distance <= 0.0 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "extrude distance must be positive".into(),
        )));
    }
    if !params.draft_angle.is_finite() || params.draft_angle.abs() >= 80.0_f64.to_radians() {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "draft angle must be within ±80°".into(),
        )));
    }
    // Symmetric draft (F-06): two mirrored tapered halves sharing the
    // sketch-plane section (each half is a frustum, base = the unscaled
    // profile). A single mesh with both caps scaled would lose the waist
    // and collapse to a plain smaller prism.
    if params.direction == ExtrudeDirection::Symmetric && params.draft_angle != 0.0 {
        let half = params.distance * 0.5;
        let mut mesh = extrude(
            profile,
            plane,
            &ExtrudeParams {
                distance: half,
                direction: ExtrudeDirection::Positive,
                draft_angle: params.draft_angle,
            },
        )?;
        let lower = extrude(
            profile,
            plane,
            &ExtrudeParams {
                distance: half,
                direction: ExtrudeDirection::Negative,
                draft_angle: params.draft_angle,
            },
        )?;
        mesh.merge(&lower);
        mesh.compute_vertex_normals();
        return Ok(mesh);
    }
    let (z0, z1) = match params.direction {
        ExtrudeDirection::Positive => (0.0, params.distance),
        ExtrudeDirection::Negative => (-params.distance, 0.0),
        ExtrudeDirection::Symmetric => (-params.distance * 0.5, params.distance * 0.5),
    };

    let tri = triangulate_with_holes(&profile.outer, &profile.holes)?;
    let v = tri.vertices.len();

    // Draft (F-06): the far cap is scaled toward the section centroid.
    // Scaling is an affine map, so the *same* triangulation and contour
    // correspondence stay valid — the side walls become ruled surfaces.
    //   tan(draft) = r_mean · (1 − k) / wall_height  ⇒  k = 1 − tan·h/r
    // k is clamped so extreme angles degenerate to a point, never flip.
    // Symmetric extrusions taper each half, so the wall height is d/2.
    let wall_height = match params.direction {
        ExtrudeDirection::Symmetric => params.distance * 0.5,
        _ => params.distance,
    };
    let scale_top = |pts: &[Point2]| -> (Point2, f64) {
        let c = pts.iter().fold(Point2::origin(), |acc, p| acc + p.coords) / pts.len() as f64;
        let r_mean = pts.iter().map(|p| (*p - c).norm()).sum::<f64>() / pts.len() as f64;
        let k = if r_mean < 1e-9 || params.draft_angle == 0.0 {
            1.0
        } else {
            let k = 1.0 - params.draft_angle.tan() * wall_height / r_mean;
            k.clamp(0.02, 50.0)
        };
        (c, k)
    };
    // Which cap is "far" (tapered): Positive → z1, Negative → z0.
    // (Symmetric + draft is handled by the recursive split above; plain
    // symmetric keeps k = 1 anyway.)
    let (c, k) = scale_top(&tri.vertices);
    let taper_top = matches!(params.direction, ExtrudeDirection::Positive);
    let taper_bottom = matches!(params.direction, ExtrudeDirection::Negative);

    let f = frame(plane);
    let origin = plane.origin;

    // Vertex layout: [ upper (z1): 0..v ][ lower (z0): v..2v ].
    let mut mesh = TriMesh::with_capacity(2 * v, tri.triangles.len() * 2 + perimeter(profile) * 2);
    let scale_pt =
        |p: &Point2, c: &Point2, k: f64| -> Point2 { Point2::from(c.coords + (*p - c) * k) };
    for p in &tri.vertices {
        let p = if taper_top { scale_pt(p, &c, k) } else { *p };
        mesh.positions.push(plane_point(f, &origin, p, z1));
    }
    for p in &tri.vertices {
        let p = if taper_bottom { scale_pt(p, &c, k) } else { *p };
        mesh.positions.push(plane_point(f, &origin, p, z0));
    }

    // Upper cap: as triangulated (CCW seen from +n).
    for [a, b, c] in &tri.triangles {
        mesh.indices.extend_from_slice(&[*a, *b, *c]);
    }
    // Lower cap: reversed winding.
    for [a, b, c] in &tri.triangles {
        mesh.indices
            .extend_from_slice(&[a + v as u32, c + v as u32, b + v as u32]);
    }

    // Side walls from the original contours (outer CCW, holes CW – the
    // winding logic below produces outward normals for both).
    let contours: Vec<Vec<u32>> = std::iter::once(0..profile.outer.len() as u32)
        .chain(profile.holes.iter().scan(profile.outer.len(), |acc, h| {
            let base = *acc as u32;
            *acc += h.len();
            Some(base..base + h.len() as u32)
        }))
        .map(|r| r.collect())
        .collect();
    // NOTE: hole vertex ranges refer to positions in
    // `triangulate_with_holes`'s combined array = outer ++ holes in order,
    // which matches `tri.vertices` exactly.
    for contour in &contours {
        let n = contour.len();
        for i in 0..n {
            let g0 = contour[i];
            let g1 = contour[(i + 1) % n];
            // (a_bot, b_bot, b_top), (a_bot, b_top, a_top)
            mesh.indices
                .extend_from_slice(&[g0 + v as u32, g1 + v as u32, g1]);
            mesh.indices.extend_from_slice(&[g0 + v as u32, g1, g0]);
        }
    }

    mesh.compute_vertex_normals();
    Ok(mesh)
}

fn perimeter(profile: &Profile2D) -> usize {
    profile.contours().map(|c| c.len()).sum::<usize>().max(1)
}

/// Parameters of a revolution.
#[derive(Debug, Clone, Copy)]
pub struct RevolveParams {
    /// Revolution angle in radians (`0 < angle <= 2π`).
    pub angle: f64,
}

impl Default for RevolveParams {
    fn default() -> Self {
        Self {
            angle: std::f64::consts::TAU,
        }
    }
}

/// Rotate `p` around the axis `(origin, axis)` (unit direction) by `angle`
/// (Rodrigues' formula).
fn rotate_about_axis(p: Point3, origin: &Point3, axis: &Vector3, angle: f64) -> Point3 {
    let v = p - origin;
    let parallel = axis * v.dot(axis);
    let perp = v - parallel;
    let cross = axis.cross(&perp);
    *origin + parallel + perp * angle.cos() + cross * angle.sin()
}

/// Revolve a closed outer `contour` (2D, on `plane`) around an axis that
/// lies in the sketch plane, given as two local points.
///
/// The contour must stay on one side of the axis (touching is allowed).
pub fn revolve(
    contour: &[Point2],
    plane: &Plane,
    axis_p0: Point2,
    axis_p1: Point2,
    params: &RevolveParams,
    cfg: &TessellationConfig,
) -> Result<TriMesh> {
    let mut outer = contour.to_vec();
    crate::triangulate::dedup(&mut outer);
    if outer.len() < 3 {
        return Err(GeometryError::EmptyProfile);
    }
    ensure_ccw(&mut outer);

    let angle = params.angle;
    if !(0.0..=std::f64::consts::TAU + 1e-9).contains(&angle) || angle <= 1e-9 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "revolution angle must be in (0, 2π]".into(),
        )));
    }
    let full = angle >= std::f64::consts::TAU - 1e-9;

    // Axis in world space.
    let origin = plane.to_world(axis_p0);
    let axis_dir = {
        let d = plane.to_world(axis_p1) - plane.to_world(axis_p0);
        let len = d.norm();
        if len < 1e-12 {
            return Err(GeometryError::Core(forge_core::CoreError::Degenerate(
                "revolution axis".into(),
            )));
        }
        d / len
    };

    // Validate: all contour points on one side of the axis (in local 2D).
    let mut side = 0.0;
    for p in &outer {
        let d = *p - axis_p0;
        let a = axis_p1 - axis_p0;
        let cross = a.x * d.y - a.y * d.x;
        if cross.abs() > 1e-9 {
            let s = cross.signum();
            if side == 0.0 {
                side = s;
            } else if s != side {
                return Err(GeometryError::Core(forge_core::CoreError::Invalid(
                    "revolution profile crosses the axis".into(),
                )));
            }
        }
    }

    // World profile points and maximum radius (for step count).
    let world: Vec<Point3> = outer.iter().map(|p| plane.to_world(*p)).collect();
    let max_r = world
        .iter()
        .map(|p| {
            let v = p - origin;
            (v - axis_dir * v.dot(&axis_dir)).norm()
        })
        .fold(1.0, f64::max);

    let steps = if full {
        cfg.steps_for_arc(max_r, std::f64::consts::TAU)
    } else {
        cfg.steps_for_arc(max_r, angle).max(3)
    };
    let ring_count = if full { steps } else { steps + 1 };

    // rings[k][i] = contour point i rotated by angle k.
    let mut rings: Vec<Vec<Point3>> = Vec::with_capacity(ring_count);
    for k in 0..ring_count {
        let a = if full {
            std::f64::consts::TAU * k as f64 / steps as f64
        } else {
            angle * k as f64 / steps as f64
        };
        rings.push(
            world
                .iter()
                .map(|p| rotate_about_axis(*p, &origin, &axis_dir, a))
                .collect(),
        );
    }

    let n = outer.len();

    // One-time winding-sense decision. For a CCW profile, quads
    // (A,B,C),(A,C,D) face outward iff the ring step direction
    // (axis x radial) agrees with the sketch plane normal; otherwise the
    // winding is mirrored. A per-quad adaptive flip would desynchronize
    // shared edges and break watertightness, so the sense is decided once.
    let v0 = world[0] - origin;
    let ringstep = axis_dir.cross(&v0);
    let plane_n = *plane.normal.as_ref();
    let ccw_outward = ringstep.dot(&plane_n) > 0.0;

    let mut mesh = TriMesh::with_capacity(ring_count * n, ring_count * n * 2);
    for ring in &rings {
        mesh.positions.extend_from_slice(ring);
    }
    let at = |k: usize, i: usize| -> u32 {
        let kk = if full { k % steps } else { k };
        (kk * n + i) as u32
    };

    for k in 0..steps {
        for i in 0..n {
            let j = (i + 1) % n;
            let a = at(k, i);
            let b = at(k, j);
            let c = at(k + 1, j);
            let d = at(k + 1, i);
            if ccw_outward {
                mesh.indices.extend_from_slice(&[a, b, c]);
                mesh.indices.extend_from_slice(&[a, c, d]);
            } else {
                mesh.indices.extend_from_slice(&[a, c, b]);
                mesh.indices.extend_from_slice(&[a, d, c]);
            }
        }
    }

    // End caps for partial revolutions.
    if !full {
        cap_from_ring(&mut mesh, &rings[0], &outer, -1.0, &origin, &axis_dir);
        cap_from_ring(&mut mesh, &rings[steps], &outer, 1.0, &origin, &axis_dir);
    }

    mesh.weld(crate::mesh::WELD_EPS);
    mesh.remove_degenerate(1e-12);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

/// Add a planar cap covering `ring` (3D points of the profile at one end of
/// the revolution). `side` = -1 for the start cap, +1 for the end cap.
fn cap_from_ring(
    mesh: &mut TriMesh,
    ring: &[Point3],
    outer: &[Point2],
    side: f64,
    origin: &Point3,
    axis_dir: &Vector3,
) {
    // Triangulate the 2D contour and map to ring vertices (same order).
    let Ok(tri) = triangulate_with_holes(outer, &[]) else {
        return;
    };
    let cap_base = mesh.positions.len();
    mesh.positions.extend_from_slice(ring);

    // Expected outward direction of the cap: +/- (axis x radial).
    let sample = *ring.first().unwrap_or(origin);
    let radial = sample - origin;
    let radial = radial - axis_dir * radial.dot(axis_dir);
    let expected = axis_dir.cross(&radial) * side;

    for [a, b, c] in &tri.triangles {
        let pa = mesh.positions[cap_base + *a as usize];
        let pb = mesh.positions[cap_base + *b as usize];
        let pc = mesh.positions[cap_base + *c as usize];
        let normal = (pb - pa).cross(&(pc - pa));
        if normal.dot(&expected) >= 0.0 {
            mesh.indices.extend_from_slice(&[
                (cap_base + *a as usize) as u32,
                (cap_base + *b as usize) as u32,
                (cap_base + *c as usize) as u32,
            ]);
        } else {
            mesh.indices.extend_from_slice(&[
                (cap_base + *a as usize) as u32,
                (cap_base + *c as usize) as u32,
                (cap_base + *b as usize) as u32,
            ]);
        }
    }
}

/// Loft between two or more profiles on parallel or non-parallel planes.
/// Holes are ignored in v0.1 (see roadmap); outer contours are resampled to
/// a common vertex count.
pub fn loft(profiles: &[Profile2D], planes: &[Plane]) -> Result<TriMesh> {
    if profiles.len() < 2 || planes.len() != profiles.len() {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "loft requires at least two profiles with matching planes".into(),
        )));
    }
    let ring_len = 64usize.max(profiles.iter().map(|p| p.outer.len()).max().unwrap_or(16));

    // Resampled rings (CCW).
    let mut rings: Vec<Vec<Point2>> = Vec::with_capacity(profiles.len());
    for p in profiles {
        let mut outer = p.outer.clone();
        ensure_ccw(&mut outer);
        rings.push(resample_closed(&outer, ring_len));
    }

    let mut mesh = TriMesh::with_capacity(rings.len() * ring_len, rings.len() * ring_len * 4);
    let mut ring_offsets: Vec<usize> = Vec::with_capacity(rings.len());
    for (r, plane) in rings.iter().zip(planes) {
        ring_offsets.push(mesh.positions.len());
        for p in r {
            mesh.positions.push(plane.to_world(*p));
        }
    }

    // Side walls: CCW rings + consistent loft direction give outward
    // winding with the fixed pattern below.
    for r in 0..rings.len() - 1 {
        let off_a = ring_offsets[r];
        let off_b = ring_offsets[r + 1];
        for i in 0..ring_len {
            let j = (i + 1) % ring_len;
            let a = (off_a + i) as u32;
            let b = (off_a + j) as u32;
            let c = (off_b + j) as u32;
            let d = (off_b + i) as u32;
            mesh.indices.extend_from_slice(&[a, b, c]);
            mesh.indices.extend_from_slice(&[a, c, d]);
        }
    }

    // End caps.
    let loft_dir = {
        let a: Vector3 = mesh.positions[ring_offsets[0]..ring_offsets[0] + ring_len]
            .iter()
            .map(|p| p.coords)
            .sum();
        let b: Vector3 = mesh.positions
            [ring_offsets[rings.len() - 1]..ring_offsets[rings.len() - 1] + ring_len]
            .iter()
            .map(|p| p.coords)
            .sum();
        (b - a) / ring_len as f64
    };
    cap_resampled(
        &mut mesh,
        &rings[0],
        &planes[0],
        ring_offsets[0],
        true,
        loft_dir,
    );
    let last = rings.len() - 1;
    cap_resampled(
        &mut mesh,
        &rings[last],
        &planes[last],
        ring_offsets[last],
        false,
        loft_dir,
    );

    mesh.weld(crate::mesh::WELD_EPS);
    mesh.remove_degenerate(1e-12);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

/// Cap a resampled ring (used by loft). `is_start` selects the first/last
/// ring; the cap faces away from the loft direction.
fn cap_resampled(
    mesh: &mut TriMesh,
    ring: &[Point2],
    plane: &Plane,
    offset: usize,
    is_start: bool,
    loft_dir: Vector3,
) {
    let Ok(tri) = triangulate_with_holes(ring, &[]) else {
        return;
    };
    let expected = if is_start { -loft_dir } else { loft_dir };
    for [a, b, c] in &tri.triangles {
        let (ia, ib, ic) = (*a as usize, *b as usize, *c as usize);
        let pa = mesh.positions[offset + ia];
        let pb = mesh.positions[offset + ib];
        let pc = mesh.positions[offset + ic];
        let tri_n = (pb - pa).cross(&(pc - pa));
        let flip = tri_n.dot(&expected) < 0.0;
        if !flip {
            mesh.indices.extend_from_slice(&[
                (offset + ia) as u32,
                (offset + ib) as u32,
                (offset + ic) as u32,
            ]);
        } else {
            mesh.indices.extend_from_slice(&[
                (offset + ia) as u32,
                (offset + ic) as u32,
                (offset + ib) as u32,
            ]);
        }
    }
    let _ = plane;
}

/// Sweep a closed outer profile along a 3D polyline path with
/// parallel-transport frames. The profile plane's normal must align with
/// the first path tangent.
pub fn sweep_along_path(
    profile: &[Point2],
    profile_plane: &Plane,
    path: &[Point3],
) -> Result<TriMesh> {
    if path.len() < 2 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "sweep path needs at least two points".into(),
        )));
    }
    let mut outer = profile.to_vec();
    crate::triangulate::dedup(&mut outer);
    if outer.len() < 3 {
        return Err(GeometryError::EmptyProfile);
    }
    ensure_ccw(&mut outer);

    // Tangents per station: average of adjacent segment directions.
    let segs: Vec<Vector3> = path
        .windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            let len = d.norm();
            if len < 1e-12 {
                Vector3::z()
            } else {
                d / len
            }
        })
        .collect();
    let mut tangents: Vec<Vector3> = Vec::with_capacity(path.len());
    tangents.push(segs[0]);
    for k in 1..path.len() - 1 {
        let t = segs[k - 1] + segs[k];
        let len = t.norm();
        tangents.push(if len > 1e-12 { t / len } else { segs[k] });
    }
    tangents.push(*segs.last().unwrap());

    // Validate the profile plane normal aligns with the start tangent.
    let mut normal = *profile_plane.normal.as_ref();
    if normal.dot(&tangents[0]) < 0.0 {
        normal = -normal; // allow flipped planes
    }
    if normal.dot(&tangents[0]) < 0.99 {
        return Err(GeometryError::Core(forge_core::CoreError::Invalid(
            "profile plane must be perpendicular to the sweep path at its start".into(),
        )));
    }

    // Parallel transport of (u, v) along the path.
    let (mut u, mut _v) = profile_plane.basis();
    let n = profile.len();
    let mut mesh = TriMesh::with_capacity(path.len() * n, path.len() * n * 4);

    for (k, station) in path.iter().enumerate() {
        if k > 0 {
            // Rotate (u, v) from tangents[k-1] to tangents[k].
            let t0 = tangents[k - 1];
            let t1 = tangents[k];
            let axis = t0.cross(&t1);
            let s = axis.norm();
            if s > 1e-9 {
                let axis = axis / s;
                let ang = t0.dot(&t1).clamp(-1.0, 1.0).acos();
                u = rotate_about_axis(*station + u, station, &axis, ang) - *station;
            } else if t0.dot(&t1) < 0.0 {
                // 180° turn: rotate u around the tangent.
                u = t1.cross(&u) + t1 * u.dot(&t1);
            }
            u = u - tangents[k] * u.dot(&tangents[k]);
            let ul = u.norm();
            u = if ul > 1e-12 { u / ul } else { Vector3::x() };
        }
        let v_axis = tangents[k].cross(&u);
        for p in &outer {
            mesh.positions.push(*station + u * p.x + v_axis * p.y);
        }
    }

    // Side walls: CCW profile + parallel-transported frames give outward
    // winding with the fixed pattern below.
    for k in 0..path.len() - 1 {
        for i in 0..n {
            let j = (i + 1) % n;
            let a = (k * n + i) as u32;
            let b = (k * n + j) as u32;
            let c = ((k + 1) * n + j) as u32;
            let d = ((k + 1) * n + i) as u32;
            mesh.indices.extend_from_slice(&[a, b, c]);
            mesh.indices.extend_from_slice(&[a, c, d]);
        }
    }

    // End caps.
    cap_sweep(&mut mesh, &outer, 0, n, true);
    cap_sweep(&mut mesh, &outer, path.len() - 1, n, false);

    mesh.weld(crate::mesh::WELD_EPS);
    mesh.remove_degenerate(1e-12);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

/// Cap for a sweep ring (triangulated profile, winding flipped for start).
fn cap_sweep(mesh: &mut TriMesh, outer: &[Point2], ring_index: usize, n: usize, is_start: bool) {
    let Ok(tri) = triangulate_with_holes(outer, &[]) else {
        return;
    };
    let offset = ring_index * n;
    for [a, b, c] in &tri.triangles {
        let (ia, ib, ic) = (*a as usize, *b as usize, *c as usize);
        let (pa, pb, pc) = (
            mesh.positions[offset + ia],
            mesh.positions[offset + ib],
            mesh.positions[offset + ic],
        );
        let normal = (pb - pa).cross(&(pc - pa));
        // The cap faces away from the body: `other` is the neighboring
        // ring, so outward = pa - other for both caps.
        let other = if is_start {
            mesh.positions[(ring_index + 1) * n + ia]
        } else {
            mesh.positions[(ring_index - 1) * n + ia]
        };
        let expected = pa - other;
        let flip = normal.dot(&expected) < 0.0;
        if !flip {
            mesh.indices.extend_from_slice(&[
                (offset + ia) as u32,
                (offset + ib) as u32,
                (offset + ic) as u32,
            ]);
        } else {
            mesh.indices.extend_from_slice(&[
                (offset + ia) as u32,
                (offset + ic) as u32,
                (offset + ib) as u32,
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn square_profile(min: (f64, f64), max: (f64, f64)) -> Profile2D {
        Profile2D::new(
            vec![
                Point2::new(min.0, min.1),
                Point2::new(max.0, min.1),
                Point2::new(max.0, max.1),
                Point2::new(min.0, max.1),
            ],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn extrude_box_volume() {
        let profile = square_profile((0.0, 0.0), (10.0, 20.0));
        let mesh = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: 5.0,
                direction: ExtrudeDirection::Positive,
                draft_angle: 0.0,
            },
        )
        .unwrap();
        assert!(mesh.is_closed());
        assert_abs_diff_eq!(mesh.volume().unwrap(), 1000.0, epsilon = 1e-6);
    }

    // ---- F-06: draft / taper ----------------------------------------------

    #[test]
    fn tapered_extrude_frustum_volume() {
        // Square 10x10, height 10, positive draft: the top cap is scaled
        // toward the centroid by k (k derived from the mean vertex radius,
        // so for a square of side a, r_mean = a/√2 and the wall slope is
        // exact on the mid-edge points). Verify against the exact frustum
        // volume V = h/3 · (A1 + A2 + √(A1·A2)).
        let a = 10.0;
        let h = 10.0;
        let draft = 10.0_f64.to_radians();
        let r_mean = a / std::f64::consts::SQRT_2; // 4 corner vertices
        let k = 1.0 - draft.tan() * h / r_mean;
        let profile = square_profile((-a / 2.0, -a / 2.0), (a / 2.0, a / 2.0));
        let mesh = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: h,
                direction: ExtrudeDirection::Positive,
                draft_angle: draft,
            },
        )
        .unwrap();
        assert!(mesh.is_closed(), "tapered extrusion must stay watertight");
        let a1 = a * a;
        let a2 = (a * k) * (a * k);
        let want = h / 3.0 * (a1 + a2 + (a1 * a2).sqrt());
        assert_abs_diff_eq!(mesh.volume().unwrap(), want, epsilon = 1e-9);
        // The height is unchanged.
        let bb = mesh.bbox();
        assert_abs_diff_eq!(bb.max.z - bb.min.z, h, epsilon = 1e-9);
    }

    #[test]
    fn tapered_extrude_symmetric_tapers_both_ends() {
        // Symmetric: both caps scaled by the same k about the section —
        // the frustum is mirrored, so the volume equals two half-height
        // frusta. bbox xy extent at the caps < section extent.
        let a = 10.0;
        let h = 8.0;
        let draft = 5.0_f64.to_radians();
        let r_mean = a / std::f64::consts::SQRT_2;
        let k = 1.0 - draft.tan() * (h / 2.0) / r_mean;
        let profile = square_profile((-a / 2.0, -a / 2.0), (a / 2.0, a / 2.0));
        let mesh = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: h,
                direction: ExtrudeDirection::Symmetric,
                draft_angle: draft,
            },
        )
        .unwrap();
        assert!(mesh.is_closed());
        // Two mirrored frusta of height h/2: bases a² at the mid-plane,
        // tops (a·k)² at the caps.
        let half = h / 2.0;
        let a1 = a * a;
        let a2 = (a * k) * (a * k);
        let want = 2.0 * (half / 3.0 * (a1 + a2 + (a1 * a2).sqrt()));
        assert_abs_diff_eq!(mesh.volume().unwrap(), want, epsilon = 1e-9);
    }

    #[test]
    fn tapered_extrude_extreme_angle_clamps_not_flips() {
        // A 70° draft on a tall extrusion would flip the section; the
        // clamp keeps k ≥ 0.02 so the solid stays valid (non-inverted).
        let profile = square_profile((-5.0, -5.0), (5.0, 5.0));
        let mesh = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: 100.0,
                direction: ExtrudeDirection::Positive,
                draft_angle: 70.0_f64.to_radians(),
            },
        )
        .unwrap();
        assert!(mesh.is_closed());
        assert!(mesh.volume().unwrap() > 0.0, "never inverted");
        // Out of-range angles are rejected outright.
        assert!(extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: 10.0,
                direction: ExtrudeDirection::Positive,
                draft_angle: 85.0_f64.to_radians(),
            },
        )
        .is_err());
    }

    #[test]
    fn extrude_symmetric_same_volume() {
        let profile = square_profile((-5.0, -5.0), (5.0, 5.0));
        let mesh = extrude(
            &profile,
            &Plane::default(),
            &ExtrudeParams {
                distance: 4.0,
                direction: ExtrudeDirection::Symmetric,
                draft_angle: 0.0,
            },
        )
        .unwrap();
        assert!(mesh.is_closed());
        assert_abs_diff_eq!(mesh.volume().unwrap(), 100.0 * 4.0, epsilon = 1e-6);
    }

    #[test]
    fn extrude_with_circular_hole() {
        let hole = crate::triangulate::circle_points(Point2::new(5.0, 10.0), 2.0, 48);
        let profile = Profile2D::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 20.0),
                Point2::new(0.0, 20.0),
            ],
            vec![hole],
        )
        .unwrap();
        let mesh = extrude(&profile, &Plane::default(), &ExtrudeParams::default()).unwrap();
        assert!(mesh.is_closed());
        // The hole is tessellated to a 48-gon.
        let hole_area = 0.5 * 48.0 * 4.0 * (std::f64::consts::TAU / 48.0).sin();
        let expected = (200.0 - hole_area) * 10.0;
        assert_abs_diff_eq!(mesh.volume().unwrap(), expected, epsilon = 1e-6);
    }

    #[test]
    fn revolve_full_ring() {
        // Rectangle (r in 5..10, z in 0..4) revolved around the local y axis
        // (x = radius). Profile plane: XY at origin; axis = the y axis.
        let contour = vec![
            Point2::new(5.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 4.0),
            Point2::new(5.0, 4.0),
        ];
        let params = RevolveParams::default();
        let mesh = revolve(
            &contour,
            &Plane::default(),
            Point2::origin(),
            Point2::new(0.0, 1.0),
            &params,
            &TessellationConfig::EXPORT,
        )
        .unwrap();
        assert!(mesh.is_closed());
        let expected = std::f64::consts::PI * (100.0 - 25.0) * 4.0;
        assert!(
            (mesh.volume().unwrap() - expected).abs() / expected < 0.01,
            "revolve volume"
        );
    }

    #[test]
    fn revolve_half_ring() {
        let contour = vec![
            Point2::new(5.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 4.0),
            Point2::new(5.0, 4.0),
        ];
        let params = RevolveParams {
            angle: std::f64::consts::PI,
        };
        let mesh = revolve(
            &contour,
            &Plane::default(),
            Point2::origin(),
            Point2::new(0.0, 1.0),
            &params,
            &TessellationConfig::EXPORT,
        )
        .unwrap();
        assert!(mesh.is_closed());
        let expected = std::f64::consts::PI * (100.0 - 25.0) * 4.0 * 0.5;
        assert!(
            (mesh.volume().unwrap() - expected).abs() / expected < 0.02,
            "half revolve volume {} vs {}",
            mesh.volume().unwrap(),
            expected
        );
    }

    #[test]
    fn loft_frustum_volume() {
        let p0 = Profile2D::new(
            crate::triangulate::circle_points(Point2::origin(), 5.0, 48),
            vec![],
        )
        .unwrap();
        let p1 = Profile2D::new(
            crate::triangulate::circle_points(Point2::origin(), 10.0, 48),
            vec![],
        )
        .unwrap();
        let plane0 = Plane::default();
        let plane1 = Plane::new(Point3::new(0.0, 0.0, 10.0), Vector3::z()).unwrap();
        let mesh = loft(&[p0, p1], &[plane0, plane1]).unwrap();
        assert!(mesh.is_closed());
        let expected = std::f64::consts::PI * 10.0 / 3.0 * (25.0 + 50.0 + 100.0);
        assert!(
            (mesh.volume().unwrap() - expected).abs() / expected < 0.05,
            "loft volume {} vs {}",
            mesh.volume().unwrap(),
            expected
        );
    }

    #[test]
    fn sweep_straight_prism() {
        let profile = square_profile((-2.0, -2.0), (2.0, 2.0));
        // Profile plane normal +z, path along +z.
        let path = vec![Point3::origin(), Point3::new(0.0, 0.0, 10.0)];
        let mesh = sweep_along_path(&profile.outer, &Plane::default(), &path).unwrap();
        assert!(mesh.is_closed());
        assert_abs_diff_eq!(mesh.volume().unwrap(), 16.0 * 10.0, epsilon = 1e-6);
    }
}
