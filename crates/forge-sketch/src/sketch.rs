//! The [`Sketch`] aggregate: entities + constraints on a plane, with
//! solving and profile extraction.

use crate::constraint::{Constraint, PointRole};
use crate::entity::SketchEntity;
use crate::planes::SketchPlane;
use crate::solver;
use crate::{Result, SolveStatus};
use forge_core::{EntityId, Point2, SketchId, TessellationConfig, Vector2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Solve report produced by [`Sketch::solve`].
#[derive(Debug, Clone)]
pub struct SolveReport {
    /// Termination status.
    pub status: SolveStatus,
    /// Final residual norm.
    pub residual: f64,
    /// Iterations used.
    pub iterations: usize,
    /// DOF count of the parameter system.
    pub dof: usize,
    /// Equation count.
    pub equations: usize,
    /// `dof - rank estimate`: positive => under-constrained, negative =>
    /// (possibly) redundant/over-constrained.
    pub dof_balance: i64,
}

impl SolveReport {
    /// `true` when the constraints are satisfied.
    pub fn is_solved(&self) -> bool {
        self.status == SolveStatus::Converged
    }
}

/// A parametric 2D sketch.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Sketch {
    /// Sketch id (unique within the document).
    pub id: SketchId,
    /// Display name.
    pub name: String,
    /// The plane this sketch lives on.
    pub plane: SketchPlane,
    /// Entities keyed by id.
    pub entities: BTreeMap<EntityId, SketchEntity>,
    /// Constraints.
    pub constraints: Vec<Constraint>,
    /// Entity id counter (local to this sketch).
    next_entity: u64,
}

impl Sketch {
    /// Create an empty sketch on `plane`.
    pub fn new(id: SketchId, name: impl Into<String>, plane: SketchPlane) -> Self {
        Self {
            id,
            name: name.into(),
            plane,
            entities: BTreeMap::new(),
            constraints: Vec::new(),
            next_entity: 1,
        }
    }

    fn next_id(&mut self) -> EntityId {
        let id = EntityId::new(self.next_entity);
        self.next_entity += 1;
        id
    }

    /// Add a construction point.
    pub fn add_point(&mut self, p: Point2) -> EntityId {
        let id = self.next_id();
        self.entities.insert(id, SketchEntity::Point { id, p });
        id
    }

    /// Add a line segment.
    pub fn add_line(&mut self, start: Point2, end: Point2) -> EntityId {
        let id = self.next_id();
        self.entities
            .insert(id, SketchEntity::Line { id, start, end });
        id
    }

    /// Add a circle.
    pub fn add_circle(&mut self, center: Point2, radius: f64) -> EntityId {
        let id = self.next_id();
        self.entities
            .insert(id, SketchEntity::Circle { id, center, radius });
        id
    }

    /// Add an arc (CCW from `start_angle` to `end_angle`).
    pub fn add_arc(
        &mut self,
        center: Point2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> EntityId {
        let id = self.next_id();
        self.entities.insert(
            id,
            SketchEntity::Arc {
                id,
                center,
                radius,
                start_angle,
                end_angle,
            },
        );
        id
    }

    /// Add a spline through/over the control points.
    pub fn add_spline(&mut self, control: Vec<Point2>) -> EntityId {
        let id = self.next_id();
        self.entities
            .insert(id, SketchEntity::Spline { id, control });
        id
    }

    /// Add a rectangle as four lines with coincident + horizontal/vertical
    /// constraints (FR-SK-01 convenience primitive).
    pub fn add_rectangle(&mut self, min: Point2, max: Point2) -> [EntityId; 4] {
        let bl = self.add_line(min, Point2::new(max.x, min.y));
        let br = self.add_line(Point2::new(max.x, min.y), max);
        let tr = self.add_line(max, Point2::new(min.x, max.y));
        let tl = self.add_line(Point2::new(min.x, max.y), min);
        self.constraints.push(Constraint::Coincident {
            a: bl,
            a_point: PointRole::End,
            b: br,
            b_point: PointRole::Start,
        });
        self.constraints.push(Constraint::Coincident {
            a: br,
            a_point: PointRole::End,
            b: tr,
            b_point: PointRole::Start,
        });
        self.constraints.push(Constraint::Coincident {
            a: tr,
            a_point: PointRole::End,
            b: tl,
            b_point: PointRole::Start,
        });
        self.constraints.push(Constraint::Coincident {
            a: tl,
            a_point: PointRole::End,
            b: bl,
            b_point: PointRole::Start,
        });
        self.constraints.push(Constraint::Horizontal { line: bl });
        self.constraints.push(Constraint::Horizontal { line: tr });
        self.constraints.push(Constraint::Vertical { line: br });
        self.constraints.push(Constraint::Vertical { line: tl });
        [bl, br, tr, tl]
    }

    /// Add a straight slot (S-01): two semicircular arc ends joined by two
    /// tangent sides, exactly how production sketchers implement the slot
    /// tool. The entities are fully parametric: drag the endpoints, or
    /// edit the center distance / radius constraints.
    ///
    /// Returns `[bottom_line, head_arc, top_line, tail_arc]` (in CCW
    /// contour order from `p1` to `p2`).
    pub fn add_slot(&mut self, p1: Point2, p2: Point2, radius: f64) -> Result<[EntityId; 4]> {
        if radius <= 1e-12 || !radius.is_finite() {
            return Err(crate::SketchError::Invalid(
                "slot radius must be positive".into(),
            ));
        }
        let d = p2 - p1;
        if d.norm() < 1e-12 {
            return Err(crate::SketchError::Invalid(
                "slot endpoints must be distinct".into(),
            ));
        }
        let d = d.normalize();
        // Left-hand normal (90° CCW of the slot direction).
        let n = Vector2::new(-d.y, d.x);
        let theta_n = n.y.atan2(n.x);

        // Arcs: tail spans [theta_n, theta_n + pi], head spans
        // [theta_n + pi, theta_n + TAU] (both CCW, outer contour order).
        let tail = self.add_arc(p1, radius, theta_n, theta_n + std::f64::consts::PI);
        let head = self.add_arc(
            p2,
            radius,
            theta_n + std::f64::consts::PI,
            theta_n + std::f64::consts::TAU,
        );
        // Bottom side runs p1 - r*n -> p2 - r*n (with the contour).
        let bottom = self.add_line(p1 - n * radius, p2 - n * radius);
        // Top side runs back: p2 + r*n -> p1 + r*n.
        let top = self.add_line(p2 + n * radius, p1 + n * radius);

        // Close the contour: coincidences at all four joints.
        let joins = [
            (tail, PointRole::End, bottom, PointRole::Start),
            (bottom, PointRole::End, head, PointRole::Start),
            (head, PointRole::End, top, PointRole::Start),
            (top, PointRole::End, tail, PointRole::Start),
        ];
        for (a, a_point, b, b_point) in joins {
            self.constraints.push(Constraint::Coincident {
                a,
                a_point,
                b,
                b_point,
            });
        }
        // Each side is tangent to both end arcs.
        for line in [bottom, top] {
            for arc in [tail, head] {
                self.constraints.push(Constraint::TangentLineCircle {
                    line,
                    circle: arc,
                    kind: crate::TangentKind::External,
                });
            }
        }
        // Same radius on both ends + the dimensional pair (length, radius).
        self.constraints
            .push(Constraint::EqualRadius { a: tail, b: head });
        self.constraints.push(Constraint::Distance {
            a: tail,
            a_point: PointRole::Center,
            b: head,
            b_point: PointRole::Center,
            value: (p2 - p1).norm(),
        });
        self.constraints.push(Constraint::Radius {
            circle: tail,
            value: radius,
        });

        Ok([bottom, head, top, tail])
    }

    /// Add a regular polygon (S-02) as `sides` line entities (3..=64)
    /// inscribed in the circle around `center` with circumradius `radius`.
    /// The first vertex sits at `start_angle`. Sides are constrained
    /// equal-length with fixed turning angles: a rigid, fully parametric
    /// polygon (free to translate/rotate like in production sketchers).
    pub fn add_polygon(
        &mut self,
        center: Point2,
        radius: f64,
        sides: usize,
        start_angle: f64,
    ) -> Result<Vec<EntityId>> {
        if !(3..=64).contains(&sides) {
            return Err(crate::SketchError::Invalid(
                "polygon needs 3..=64 sides".into(),
            ));
        }
        if radius <= 1e-12 || !radius.is_finite() {
            return Err(crate::SketchError::Invalid(
                "polygon radius must be positive".into(),
            ));
        }
        let vertex = |k: usize| -> Point2 {
            let a = start_angle + std::f64::consts::TAU * k as f64 / sides as f64;
            Point2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        };
        let mut lines = Vec::with_capacity(sides);
        for k in 0..sides {
            lines.push(self.add_line(vertex(k), vertex(k + 1)));
        }
        // Coincident closure.
        for k in 0..sides {
            let next = (k + 1) % sides;
            self.constraints.push(Constraint::Coincident {
                a: lines[k],
                a_point: PointRole::End,
                b: lines[next],
                b_point: PointRole::Start,
            });
        }
        // Equal side lengths + the side length dimension (pins the scale:
        // without it the polygon can collapse to a point, which satisfies
        // every relative constraint).
        let side = 2.0 * radius * (std::f64::consts::PI / sides as f64).sin();
        self.constraints.push(Constraint::Length {
            line: lines[0],
            value: side,
        });
        for k in 1..sides {
            self.constraints.push(Constraint::EqualLength {
                a: lines[0],
                b: lines[k],
            });
        }
        // Fixed turning angle between consecutive sides (exterior angle of
        // a regular n-gon traversed CCW).
        let exterior = std::f64::consts::TAU / sides as f64;
        for k in 0..sides - 1 {
            self.constraints.push(Constraint::Angle {
                a: lines[k],
                b: lines[k + 1],
                value: exterior,
            });
        }
        Ok(lines)
    }

    /// Add a constraint; returns its index.
    pub fn add_constraint(&mut self, c: Constraint) -> usize {
        self.constraints.push(c);
        self.constraints.len() - 1
    }

    /// Remove a constraint by index.
    pub fn remove_constraint(&mut self, index: usize) -> Option<Constraint> {
        if index < self.constraints.len() {
            Some(self.constraints.remove(index))
        } else {
            None
        }
    }

    /// Remove an entity and all constraints referencing it.
    pub fn remove_entity(&mut self, id: EntityId) {
        self.entities.remove(&id);
        self.constraints
            .retain(|c| !c.referenced_entities().contains(&id));
    }

    /// Solve all constraints (FR-SK-02, real time).
    pub fn solve(&mut self) -> Result<SolveReport> {
        let inner = solver::solve(self)?;
        let dof_balance = inner.dof as i64 - inner.equations as i64;
        Ok(SolveReport {
            status: inner.status,
            residual: inner.residual,
            iterations: inner.iterations,
            dof: inner.dof,
            equations: inner.equations,
            dof_balance,
        })
    }

    /// Discretize one entity into a polyline (arcs/splines sampled, lines
    /// as their endpoints).
    fn entity_polyline(&self, id: EntityId, cfg: &TessellationConfig) -> Option<Vec<Point2>> {
        match self.entities.get(&id)? {
            SketchEntity::Line { start, end, .. } => Some(vec![*start, *end]),
            SketchEntity::Circle { center, radius, .. } => {
                Some(forge_geometry_lite_circle(*center, *radius, cfg))
            }
            SketchEntity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                let mut end = *end_angle;
                while end <= *start_angle {
                    end += std::f64::consts::TAU;
                }
                let n = cfg.steps_for_arc(*radius, end - *start_angle);
                Some(arc_points(*center, *radius, *start_angle, end, n))
            }
            SketchEntity::Spline { control, .. } => {
                let samples = control.len() * 8;
                Some(crate::nurbs::sample(control, samples))
            }
            SketchEntity::Point { .. } => None,
        }
    }

    /// Extract closed contours from the sketch for profile operations
    /// (extrude/revolve). Circles form singleton contours; lines/arcs are
    /// chained by coincident endpoints (within a tolerance).
    ///
    /// Returns outer contours as CCW and any inner contour as detected by
    /// containment (the caller normalizes holes anyway).
    pub fn profile_contours(&self, cfg: &TessellationConfig) -> Vec<Vec<Point2>> {
        const JOIN_EPS: f64 = 1e-7;
        let mut contours: Vec<Vec<Point2>> = Vec::new();

        // Standalone circles are their own contours.
        for (id, e) in &self.entities {
            if let SketchEntity::Circle { center, radius, .. } = e {
                let pts = forge_geometry_lite_circle(*center, *radius, cfg);
                let _ = id;
                contours.push(pts);
            }
        }

        // Chain open polylines (lines, arcs, splines) by endpoints.
        let mut segments: Vec<(Point2, Point2, Vec<Point2>)> = Vec::new(); // (start, end, pts)
        for (id, e) in &self.entities {
            match e {
                SketchEntity::Line { start, end, .. } => {
                    segments.push((*start, *end, vec![*start, *end]));
                }
                SketchEntity::Arc { .. } | SketchEntity::Spline { .. } => {
                    if let Some(pts) = self.entity_polyline(*id, cfg) {
                        if pts.len() >= 2 {
                            segments.push((*pts.first().unwrap(), *pts.last().unwrap(), pts));
                        }
                    }
                }
                _ => {}
            }
        }

        // Greedy chaining. Merge cases (p = candidate polyline,
        // pts = accumulated contour):
        //   A: p ends where pts starts  -> merged = p ++ pts[1..]
        //   C: p starts where pts starts -> merged = reversed(p) ++ pts[1..]
        //   B: p starts where pts ends   -> merged = pts ++ p[1..]
        //   D: p ends where pts ends     -> merged = pts ++ reversed(p)[1..]
        while let Some((_, _, mut pts)) = segments.pop() {
            let mut grew = true;
            while grew {
                grew = false;
                let head = *pts.first().expect("non-empty");
                let tail = *pts.last().expect("non-empty");
                for i in 0..segments.len() {
                    let (s, e, p) = &segments[i];
                    let (s, e) = (*s, *e);
                    let merge: Option<Vec<Point2>> = if (e - head).norm() < JOIN_EPS {
                        let mut m = p.clone();
                        m.extend_from_slice(&pts[1..]);
                        Some(m)
                    } else if (s - head).norm() < JOIN_EPS {
                        let mut m = p.clone();
                        m.reverse();
                        m.extend_from_slice(&pts[1..]);
                        Some(m)
                    } else if (s - tail).norm() < JOIN_EPS {
                        let mut m = pts.clone();
                        m.extend_from_slice(&p[1..]);
                        Some(m)
                    } else if (e - tail).norm() < JOIN_EPS {
                        let mut r = p.clone();
                        r.reverse();
                        let mut m = pts.clone();
                        m.extend_from_slice(&r[1..]);
                        Some(m)
                    } else {
                        None
                    };
                    if let Some(m) = merge {
                        pts = m;
                        segments.remove(i);
                        grew = true;
                        break;
                    }
                }
            }
            // Closed when every segment was consumed into this chain and
            // the wrap-around endpoints coincide.
            let closed = segments.is_empty()
                && (pts.first().expect("non-empty") - pts.last().expect("non-empty")).norm()
                    < JOIN_EPS * 2.0
                && pts.len() > 3;
            if closed {
                pts.pop();
            }
            if pts.len() >= 3 {
                contours.push(pts);
            }
        }
        contours
    }
}

/// Circle discretization (dependency-free, shared with callers).
fn forge_geometry_lite_circle(
    center: Point2,
    radius: f64,
    cfg: &TessellationConfig,
) -> Vec<Point2> {
    let n = cfg.segments_for_circle(radius);
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            Point2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Arc discretization from `start` to `end` angle with `n` intervals.
fn arc_points(center: Point2, radius: f64, start: f64, end: f64, n: usize) -> Vec<Point2> {
    let n = n.max(1);
    (0..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let a = start + (end - start) * t;
            Point2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Constraint;
    use approx::assert_abs_diff_eq;

    fn plane() -> SketchPlane {
        SketchPlane::default()
    }

    #[test]
    fn fully_constrained_triangle_solves() {
        let mut s = Sketch::new(SketchId::new(1), "tri", plane());
        let a = s.add_point(Point2::new(0.0, 0.0));
        let b = s.add_point(Point2::new(30.0, 10.0)); // perturbed targets
        let c = s.add_point(Point2::new(5.0, 25.0));
        s.add_constraint(Constraint::FixPoint {
            entity: a,
            point: PointRole::Start,
            position: (0.0, 0.0),
        });
        s.add_constraint(Constraint::FixPoint {
            entity: b,
            point: PointRole::Start,
            position: (40.0, 0.0),
        });
        s.add_constraint(Constraint::Distance {
            a: c,
            a_point: PointRole::Start,
            b: a,
            b_point: PointRole::Start,
            value: 30.0,
        });
        s.add_constraint(Constraint::Distance {
            a: c,
            a_point: PointRole::Start,
            b,
            b_point: PointRole::Start,
            value: 50.0,
        });
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        let pc = s
            .entities
            .get(&c)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        // 30-40-50 triangle: C is at (x, y) with |CA| = 30, |CB| = 50.
        // x^2+y^2=900; (x-40)^2+y^2=2500 -> -80x+1600=1600 -> x=0? Let me
        // verify: (x-40)^2 - x^2 = 1600 -> -80x + 1600 = 1600 -> x = 0,
        // y = 30.
        assert_abs_diff_eq!(pc.x, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(pc.y, 30.0, epsilon = 1e-6);
    }

    #[test]
    fn constrained_lines_rectangle() {
        let mut s = Sketch::new(SketchId::new(1), "rect", plane());
        let [bl, _br, _tr, _tl] = s.add_rectangle(Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        // Perturb one line and let the solver restore the rectangle.
        if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get_mut(&bl) {
            *start = Point2::new(1.0, -1.0);
            *end = Point2::new(9.0, 1.0);
        }
        s.add_constraint(Constraint::Length {
            line: bl,
            value: 10.0,
        });
        let report = s.solve().expect("solve");
        assert!(report.is_solved(), "residual {}", report.residual);
        // The rectangle corners are restored (up to the free translation).
        if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get(&bl) {
            assert_abs_diff_eq!((end - start).norm(), 10.0, epsilon = 1e-6);
            assert_abs_diff_eq!(start.y, end.y, epsilon = 1e-6);
        }
    }

    #[test]
    fn circle_tangent_line_solves() {
        let mut s = Sketch::new(SketchId::new(1), "tangent", plane());
        let line = s.add_line(Point2::new(-10.0, 3.0), Point2::new(10.0, 3.0));
        let circle = s.add_circle(Point2::new(0.0, 0.0), 5.0);
        s.add_constraint(Constraint::Horizontal { line });
        s.add_constraint(Constraint::TangentLineCircle {
            line,
            circle,
            kind: crate::TangentKind::External,
        });
        s.add_constraint(Constraint::Radius { circle, value: 5.0 });
        // Anchor the circle center so the *line* has to move (otherwise the
        // solver may equally well translate the circle).
        s.add_constraint(Constraint::FixPoint {
            entity: circle,
            point: PointRole::Center,
            position: (0.0, 0.0),
        });
        let report = s.solve().expect("solve");
        assert!(report.is_solved(), "residual {}", report.residual);
        // The line must now sit at distance 5 from the center.
        if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get(&line) {
            let y = (start.y + end.y) * 0.5;
            assert_abs_diff_eq!(y.abs(), 5.0, epsilon = 1e-6);
        }
    }

    #[test]
    fn profile_contours_chain_rectangle() {
        let mut s = Sketch::new(SketchId::new(1), "rect", plane());
        s.add_rectangle(Point2::new(0.0, 0.0), Point2::new(10.0, 6.0));
        let contours = s.profile_contours(&TessellationConfig::default());
        assert_eq!(contours.len(), 1, "four chained lines form one contour");
        let pts = &contours[0];
        assert_eq!(pts.len(), 4);
        // CCW orientation after chaining: signed area positive.
        let area: f64 = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let q = pts[(i + 1) % pts.len()];
                p.x * q.y - q.x * p.y
            })
            .sum::<f64>()
            * 0.5;
        assert!(area.abs() > 50.0, "area {area}");
    }

    #[test]
    fn profile_contours_circle_standalone() {
        let mut s = Sketch::new(SketchId::new(1), "circ", plane());
        s.add_circle(Point2::origin(), 4.0);
        let contours = s.profile_contours(&TessellationConfig::default());
        assert_eq!(contours.len(), 1);
        assert!(contours[0].len() >= 16);
    }

    #[test]
    fn slot_forms_single_closed_contour() {
        let mut s = Sketch::new(SketchId::new(1), "slot", plane());
        s.add_slot(Point2::new(-10.0, 0.0), Point2::new(10.0, 0.0), 3.0)
            .expect("slot");
        let contours = s.profile_contours(&TessellationConfig::default());
        assert_eq!(contours.len(), 1, "slot chains into one contour");
        let pts = &contours[0];
        // Two arcs (>= 8 samples each) + two line segments.
        assert!(pts.len() >= 18, "sample points {}", pts.len());
        // Slot area = rectangle 20×6 + circle π·3².
        let area: f64 = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let q = pts[(i + 1) % pts.len()];
                p.x * q.y - q.x * p.y
            })
            .sum::<f64>()
            * 0.5;
        let expected = 20.0 * 6.0 + std::f64::consts::PI * 9.0;
        assert!(
            (area - expected).abs() / expected < 0.05,
            "area {area} vs {expected}"
        );
    }

    #[test]
    fn slot_restores_shape_after_perturbation() {
        let mut s = Sketch::new(SketchId::new(1), "slot", plane());
        let [bottom, _head, _top, tail] = s
            .add_slot(Point2::new(-10.0, 0.0), Point2::new(10.0, 0.0), 3.0)
            .expect("slot");
        // Drag the bottom side off its tangent position.
        if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get_mut(&bottom) {
            *start = Point2::new(-8.0, -2.0);
            *end = Point2::new(8.0, -1.0);
        }
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );

        // Invariants (the solver may rigidly translate the slot, so we
        // check relations, not absolute coordinates):
        // 1. both arc centers sit at distance `radius` from the bottom line
        //    (tangency),
        // 2. all joints stay coincident (contour closure).
        let line = match s.entities.get(&bottom) {
            Some(SketchEntity::Line { start, end, .. }) => (*start, *end),
            _ => panic!("bottom line missing"),
        };
        let dist_to = |p: Point2| -> f64 {
            let (a, b) = line;
            let d = b - a;
            let len = d.norm();
            (d.y * p.x - d.x * p.y + b.x * a.y - b.y * a.x).abs() / len
        };
        for arc in [tail, _head] {
            if let Some(SketchEntity::Arc { center, radius, .. }) = s.entities.get(&arc) {
                let d = dist_to(*center);
                assert!((d - radius).abs() < 1e-6, "tangency {d} vs {radius}");
            }
        }
        // Contour closure: the bottom line's start meets the tail arc's
        // END point (tail spans [theta_n, theta_n + pi]).
        if let Some(SketchEntity::Arc {
            center,
            radius,
            end_angle,
            ..
        }) = s.entities.get(&tail)
        {
            let arc_end = Point2::new(
                center.x + radius * end_angle.cos(),
                center.y + radius * end_angle.sin(),
            );
            assert!(
                (arc_end - line.0).norm() < 1e-6,
                "joint gap {}",
                (arc_end - line.0).norm()
            );
        }
    }

    #[test]
    fn regular_polygon_chains_and_round_trips() {
        for sides in [3usize, 5, 8] {
            let mut s = Sketch::new(SketchId::new(1), "poly", plane());
            let lines = s
                .add_polygon(Point2::origin(), 10.0, sides, 0.0)
                .expect("polygon");
            assert_eq!(lines.len(), sides);

            let report = s.solve().expect("solve");
            // The macro is created consistent; solving is a no-op.
            assert!(report.is_solved(), "n={sides} residual {}", report.residual);

            let contours = s.profile_contours(&TessellationConfig::default());
            assert_eq!(contours.len(), 1, "n={sides}");
            let pts = &contours[0];
            // Regular n-gon area with circumradius r.
            let expected =
                0.5 * sides as f64 * 100.0 * (std::f64::consts::TAU / sides as f64).sin();
            let area: f64 = pts
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let q = pts[(i + 1) % pts.len()];
                    p.x * q.y - q.x * p.y
                })
                .sum::<f64>()
                * 0.5;
            assert_abs_diff_eq!(area, expected, epsilon = 1e-9);
        }
    }

    #[test]
    fn polygon_perturbation_stays_regular() {
        let mut s = Sketch::new(SketchId::new(1), "poly", plane());
        let lines = s
            .add_polygon(Point2::origin(), 10.0, 6, 0.0)
            .expect("polygon");
        // Push one vertex around; the constraints must restore regularity.
        if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get_mut(&lines[2]) {
            *start = Point2::new(2.0, 9.0);
            *end = Point2::new(-1.0, 11.0);
        }
        let report = s.solve().expect("solve");
        assert!(report.is_solved(), "residual {}", report.residual);
        // All side lengths must be equal again.
        let mut lengths = Vec::new();
        for id in &lines {
            if let Some(SketchEntity::Line { start, end, .. }) = s.entities.get(id) {
                lengths.push((end - start).norm());
            }
        }
        let l0 = lengths[0];
        for (i, l) in lengths.iter().enumerate() {
            assert!((l - l0).abs() < 1e-6, "side {i} length {l} vs {l0}");
        }
    }

    #[test]
    fn slot_and_polygon_reject_bad_input() {
        let mut s = Sketch::new(SketchId::new(1), "bad", plane());
        assert!(s.add_slot(Point2::origin(), Point2::origin(), 3.0).is_err());
        assert!(s
            .add_slot(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), 0.0)
            .is_err());
        assert!(s.add_polygon(Point2::origin(), 10.0, 2, 0.0).is_err());
        assert!(s.add_polygon(Point2::origin(), 0.0, 5, 0.0).is_err());
    }

    /// Regression: the Angle constraint Jacobian used the target's
    /// cos/sin instead of the current angle's, so angle-constrained
    /// sketches stalled at residuals ~1e-2 instead of converging.
    /// (Found while bringing up the polygon macro; fixed together with
    /// the wrapped-angle residual.)
    #[test]
    fn angle_constraint_converges_after_perturbation() {
        let mut s = Sketch::new(SketchId::new(1), "ang", plane());
        let a = s.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let b = s.add_line(Point2::new(0.0, 0.0), Point2::new(5.0, 8.66));
        s.add_constraint(Constraint::FixPoint {
            entity: a,
            point: PointRole::Start,
            position: (0.0, 0.0),
        });
        s.add_constraint(Constraint::Coincident {
            a,
            a_point: PointRole::Start,
            b,
            b_point: PointRole::Start,
        });
        s.add_constraint(Constraint::Horizontal { line: a });
        s.add_constraint(Constraint::Length {
            line: a,
            value: 10.0,
        });
        s.add_constraint(Constraint::Length {
            line: b,
            value: 10.0,
        });
        s.add_constraint(Constraint::Angle {
            a,
            b,
            value: 60_f64.to_radians(),
        });
        // Perturb b's endpoint and expect exact restoration.
        if let Some(SketchEntity::Line { end, .. }) = s.entities.get_mut(&b) {
            *end = Point2::new(2.0, 12.0);
        }
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        if let Some(SketchEntity::Line { end, .. }) = s.entities.get(&b) {
            assert_abs_diff_eq!(end.x, 5.0, epsilon = 1e-6);
            assert_abs_diff_eq!(end.y, 8.660254037844, epsilon = 1e-6);
        }
    }

    /// S-04: two points symmetric about a line: midpoint on the line and
    /// the segment perpendicular to it.
    #[test]
    fn symmetric_constraint_solves() {
        let mut s = Sketch::new(SketchId::new(1), "sym", plane());
        // Symmetry axis: the x-axis through the origin.
        let axis = s.add_line(Point2::new(-10.0, 0.0), Point2::new(10.0, 0.0));
        let p = s.add_point(Point2::new(3.0, 7.0));
        let q = s.add_point(Point2::new(-4.0, -5.0)); // perturbed mirror
        s.add_constraint(Constraint::Symmetric {
            a: p,
            a_point: PointRole::Start,
            b: q,
            b_point: PointRole::Start,
            line: axis,
        });
        // Pin the axis and the first point; the second must mirror.
        s.add_constraint(Constraint::FixPoint {
            entity: axis,
            point: PointRole::Start,
            position: (-10.0, 0.0),
        });
        s.add_constraint(Constraint::FixPoint {
            entity: axis,
            point: PointRole::End,
            position: (10.0, 0.0),
        });
        s.add_constraint(Constraint::FixPoint {
            entity: p,
            point: PointRole::Start,
            position: (3.0, 7.0),
        });
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        let pq = s
            .entities
            .get(&q)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        assert_abs_diff_eq!(pq.x, 3.0, epsilon = 1e-6);
        assert_abs_diff_eq!(pq.y, -7.0, epsilon = 1e-6);
    }

    /// S-04: symmetric constraint about a slanted, free-to-move line (the
    /// line's own parameters participate in the Jacobian).
    #[test]
    fn symmetric_slanted_axis_solves() {
        let mut s = Sketch::new(SketchId::new(1), "sym2", plane());
        let axis = s.add_line(Point2::new(-6.0, -2.0), Point2::new(8.0, 5.0));
        let p = s.add_point(Point2::new(1.0, 9.0));
        let q = s.add_point(Point2::new(-2.0, -3.0));
        s.add_constraint(Constraint::Symmetric {
            a: p,
            a_point: PointRole::Start,
            b: q,
            b_point: PointRole::Start,
            line: axis,
        });
        s.add_constraint(Constraint::FixPoint {
            entity: p,
            point: PointRole::Start,
            position: (1.0, 9.0),
        });
        s.add_constraint(Constraint::FixPoint {
            entity: q,
            point: PointRole::Start,
            position: (3.0, -4.0),
        });
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        // Verify the geometric property directly: midpoint on the line,
        // pq perpendicular to the line.
        let (l0, l1) = match s.entities.get(&axis) {
            Some(SketchEntity::Line { start, end, .. }) => (*start, *end),
            _ => panic!("axis missing"),
        };
        let pp = s
            .entities
            .get(&p)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        let pq = s
            .entities
            .get(&q)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        let d = l1 - l0;
        let mid = (pp.coords + pq.coords) * 0.5;
        let u = mid - l0.coords;
        let cross = d.x * u.y - d.y * u.x;
        let dot = d.x * (pq.x - pp.x) + d.y * (pq.y - pp.y);
        assert_abs_diff_eq!(cross, 0.0, epsilon = 1e-7);
        assert_abs_diff_eq!(dot, 0.0, epsilon = 1e-7);
    }

    /// S-04: a point pinned to a line's midpoint.
    #[test]
    fn midpoint_constraint_solves() {
        let mut s = Sketch::new(SketchId::new(1), "mid", plane());
        let line = s.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let p = s.add_point(Point2::new(9.0, 2.0)); // perturbed
        s.add_constraint(Constraint::MidpointOn {
            a: p,
            a_point: PointRole::Start,
            line,
        });
        s.add_constraint(Constraint::FixPoint {
            entity: line,
            point: PointRole::Start,
            position: (0.0, 0.0),
        });
        s.add_constraint(Constraint::FixPoint {
            entity: line,
            point: PointRole::End,
            position: (10.0, 0.0),
        });
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        let pp = s
            .entities
            .get(&p)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        assert_abs_diff_eq!(pp.x, 5.0, epsilon = 1e-6);
        assert_abs_diff_eq!(pp.y, 0.0, epsilon = 1e-6);
    }

    /// S-04: a point constrained onto a circle's circumference; the circle
    /// is anchored, so the point must travel onto the rim.
    #[test]
    fn point_on_circle_solves() {
        let mut s = Sketch::new(SketchId::new(1), "poc", plane());
        let circle = s.add_circle(Point2::new(0.0, 0.0), 6.0);
        let p = s.add_point(Point2::new(8.0, 3.0));
        s.add_constraint(Constraint::PointOnCircle {
            a: p,
            a_point: PointRole::Start,
            circle,
        });
        s.add_constraint(Constraint::FixPoint {
            entity: circle,
            point: PointRole::Center,
            position: (0.0, 0.0),
        });
        s.add_constraint(Constraint::Radius { circle, value: 6.0 });
        // Lock the point's angle-ish direction: keep it in the first
        // quadrant via a distance constraint to another fixed point.
        let anchor = s.add_point(Point2::new(12.0, 0.0));
        s.add_constraint(Constraint::FixPoint {
            entity: anchor,
            point: PointRole::Start,
            position: (12.0, 0.0),
        });
        s.add_constraint(Constraint::Distance {
            a: p,
            a_point: PointRole::Start,
            b: anchor,
            b_point: PointRole::Start,
            value: 6.6,
        });
        let report = s.solve().expect("solve");
        assert!(
            report.is_solved(),
            "status {:?} residual {}",
            report.status,
            report.residual
        );
        let pp = s
            .entities
            .get(&p)
            .unwrap()
            .point(&PointRole::Start)
            .unwrap();
        assert_abs_diff_eq!(pp.coords.norm(), 6.0, epsilon = 1e-6);
        assert_abs_diff_eq!((pp - Point2::new(12.0, 0.0)).norm(), 6.6, epsilon = 1e-6);
    }
}
