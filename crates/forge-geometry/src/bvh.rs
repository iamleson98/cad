//! Bounding volume hierarchy for CPU raycasting (picking fallback and
//! geometric queries).
//!
//! The hierarchy is a flat array of nodes built by recursive median splits
//! along the longest bounding-box axis. One triangle per leaf keeps the
//! traversal code simple; query performance is dominated by the memory
//! layout rather than leaf occupancy at CAD mesh sizes.

use forge_core::{ray_triangle, BBox3, Point3, Ray3, Vector3};

/// Flat BVH over the triangles of one mesh.
#[derive(Debug, Clone)]
pub struct Bvh {
    nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
struct Node {
    bbox: BBox3,
    /// Index into `nodes` of the left child; `NONE` for leaves.
    left: u32,
    /// Index into `nodes` of the right child; unused for leaves.
    right: u32,
    /// Triangle index for leaves; `NONE` for internal nodes.
    tri: u32,
}

const NONE: u32 = u32::MAX;

/// Closest-hit result of a raycast.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    /// Triangle index hit.
    pub triangle: u32,
    /// Distance along the ray.
    pub t: f64,
    /// Barycentric coordinates of the hit point.
    pub u: f64,
    pub v: f64,
}

impl Bvh {
    /// Build a BVH for `tri_count` triangles given per-triangle bounding
    /// boxes and centroids.
    pub fn from_tri_boxes(tri_boxes: Vec<BBox3>, centroids: Vec<Point3>, tri_count: usize) -> Self {
        debug_assert_eq!(tri_boxes.len(), tri_count);
        debug_assert_eq!(centroids.len(), tri_count);
        let mut tris: Vec<u32> = (0..tri_count as u32).collect();
        let mut nodes = Vec::with_capacity(tri_count * 2);
        build_node(&mut nodes, &mut tris, &tri_boxes, &centroids);
        Self { nodes }
    }

    /// Build a BVH for a triangle soup (positions + flat indices).
    pub fn from_mesh_positions(positions: &[Point3], indices: &[u32]) -> Self {
        let tri_count = indices.len() / 3;
        let mut boxes = Vec::with_capacity(tri_count);
        let mut centroids = Vec::with_capacity(tri_count);
        for t in 0..tri_count {
            let a = positions[indices[t * 3] as usize];
            let b = positions[indices[t * 3 + 1] as usize];
            let c = positions[indices[t * 3 + 2] as usize];
            boxes.push(BBox3::from_points([a, b, c]));
            centroids.push(Point3::from((a.coords + b.coords + c.coords) / 3.0));
        }
        Self::from_tri_boxes(boxes, centroids, tri_count)
    }

    /// Closest hit of `ray` against the triangle soup (Möller–Trumbore).
    pub fn ray_cast(
        &self,
        positions: &[Point3],
        indices: &[u32],
        ray: &Ray3,
        max_t: f64,
    ) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        if self.nodes.is_empty() {
            return None;
        }
        stack.push(0);
        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni as usize];
            // Slab test with early exit relative to current best.
            if let Some(t_box) = node.bbox.ray_cast(ray) {
                if t_box > max_t {
                    continue;
                }
                if let Some(hit) = &best {
                    // The box must at least overlap the current best hit
                    // distance along the ray.
                    if t_box > hit.t {
                        continue;
                    }
                }
            } else {
                continue;
            }

            if node.tri != NONE {
                let k = node.tri as usize * 3;
                let a = positions[indices[k] as usize];
                let b = positions[indices[k + 1] as usize];
                let c = positions[indices[k + 2] as usize];
                if let Some((t, u, v)) = ray_triangle(ray, &a, &b, &c) {
                    if t <= max_t && best.as_ref().map(|h| t < h.t).unwrap_or(true) {
                        best = Some(RayHit {
                            triangle: node.tri,
                            t,
                            u,
                            v,
                        });
                    }
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        best
    }
}

fn build_node(
    nodes: &mut Vec<Node>,
    tris: &mut Vec<u32>,
    boxes: &[BBox3],
    centroids: &[Point3],
) -> u32 {
    let node_index = nodes.len() as u32;
    nodes.push(Node {
        bbox: BBox3::default(),
        left: NONE,
        right: NONE,
        tri: NONE,
    });

    // Node bounds.
    let mut bbox = BBox3::default();
    for &t in tris.iter() {
        bbox = bbox.union(&boxes[t as usize]);
    }

    if tris.len() <= 1 {
        let tri = tris.first().copied().unwrap_or(NONE);
        nodes[node_index as usize].bbox = bbox;
        nodes[node_index as usize].tri = tri;
        return node_index;
    }

    // Split along the longest axis of the centroid bounds.
    let mut cb = BBox3::default();
    for &t in tris.iter() {
        cb.extend(&centroids[t as usize]);
    }
    let size = cb.size();
    let (axis, axis_len) = {
        let mut best = 0usize;
        let mut best_len = size[0];
        for i in 1..3 {
            if size[i] > best_len {
                best = i;
                best_len = size[i];
            }
        }
        (best, best_len)
    };
    if axis_len < 1e-12 {
        // Coincident centroids: fan out sequentially.
        let mid = tris.len() / 2;
        let right = tris.split_off(mid);
        let left = std::mem::take(tris);
        let l = build_node(nodes, &mut left.clone(), boxes, centroids);
        let r = build_node(nodes, &mut right.clone(), boxes, centroids);
        nodes[node_index as usize].bbox = bbox;
        nodes[node_index as usize].left = l;
        nodes[node_index as usize].right = r;
        return node_index;
    }

    // Median split by centroid coordinate.
    tris.sort_by(|&a, &b| {
        centroids[a as usize][axis]
            .partial_cmp(&centroids[b as usize][axis])
            .expect("no NaN centroids")
    });
    let mid = tris.len() / 2;
    let mut right = tris.split_off(mid);
    let mut left = std::mem::take(tris);

    let l = build_node(nodes, &mut left, boxes, centroids);
    let r = build_node(nodes, &mut right, boxes, centroids);
    nodes[node_index as usize].bbox = bbox;
    nodes[node_index as usize].left = l;
    nodes[node_index as usize].right = r;
    node_index
}

/// Convenience: triangle normal of a flat-indexed soup (used by callers to
/// identify the picked face).
pub fn triangle_normal(a: Point3, b: Point3, c: Point3) -> Vector3 {
    let n = (b - a).cross(&(c - a));
    let len = n.norm();
    if len > 1e-20 {
        n / len
    } else {
        Vector3::z()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;
    use forge_core::Vector3;

    #[test]
    fn raycast_hits_box_face() {
        let mesh =
            primitives::box_from_center_extents(Point3::origin(), Vector3::new(10.0, 10.0, 10.0));
        let bvh = Bvh::from_mesh_positions(&mesh.positions, &mesh.indices);
        let ray = Ray3::new(Point3::new(0.0, 0.0, 20.0), Vector3::new(0.0, 0.0, -1.0)).unwrap();
        let hit = bvh.ray_cast(&mesh.positions, &mesh.indices, &ray, 1000.0);
        let hit = hit.expect("ray must hit the box");
        assert!((hit.t - 15.0).abs() < 1e-9);
        let k = hit.triangle as usize * 3;
        let a = mesh.positions[mesh.indices[k] as usize];
        let b = mesh.positions[mesh.indices[k + 1] as usize];
        let c = mesh.positions[mesh.indices[k + 2] as usize];
        let n = triangle_normal(a, b, c);
        assert!(n.z > 0.99, "top face normal, got {n:?}");
    }

    #[test]
    fn raycast_miss() {
        let mesh =
            primitives::box_from_center_extents(Point3::origin(), Vector3::new(10.0, 10.0, 10.0));
        let bvh = Bvh::from_mesh_positions(&mesh.positions, &mesh.indices);
        let ray = Ray3::new(Point3::new(50.0, 50.0, 50.0), Vector3::new(0.0, 0.0, -1.0)).unwrap();
        assert!(bvh
            .ray_cast(&mesh.positions, &mesh.indices, &ray, 1000.0)
            .is_none());
    }
}
