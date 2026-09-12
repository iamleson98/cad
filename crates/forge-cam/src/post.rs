//! G-code post-processor: `Vec<Toolpath>` → Fanuc-style 3-axis program.
//!
//! Design goals: deterministic output (stable ordering, fixed formatting),
//! modal feeds (only emit F when it changes), drill cycles as native G81 /
//! G83 with R-planes, and a program header/footer that any hobby-to-pro
//! controller accepts. The output is intentionally controller-generic
//! (no vendor cycles beyond drill, no tool-length compensation assumptions
//! — the post emits G43 H with the tool number, which most controllers
//! accept and ignore when unset).

use crate::path::{Move, Toolpath};

/// Post-processor options.
#[derive(Debug, Clone)]
pub struct PostOptions {
    /// Emit line numbers `N10 ...` (useful for older controllers).
    pub line_numbers: bool,
    /// Line-number step.
    pub line_step: u32,
    /// Fixture offset emitted in the header (e.g. "G54").
    pub fixture: String,
    /// Comment out toolpath labels (program readability).
    pub comments: bool,
    /// Arc output is not used in v1 (all moves linear); reserved.
    pub _reserved: (),
}

impl Default for PostOptions {
    fn default() -> Self {
        PostOptions {
            line_numbers: false,
            line_step: 10,
            fixture: "G54".into(),
            comments: true,
            _reserved: (),
        }
    }
}

/// Post-process toolpaths into a complete G-code program.
///
/// Operations without any cutting moves (empty or rapid-only) are skipped.
pub fn post(paths: &[Toolpath], opts: &PostOptions) -> String {
    let mut out = GcodeWriter::new(opts.clone());
    out.header();
    for path in paths {
        if !path.moves.iter().any(|m| m.cuts()) {
            continue;
        }
        out.operation(path);
    }
    out.footer();
    out.finish()
}

/// Incremental G-code line writer with modal state.
struct GcodeWriter {
    opts: PostOptions,
    lines: Vec<String>,
    /// Modal motion mode + last feed to suppress redundant codes.
    motion: Option<&'static str>,
    last_feed: Option<f64>,
    /// Current tool (0 = none loaded).
    tool: u32,
    line_no: u32,
}

impl GcodeWriter {
    fn new(opts: PostOptions) -> Self {
        GcodeWriter {
            opts,
            lines: Vec::new(),
            motion: None,
            last_feed: None,
            tool: 0,
            line_no: 0,
        }
    }

    fn push(&mut self, body: String) {
        let mut line = body;
        if self.opts.line_numbers {
            self.line_no += self.opts.line_step;
            line = format!("N{} {}", self.line_no, line);
        }
        self.lines.push(line);
    }

    fn comment(&mut self, text: &str) {
        if self.opts.comments {
            let sanitized: String = text
                .chars()
                .map(|c| if c == '(' || c == ')' { '[' } else { c })
                .collect();
            self.push(format!("({sanitized})"));
        }
    }

    fn header(&mut self) {
        self.push("%".into());
        self.push("O1000 (FORGECAM PROGRAM)".into());
        self.push("G21 (MM)".into()); // metric
        self.push("G90 (ABSOLUTE)".into());
        self.push(self.opts.fixture.clone());
        self.push("G17 (XY PLANE)".into());
        self.push("M5 (SPINDLE OFF)".into());
    }

    fn footer(&mut self) {
        // Retract + park.
        self.push("G0 Z50.000".into());
        self.push("M5 (SPINDLE OFF)".into());
        self.push("M9 (COOLANT OFF)".into());
        self.push("G53 G0 Z0.000 (PARK)".into());
        self.push("M30 (PROGRAM END)".into());
        self.push("%".into());
    }

    fn operation(&mut self, path: &Toolpath) {
        self.comment(&format!("OP: {}", path.label));
        self.comment(&format!(
            "CUT {:.1}mm RAPID {:.1}mm TIME {:.1}min",
            path.cut_length(),
            path.rapid_length(),
            path.time_minutes()
        ));
        // Tool change if needed.
        if self.tool != path.tool_number {
            self.push("G53 G0 Z0.000 (SAFE TOOL CHANGE)".into());
            self.push(format!("T{} M6", path.tool_number));
            self.tool = path.tool_number;
            self.push(format!("G43 H{}", path.tool_number));
        }
        // Spindle.
        self.push(format!("S{:.0} M3", path.feeds.rpm));
        // Emit moves.
        let mut first = true;
        for m in &path.moves {
            match *m {
                Move::Rapid { p } => self.rapid(p),
                Move::Feed { p } => self.feed(p, path.feeds.feed),
                Move::Plunge { p } => self.feed(p, path.feeds.plunge),
                Move::Drill {
                    p,
                    top,
                    depth,
                    peck,
                } => {
                    self.drill_cycle(p, top, depth, peck, path.feeds.plunge, first);
                }
            }
            first = false;
        }
        // Coolant / spindle stays per controller; we switch off at the end
        // of the program (footer) to keep ops chainable.
    }

    fn rapid(&mut self, p: [f64; 3]) {
        if self.motion != Some("G0") {
            self.motion = Some("G0");
            self.push(format!("G0 {}", fmt_xyz(p)));
        } else {
            self.push(fmt_xyz(p));
        }
    }

    fn feed(&mut self, p: [f64; 3], f: f64) {
        let mut line = String::new();
        if self.motion != Some("G1") {
            self.motion = Some("G1");
            line.push_str("G1 ");
        }
        line.push_str(&fmt_xyz(p));
        // Modal feed: only when changed.
        if self.last_feed != Some(f) {
            line.push_str(&format!(" F{:.0}", f));
            self.last_feed = Some(f);
        }
        self.push(line);
    }

    fn drill_cycle(
        &mut self,
        p: [f64; 3],
        top: f64,
        depth: f64,
        peck: Option<f64>,
        plunge: f64,
        _first: bool,
    ) {
        // Position above the hole first (rapid XY + R-plane).
        self.rapid([p[0], p[1], top]);
        let cycle = match peck {
            Some(_) => "G83",
            None => "G81",
        };
        let mut line = format!(
            "{cycle} X{} Y{} Z{} R{}",
            f3(p[0]),
            f3(p[1]),
            f3(depth),
            f3(top)
        );
        if let Some(peck) = peck {
            line.push_str(&format!(" Q{}", f3(peck)));
        }
        if self.last_feed != Some(plunge) {
            line.push_str(&format!(" F{:.0}", plunge));
            self.last_feed = Some(plunge);
        }
        self.push(line);
        // G80 cancels the cycle.
        self.push("G80".into());
        self.motion = None;
    }

    fn finish(self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

/// `X… Y… Z…` triple with only changed-axis suppression is overkill for
/// v1 (controllers handle full triples); emit all three, fixed 3 decimals.
fn fmt_xyz(p: [f64; 3]) -> String {
    format!("X{} Y{} Z{}", f3(p[0]), f3(p[1]), f3(p[2]))
}

/// Fixed 3-decimals, no trailing zeros beyond the format.
fn f3(v: f64) -> String {
    if v == 0.0 {
        "0.000".into()
    } else {
        format!("{:.3}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Feeds;

    fn sample_path() -> Toolpath {
        let mut tp = Toolpath::new(
            "Rough 6.0mm @ 1.0 DOC",
            2,
            3.0,
            Feeds {
                feed: 900.0,
                plunge: 200.0,
                rapid: 4000.0,
                rpm: 11000.0,
            },
        );
        tp.rapid([0.0, 0.0, 15.0]);
        tp.plunge([0.0, 0.0, 2.0]);
        tp.feed([10.0, 0.0, 2.0]);
        tp.feed([10.0, 4.0, 2.0]);
        tp.rapid([10.0, 4.0, 15.0]);
        tp
    }

    #[test]
    fn header_footer_and_tool_change_present() {
        let g = post(&[sample_path()], &PostOptions::default());
        assert!(g.starts_with("%\n"));
        assert!(g.contains("O1000"));
        assert!(g.contains("G21"));
        assert!(g.contains("G90"));
        assert!(g.contains("G54"));
        assert!(g.contains("T2 M6"));
        assert!(g.contains("G43 H2"));
        assert!(g.contains("S11000 M3"));
        assert!(g.contains("M30"));
        assert!(g.trim_end().ends_with('%'));
        assert!(g.contains("(OP: Rough 6.0mm @ 1.0 DOC)"));
    }

    #[test]
    fn modal_feed_only_emitted_on_change() {
        let g = post(&[sample_path()], &PostOptions::default());
        // Plunge F200 once, feed F900 once → exactly two F-words.
        let f_count = g.lines().filter(|l| l.contains(" F")).count();
        assert_eq!(f_count, 2, "F words:\n{g}");
    }

    #[test]
    fn drill_cycles_use_g81_and_g83() {
        let mut tp = Toolpath::new("Drill 5.0mm x2", 5, 2.5, Feeds::default());
        tp.drill([1.0, 1.0, 0.0], 2.0, -8.0, None);
        tp.drill([4.0, 1.0, 0.0], 2.0, -8.0, Some(2.0));
        let g = post(&[tp], &PostOptions::default());
        assert!(g.contains("G81 X1.000 Y1.000 Z-8.000 R2.000"));
        assert!(g.contains("G83 X4.000 Y1.000 Z-8.000 R2.000 Q2.000"));
        assert_eq!(g.matches("G80").count(), 2);
        assert!(g.contains("T5 M6"));
    }

    #[test]
    fn line_numbers_when_enabled() {
        let g = post(
            &[sample_path()],
            &PostOptions {
                line_numbers: true,
                ..Default::default()
            },
        );
        assert!(g.contains("N10 "));
        assert!(g.contains("N20 "));
    }

    #[test]
    fn deterministic_output() {
        let a = post(&[sample_path()], &PostOptions::default());
        let b = post(&[sample_path()], &PostOptions::default());
        assert_eq!(a, b);
    }

    #[test]
    fn empty_operations_are_skipped() {
        let mut empty = Toolpath::new("rapid only", 9, 1.0, Feeds::default());
        empty.rapid([0.0, 0.0, 10.0]); // no cutting moves → skipped
        let mut paths = vec![
            sample_path(),
            Toolpath::new("really empty", 9, 1.0, Feeds::default()),
        ];
        paths.push(empty);
        let g = post(&paths, &PostOptions::default());
        assert!(!g.contains("T9"));
    }

    #[test]
    fn multi_op_tool_change_only_when_tool_differs() {
        let mut a = sample_path();
        a.tool_number = 1;
        let mut b = sample_path();
        b.tool_number = 1; // same tool
        b.label = "Second pass".into();
        let g = post(&[a, b], &PostOptions::default());
        assert_eq!(g.matches("M6").count(), 1, "one tool change expected:\n{g}");
    }
}
