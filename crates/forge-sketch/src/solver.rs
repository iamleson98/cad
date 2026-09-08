//! Levenberg–Marquardt constraint solver.
//!
//! Assembles all constraint equations into a dense residual vector `r`
//! and Jacobian `J` over the packed entity parameter vector `x`, then
//! iterates damped Gauss–Newton steps
//! `(JᵀJ + λ·diag) δ = −Jᵀr`, accepting a step only when the residual
//! norm decreases (classic LM trust-region behavior).

use crate::constraint::{Constraint, PointRole, TangentKind};
use crate::entity::SketchEntity;
use crate::sketch::Sketch;
use forge_core::Point2;
use nalgebra::{DMatrix, DVector};
use std::collections::BTreeMap;

/// Solver termination status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// Residual norm below tolerance.
    Converged,
    /// Iteration budget exhausted; the sketch is typically
    /// under-constrained, and the result is the least-squares optimum.
    MaxIterations,
    /// Some constraints reference missing entities.
    BrokenReferences,
}

impl std::fmt::Display for SolveStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveStatus::Converged => f.write_str("converged"),
            SolveStatus::MaxIterations => f.write_str("max iterations"),
            SolveStatus::BrokenReferences => f.write_str("broken references"),
        }
    }
}

/// Detailed solver outcome.
#[derive(Debug, Clone)]
pub struct SolveReportInner {
    /// Termination status.
    pub status: SolveStatus,
    /// Final residual norm.
    pub residual: f64,
    /// Iterations used.
    pub iterations: usize,
    /// Parameter DOF count.
    pub dof: usize,
    /// Equation count (rank indicator).
    pub equations: usize,
}

/// Packed system snapshot of one sketch state.
struct System {
    x: Vec<f64>,
    entities: BTreeMap<forge_core::EntityId, SketchEntity>,
    spans: BTreeMap<forge_core::EntityId, (usize, usize)>,
}

impl System {
    fn build(sketch: &Sketch) -> Self {
        let mut x = Vec::new();
        let mut spans = BTreeMap::new();
        for (id, entity) in &sketch.entities {
            let offset = x.len();
            entity.pack(&mut x);
            spans.insert(*id, (offset, entity.dof()));
        }
        Self {
            x,
            entities: sketch.entities.clone(),
            spans,
        }
    }

    /// Write a parameter vector back into the entity snapshot.
    fn set_params(&mut self, x: Vec<f64>) {
        for (id, entity) in self.entities.iter_mut() {
            if let Some((offset, _)) = self.spans.get(id) {
                entity.unpack(&x, *offset);
            }
        }
        self.x = x;
    }

    fn entity(&self, id: forge_core::EntityId) -> Option<&SketchEntity> {
        self.entities.get(&id)
    }

    fn param_span(&self, id: forge_core::EntityId) -> Option<(usize, usize)> {
        self.spans.get(&id).copied()
    }

    fn point(&self, id: forge_core::EntityId, role: PointRole) -> Option<(Point2, Vec<[f64; 2]>)> {
        self.entity(id)?.point_with_jacobian(&role)
    }

    /// Line direction `(dx, dy)` and Jacobian (per param: [∂dx, ∂dy]).
    fn line_dir(&self, id: forge_core::EntityId) -> Option<([f64; 2], Vec<[f64; 2]>)> {
        let (p0, j0) = self.point(id, PointRole::Start)?;
        let (p1, j1) = self.point(id, PointRole::End)?;
        let dof = j0.len().max(j1.len());
        let mut j = vec![[0.0f64; 2]; dof];
        for (k, row) in j.iter_mut().enumerate() {
            let d0 = j0.get(k).copied().unwrap_or([0.0, 0.0]);
            let d1 = j1.get(k).copied().unwrap_or([0.0, 0.0]);
            *row = [d1[0] - d0[0], d1[1] - d0[1]];
        }
        Some(([p1.x - p0.x, p1.y - p0.y], j))
    }

    fn line_length(&self, id: forge_core::EntityId) -> Option<(f64, Vec<f64>)> {
        let (p0, j0) = self.point(id, PointRole::Start)?;
        let (p1, j1) = self.point(id, PointRole::End)?;
        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            return None;
        }
        let dof = j0.len().max(j1.len());
        let mut dl = vec![0.0f64; dof];
        for (k, dv) in dl.iter_mut().enumerate() {
            let d0 = j0.get(k).copied().unwrap_or([0.0, 0.0]);
            let d1 = j1.get(k).copied().unwrap_or([0.0, 0.0]);
            *dv = (dx * (d1[0] - d0[0]) + dy * (d1[1] - d0[1])) / len;
        }
        Some((len, dl))
    }

    /// Circle `(cx, cy, r)`. The center Jacobian covers params 0..1; the
    /// radius derivative is handled separately via [`Self::radius_row`].
    fn circle(&self, id: forge_core::EntityId) -> Option<(f64, f64, f64)> {
        let e = self.entity(id)?;
        match e {
            SketchEntity::Circle { center, radius, .. } => Some((center.x, center.y, *radius)),
            SketchEntity::Arc { center, radius, .. } => Some((center.x, center.y, *radius)),
            _ => None,
        }
    }

    /// `∂r/∂params` as a row (1 at the radius parameter index).
    fn radius_row(&self, id: forge_core::EntityId) -> Vec<f64> {
        match self.param_span(id) {
            Some((_, dof)) => {
                let mut d = vec![0.0f64; dof];
                if dof >= 3 {
                    d[2] = 1.0;
                }
                d
            }
            None => Vec::new(),
        }
    }
}

/// One equation block: residuals plus sparse-like dense Jacobian rows.
struct Eq {
    r: Vec<f64>,
    j: DMatrix<f64>,
}

fn build_equation(sys: &System, c: &Constraint) -> Option<Eq> {
    let n = sys.x.len();
    let m = c.equation_count();
    let mut r = vec![0.0f64; m];
    let mut j = DMatrix::zeros(m, n);

    match c {
        Constraint::Coincident { a, a_point, b, b_point } => {
            let (pa, ja) = sys.point(*a, *a_point)?;
            let (pb, jb) = sys.point(*b, *b_point)?;
            r[0] = pa.x - pb.x;
            r[1] = pa.y - pb.y;
            let (oa, da) = sys.param_span(*a)?;
            for (k, blk) in ja.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += blk[0];
                    j[(1, oa + k)] += blk[1];
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, blk) in jb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] -= blk[0];
                    j[(1, ob + k)] -= blk[1];
                }
            }
        }
        Constraint::Horizontal { line } => {
            let (d, jd) = sys.line_dir(*line)?;
            r[0] = d[1];
            let (o, dof) = sys.param_span(*line)?;
            for (k, blk) in jd.iter().enumerate() {
                if k < dof {
                    j[(0, o + k)] += blk[1];
                }
            }
        }
        Constraint::Vertical { line } => {
            let (d, jd) = sys.line_dir(*line)?;
            r[0] = d[0];
            let (o, dof) = sys.param_span(*line)?;
            for (k, blk) in jd.iter().enumerate() {
                if k < dof {
                    j[(0, o + k)] += blk[0];
                }
            }
        }
        Constraint::Parallel { a, b } => {
            let ([ax, ay], ja) = sys.line_dir(*a)?;
            let ([bx, by], jb) = sys.line_dir(*b)?;
            r[0] = ax * by - ay * bx;
            let (oa, da) = sys.param_span(*a)?;
            for (k, blk) in ja.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += by * blk[0] - bx * blk[1];
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, blk) in jb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] += ax * blk[1] - ay * blk[0];
                }
            }
        }
        Constraint::Perpendicular { a, b } => {
            let ([ax, ay], ja) = sys.line_dir(*a)?;
            let ([bx, by], jb) = sys.line_dir(*b)?;
            r[0] = ax * bx + ay * by;
            let (oa, da) = sys.param_span(*a)?;
            for (k, blk) in ja.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += bx * blk[0] + by * blk[1];
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, blk) in jb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] += ax * blk[0] + ay * blk[1];
                }
            }
        }
        Constraint::TangentLineCircle { line, circle, kind } => {
            // Signed point-line distance: cross(d, c - p0) / |d|.
            let (d, jd) = sys.line_dir(*line)?;
            let (p0, j0) = sys.point(*line, PointRole::Start)?;
            let (cx, cy, cr) = sys.circle(*circle)?;
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len < 1e-12 {
                return None;
            }
            let ux = cx - p0.x;
            let uy = cy - p0.y;
            let dist = (d[0] * uy - d[1] * ux) / len;
            let sgn = if dist >= 0.0 { 1.0 } else { -1.0 };
            // A tangent line keeps the center at distance r; the "internal"
            // flavor is kept for API symmetry and behaves identically for
            // lines.
            let _ = kind;
            r[0] = dist.abs() - cr;

            let (ol, dofl) = sys.param_span(*line)?;
            for (k, blk) in j0.iter().enumerate() {
                if k < dofl {
                    // d(dist)/dp0 = (d[1], -d[0]) / len
                    j[(0, ol + k)] += sgn * (d[1] * blk[0] - d[0] * blk[1]) / len;
                }
            }
            for (k, blk) in jd.iter().enumerate() {
                if k < dofl {
                    // d(dist)/dd = (uy, -ux)/len - dist * d / len^2
                    j[(0, ol + k)] += sgn
                        * ((uy * blk[0] - ux * blk[1]) / len
                            - dist * (d[0] * blk[0] + d[1] * blk[1]) / (len * len));
                }
            }
            let (oc, dofc) = sys.param_span(*circle)?;
            j[(0, oc)] += sgn * (-d[1]) / len; // d(dist)/dcx
            j[(0, oc + 1)] += sgn * d[0] / len; // d(dist)/dcy
            let rr = sys.radius_row(*circle);
            for (k, dv) in rr.iter().enumerate() {
                if k < dofc {
                    j[(0, oc + k)] -= dv;
                }
            }
        }
        Constraint::TangentCircles { a, b, kind } => {
            let (ax, ay, ar) = sys.circle(*a)?;
            let (bx, by, br) = sys.circle(*b)?;
            let dx = bx - ax;
            let dy = by - ay;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 1e-12 {
                return None;
            }
            let (target, dta, dtb) = match kind {
                TangentKind::External => (ar + br, 1.0, 1.0),
                TangentKind::Internal => {
                    if ar >= br {
                        (ar - br, 1.0, -1.0)
                    } else {
                        (br - ar, -1.0, 1.0)
                    }
                }
            };
            r[0] = dist - target;
            let (oa, da) = sys.param_span(*a)?;
            j[(0, oa)] += -dx / dist;
            j[(0, oa + 1)] += -dy / dist;
            let ra = sys.radius_row(*a);
            for (k, dv) in ra.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] -= dta * dv;
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            j[(0, ob)] += dx / dist;
            j[(0, ob + 1)] += dy / dist;
            let rb = sys.radius_row(*b);
            for (k, dv) in rb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] -= dtb * dv;
                }
            }
        }
        Constraint::Concentric { a, b } => {
            let (ax, ay, _) = sys.circle(*a)?;
            let (bx, by, _) = sys.circle(*b)?;
            r[0] = ax - bx;
            r[1] = ay - by;
            let (oa, _) = sys.param_span(*a)?;
            let (ob, _) = sys.param_span(*b)?;
            j[(0, oa)] += 1.0;
            j[(1, oa + 1)] += 1.0;
            j[(0, ob)] -= 1.0;
            j[(1, ob + 1)] -= 1.0;
        }
        Constraint::Length { line, value } => {
            let (len, dl) = sys.line_length(*line)?;
            r[0] = len - value;
            let (o, dof) = sys.param_span(*line)?;
            for (k, dv) in dl.iter().enumerate() {
                if k < dof {
                    j[(0, o + k)] += dv;
                }
            }
        }
        Constraint::Radius { circle, value } => {
            let (_, _, cr) = sys.circle(*circle)?;
            r[0] = cr - value;
            let (oc, dof) = sys.param_span(*circle)?;
            let rr = sys.radius_row(*circle);
            for (k, dv) in rr.iter().enumerate() {
                if k < dof {
                    j[(0, oc + k)] += dv;
                }
            }
        }
        Constraint::Distance { a, a_point, b, b_point, value } => {
            let (pa, ja) = sys.point(*a, *a_point)?;
            let (pb, jb) = sys.point(*b, *b_point)?;
            let dx = pb.x - pa.x;
            let dy = pb.y - pa.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 1e-12 {
                return None;
            }
            r[0] = dist - value;
            let (oa, da) = sys.param_span(*a)?;
            for (k, blk) in ja.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += (-dx / dist) * blk[0] + (-dy / dist) * blk[1];
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, blk) in jb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] += (dx / dist) * blk[0] + (dy / dist) * blk[1];
                }
            }
        }
        Constraint::Angle { a, b, value } => {
            let ([ax, ay], ja) = sys.line_dir(*a)?;
            let ([bx, by], jb) = sys.line_dir(*b)?;
            let la = (ax * ax + ay * ay).sqrt();
            let lb = (bx * bx + by * by).sqrt();
            if la < 1e-12 || lb < 1e-12 {
                return None;
            }
            let sin_t = (ax * by - ay * bx) / (la * lb);
            let cos_t = (ax * bx + ay * by) / (la * lb);
            let target = *value;
            // residual = sin(theta - target)
            r[0] = sin_t * target.cos() - cos_t * target.sin();

            let ct = target.cos();
            let st = target.sin();
            let nl = la * lb;
            let cross = ax * by - ay * bx;
            let dot = ax * bx + ay * by;

            // d(sin_t), d(cos_t) w.r.t. a's direction:
            let mut dsda = [0.0f64; 2];
            let mut dcda = [0.0f64; 2];
            {
                // d/dax
                dsda[0] = by / nl - cross * ax / (la * la * la * lb);
                dcda[0] = bx / nl - dot * ax / (la * la * la * lb);
                // d/day
                dsda[1] = -bx / nl - cross * ay / (la * la * la * lb);
                dcda[1] = by / nl - dot * ay / (la * la * la * lb);
            }
            let mut dsdb = [0.0f64; 2];
            let mut dcdb = [0.0f64; 2];
            {
                dsdb[0] = -ay / nl - cross * bx / (la * lb * lb * lb);
                dcdb[0] = ax / nl - dot * bx / (la * lb * lb * lb);
                dsdb[1] = ax / nl - cross * by / (la * lb * lb * lb);
                dcdb[1] = ay / nl - dot * by / (la * lb * lb * lb);
            }

            let (oa, da) = sys.param_span(*a)?;
            for (k, blk) in ja.iter().enumerate() {
                if k < da {
                    let dr_dax = dsda[0] * ct - dcda[0] * st;
                    let dr_day = dsda[1] * ct - dcda[1] * st;
                    j[(0, oa + k)] += dr_dax * blk[0] + dr_day * blk[1];
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, blk) in jb.iter().enumerate() {
                if k < db {
                    let dr_dbx = dsdb[0] * ct - dcdb[0] * st;
                    let dr_dby = dsdb[1] * ct - dcdb[1] * st;
                    j[(0, ob + k)] += dr_dbx * blk[0] + dr_dby * blk[1];
                }
            }
        }
        Constraint::FixPoint { entity, point, position } => {
            let (p, jp) = sys.point(*entity, *point)?;
            r[0] = p.x - position.0;
            r[1] = p.y - position.1;
            let (o, dof) = sys.param_span(*entity)?;
            for (k, blk) in jp.iter().enumerate() {
                if k < dof {
                    j[(0, o + k)] += blk[0];
                    j[(1, o + k)] += blk[1];
                }
            }
        }
        Constraint::EqualLength { a, b } => {
            let (la_, dla) = sys.line_length(*a)?;
            let (lb_, dlb) = sys.line_length(*b)?;
            r[0] = la_ - lb_;
            let (oa, da) = sys.param_span(*a)?;
            for (k, dv) in dla.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += dv;
                }
            }
            let (ob, db) = sys.param_span(*b)?;
            for (k, dv) in dlb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] -= dv;
                }
            }
        }
        Constraint::EqualRadius { a, b } => {
            let (_, _, ra_) = sys.circle(*a)?;
            let (_, _, rb_) = sys.circle(*b)?;
            r[0] = ra_ - rb_;
            let (oa, da) = sys.param_span(*a)?;
            let (ob, db) = sys.param_span(*b)?;
            let ra = sys.radius_row(*a);
            let rb = sys.radius_row(*b);
            for (k, dv) in ra.iter().enumerate() {
                if k < da {
                    j[(0, oa + k)] += dv;
                }
            }
            for (k, dv) in rb.iter().enumerate() {
                if k < db {
                    j[(0, ob + k)] -= dv;
                }
            }
        }
    }
    Some(Eq { r, j })
}

/// Solve the sketch's constraint system in place.
pub fn solve(sketch: &mut Sketch) -> crate::Result<SolveReportInner> {
    let constraints = sketch.constraints.clone();
    let mut sys = System::build(sketch);
    let dof = sys.x.len();
    let equations: usize = constraints.iter().map(|c| c.equation_count()).sum();

    if constraints.is_empty() {
        return Ok(SolveReportInner {
            status: SolveStatus::Converged,
            residual: 0.0,
            iterations: 0,
            dof,
            equations,
        });
    }

    let mut broken = false;
    let (r, j) = assemble(&sys, &constraints, &mut broken);
    if broken {
        return Ok(SolveReportInner {
            status: SolveStatus::BrokenReferences,
            residual: r.norm(),
            iterations: 0,
            dof,
            equations,
        });
    }

    const MAX_ITER: usize = 200;
    const TOL: f64 = 1e-10;
    let mut lambda = 1e-3;
    let mut r = r;
    let mut j = j;
    let mut residual = r.norm();
    let mut iterations = 0usize;

    for it in 0..MAX_ITER {
        if residual < TOL {
            break;
        }
        iterations = it + 1;
        let jtj = j.transpose() * &j;
        // Gauss-Newton normal equations: (JtJ + lambda D) delta = -Jt r.
        let jtr = j.transpose() * &r;
        let neg_jtr = -jtr.clone();
        let mut a = jtj;
        for k in 0..a.nrows() {
            let d = a[(k, k)].abs().max(1e-10);
            a[(k, k)] += lambda * d;
        }
        let delta = match a.lu().solve(&neg_jtr) {
            Some(d) => d,
            None => {
                lambda *= 10.0;
                if lambda > 1e12 {
                    break;
                }
                continue;
            }
        };

        let trial: Vec<f64> = sys
            .x
            .iter()
            .zip(delta.iter())
            .map(|(a, b)| a + b)
            .collect();
        let saved = sys.x.clone();
        sys.set_params(trial);
        let mut broken2 = false;
        let (r2, j2) = assemble(&sys, &constraints, &mut broken2);
        let res2 = r2.norm();
        if !broken2 && res2 < residual {
            r = r2;
            j = j2;
            residual = res2;
            lambda = (lambda / 3.0).max(1e-14);
        } else {
            sys.set_params(saved);
            lambda *= 10.0;
            if lambda > 1e12 {
                break;
            }
        }
    }

    let status = if residual < TOL {
        SolveStatus::Converged
    } else {
        SolveStatus::MaxIterations
    };

    // Write the solution back.
    for (id, entity) in sketch.entities.iter_mut() {
        if let Some((offset, _)) = sys.spans.get(id) {
            entity.unpack(&sys.x, *offset);
        }
    }

    Ok(SolveReportInner {
        status,
        residual,
        iterations,
        dof,
        equations,
    })
}

fn assemble(
    sys: &System,
    constraints: &[Constraint],
    broken: &mut bool,
) -> (DVector<f64>, DMatrix<f64>) {
    let n = sys.x.len();
    let m: usize = constraints.iter().map(|c| c.equation_count()).sum();
    let mut r = DVector::zeros(m);
    let mut j = DMatrix::zeros(m, n);
    let mut row = 0;
    for c in constraints {
        match build_equation(sys, c) {
            Some(eq) => {
                for (k, v) in eq.r.iter().enumerate() {
                    r[row + k] = *v;
                }
                for i in 0..c.equation_count() {
                    for k in 0..n {
                        j[(row + i, k)] = eq.j[(i, k)];
                    }
                }
                row += c.equation_count();
            }
            None => {
                *broken = true;
                row += c.equation_count();
            }
        }
    }
    (r, j)
}
