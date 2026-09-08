//! Tessellation quality configuration.
//!
//! The same settings drive:
//! - evaluation-time tessellation (how fine circles / NURBS are meshed),
//! - export-time mesh density for STL / OBJ / glTF
//!   (FR-IO-04: "configurable mesh tessellation density").

use serde::{Deserialize, Serialize};

/// How finely curved geometry is approximated by triangles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TessellationConfig {
    /// Maximum allowed deviation between the true curve/surface and the
    /// approximating chord, in millimeters (chordal tolerance).
    pub chord_tolerance_mm: f64,
    /// Maximum angular step when discretizing arcs and revolutions,
    /// in radians. Keeps tiny circles from becoming degenerate triangles.
    pub max_segment_angle_rad: f64,
    /// Hard cap on the number of segments per full circle. Prevents
    /// pathological blow-up when the tolerance is tiny.
    pub max_segments_per_circle: usize,
    /// Minimum number of segments per full circle (keeps quality sane for
    /// very small radii).
    pub min_segments_per_circle: usize,
}

impl Default for TessellationConfig {
    fn default() -> Self {
        Self {
            chord_tolerance_mm: 0.05,
            max_segment_angle_rad: 12.0_f64.to_radians(),
            max_segments_per_circle: 256,
            min_segments_per_circle: 16,
        }
    }
}

impl TessellationConfig {
    /// Number of segments to use for a full circle of radius `r`,
    /// respecting the chordal tolerance: the sagitta of a chord spanning
    /// angle `a` is `r * (1 - cos(a/2))`, so
    /// `a <= 2 * acos(1 - tol/r)` (clamped when `tol >= r`).
    pub fn segments_for_circle(&self, radius: f64) -> usize {
        if radius <= 1e-12 {
            return self.min_segments_per_circle;
        }
        let ratio = 1.0 - self.chord_tolerance_mm / radius;
        let max_angle = if ratio <= -1.0 {
            std::f64::consts::PI
        } else {
            2.0 * ratio.clamp(-1.0, 1.0).acos()
        };
        let by_tolerance = std::f64::consts::TAU / max_angle;
        let by_angle = std::f64::consts::TAU / self.max_segment_angle_rad;
        let n = by_tolerance.min(by_angle).ceil().max(0.0) as usize;
        n.clamp(self.min_segments_per_circle, self.max_segments_per_circle)
    }

    /// Number of steps to use when revolving through `angle_rad`.
    pub fn steps_for_arc(&self, radius: f64, angle_rad: f64) -> usize {
        let full = self.segments_for_circle(radius);
        let frac = angle_rad.abs() / std::f64::consts::TAU;
        ((full as f64) * frac).ceil().max(2.0) as usize
    }

    /// A coarser preset for interactive previews.
    pub const PREVIEW: Self = Self {
        chord_tolerance_mm: 0.2,
        max_segment_angle_rad: 20.0_f64.to_radians(),
        max_segments_per_circle: 96,
        min_segments_per_circle: 12,
    };

    /// A fine preset for final export.
    pub const EXPORT: Self = Self {
        chord_tolerance_mm: 0.01,
        max_segment_angle_rad: 6.0_f64.to_radians(),
        max_segments_per_circle: 512,
        min_segments_per_circle: 24,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_segments_respect_bounds() {
        let cfg = TessellationConfig::default();
        let n = cfg.segments_for_circle(50.0);
        assert!(n >= cfg.min_segments_per_circle && n <= cfg.max_segments_per_circle);
        // Larger circles need more segments for the same tolerance.
        assert!(cfg.segments_for_circle(500.0) >= n);
        // A degenerate radius still yields a sane minimum.
        assert_eq!(cfg.segments_for_circle(0.0), cfg.min_segments_per_circle);
    }

    #[test]
    fn arc_steps_at_least_two() {
        let cfg = TessellationConfig::default();
        assert!(cfg.steps_for_arc(10.0, 1e-6) >= 2);
    }
}
