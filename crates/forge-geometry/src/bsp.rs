//! Binary space partitioning tree for CSG boolean operations.
//!
//! This is an `f64` port of the classic BSP classification algorithm
//! (popularized by `csg.js`): every triangle is treated as a convex
//! polygon; polygons are split recursively by splitting planes until each
//! node holds only coplanar polygons. The set operations in
//! [`crate::boolean`] combine two trees via clipping.

use forge_core::{Plane, Point3};
use nalgebra::Unit;
use std::collections::VecDeque;

type UnitVector3 = forge_core::UnitVector3;

/// Plane classification epsilon (mm).
const EPS: f64 = 1e-7;

/// A convex polygon (triangle or split fragment) with CCW winding relative
/// to its plane normal.
#[derive(Debug, Clone)]
pub struct Polygon {
    /// Convex vertex ring.
    pub vertices: Vec<Point3>,
}

impl Polygon {
    /// Construct from a triangle.
    pub fn from_triangle(a: Point3, b: Point3, c: Point3) -> Self {
        Self {
            vertices: vec![a, b, c],
        }
    }

    /// Plane of the polygon (`None` if degenerate).
    pub fn plane(&self) -> Option<Plane> {
        let n = self.vertices.len();
        if n < 3 {
            return None;
        }
        // Find the first non-degenerate corner for a stable normal.
        for i in 0..n {
            let a = self.vertices[i];
            let b = self.vertices[(i + 1) % n];
            let c = self.vertices[(i + 2) % n];
            let normal = (b - a).cross(&(c - a));
            if normal.norm() > 1e-20 {
                return Plane::new(a, normal);
            }
        }
        None
    }

    /// Flip orientation (reverse vertex ring).
    pub fn flip(&mut self) {
        self.vertices.reverse();
    }
}

/// Classification of a vertex relative to a plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Coplanar,
    Front,
    Back,
}

/// A BSP node.
#[derive(Debug, Default)]
pub struct BspNode {
    plane: Option<Plane>,
    front: Option<Box<BspNode>>,
    back: Option<Box<BspNode>>,
    polygons: Vec<Polygon>,
}

impl BspNode {
    /// Build a tree from a polygon soup.
    pub fn build(polygons: Vec<Polygon>) -> Option<Box<BspNode>> {
        if polygons.is_empty() {
            return None;
        }
        let mut root = Box::new(BspNode::default());
        let mut queue: VecDeque<Polygon> = VecDeque::from(polygons);
        let mut count = 0usize;
        while let Some(poly) = queue.pop_front() {
            root.insert(poly, &mut queue);
            count += 1;
            if count > 2_000_000 {
                break; // safety valve against pathological inputs
            }
        }
        if root.plane.is_none() {
            None // only degenerate polygons
        } else {
            Some(root)
        }
    }

    /// Insert one polygon, splitting as needed. Fragments that still need
    /// classification deeper in the tree are pushed onto `queue` by child
    /// creation.
    fn insert(&mut self, polygon: Polygon, queue: &mut VecDeque<Polygon>) {
        let mut coplanar_front = Vec::new();
        let mut coplanar_back = Vec::new();
        let mut front = Vec::new();
        let mut back = Vec::new();

        let plane = match self.plane {
            // No plane yet: this polygon defines the node plane.
            None => match polygon.plane() {
                Some(p) => {
                    self.plane = Some(p);
                    self.polygons.push(polygon);
                    return;
                }
                None => return, // degenerate polygon, drop
            },
            Some(p) => p,
        };

        split_polygon(
            &plane,
            polygon,
            &mut coplanar_front,
            &mut coplanar_back,
            &mut front,
            &mut back,
        );

        // Coplanar polygons of *both* orientations live in this node,
        // mirroring the reference algorithm.
        self.polygons.append(&mut coplanar_front);
        self.polygons.append(&mut coplanar_back);

        for poly in front {
            if let Some(child) = &mut self.front {
                child.insert(poly, queue);
            } else {
                self.front = BspNode::build_one(poly, queue);
            }
        }
        for poly in back {
            if let Some(child) = &mut self.back {
                child.insert(poly, queue);
            } else {
                self.back = BspNode::build_one(poly, queue);
            }
        }
    }

    /// Build a fresh child node around one polygon.
    fn build_one(polygon: Polygon, queue: &mut VecDeque<Polygon>) -> Option<Box<BspNode>> {
        let mut node = Box::new(BspNode::default());
        node.insert(polygon, queue);
        Some(node)
    }

    /// Flip the solid: reverse all polygon windings and swap front/back.
    pub fn invert(&mut self) {
        for poly in &mut self.polygons {
            poly.flip();
        }
        if let Some(p) = &mut self.plane {
            let n: forge_core::Vector3 = *p.normal.as_ref();
            p.normal = negate_unit(n);
        }
        std::mem::swap(&mut self.front, &mut self.back);
        if let Some(c) = &mut self.front {
            c.invert();
        }
        if let Some(c) = &mut self.back {
            c.invert();
        }
    }

    /// Remove all polygons in `self` that are inside `bsp`.
    pub fn clip_to(&mut self, bsp: &BspNode) {
        self.polygons = bsp.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(c) = &mut self.front {
            c.clip_to(bsp);
        }
        if let Some(c) = &mut self.back {
            c.clip_to(bsp);
        }
    }

    /// Clip `polygons` against this subtree: returns the polygons that are
    /// inside the solid represented by this tree.
    pub fn clip_polygons(&self, mut polygons: Vec<Polygon>) -> Vec<Polygon> {
        let plane = match self.plane {
            None => return polygons,
            Some(p) => p,
        };

        let mut front: Vec<Polygon> = Vec::with_capacity(polygons.len());
        let mut back: Vec<Polygon> = Vec::with_capacity(polygons.len());

        for poly in polygons.drain(..) {
            // Coplanar-front -> front (kept, it is on the surface);
            // coplanar-back -> back (interior classification).
            let mut cf = Vec::new();
            let mut cb = Vec::new();
            split_polygon(&plane, poly, &mut cf, &mut cb, &mut front, &mut back);
            front.extend(cf);
            back.extend(cb);
        }

        let front = match &self.front {
            Some(child) => child.clip_polygons(front),
            // No front child: polygons in front of this leaf's plane are
            // outside the solid -> kept.
            None => front,
        };
        let back = match &self.back {
            Some(child) => child.clip_polygons(back),
            // No back child: polygons behind this leaf's plane are inside
            // the solid -> removed.
            None => Vec::new(),
        };

        let mut result = front;
        result.extend(back);
        result
    }

    /// Insert additional polygons into the tree.
    pub fn add_polygons(&mut self, polygons: Vec<Polygon>) {
        let mut queue = VecDeque::from(polygons);
        while let Some(poly) = queue.pop_front() {
            self.insert(poly, &mut queue);
        }
    }

    /// Collect all polygons (consumes the tree).
    pub fn into_polygons(mut self) -> Vec<Polygon> {
        let mut out = std::mem::take(&mut self.polygons);
        if let Some(c) = self.front {
            out.extend(c.into_polygons());
        }
        if let Some(c) = self.back {
            out.extend(c.into_polygons());
        }
        out
    }

    /// Polygon count (diagnostics).
    pub fn polygon_count(&self) -> usize {
        self.polygons.len()
            + self.front.as_ref().map(|c| c.polygon_count()).unwrap_or(0)
            + self.back.as_ref().map(|c| c.polygon_count()).unwrap_or(0)
    }
}

fn negate_unit(v: forge_core::Vector3) -> UnitVector3 {
    Unit::new_unchecked(-v)
}

/// Split `polygon` by `plane`, distributing the pieces:
/// - coplanar with matching normal -> `coplanar_front`,
/// - coplanar with opposite normal -> `coplanar_back`,
/// - entirely in front / back -> `front` / `back`,
/// - spanning -> split into a front piece and a back piece.
fn split_polygon(
    plane: &Plane,
    polygon: Polygon,
    coplanar_front: &mut Vec<Polygon>,
    coplanar_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let n = polygon.vertices.len();
    let mut types = Vec::with_capacity(n);
    let mut polygon_type = 0u8; // bit 1 = front, bit 2 = back

    for v in &polygon.vertices {
        let d = plane.signed_distance(v);
        let t = if d < -EPS {
            polygon_type |= 2;
            Side::Back
        } else if d > EPS {
            polygon_type |= 1;
            Side::Front
        } else {
            Side::Coplanar
        };
        types.push((t, d));
    }

    match polygon_type {
        0 => {
            // Coplanar: orientation decides the bucket.
            if let Some(pp) = polygon.plane() {
                if pp.normal.dot(&plane.normal) > 0.0 {
                    coplanar_front.push(polygon);
                } else {
                    coplanar_back.push(polygon);
                }
            } else {
                coplanar_front.push(polygon);
            }
        }
        1 => front.push(polygon),
        2 => back.push(polygon),
        _ => {
            // Spanning: split into two convex pieces.
            let mut f: Vec<Point3> = Vec::with_capacity(n + 1);
            let mut b: Vec<Point3> = Vec::with_capacity(n + 1);
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, di) = types[i];
                let (tj, dj) = types[j];
                let vi = polygon.vertices[i];
                let vj = polygon.vertices[j];

                if ti != Side::Back {
                    f.push(vi);
                }
                if ti != Side::Front {
                    b.push(vi);
                }
                let spanning = (ti == Side::Front && tj == Side::Back)
                    || (ti == Side::Back && tj == Side::Front);
                if spanning {
                    let t = di / (di - dj);
                    let v = vi + (vj - vi) * t;
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Polygon { vertices: f });
            }
            if b.len() >= 3 {
                back.push(Polygon { vertices: b });
            }
        }
    }
}
