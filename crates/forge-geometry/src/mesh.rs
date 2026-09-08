//! Triangle mesh: the interchange data structure between the geometry
//! kernel, the feature evaluator and the GPU renderer.
//!
//! The mesh is an indexed triangle set in **model units, `f64`**
//! (NFR-PREC-01). Per-vertex normals are optional and always recomputed
//! from topology rather than stored by producers.
//!
//! Face/edge/vertex identification (used by picking and selection) is
//! derived on demand:
//! - *faces* map to triangle ranges (one triangle group per body region in
//!   v0.1, refined to topological faces when the B-Rep kernel lands),
//! - *edges* are extracted by [`TriMesh::sharp_edges`] / boundary edges,
//! - *vertices* are mesh corner positions after welding.

use forge_core::{BBox3, Point3, Transform, Vector3};
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default weld epsilon (mm). Vertices closer than this are considered
/// identical, which removes the seam duplicates produced by CSG bridging
/// and cap stitching.
pub const WELD_EPS: f64 = 1e-7;

/// Indexed triangle mesh.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriMesh {
    /// Vertex positions.
    pub positions: Vec<Point3>,
    /// Flat triangle index list; length is always a multiple of 3.
    pub indices: Vec<u32>,
    /// Optional per-vertex normals (normalized, outward).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Vec<Vector3>>,
}

impl TriMesh {
    /// Create a mesh with pre-allocated capacity.
    pub fn with_capacity(vertices: usize, triangles: usize) -> Self {
        Self {
            positions: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(triangles * 3),
            normals: None,
        }
    }

    /// Number of triangles.
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Append a single triangle by position.
    pub fn push_triangle(&mut self, a: Point3, b: Point3, c: Point3) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
        self.normals = None;
    }

    /// Triangle vertex indices `[a, b, c]`.
    pub fn triangle_idx(&self, i: usize) -> [u32; 3] {
        let k = i * 3;
        [self.indices[k], self.indices[k + 1], self.indices[k + 2]]
    }

    /// Triangle corner positions `[a, b, c]`.
    pub fn triangle(&self, i: usize) -> [Point3; 3] {
        let [a, b, c] = self.triangle_idx(i);
        [
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        ]
    }

    /// Iterate over all triangles as position triples.
    pub fn triangles(&self) -> impl Iterator<Item = [Point3; 3]> + '_ {
        (0..self.tri_count()).map(move |i| self.triangle(i))
    }

    /// Geometric (unnormalized) normal of triangle `i`.
    pub fn triangle_normal_raw(&self, i: usize) -> Vector3 {
        let [a, b, c] = self.triangle(i);
        (b - a).cross(&(c - a))
    }

    /// Unit normal of triangle `i`, or `None` if degenerate.
    pub fn triangle_normal(&self, i: usize) -> Option<Vector3> {
        let n = self.triangle_normal_raw(i);
        let len = n.norm();
        if len < 1e-20 {
            None
        } else {
            Some(n / len)
        }
    }

    /// Angle-weighted per-vertex normals (corner angle of each incident
    /// triangle as weight), computed in parallel with `rayon`. This avoids
    /// fan-triangulation double counting and yields exact face-average
    /// normals on boxes. Degenerate vertices get `+Z`.
    pub fn compute_vertex_normals(&mut self) {
        let tri_count = self.tri_count();
        let mut acc = vec![Vector3::zeros(); self.positions.len()];

        // Parallel: triangle normals + corner angles.
        // (Per-triangle indices + weighted corner contributions.)
        // wasm32 (no threads): the same closure runs on a serial iterator.
        type TriAcc = ([u32; 3], Option<(Vector3, [f64; 3])>);
        let corner = |i: usize| {
            let idx = self.triangle_idx(i);
            let n = self.triangle_normal(i);
            let angles = n.map(|n| {
                let [a, b, c] = self.triangle(i);
                let ang = |p: Point3, q: Point3, r: Point3| -> f64 {
                    let u = q - p;
                    let v = r - p;
                    let denom = u.norm() * v.norm();
                    if denom < 1e-20 {
                        0.0
                    } else {
                        (u.dot(&v) / denom).clamp(-1.0, 1.0).acos()
                    }
                };
                (n, [ang(a, b, c), ang(b, c, a), ang(c, a, b)])
            });
            (idx, angles)
        };
        #[cfg(not(target_arch = "wasm32"))]
        let tri_data: Vec<TriAcc> = (0..tri_count).into_par_iter().map(corner).collect();
        #[cfg(target_arch = "wasm32")]
        let tri_data: Vec<TriAcc> = (0..tri_count).map(corner).collect();

        for ([a, b, c], data) in tri_data {
            if let Some((n, [wa, wb, wc])) = data {
                acc[a as usize] += n * wa;
                acc[b as usize] += n * wb;
                acc[c as usize] += n * wc;
            }
        }

        // Parallel: normalize.
        let normalize = |n: &mut Vector3| {
            let len = n.norm();
            if len > 1e-20 {
                *n /= len;
            } else {
                *n = Vector3::z();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        acc.par_iter_mut().for_each(normalize);
        #[cfg(target_arch = "wasm32")]
        acc.iter_mut().for_each(normalize);

        self.normals = Some(acc);
    }

    /// Return a reference to the normals, computing them if missing.
    pub fn ensure_normals(&mut self) -> &Vec<Vector3> {
        if self.normals.is_none() {
            self.compute_vertex_normals();
        }
        self.normals.as_ref().expect("normals just computed")
    }

    /// Bounding box of all vertex positions.
    pub fn bbox(&self) -> BBox3 {
        BBox3::from_points(self.positions.iter().copied())
    }

    /// Rigid-transformed copy.
    pub fn transformed(&self, t: &Transform) -> Self {
        Self {
            positions: self.positions.iter().map(|p| t * *p).collect(),
            indices: self.indices.clone(),
            normals: self
                .normals
                .as_ref()
                .map(|ns| ns.iter().map(|n| t * *n).collect()),
        }
    }

    /// Transform in place.
    pub fn apply_transform(&mut self, t: &Transform) {
        for p in &mut self.positions {
            *p = t * *p;
        }
        if let Some(ns) = &mut self.normals {
            for n in ns {
                *n = t * *n;
            }
        }
    }

    /// Mirror (reflect) a copy of the mesh across the plane through `point`
    /// with unit `normal`.
    ///
    /// A reflection has determinant −1, so the triangle winding order is
    /// reversed to keep the outward orientation (and therefore a positive
    /// signed volume). Normals are recomputed after the flip.
    pub fn mirrored(&self, point: &Point3, normal: &Vector3) -> Self {
        let n = normal.normalize();
        let reflect = |p: &Point3| -> Point3 {
            let d = p - point;
            *p - (d.dot(&n) * 2.0) * n
        };
        let mut out = Self {
            positions: self.positions.iter().map(reflect).collect(),
            indices: self.indices.clone(),
            normals: None,
        };
        // Handedness flip: reverse every triangle's winding.
        for t in 0..out.tri_count() {
            let k = t * 3;
            out.indices.swap(k + 1, k + 2);
        }
        out.compute_vertex_normals();
        out
    }

    /// Merge `other` into `self`, offsetting indices.
    pub fn merge(&mut self, other: &TriMesh) {
        let offset = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.indices
            .extend(other.indices.iter().map(|i| i + offset));
        self.normals = None;
    }

    /// `true` when every directed edge is used exactly once and has an
    /// opposite partner – i.e. the mesh is watertight and consistently
    /// oriented.
    pub fn is_closed(&self) -> bool {
        let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
        for t in 0..self.tri_count() {
            let [a, b, c] = self.triangle_idx(t);
            for e in [(a, b), (b, c), (c, a)] {
                *directed.entry(e).or_insert(0) += 1;
            }
        }
        directed.values().all(|c| *c == 1)
            && directed
                .keys()
                .all(|(a, b)| directed.contains_key(&(*b, *a)))
    }

    /// Signed volume via the divergence theorem. Returns `None` if the mesh
    /// is not closed. Requires consistent outward orientation (guaranteed
    /// by all producers in this crate).
    pub fn volume(&self) -> Option<f64> {
        if !self.is_closed() {
            return None;
        }
        Some(
            self.triangles()
                .map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords)) / 6.0)
                .sum(),
        )
    }

    /// Signed volume without the watertightness precondition. Exact for
    /// valid solids whose seams contain only collinear T-junctions (the
    /// BSP CSG output contract); used where [`Self::volume`] would refuse.
    pub fn volume_signed(&self) -> f64 {
        self.triangles()
            .map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords)) / 6.0)
            .sum()
    }

    /// Surface area.
    pub fn area(&self) -> f64 {
        self.triangles()
            .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() * 0.5)
            .sum()
    }

    /// Centroid of the vertex cloud (not the volumetric centroid; good
    /// enough for camera framing).
    pub fn centroid(&self) -> Point3 {
        if self.positions.is_empty() {
            return Point3::origin();
        }
        let sum: Vector3 = self.positions.iter().map(|p| p.coords).sum();
        Point3::from(sum / self.positions.len() as f64)
    }

    /// Boundary edges: directed edges without an opposite partner.
    pub fn boundary_edges(&self) -> Vec<[u32; 2]> {
        let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
        for t in 0..self.tri_count() {
            let [a, b, c] = self.triangle_idx(t);
            for e in [(a, b), (b, c), (c, a)] {
                *directed.entry(e).or_insert(0) += 1;
            }
        }
        let keys: Vec<(u32, u32)> = directed.keys().copied().collect();
        let mut out: Vec<[u32; 2]> = keys
            .into_iter()
            .filter(|(a, b)| !directed.contains_key(&(*b, *a)))
            .map(|(a, b)| [a, b])
            .collect();
        out.sort();
        out
    }

    /// Edges whose adjacent faces meet at more than `angle_tol` (radians)
    /// – "sharp" feature edges – plus boundary edges. Used for crisp edge
    /// line rendering.
    pub fn sharp_edges(&self, angle_tol: f64) -> Vec<[u32; 2]> {
        // Map undirected edge -> the two adjacent triangles (face normals).
        let edge_tris = self.edge_triangle_map();

        let mut out = Vec::with_capacity(edge_tris.len());
        for ((a, b), tris) in edge_tris {
            let include = match tris.as_slice() {
                [_] => true, // boundary edge
                [t0, t1] => {
                    let n0 = self.triangle_normal(*t0).unwrap_or(Vector3::z());
                    let n1 = self.triangle_normal(*t1).unwrap_or(Vector3::z());
                    n0.dot(&n1) < (angle_tol).cos()
                }
                _ => true, // non-manifold: draw it so it is visible
            };
            if include {
                out.push([a, b]);
            }
        }
        out.sort();
        out
    }

    /// Map undirected edge -> adjacent triangle indices (shared by the
    /// edge-query methods).
    fn edge_triangle_map(&self) -> HashMap<(u32, u32), Vec<usize>> {
        let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for t in 0..self.tri_count() {
            let [a, b, c] = self.triangle_idx(t);
            let key = |x: u32, y: u32| (x.min(y), x.max(y));
            edge_tris.entry(key(a, b)).or_default().push(t);
            edge_tris.entry(key(b, c)).or_default().push(t);
            edge_tris.entry(key(c, a)).or_default().push(t);
        }
        edge_tris
    }

    /// Coplanar face cluster (W-04): flood-fill from `seed` across shared
    /// edges while the neighbor normal stays within `normal_tol` of the
    /// *seed* normal (flat-patch semantics: the whole cluster is coplanar
    /// with the seed triangle). Sorted, deterministic.
    pub fn face_cluster(&self, seed: usize, normal_tol: f64) -> Vec<usize> {
        if seed >= self.tri_count() {
            return Vec::new();
        }
        let edge_tris = self.edge_triangle_map();
        let cos_tol = normal_tol.cos();
        let seed_normal = self.triangle_normal(seed).unwrap_or(Vector3::z());
        let mut visited = vec![false; self.tri_count()];
        visited[seed] = true;
        let mut stack = vec![seed];
        let mut cluster = Vec::new();
        while let Some(t) = stack.pop() {
            cluster.push(t);
            let [a, b, c] = self.triangle_idx(t);
            for (x, y) in [(a, b), (b, c), (c, a)] {
                let key = (x.min(y), x.max(y));
                if let Some(tris) = edge_tris.get(&key) {
                    for &u in tris {
                        if visited[u] {
                            continue;
                        }
                        let nu = self.triangle_normal(u).unwrap_or(Vector3::z());
                        if nu.dot(&seed_normal) >= cos_tol {
                            visited[u] = true;
                            stack.push(u);
                        }
                    }
                }
            }
        }
        cluster.sort_unstable();
        cluster
    }

    /// Boundary of a triangle cluster: edges used by exactly one cluster
    /// triangle (manifold clusters only). Sorted, deterministic.
    pub fn cluster_boundary_edges(&self, cluster: &[usize]) -> Vec<[u32; 2]> {
        let mut count: HashMap<(u32, u32), usize> = HashMap::new();
        for &t in cluster {
            let [a, b, c] = self.triangle_idx(t);
            for (x, y) in [(a, b), (b, c), (c, a)] {
                *count.entry((x.min(y), x.max(y))).or_insert(0) += 1;
            }
        }
        let mut out: Vec<[u32; 2]> = count
            .into_iter()
            .filter(|(_, n)| *n == 1)
            .map(|((a, b), _)| [a, b])
            .collect();
        out.sort();
        out
    }

    /// Sharp feature edges grouped into tangent chains (W-04): two sharp
    /// edges join a chain when they share an endpoint and their directions
    /// away from that endpoint stay within `tangent_tol` of collinear —
    /// the "fillet this run of edges" grouping. A cube yields 12 chains
    /// of one edge; a cylinder rim yields one closed loop. Chains are
    /// sorted (internally and between each other), deterministic.
    pub fn sharp_edge_chains(&self, angle_tol: f64, tangent_tol: f64) -> Vec<Vec<[u32; 2]>> {
        let edges = self.sharp_edges(angle_tol);
        let mut at: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            at.entry(e[0]).or_default().push(i);
            at.entry(e[1]).or_default().push(i);
        }
        // Unit direction of edge `e` pointing *away* from vertex `v`.
        let dir_from = |e: &[u32; 2], v: u32| -> Vector3 {
            let other = if e[0] == v { e[1] } else { e[0] };
            (self.positions[other as usize] - self.positions[v as usize])
                .try_normalize(1e-12)
                .unwrap_or_else(Vector3::zeros)
        };
        let cos_t = tangent_tol.cos();
        let mut visited = vec![false; edges.len()];
        let mut chains = Vec::new();
        for start in 0..edges.len() {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            let mut chain = vec![edges[start]];
            let mut queue = vec![start];
            while let Some(ei) = queue.pop() {
                for v in [edges[ei][0], edges[ei][1]] {
                    if let Some(neighbors) = at.get(&v) {
                        for &nj in neighbors {
                            if visited[nj] {
                                continue;
                            }
                            let d_cur = dir_from(&edges[ei], v);
                            let d_new = dir_from(&edges[nj], v);
                            // Collinear continuation: a chain (e.g. a full
                            // circle rim) passes *through* the shared
                            // vertex, so consecutive directions away from
                            // it are nearly antiparallel — hence |dot|.
                            if d_cur.dot(&d_new).abs() >= cos_t {
                                visited[nj] = true;
                                chain.push(edges[nj]);
                                queue.push(nj);
                            }
                        }
                    }
                }
            }
            chain.sort();
            chains.push(chain);
        }
        chains.sort();
        chains
    }

    /// Turn a list of index edges into world-space segments.
    pub fn edge_segments(&self, edges: &[[u32; 2]]) -> Vec<(Point3, Point3)> {
        edges
            .iter()
            .filter_map(|[a, b]| {
                let pa = self.positions.get(*a as usize)?;
                let pb = self.positions.get(*b as usize)?;
                Some((*pa, *pb))
            })
            .collect()
    }

    /// Weld vertices that are closer than `eps` and drop triangles that
    /// collapse as a result. Keeps the first position of every cluster.
    /// Normals are invalidated.
    pub fn weld(&mut self, eps: f64) {
        let inv = 1.0 / eps;
        let mut cluster: HashMap<[i64; 3], u32> = HashMap::with_capacity(self.positions.len());
        let mut remap = vec![0u32; self.positions.len()];
        let mut new_positions: Vec<Point3> = Vec::with_capacity(self.positions.len());

        for (i, p) in self.positions.iter().enumerate() {
            let key = [
                (p.x * inv).round() as i64,
                (p.y * inv).round() as i64,
                (p.z * inv).round() as i64,
            ];
            match cluster.get(&key) {
                Some(dst) => remap[i] = *dst,
                None => {
                    let dst = new_positions.len() as u32;
                    new_positions.push(*p);
                    cluster.insert(key, dst);
                    remap[i] = dst;
                }
            }
        }

        let mut new_indices = Vec::with_capacity(self.indices.len());
        let base = self.indices.len() - self.indices.len() % 3;
        let mut k = 0;
        while k < base {
            let a = remap[self.indices[k] as usize];
            let b = remap[self.indices[k + 1] as usize];
            let c = remap[self.indices[k + 2] as usize];
            if a != b && b != c && a != c {
                new_indices.extend_from_slice(&[a, b, c]);
            }
            k += 3;
        }

        self.positions = new_positions;
        self.indices = new_indices;
        self.normals = None;
    }

    /// Remove triangles whose area is below `eps_area`.
    pub fn remove_degenerate(&mut self, eps_area: f64) {
        let keep: Vec<bool> = (0..self.tri_count())
            .map(|i| self.triangle_normal_raw(i).norm() * 0.5 > eps_area)
            .collect();
        let mut new_indices = Vec::with_capacity(self.indices.len());
        for (t, keep_t) in keep.iter().enumerate() {
            if *keep_t {
                let k = t * 3;
                new_indices.extend_from_slice(&self.indices[k..k + 3]);
            }
        }
        self.indices = new_indices;
        self.normals = None;
    }

    /// Make triangle orientations consistent by BFS propagation across
    /// shared edges (I-01): whenever two triangles share an edge, they must
    /// traverse it in opposite directions. Triangles on the negative side
    /// of a shared edge are flipped. Disconnected components are repaired
    /// independently; non-manifold edges do not propagate.
    ///
    /// After propagation, the largest component's global orientation is
    /// corrected to *outward* by the sign of the signed volume, so an
    /// inside-out soup imports as a valid solid.
    ///
    /// Returns `true` if any triangle was flipped.
    pub fn repair_orientation(&mut self) -> bool {
        let tri_count = self.tri_count();
        if tri_count == 0 {
            return false;
        }

        // Undirected edge -> incident (triangle, direction) pairs.
        let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for t in 0..tri_count {
            let [a, b, c] = self.triangle_idx(t);
            let key = |x: u32, y: u32| (x.min(y), x.max(y));
            edge_tris.entry(key(a, b)).or_default().push(t);
            edge_tris.entry(key(b, c)).or_default().push(t);
            edge_tris.entry(key(c, a)).or_default().push(t);
        }

        let mut flipped = false;
        let mut visited = vec![false; tri_count];

        for seed in 0..tri_count {
            if visited[seed] {
                continue;
            }
            // BFS the connected component, flipping neighbors that disagree.
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(seed);
            visited[seed] = true;
            while let Some(t) = queue.pop_front() {
                let [a, b, c] = self.triangle_idx(t);
                // Each directed edge (a, b) of triangle t: the neighbor must
                // use (b, a).
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    let key = (x.min(y), x.max(y));
                    let Some(tris) = edge_tris.get(&key) else {
                        continue;
                    };
                    // Only propagate through manifold edges (exactly 2
                    // incident triangles).
                    if tris.len() != 2 {
                        continue;
                    }
                    let other = tris[0] ^ tris[1] ^ t; // the partner index
                    if visited[other] {
                        continue;
                    }
                    visited[other] = true;
                    // Does the neighbor traverse the shared edge in the
                    // opposite direction?
                    let [p, q, r] = self.triangle_idx(other);
                    let agree = (p, q) == (y, x) || (q, r) == (y, x) || (r, p) == (y, x);
                    if !agree {
                        let k = other * 3;
                        self.indices.swap(k + 1, k + 2);
                        flipped = true;
                    }
                    queue.push_back(other);
                }
            }
        }

        if flipped {
            self.normals = None;
        }

        // Global outward correction: a consistently oriented *closed* mesh
        // must have positive signed volume. If the soup was inside-out,
        // flip every triangle.
        if self.is_closed() && self.volume_signed() < 0.0 {
            for t in 0..self.tri_count() {
                let k = t * 3;
                self.indices.swap(k + 1, k + 2);
            }
            flipped = true;
            self.normals = None;
        }

        flipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> TriMesh {
        // Full extents (1, 1, 1): a 1×1×1 cube, volume 1, area 6.
        crate::primitives::box_from_center_extents(Point3::origin(), Vector3::new(1.0, 1.0, 1.0))
    }

    #[test]
    fn box_is_closed_with_correct_volume() {
        let m = unit_box();
        assert!(m.is_closed());
        assert!((m.volume().unwrap() - 1.0).abs() < 1e-9);
        assert!((m.area() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn normals_point_outward() {
        let mut m = unit_box();
        m.compute_vertex_normals();
        let normals = m.normals.as_ref().unwrap();
        // Every vertex of the half-extent box sits on a face; its normal must
        // point away from the center in the dominant axis.
        for (p, n) in m.positions.iter().zip(normals) {
            let dominant = p.coords.iamax();
            assert!(n[dominant] * p.coords[dominant].signum() > 0.4);
        }
    }

    #[test]
    fn weld_collapses_duplicate_vertices() {
        let mut m = TriMesh::default();
        // Two separate copies of one triangle -> welding yields one.
        m.push_triangle(
            Point3::origin(),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        );
        m.push_triangle(
            Point3::origin(),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        );
        m.weld(WELD_EPS);
        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.tri_count(), 2); // triangles are kept, indices shared
    }

    #[test]
    fn sharp_edges_of_a_box() {
        let m = unit_box();
        let sharp = m.sharp_edges(45.0_f64.to_radians());
        assert_eq!(sharp.len(), 12, "a cube has 12 feature edges");
        assert!(m.boundary_edges().is_empty());
    }

    // ---- W-04 topology queries -------------------------------------------

    #[test]
    fn face_cluster_picks_one_flat_face_of_a_box() {
        let m = unit_box();
        // Any seed triangle belongs to a face of exactly 2 coplanar
        // triangles (1×1×1 box, 12 triangles total).
        for seed in 0..m.tri_count() {
            let cluster = m.face_cluster(seed, 1.0_f64.to_radians());
            assert_eq!(cluster.len(), 2, "seed {seed}: {:?}", cluster);
            assert!(cluster.contains(&seed));
            // The boundary is the 4-edge rectangle of that face.
            let boundary = m.cluster_boundary_edges(&cluster);
            assert_eq!(boundary.len(), 4, "seed {seed}");
            // Cluster triangles share the same normal (coplanar).
            let n0 = m.triangle_normal(cluster[0]).unwrap();
            let n1 = m.triangle_normal(cluster[1]).unwrap();
            assert!(n0.dot(&n1) > 0.999);
        }
    }

    #[test]
    fn face_cluster_of_cylinder_side_is_a_narrow_band() {
        let cfg = forge_core::TessellationConfig::default();
        let m = crate::primitives::cylinder(Point3::origin(), 5.0, 10.0, &cfg);
        // Side triangles: normals vary smoothly, so a 1° cluster stays a
        // narrow band (never the whole side, never empty).
        let cluster = m.face_cluster(4, 1.0_f64.to_radians());
        assert!(!cluster.is_empty());
        assert!(
            cluster.len() < m.tri_count() / 2,
            "band, not the whole side"
        );
    }

    #[test]
    fn sharp_edge_chains_box_yields_single_edge_chains() {
        let m = unit_box();
        let chains = m.sharp_edge_chains(45.0_f64.to_radians(), 30.0_f64.to_radians());
        // 12 cube edges; corners are perpendicular, so no chaining.
        assert_eq!(chains.len(), 12);
        for c in &chains {
            assert_eq!(c.len(), 1);
        }
    }

    #[test]
    fn sharp_edge_chains_cylinder_rims_are_two_loops() {
        let cfg = forge_core::TessellationConfig::default();
        let m = crate::primitives::cylinder(Point3::origin(), 5.0, 10.0, &cfg);
        let chains = m.sharp_edge_chains(45.0_f64.to_radians(), 30.0_f64.to_radians());
        // Two rims (bottom + top), each one closed tangent loop. The side
        // seams are smooth (no sharp edges between side triangles).
        assert_eq!(
            chains.len(),
            2,
            "{:?}",
            chains.iter().map(|c| c.len()).collect::<Vec<_>>()
        );
        let n = m.tri_count();
        for chain in &chains {
            assert_eq!(chain.len(), n / 4, "each rim is one full circle");
        }
    }

    #[test]
    fn repair_orientation_flips_inconsistent_soup() {
        // A box with one face's winding flipped: volume is wrong and the
        // soup is inconsistently oriented.
        let mut m = unit_box();
        let k = 3 * 3; // triangle 3
        m.indices.swap(k + 1, k + 2);
        assert!(m.volume_signed() - 1.0 < -0.1, "volume disturbed");

        let flipped = m.repair_orientation();
        assert!(flipped, "repair must report flips");
        assert!(m.is_closed(), "topology untouched");
        assert!(
            (m.volume_signed() - 1.0).abs() < 1e-9,
            "volume restored, got {}",
            m.volume_signed()
        );
    }

    #[test]
    fn repair_orientation_fixes_inside_out_mesh() {
        // Fully inside-out box (all windings reversed) -> negative volume.
        let mut m = unit_box();
        for t in 0..m.tri_count() {
            let k = t * 3;
            m.indices.swap(k + 1, k + 2);
        }
        assert!(m.volume_signed() < 0.0);
        assert!(m.repair_orientation());
        assert!(
            (m.volume_signed() - 1.0).abs() < 1e-9,
            "inside-out soup imports as a valid solid"
        );
    }

    #[test]
    fn repair_orientation_idempotent_on_valid_mesh() {
        let mut m = unit_box();
        assert!(!m.repair_orientation(), "valid mesh needs no flips");
        assert!((m.volume_signed() - 1.0).abs() < 1e-9);
    }
}
