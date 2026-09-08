//! Uniform cubic B-spline evaluation (the "NURBS spline" of v0.1).
//!
//! v0.1 represents spline entities as uniform non-rational cubic B-splines
//! over their control polygon. This is the subset of NURBS with uniform
//! knots and unit weights; upgrading to full NURBS (non-uniform knots,
//! rational weights) is a Phase-3 roadmap item with the same entity
//! interface.

use forge_core::Point2;

/// Evaluate a point on a uniform cubic B-spline.
///
/// `t` spans `[0, control.len() - 3]`; the segment `i = floor(t)` blends
/// the four control points `control[i .. i+4]` with the classic uniform
/// cubic basis.
pub fn eval_point(control: &[Point2], t: f64) -> Point2 {
    let n = control.len();
    match n {
        0 => return Point2::origin(),
        1 => return control[0],
        2 | 3 => {
            // Linear fallback for degenerate control polygons.
            let idx = ((t.max(0.0) as usize).min(n - 2), t);
            let a = control[idx.0];
            let b = control[idx.0 + 1];
            let f = (idx.1 - idx.0 as f64).clamp(0.0, 1.0);
            return Point2::from(a.coords * (1.0 - f) + b.coords * f);
        }
        _ => {}
    }

    let seg = (t.floor() as usize).clamp(0, n - 4);
    let u = (t - seg as f64).clamp(0.0, 1.0);
    let p = &control[seg..seg + 4];

    // Uniform cubic B-spline basis.
    let b0 = (1.0 - u).powi(3) / 6.0;
    let b1 = (3.0 * u.powi(3) - 6.0 * u.powi(2) + 4.0) / 6.0;
    let b2 = (-3.0 * u.powi(3) + 3.0 * u.powi(2) + 3.0 * u + 1.0) / 6.0;
    let b3 = u.powi(3) / 6.0;

    Point2::from(p[0].coords * b0 + p[1].coords * b1 + p[2].coords * b2 + p[3].coords * b3)
}

/// Sample the spline into a polyline with `samples` intervals.
pub fn sample(control: &[Point2], samples: usize) -> Vec<Point2> {
    if control.len() < 2 {
        return control.to_vec();
    }
    let t_max = (control.len() as i64 - 3).max(1) as f64;
    let n = samples.max(1);
    (0..=n)
        .map(|k| eval_point(control, t_max * k as f64 / n as f64))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn spline_passes_near_control_points() {
        // A uniform cubic B-spline approximates, not interpolates; but the
        // curve must stay inside the convex hull of the control polygon.
        let control = vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
        ];
        let pts = sample(&control, 40);
        assert_eq!(pts.len(), 41);
        for p in &pts {
            assert!(p.x >= -1e-9 && p.x <= 10.0 + 1e-9);
            assert!(p.y >= -1e-9 && p.y <= 10.0 + 1e-9);
        }
    }

    #[test]
    fn spline_endpoints() {
        let control = vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 2.0),
            Point2::new(4.0, 2.0),
            Point2::new(5.0, 0.0),
            Point2::new(8.0, 0.0),
            Point2::new(9.0, -2.0),
        ];
        let a = eval_point(&control, 0.0);
        let b = eval_point(&control, 3.0);
        // Uniform cubic: endpoints are (P0+4P1+P2)/6 and
        // (P_{n-3}+4P_{n-2}+P_{n-1})/6.
        assert_abs_diff_eq!(a.x, (0.0 + 4.0 + 4.0) / 6.0, epsilon = 1e-12);
        assert_abs_diff_eq!(a.y, (0.0 + 8.0 + 2.0) / 6.0, epsilon = 1e-12);
        assert_abs_diff_eq!(b.x, (5.0 + 4.0 * 8.0 + 9.0) / 6.0, epsilon = 1e-12);
        assert_abs_diff_eq!(b.y, (0.0 + 0.0 - 2.0) / 6.0, epsilon = 1e-12);
    }
}
