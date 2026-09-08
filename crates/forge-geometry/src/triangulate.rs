//! 2D contour utilities and constrained polygon triangulation.
//!
//! The triangulator handles an outer contour plus interior holes (e.g. a
//! plate with circular cutouts) using the classic two-step approach:
//!
//! 1. **Hole bridging** – each hole is spliced into the outer contour at a
//!    mutually-visible vertex pair, producing one simple polygon.
//!    The splice is *verified*: the resulting polygon is rejected unless it
//!    stays simple (non-self-intersecting); a fallback search over all
//!    bridge candidates runs when the nearest candidate fails.
//! 2. **Ear clipping** – O(n²) fan-ear removal with epsilon guards for the
//!    degenerate triangles introduced by bridging.
//!
//! Orientation convention (right-handed, y-up):
//! - outer contours are counter-clockwise (positive signed area),
//! - holes are clockwise,
//! - triangle winding in the result is counter-clockwise.

use crate::error::{GeometryError, Result};
use forge_core::Point2;

/// Intersection epsilon for bridge/ear tests.
const EPS: f64 = 1e-10;
/// Vertices closer than this are treated as identical.
const DUP_EPS: f64 = 1e-9;

/// Signed area of a closed contour (positive = counter-clockwise).
pub fn signed_area(pts: &[Point2]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }
    a * 0.5
}

/// Reverse the contour in place if needed so that it becomes
/// counter-clockwise.
pub fn ensure_ccw(pts: &mut [Point2]) {
    if signed_area(pts) < 0.0 {
        pts.reverse();
    }
}

/// Reverse the contour in place if needed so that it becomes clockwise.
pub fn ensure_cw(pts: &mut [Point2]) {
    if signed_area(pts) > 0.0 {
        pts.reverse();
    }
}

/// Remove consecutive duplicate points (and a duplicated closing point).
pub fn dedup(pts: &mut Vec<Point2>) {
    let mut out: Vec<Point2> = Vec::with_capacity(pts.len());
    for p in pts.iter().copied() {
        match out.last() {
            Some(last) if (last - p).norm() < DUP_EPS => {}
            _ => out.push(p),
        }
    }
    while out.len() > 1 && (out[0] - *out.last().unwrap()).norm() < DUP_EPS {
        out.pop();
    }
    *pts = out;
}

/// Points along a circular arc from `start_angle` to `end_angle`
/// (radians, CCW), `segments` intervals, endpoints included.
/// If `end < start` the arc is traversed clockwise.
pub fn arc_points(
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    segments: usize,
) -> Vec<Point2> {
    let n = segments.max(1);
    let mut out = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let t = k as f64 / n as f64;
        let ang = start_angle + (end_angle - start_angle) * t;
        out.push(Point2::new(
            center.x + radius * ang.cos(),
            center.y + radius * ang.sin(),
        ));
    }
    out
}

/// Points along a full CCW circle.
pub fn circle_points(center: Point2, radius: f64, segments: usize) -> Vec<Point2> {
    let mut pts = arc_points(center, radius, 0.0, std::f64::consts::TAU, segments.max(3));
    pts.pop(); // drop duplicated closing point
    pts
}

/// Arc-length uniform resampling of a closed contour to exactly `n` points.
/// Used by loft to obtain matching rings.
pub fn resample_closed(pts: &[Point2], n: usize) -> Vec<Point2> {
    if pts.is_empty() || n == 0 {
        return Vec::new();
    }
    if pts.len() == 1 {
        return vec![pts[0]; n];
    }
    // Closed perimeter: segment i goes from pts[i] to pts[(i+1)%len].
    let m = pts.len();
    let mut cum = Vec::with_capacity(m + 1);
    cum.push(0.0);
    for i in 0..m {
        cum.push(cum[i] + (pts[(i + 1) % m] - pts[i]).norm());
    }
    let total = cum[m];
    if total < 1e-12 {
        return vec![pts[0]; n];
    }
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let target = total * (k as f64) / (n as f64);
        // Binary search the segment containing `target`.
        let seg = match cum.binary_search_by(|v| v.partial_cmp(&target).unwrap()) {
            Ok(s) => s.min(m - 1),
            Err(s) => (s - 1).min(m - 1),
        };
        let seg_len = cum[seg + 1] - cum[seg];
        let t = if seg_len > 1e-12 {
            ((target - cum[seg]) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let a = pts[seg];
        let b = pts[(seg + 1) % m];
        out.push(Point2::new(
            forge_core::lerp(a.x, b.x, t),
            forge_core::lerp(a.y, b.y, t),
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Polygon triangulation
// ---------------------------------------------------------------------------

/// Result of triangulating a profile: vertex array (outer followed by
/// holes) plus CCW triangles indexing into it.
#[derive(Debug, Clone)]
pub struct Triangulation {
    /// Combined vertices: outer contour first, then each hole's vertices.
    pub vertices: Vec<Point2>,
    /// CCW triangles as index triples into [`Self::vertices`].
    pub triangles: Vec<[u32; 3]>,
}

impl Triangulation {
    /// Total triangulated area (should match `|outer| − Σ|holes|`).
    pub fn area(&self) -> f64 {
        self.triangles
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (
                    self.vertices[*a as usize],
                    self.vertices[*b as usize],
                    self.vertices[*c as usize],
                );
                0.5 * ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y))
            })
            .sum()
    }
}

/// Triangulate an outer contour (any orientation) with interior holes
/// (any orientation) into CCW triangles.
pub fn triangulate_with_holes(outer: &[Point2], holes: &[Vec<Point2>]) -> Result<Triangulation> {
    // 1. Combined vertex array and orientation normalization.
    let mut vertices: Vec<Point2> = outer.to_vec();
    dedup(&mut vertices);
    if vertices.len() < 3 {
        return Err(GeometryError::EmptyProfile);
    }
    ensure_ccw(&mut vertices);

    let mut hole_rings: Vec<Vec<Point2>> = Vec::with_capacity(holes.len());
    for h in holes {
        let mut ring = h.clone();
        dedup(&mut ring);
        if ring.len() < 3 {
            continue; // ignore degenerate holes
        }
        ensure_cw(&mut ring);
        hole_rings.push(ring);
    }

    // 2. Build the index-based polygon: outer indices then splice holes.
    let mut poly: Vec<u32> = (0..vertices.len() as u32).collect();
    let mut hole_index_ranges: Vec<(usize, usize)> = Vec::new();
    for ring in &hole_rings {
        let base = vertices.len();
        vertices.extend_from_slice(ring);
        hole_index_ranges.push((base, vertices.len()));
    }

    // Splice each hole into the polygon (process left-to-right by the
    // hole's leftmost vertex for stability).
    let mut order: Vec<usize> = (0..hole_rings.len()).collect();
    order.sort_by_key(|&i| {
        let (base, end) = hole_index_ranges[i];
        vertices[base..end]
            .iter()
            .map(|p| p.x)
            .fold(f64::INFINITY, f64::min)
            .to_bits() // sortable key
    });

    for hi in order {
        let (base, end) = hole_index_ranges[hi];
        splice_hole(&mut poly, &vertices, base, end)?;
    }

    // 3. Ear-clip the combined simple polygon.
    let triangles = ear_clip(&poly, &vertices)?;

    Ok(Triangulation {
        vertices,
        triangles,
    })
}

/// Splice the hole `vertices[base..end]` (already CW) into `poly`.
///
/// Strategy: take the hole's leftmost vertex `m`, cast a ray towards −x,
/// find the nearest polygon edge crossing it, and bridge to one of that
/// edge's endpoints. Every candidate splice is *validated* for simplicity
/// of the resulting polygon; if the nearest candidates fail, all polygon
/// vertices are tried.
fn splice_hole(poly: &mut Vec<u32>, vertices: &[Point2], base: usize, end: usize) -> Result<()> {
    let hole = &vertices[base..end];
    let m_global = base
        + hole
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.x, a.1.y).partial_cmp(&(b.1.x, b.1.y)).expect("no NaN"))
            .map(|(i, _)| i)
            .expect("non-empty hole");
    let m = vertices[m_global];

    // Candidate bridge vertices (as *positions* in the polygon ring):
    // endpoints of polygon edges crossing the horizontal ray from m
    // towards -x, nearest first.
    let mut candidates: Vec<usize> = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let ia = poly[i] as usize;
        let ib = poly[(i + 1) % n] as usize;
        let (a, b) = (vertices[ia], vertices[ib]);
        if (a.y <= m.y + EPS && b.y > m.y + EPS) || (b.y <= m.y + EPS && a.y > m.y + EPS) {
            let x = a.x + (m.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if x < m.x - EPS {
                // Prefer the endpoint with the smaller x (nearer the hole);
                // the other endpoint of the same edge is the fallback.
                candidates.push(if a.x <= b.x { i } else { (i + 1) % n });
                candidates.push(if a.x <= b.x { (i + 1) % n } else { i });
            }
        }
    }
    // Sort candidates by distance to m (nearest first).
    candidates.sort_by_key(|&pos| (vertices[poly[pos] as usize] - m).norm().to_bits());

    // Fallback pool: every polygon position, nearest first.
    if candidates.is_empty() {
        candidates = (0..n)
            .filter(|&i| (vertices[poly[i] as usize] - m).norm() > DUP_EPS)
            .collect();
        candidates.sort_by_key(|&i| (vertices[poly[i] as usize] - m).norm().to_bits());
    }

    for v in candidates {
        if let Some(combined) = try_splice(poly, vertices, base, end, m_global, v) {
            *poly = combined;
            return Ok(());
        }
    }
    Err(GeometryError::SelfIntersecting(format!(
        "no valid bridge found for hole with {} vertices",
        end - base
    )))
}

/// Attempt to splice the hole after polygon vertex index `v_idx`; returns
/// `true` when the resulting polygon is simple.
///
/// Splice layout: `[..., v, m, hole ring from m (CW), m, v, v.next, ...]`
/// – the detour leaves `v`, wraps the hole and *returns to `v`*, so the
/// original outer edge `v -> v.next` is preserved and the two bridge
/// segments traverse the same path in opposite directions (zero net
/// area). This is the classic bridging technique; the doubled bridge edge
/// is excluded from the simplicity check as an exact reversal pair.
fn try_splice(
    poly: &[u32],
    vertices: &[Point2],
    base: usize,
    end: usize,
    m_global: usize,
    v_pos: usize,
) -> Option<Vec<u32>> {
    let v = poly[v_pos];
    let m = vertices[m_global];

    // Sequence inserted after v: [m, hole vertices following m (CW, as
    // stored), ..., m, v].
    let hole_len = end - base;
    let m_local = m_global - base;
    let mut seq: Vec<u32> = Vec::with_capacity(hole_len + 3);
    seq.push(m_global as u32);
    for k in 1..hole_len {
        seq.push((base + (m_local + k) % hole_len) as u32);
    }
    seq.push(m_global as u32);
    seq.push(v);

    let mut combined: Vec<u32> = Vec::with_capacity(poly.len() + seq.len());
    combined.extend_from_slice(&poly[..=v_pos]);
    combined.extend_from_slice(&seq);
    combined.extend_from_slice(&poly[v_pos + 1..]);

    // The bridge segment (v, m) must not cut through the hole interior.
    let combined_pts: Vec<Point2> = combined.iter().map(|&i| vertices[i as usize]).collect();
    let mid1 = Point2::from((vertices[v as usize].coords + m.coords) * 0.5);
    let hole_pts: Vec<Point2> = (base..end).map(|i| vertices[i]).collect();
    if point_in_polygon(mid1, &hole_pts) {
        return None;
    }

    if is_simple_polygon(&combined_pts, DUP_EPS * 10.0) {
        Some(combined)
    } else {
        None
    }
}

/// Strict "point strictly inside convex-or-concave polygon" test (winding
/// number parity).
fn point_in_polygon(p: Point2, poly: &[Point2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// `true` when no two non-adjacent edges of the closed polygon intersect.
/// Exact reversal pairs (an edge traversed in both directions – the bridge
/// technique) are allowed.
fn is_simple_polygon(pts: &[Point2], eps: f64) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        for j in (i + 1)..n {
            // Adjacent edges share an endpoint; skip (also skip the
            // wrap-around pair).
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (c, d) = (pts[j], pts[(j + 1) % n]);
            // Exact reversal pair (doubled bridge edge): allowed.
            let same = |p: Point2, q: Point2, r: Point2, s: Point2| -> bool {
                (p - r).norm() < eps && (q - s).norm() < eps
            };
            if same(a, b, d, c) || same(a, b, c, d) {
                continue;
            }
            if segments_intersect(a, b, c, d, eps) {
                return false;
            }
        }
    }
    true
}

/// Proper segment intersection test (endpoints excluded with tolerance).
fn segments_intersect(a: Point2, b: Point2, c: Point2, d: Point2, eps: f64) -> bool {
    fn orient(o: Point2, p: Point2, q: Point2) -> f64 {
        (p.x - o.x) * (q.y - o.y) - (q.x - o.x) * (p.y - o.y)
    }
    let eps = eps.max(1e-12);
    let d1 = orient(c, d, a);
    let d2 = orient(c, d, b);
    let d3 = orient(a, b, c);
    let d4 = orient(a, b, d);
    if ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps))
        && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
    {
        return true;
    }
    // Collinear overlap: only flag if projections overlap substantially.
    if d1.abs() <= eps && d2.abs() <= eps && d3.abs() <= eps && d4.abs() <= eps {
        return true;
    }
    false
}

/// Ear clipping on an index-based polygon. Triangles with near-zero area
/// (introduced by bridging duplicates) are dropped.
fn ear_clip(poly: &[u32], vertices: &[Point2]) -> Result<Vec<[u32; 3]>> {
    let mut ring: Vec<u32> = poly.to_vec();
    let mut out = Vec::with_capacity(ring.len().max(3) - 2);

    loop {
        match ring.len() {
            0..=2 => break,
            3 => {
                let area = tri_area(
                    vertices[ring[0] as usize],
                    vertices[ring[1] as usize],
                    vertices[ring[2] as usize],
                );
                if area > EPS {
                    out.push([ring[0], ring[1], ring[2]]);
                }
                break;
            }
            _ => {}
        }

        let n = ring.len();
        let mut clipped = false;
        for i in 0..n {
            let ia = ring[(i + n - 1) % n] as usize;
            let ib = ring[i] as usize;
            let ic = ring[(i + 1) % n] as usize;
            let (a, b, c) = (vertices[ia], vertices[ib], vertices[ic]);

            // Convex corner?
            if tri_area(a, b, c) <= EPS {
                continue;
            }
            // Any other vertex inside the ear?
            let mut blocked = false;
            for &rv in ring.iter() {
                let idx = rv as usize;
                if idx == ia || idx == ib || idx == ic {
                    continue;
                }
                let p = vertices[idx];
                // Skip points lying (nearly) on the ear boundary: they do
                // not invalidate the ear but produce duplicate slivers.
                if point_strictly_in_triangle(p, a, b, c, EPS) {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                continue;
            }

            if tri_area(a, b, c) > DUP_EPS {
                out.push([ia as u32, ib as u32, ic as u32]);
            }
            ring.remove(i);
            clipped = true;
            break;
        }

        if !clipped {
            // Numerically stuck (should not happen for simple polygons).
            return Err(GeometryError::SelfIntersecting(format!(
                "ear clipping stalled on polygon with {} vertices",
                ring.len()
            )));
        }
    }
    Ok(out)
}

fn tri_area(a: Point2, b: Point2, c: Point2) -> f64 {
    0.5 * ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y))
}

/// Strict interior test with tolerance shrink.
fn point_strictly_in_triangle(p: Point2, a: Point2, b: Point2, c: Point2, eps: f64) -> bool {
    let d1 = tri_side(a, b, p);
    let d2 = tri_side(b, c, p);
    let d3 = tri_side(c, a, p);
    // Strictly inside = same sign, with margin.
    let m = eps * 10.0;
    (d1 > m && d2 > m && d3 > m) || (d1 < -m && d2 < -m && d3 < -m)
}

fn tri_side(a: Point2, b: Point2, p: Point2) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn square(min: (f64, f64), max: (f64, f64)) -> Vec<Point2> {
        vec![
            Point2::new(min.0, min.1),
            Point2::new(max.0, min.1),
            Point2::new(max.0, max.1),
            Point2::new(min.0, max.1),
        ]
    }

    #[test]
    fn triangulates_simple_square() {
        let tri = triangulate_with_holes(&square((-1.0, -1.0), (1.0, 1.0)), &[]).unwrap();
        assert!(!tri.triangles.is_empty());
        assert_abs_diff_eq!(tri.area(), 4.0, epsilon = 1e-9);
    }

    #[test]
    fn triangulates_square_with_circular_hole() {
        let hole = circle_points(Point2::new(0.0, 0.0), 0.5, 48);
        // The hole is tessellated to a 48-gon, so compare against the
        // polygonal area, not pi * r^2.
        let hole_area = signed_area(&hole).abs();
        let tri = triangulate_with_holes(&square((-1.0, -1.0), (1.0, 1.0)), &[hole]).unwrap();
        let expected = 4.0 - hole_area;
        assert!(
            (tri.area() - expected).abs() < 1e-9,
            "area {} vs expected {}",
            tri.area(),
            expected
        );
    }

    #[test]
    fn triangulates_two_holes() {
        let h1 = circle_points(Point2::new(-0.5, 0.0), 0.25, 24);
        let h2 = square((0.2, 0.2), (0.7, 0.7));
        let expected = 4.0 - signed_area(&h1).abs() - signed_area(&h2).abs();
        let tri = triangulate_with_holes(&square((-1.0, -1.0), (1.0, 1.0)), &[h1, h2]).unwrap();
        assert!(
            (tri.area() - expected).abs() < 2e-3,
            "area {} vs expected {}",
            tri.area(),
            expected
        );
    }

    #[test]
    fn resample_keeps_length() {
        let pts = circle_points(Point2::origin(), 1.0, 16);
        let res = resample_closed(&pts, 64);
        assert_eq!(res.len(), 64);
        let hull = signed_area(&res);
        assert_abs_diff_eq!(hull, std::f64::consts::PI, epsilon = 0.15);
    }
}
