//! Sketch entity types with their parametric degree-of-freedom layout.

use forge_core::{EntityId, Point2};
use serde::{Deserialize, Serialize};

/// A sketch entity. Each variant documents its DOF layout – the order in
/// which parameters are packed into the solver vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SketchEntity {
    /// Construction point. DOFs: `[x, y]`.
    Point {
        /// Entity id.
        id: EntityId,
        /// Position.
        p: Point2,
    },
    /// Straight segment. DOFs: `[x1, y1, x2, y2]`.
    Line {
        /// Entity id.
        id: EntityId,
        /// Start point.
        start: Point2,
        /// End point.
        end: Point2,
    },
    /// Full circle. DOFs: `[cx, cy, r]`.
    Circle {
        /// Entity id.
        id: EntityId,
        /// Center.
        center: Point2,
        /// Radius.
        radius: f64,
    },
    /// Circular arc, CCW from `start_angle` to `end_angle`.
    /// DOFs: `[cx, cy, r, start_angle, end_angle]`.
    Arc {
        /// Entity id.
        id: EntityId,
        /// Center.
        center: Point2,
        /// Radius.
        radius: f64,
        /// Start angle (radians).
        start_angle: f64,
        /// End angle (radians); the arc sweeps CCW from start to end.
        end_angle: f64,
    },
    /// Full ellipse. DOFs: `[cx, cy, rx, ry, tilt]` (S-03): center,
    /// the `rx` semi-axis along the tilted x direction, the `ry`
    /// semi-axis along the tilted y direction, and the tilt angle of
    /// the rx axis.
    Ellipse {
        /// Entity id.
        id: EntityId,
        /// Center.
        center: Point2,
        /// Semi-axis along the tilted x direction.
        rx: f64,
        /// Semi-axis along the tilted y direction.
        ry: f64,
        /// Rotation of the rx axis (radians).
        tilt: f64,
    },
    /// Elliptical arc, CCW (in the tilted frame) from `start_angle` to
    /// `end_angle`. DOFs: `[cx, cy, rx, ry, tilt, start_angle, end_angle]`
    /// (S-03). The parametric point at angle a is
    /// `center + R(tilt) * (rx*cos a, ry*sin a)`.
    EllipseArc {
        /// Entity id.
        id: EntityId,
        /// Center.
        center: Point2,
        /// Semi-axis along the tilted x direction.
        rx: f64,
        /// Semi-axis along the tilted y direction.
        ry: f64,
        /// Rotation of the rx axis (radians).
        tilt: f64,
        /// Start parameter (radians).
        start_angle: f64,
        /// End parameter (radians); the arc sweeps CCW from start to end.
        end_angle: f64,
    },
    /// Interpolating-ish uniform cubic B-spline (control polygon).
    /// DOFs: `[x0, y0, x1, y1, …]` of the control points.
    Spline {
        /// Entity id.
        id: EntityId,
        /// Control points.
        control: Vec<Point2>,
    },
}

impl SketchEntity {
    /// Entity id.
    pub fn id(&self) -> EntityId {
        match self {
            SketchEntity::Point { id, .. }
            | SketchEntity::Line { id, .. }
            | SketchEntity::Circle { id, .. }
            | SketchEntity::Arc { id, .. }
            | SketchEntity::Ellipse { id, .. }
            | SketchEntity::EllipseArc { id, .. }
            | SketchEntity::Spline { id, .. } => *id,
        }
    }

    /// Number of solver parameters (DOFs).
    pub fn dof(&self) -> usize {
        match self {
            SketchEntity::Point { .. } => 2,
            SketchEntity::Line { .. } => 4,
            SketchEntity::Circle { .. } => 3,
            SketchEntity::Arc { .. } => 5,
            SketchEntity::Ellipse { .. } => 5,
            SketchEntity::EllipseArc { .. } => 7,
            SketchEntity::Spline { control, .. } => control.len() * 2,
        }
    }

    /// Pack parameters into `x`.
    pub fn pack(&self, x: &mut Vec<f64>) {
        match self {
            SketchEntity::Point { p, .. } => x.extend([p.x, p.y]),
            SketchEntity::Line { start, end, .. } => x.extend([start.x, start.y, end.x, end.y]),
            SketchEntity::Circle { center, radius, .. } => x.extend([center.x, center.y, *radius]),
            SketchEntity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => x.extend([center.x, center.y, *radius, *start_angle, *end_angle]),
            SketchEntity::Ellipse {
                center,
                rx,
                ry,
                tilt,
                ..
            } => x.extend([center.x, center.y, *rx, *ry, *tilt]),
            SketchEntity::EllipseArc {
                center,
                rx,
                ry,
                tilt,
                start_angle,
                end_angle,
                ..
            } => x.extend([
                center.x,
                center.y,
                *rx,
                *ry,
                *tilt,
                *start_angle,
                *end_angle,
            ]),
            SketchEntity::Spline { control, .. } => {
                for p in control {
                    x.extend([p.x, p.y]);
                }
            }
        }
    }

    /// Unpack parameters from `x` starting at `offset`.
    pub fn unpack(&mut self, x: &[f64], offset: usize) {
        let g = |k: usize| x[offset + k];
        match self {
            SketchEntity::Point { p, .. } => {
                *p = Point2::new(g(0), g(1));
            }
            SketchEntity::Line { start, end, .. } => {
                *start = Point2::new(g(0), g(1));
                *end = Point2::new(g(2), g(3));
            }
            SketchEntity::Circle { center, radius, .. } => {
                *center = Point2::new(g(0), g(1));
                *radius = g(2);
            }
            SketchEntity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                *center = Point2::new(g(0), g(1));
                *radius = g(2);
                *start_angle = g(3);
                *end_angle = g(4);
            }
            SketchEntity::Ellipse {
                center,
                rx,
                ry,
                tilt,
                ..
            } => {
                *center = Point2::new(g(0), g(1));
                *rx = g(2);
                *ry = g(3);
                *tilt = g(4);
            }
            SketchEntity::EllipseArc {
                center,
                rx,
                ry,
                tilt,
                start_angle,
                end_angle,
                ..
            } => {
                *center = Point2::new(g(0), g(1));
                *rx = g(2);
                *ry = g(3);
                *tilt = g(4);
                *start_angle = g(5);
                *end_angle = g(6);
            }
            SketchEntity::Spline { control, .. } => {
                for (k, p) in control.iter_mut().enumerate() {
                    *p = Point2::new(g(k * 2), g(k * 2 + 1));
                }
            }
        }
    }

    /// Point addressed by a constraint role (start/end/center).
    /// Returns the point *and* the Jacobian rows with respect to this
    /// entity's parameters: `dp/dparams` as a 2×dof matrix.
    pub fn point_with_jacobian(&self, role: &crate::PointRole) -> Option<(Point2, Vec<[f64; 2]>)> {
        match (self, role) {
            (SketchEntity::Point { p, .. }, crate::PointRole::Start)
            | (SketchEntity::Point { p, .. }, crate::PointRole::End) => {
                Some((*p, vec![[1.0, 0.0], [0.0, 1.0]]))
            }
            (SketchEntity::Line { start, .. }, crate::PointRole::Start) => {
                Some((*start, vec![[1.0, 0.0], [0.0, 1.0], [0.0, 0.0], [0.0, 0.0]]))
            }
            (SketchEntity::Line { end, .. }, crate::PointRole::End) => {
                Some((*end, vec![[0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]))
            }
            (SketchEntity::Circle { center, .. }, crate::PointRole::Center)
            | (SketchEntity::Arc { center, .. }, crate::PointRole::Center)
            | (SketchEntity::Ellipse { center, .. }, crate::PointRole::Center) => Some((
                *center,
                vec![[1.0, 0.0], [0.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
            )),
            (SketchEntity::EllipseArc { center, .. }, crate::PointRole::Center) => Some((
                *center,
                vec![
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [0.0, 0.0],
                    [0.0, 0.0],
                    [0.0, 0.0],
                    [0.0, 0.0],
                    [0.0, 0.0],
                ],
            )),
            (
                SketchEntity::EllipseArc {
                    center,
                    rx,
                    ry,
                    tilt,
                    start_angle,
                    ..
                },
                crate::PointRole::Start,
            ) => {
                // p(a) = c + R(t) * (rx cos a, ry sin a).
                let a = *start_angle;
                let (ct, st) = (tilt.cos(), tilt.sin());
                let (ca, sa) = (a.cos(), a.sin());
                let p = Point2::new(
                    center.x + ct * rx * ca - st * ry * sa,
                    center.y + st * rx * ca + ct * ry * sa,
                );
                // d/d(cx, cy, rx, ry, tilt, a0, a1)
                let j = vec![
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [ct * ca, st * ca],
                    [-st * sa, ct * sa],
                    [-st * rx * ca - ct * ry * sa, ct * rx * ca - st * ry * sa],
                    [-ct * rx * sa - st * ry * ca, -st * rx * sa + ct * ry * ca],
                    [0.0, 0.0],
                ];
                Some((p, j))
            }
            (
                SketchEntity::EllipseArc {
                    center,
                    rx,
                    ry,
                    tilt,
                    end_angle,
                    ..
                },
                crate::PointRole::End,
            ) => {
                let a = *end_angle;
                let (ct, st) = (tilt.cos(), tilt.sin());
                let (ca, sa) = (a.cos(), a.sin());
                let p = Point2::new(
                    center.x + ct * rx * ca - st * ry * sa,
                    center.y + st * rx * ca + ct * ry * sa,
                );
                // d/d(cx, cy, rx, ry, tilt, a0, a1)
                let j = vec![
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [ct * ca, st * ca],
                    [-st * sa, ct * sa],
                    [-st * rx * ca - ct * ry * sa, ct * rx * ca - st * ry * sa],
                    [0.0, 0.0],
                    [-ct * rx * sa - st * ry * ca, -st * rx * sa + ct * ry * ca],
                ];
                Some((p, j))
            }
            (
                SketchEntity::Arc {
                    center,
                    radius,
                    start_angle,
                    ..
                },
                crate::PointRole::Start,
            ) => {
                let p = Point2::new(
                    center.x + radius * start_angle.cos(),
                    center.y + radius * start_angle.sin(),
                );
                // d/d(cx, cy, r, a0, a1)
                let j = vec![
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [start_angle.cos(), start_angle.sin()],
                    [-radius * start_angle.sin(), radius * start_angle.cos()],
                    [0.0, 0.0],
                ];
                Some((p, j))
            }
            (
                SketchEntity::Arc {
                    center,
                    radius,
                    end_angle,
                    ..
                },
                crate::PointRole::End,
            ) => {
                let p = Point2::new(
                    center.x + radius * end_angle.cos(),
                    center.y + radius * end_angle.sin(),
                );
                let j = vec![
                    [1.0, 0.0],
                    [0.0, 1.0],
                    [end_angle.cos(), end_angle.sin()],
                    [0.0, 0.0],
                    [-radius * end_angle.sin(), radius * end_angle.cos()],
                ];
                Some((p, j))
            }
            _ => None,
        }
    }

    /// Convenience: just the point (no Jacobian).
    pub fn point(&self, role: &crate::PointRole) -> Option<Point2> {
        self.point_with_jacobian(role).map(|(p, _)| p)
    }

    /// Center (circles/arcs/ellipses) or midpoint (lines).
    pub fn center(&self) -> Option<Point2> {
        match self {
            SketchEntity::Circle { center, .. }
            | SketchEntity::Arc { center, .. }
            | SketchEntity::Ellipse { center, .. }
            | SketchEntity::EllipseArc { center, .. } => Some(*center),
            SketchEntity::Line { start, end, .. } => {
                Some(Point2::from((start.coords + end.coords) * 0.5))
            }
            SketchEntity::Point { p, .. } => Some(*p),
            SketchEntity::Spline { control, .. } => control.first().copied(),
        }
    }

    /// Implicit-curve value for a point against this ellipse-shaped
    /// entity: `(u/rx)^2 + (v/ry)^2 - 1`, where `(u, v)` is the point
    /// offset rotated into the ellipse frame. Zero = on the curve
    /// (S-03, shared by `Ellipse` and `EllipseArc`).
    pub fn ellipse_implicit(&self, p: Point2) -> Option<f64> {
        let (c, rx, ry, tilt) = match self {
            SketchEntity::Ellipse {
                center,
                rx,
                ry,
                tilt,
                ..
            }
            | SketchEntity::EllipseArc {
                center,
                rx,
                ry,
                tilt,
                ..
            } => (*center, *rx, *ry, *tilt),
            _ => return None,
        };
        let dx = p.x - c.x;
        let dy = p.y - c.y;
        let u = tilt.cos() * dx + tilt.sin() * dy;
        let v = -tilt.sin() * dx + tilt.cos() * dy;
        Some((u / rx) * (u / rx) + (v / ry) * (v / ry) - 1.0)
    }
}
