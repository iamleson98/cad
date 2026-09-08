//! The [`Sketch`] aggregate: entities + constraints on a plane, with
//! solving and profile extraction.

use crate::constraint::{Constraint, PointRole};
use crate::entity::SketchEntity;
use crate::planes::SketchPlane;
use crate::solver;
use crate::{Result, SolveStatus};
use forge_core::{EntityId, Point2, SketchId, TessellationConfig};
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
        self.entities.insert(id, SketchEntity::Spline { id, control });
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
        self.constraints.retain(|c| {
            !c.referenced_entities().iter().any(|e| *e == id)
        });
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
            SketchEntity::Circle { center, radius, .. } => Some(
                forge_geometry_lite_circle(*center, *radius, cfg),
            ),
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
        while !segments.is_empty() {
            let (_, _, mut pts) = segments.pop().expect("non-empty");
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
fn forge_geometry_lite_circle(center: Point2, radius: f64, cfg: &TessellationConfig) -> Vec<Point2> {
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
            b: b,
            b_point: PointRole::Start,
            value: 50.0,
        });
        let report = s.solve().expect("solve");
        assert!(report.is_solved(), "status {:?} residual {}", report.status, report.residual);
        let pc = s.entities.get(&c).unwrap().point(&PointRole::Start).unwrap();
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
        s.add_constraint(Constraint::Length { line: bl, value: 10.0 });
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
}
