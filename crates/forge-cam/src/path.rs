//! Toolpath representation: moves, operations, statistics.
//!
//! A [`Toolpath`] is a flat, ordered list of [`Move`]s (rapid / cut /
//! plunge / drill cycles) in **machine coordinates** (the setup's WCS), plus
//! derived statistics (cut/rapid length, estimated time). Strategies emit
//! toolpaths; the G-code post consumes them; the viewport renders them as
//! polylines split by move kind.

use serde::{Deserialize, Serialize};

/// One tool motion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Move {
    /// Rapid traverse (G0) — never inside material.
    Rapid { p: [f64; 3] },
    /// Linear feed (G1) — cutting.
    Feed { p: [f64; 3] },
    /// Plunge (G1 at plunge feed) — entering material vertically.
    Plunge { p: [f64; 3] },
    /// Drill cycle (G81 single-shot / G83 peck; R = `top`).
    Drill {
        /// Hole axis XY.
        p: [f64; 3],
        /// R-plane / top of hole.
        top: f64,
        /// Final hole depth (Z).
        depth: f64,
        /// Peck increment (`None` = G81).
        peck: Option<f64>,
    },
}

impl Move {
    /// Target point of the move.
    pub fn target(&self) -> [f64; 3] {
        match *self {
            Move::Rapid { p } | Move::Feed { p } | Move::Plunge { p } => p,
            Move::Drill { p, .. } => p,
        }
    }

    /// True when the move cuts material (feed/plunge/drill).
    pub fn cuts(&self) -> bool {
        !matches!(self, Move::Rapid { .. })
    }
}

/// Feeds used for time estimation and the post.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Feeds {
    /// XY cutting feed (mm/min).
    pub feed: f64,
    /// Z plunge feed (mm/min).
    pub plunge: f64,
    /// Rapid traverse rate (mm/min).
    pub rapid: f64,
    /// Spindle speed (RPM).
    pub rpm: f64,
}

impl Default for Feeds {
    fn default() -> Self {
        Feeds {
            feed: 1200.0,
            plunge: 300.0,
            rapid: 5000.0,
            rpm: 12000.0,
        }
    }
}

/// One machining operation's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toolpath {
    /// Ordered moves.
    pub moves: Vec<Move>,
    /// Feeds/rpm for this operation.
    pub feeds: Feeds,
    /// Tool pocket number (post emits T{n} M6).
    pub tool_number: u32,
    /// Tool radius (mm) — used by simulation & rendering.
    pub tool_radius: f64,
    /// Human label ("Rough 6.0mm @ 1.5 DOC").
    pub label: String,
}

impl Toolpath {
    /// Empty path with a label.
    pub fn new(label: impl Into<String>, tool_number: u32, tool_radius: f64, feeds: Feeds) -> Self {
        Toolpath {
            moves: Vec::new(),
            feeds,
            tool_number,
            tool_radius,
            label: label.into(),
        }
    }

    /// Push a rapid move.
    pub fn rapid(&mut self, p: [f64; 3]) {
        self.moves.push(Move::Rapid { p });
    }

    /// Push a feed move.
    pub fn feed(&mut self, p: [f64; 3]) {
        self.moves.push(Move::Feed { p });
    }

    /// Push a plunge move.
    pub fn plunge(&mut self, p: [f64; 3]) {
        self.moves.push(Move::Plunge { p });
    }

    /// Push a drill cycle.
    pub fn drill(&mut self, p: [f64; 3], top: f64, depth: f64, peck: Option<f64>) {
        self.moves.push(Move::Drill {
            p,
            top,
            depth,
            peck,
        });
    }

    /// Total XYZ length of cutting moves (mm).
    pub fn cut_length(&self) -> f64 {
        let mut total = 0.0;
        let mut prev: Option<[f64; 3]> = None;
        for m in &self.moves {
            let t = m.target();
            if let (Some(a), true) = (prev, m.cuts()) {
                total += dist3(a, t);
            }
            prev = Some(t);
        }
        total
    }

    /// Total length of rapid moves (mm).
    pub fn rapid_length(&self) -> f64 {
        let mut total = 0.0;
        let mut prev: Option<[f64; 3]> = None;
        for m in &self.moves {
            let t = m.target();
            if let (Some(a), false) = (prev, m.cuts()) {
                total += dist3(a, t);
            }
            prev = Some(t);
        }
        total
    }

    /// Estimated machine time (minutes) from feeds and rapid rate.
    /// Drill cycles: plunge depth at plunge feed + retract at rapid.
    pub fn time_minutes(&self) -> f64 {
        let f = self.feeds;
        let mut t = 0.0;
        let mut prev: Option<[f64; 3]> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid { p } => {
                    if let Some(a) = prev {
                        t += dist3(a, p) / f.rapid.max(1.0);
                    }
                    prev = Some(p);
                }
                Move::Feed { p } => {
                    if let Some(a) = prev {
                        t += dist3(a, p) / f.feed.max(1.0);
                    }
                    prev = Some(p);
                }
                Move::Plunge { p } => {
                    if let Some(a) = prev {
                        t += dist3(a, p) / f.plunge.max(1.0);
                    }
                    prev = Some(p);
                }
                Move::Drill {
                    p,
                    top,
                    depth,
                    peck,
                } => {
                    let d = (top - depth).max(0.0);
                    if let Some(peck) = peck {
                        // G83: total feed travel = depth + re-cut overlaps.
                        let n = (d / peck).ceil().max(1.0);
                        t += n * peck / f.plunge.max(1.0);
                    } else {
                        t += d / f.plunge.max(1.0);
                    }
                    // Retract to R-plane at rapid.
                    t += d / f.rapid.max(1.0);
                    prev = Some([p[0], p[1], top]);
                }
            }
        }
        t
    }

    /// Cutting-move polyline segments for rendering: (start, end) pairs
    /// (drill cycles render as their axis line).
    pub fn cut_segments(&self) -> Vec<([f64; 3], [f64; 3])> {
        let mut segs = Vec::new();
        let mut prev: Option<[f64; 3]> = None;
        for m in &self.moves {
            if let Move::Drill { p, top, depth, .. } = *m {
                segs.push(([p[0], p[1], top], [p[0], p[1], depth]));
                continue;
            }
            let t = m.target();
            if let (Some(a), true) = (prev, m.cuts()) {
                segs.push((a, t));
            }
            prev = Some(t);
        }
        segs
    }

    /// Rapid-move polyline segments for rendering.
    pub fn rapid_segments(&self) -> Vec<([f64; 3], [f64; 3])> {
        let mut segs = Vec::new();
        let mut prev: Option<[f64; 3]> = None;
        for m in &self.moves {
            if let Move::Drill { p, top, .. } = *m {
                if let Some(a) = prev {
                    segs.push((a, [p[0], p[1], top]));
                }
                continue;
            }
            let t = m.target();
            if let (Some(a), false) = (prev, m.cuts()) {
                segs.push((a, t));
            }
            prev = Some(t);
        }
        segs
    }

    /// Z bounds of all targets (for stock checks / stats).
    pub fn z_bounds(&self) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for m in &self.moves {
            match *m {
                Move::Drill { depth, .. } => {
                    lo = lo.min(depth);
                    hi = hi.max(depth);
                    any = true;
                }
                _ => {
                    let z = m.target()[2];
                    lo = lo.min(z);
                    hi = hi.max(z);
                    any = true;
                }
            }
        }
        any.then_some((lo, hi))
    }

    /// Z bounds of *cutting* moves only (excludes rapids to safe Z).
    pub fn cut_z_bounds(&self) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for m in &self.moves {
            if !m.cuts() {
                continue;
            }
            match *m {
                Move::Drill { depth, .. } => {
                    lo = lo.min(depth);
                    hi = hi.max(depth);
                    any = true;
                }
                _ => {
                    let z = m.target()[2];
                    lo = lo.min(z);
                    hi = hi.max(z);
                    any = true;
                }
            }
        }
        any.then_some((lo, hi))
    }
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_time_accounting() {
        let mut tp = Toolpath::new(
            "test",
            1,
            3.0,
            Feeds {
                feed: 600.0,
                plunge: 120.0,
                rapid: 6000.0,
                rpm: 1.0,
            },
        );
        tp.rapid([0.0, 0.0, 10.0]);
        tp.plunge([0.0, 0.0, 0.0]); // 10mm plunge @ 120
        tp.feed([30.0, 0.0, 0.0]); // 30mm cut @ 600
        tp.rapid([30.0, 0.0, 10.0]); // 10mm rapid @ 6000
        assert!((tp.cut_length() - 40.0).abs() < 1e-9); // 10 plunge + 30 feed
                                                        // rapid_length: only the exit move (10mm) — the first rapid has
                                                        // no prior point, and the plunge is a cut.
        assert!((tp.rapid_length() - 10.0).abs() < 1e-9);
        let t = tp.time_minutes();
        let want = 10.0 / 120.0 + 30.0 / 600.0 + 10.0 / 6000.0;
        assert!((t - want).abs() < 1e-9, "t {t} want {want}");
    }

    #[test]
    fn drill_time_includes_peck_travel() {
        let mut tp = Toolpath::new("drill", 5, 2.5, Feeds::default());
        tp.drill([1.0, 1.0, 0.0], 2.0, -8.0, Some(2.0)); // 10mm @ 2mm pecks
        let f = tp.feeds;
        let n = 5.0; // pecks
        let want = n * 2.0 / f.plunge + 10.0 / f.rapid;
        assert!((tp.time_minutes() - want).abs() < 1e-9);
    }

    #[test]
    fn segment_split_by_kind() {
        let mut tp = Toolpath::new("seg", 1, 3.0, Feeds::default());
        tp.rapid([0.0, 0.0, 5.0]);
        tp.plunge([0.0, 0.0, 0.0]);
        tp.feed([5.0, 0.0, 0.0]);
        tp.rapid([10.0, 10.0, 5.0]);
        assert_eq!(tp.cut_segments().len(), 2); // plunge + feed
        assert_eq!(tp.rapid_segments().len(), 1); // feed → rapid out
        assert_eq!(tp.z_bounds(), Some((0.0, 5.0)));
    }
}
