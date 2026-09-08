//! Lucide icon glyphs (generated file - do not hand-edit).
//!
//! Constants are the private-use-area codepoints of the Lucide icon
//! font (`assets/lucide.ttf`, ISC license, see
//! `assets/licenses/Lucide-ISC.txt`), installed as the `icons` font
//! family by [`crate::theme::install_fonts`].
//!
//! Regenerate with `python3 scripts/gen_icons.py` after swapping
//! `assets/lucide.css` / `assets/lucide.ttf` for a newer Lucide
//! release. The script rejects unknown icon names, so upgrades can
//! never silently regress into tofu boxes.
//!
//! Glyphs render via the helpers below, e.g.
//! `icons::glyph(icons::UNDO)` or `icons::icon_label(icons::BOX, "Box")`.

use egui::{Color32, FontFamily, FontId, TextFormat};

/// The icon font family name (must match `crate::theme`).
pub const FAMILY: &str = "icons";

/// The icon font family handle.
#[must_use]
pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

/// One icon glyph as [`egui::RichText`] (default size 15).
#[must_use]
pub fn glyph(code: char) -> egui::RichText {
    egui::RichText::new(code).family(family())
}

/// One icon glyph at a custom size.
#[must_use]
pub fn sized(code: char, size: f32) -> egui::RichText {
    egui::RichText::new(code).family(family()).size(size)
}

/// One icon glyph at a custom size and color.
#[must_use]
pub fn colored(code: char, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(code)
        .family(family())
        .size(size)
        .color(color)
}

/// Icon + label as a mixed-font layout job for buttons, menu items,
/// combos and list rows (icons family for the glyph, proportional for
/// the text). Colors stay unspecified so widget styling applies.
#[must_use]
pub fn icon_label(code: char, label: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let text = |font: FontId| TextFormat {
        font_id: font,
        color: Color32::PLACEHOLDER,
        ..Default::default()
    };
    job.append(&code.to_string(), 0.0, text(FontId::new(15.0, family())));
    job.append("  ", 0.0, text(FontId::new(13.0, FontFamily::Proportional)));
    job.append(
        label,
        0.0,
        text(FontId::new(13.0, FontFamily::Proportional)),
    );
    job
}

// ----------------------------------------------------------------------------
// Glyph constants (lucide slug in the comment).

/// lucide "hammer"
pub const HAMMER: char = '\u{e0f0}';

/// lucide "undo-2"
pub const UNDO: char = '\u{e2a1}';

/// lucide "redo-2"
pub const REDO: char = '\u{e2a0}';

/// lucide "box"
pub const BOX: char = '\u{e065}';

/// lucide "circle"
pub const SPHERE: char = '\u{e07a}';

/// lucide "cylinder"
pub const CYLINDER: char = '\u{e52a}';

/// lucide "cone"
pub const CONE: char = '\u{e528}';

/// lucide "torus"
pub const TORUS: char = '\u{e534}';

/// lucide "plus"
pub const PLUS: char = '\u{e141}';

/// lucide "pencil-ruler"
pub const SKETCH: char = '\u{e4f6}';

/// lucide "egg"
pub const ELLIPSE: char = '\u{e25d}';

/// lucide "square-pen"
pub const SKETCH_ON_FACE: char = '\u{e176}';

/// lucide "arrow-up-from-line"
pub const EXTRUDE: char = '\u{e45f}';

/// lucide "circle-minus"
pub const DRILL: char = '\u{e082}';

/// lucide "combine"
pub const COMBINE: char = '\u{e451}';

/// lucide "squares-unite"
pub const UNION: char = '\u{e65f}';

/// lucide "squares-subtract"
pub const SUBTRACT: char = '\u{e65e}';

/// lucide "squares-intersect"
pub const INTERSECT: char = '\u{e65d}';

/// lucide "squares-exclude"
pub const EXCLUDE: char = '\u{e65c}';

/// lucide "move-3d"
pub const MOVE_3D: char = '\u{e2e5}';

/// lucide "rotate-3d"
pub const ROTATE_3D: char = '\u{e2ea}';

/// lucide "rotate-cw"
pub const REVOLVE: char = '\u{e14d}';

/// lucide "flip-horizontal"
pub const MIRROR: char = '\u{e361}';

/// lucide "columns-3"
pub const LINEAR_PATTERN: char = '\u{e09d}';

/// lucide "life-buoy"
pub const CIRCULAR_PATTERN: char = '\u{e107}';

/// lucide "square-dashed"
pub const DATUM: char = '\u{e1cb}';

/// lucide "package"
pub const IMPORTED_MESH: char = '\u{e12d}';

/// lucide "axis-3d"
pub const AXIS_3D: char = '\u{e2fe}';

/// lucide "maximize"
pub const FIT: char = '\u{e116}';

/// lucide "camera"
pub const CAMERA: char = '\u{e068}';

/// lucide "grid-3x3"
pub const GRID: char = '\u{e0ed}';

/// lucide "vector-square"
pub const EDGES: char = '\u{e681}';

/// lucide "contrast"
pub const SHADED: char = '\u{e0a1}';

/// lucide "cuboid"
pub const WIREFRAME: char = '\u{e529}';

/// lucide "scan"
pub const XRAY: char = '\u{e257}';

/// lucide "slice"
pub const SECTION: char = '\u{e2f0}';

/// lucide "ruler"
pub const MEASURE: char = '\u{e14f}';

/// lucide "eye"
pub const EYE: char = '\u{e0be}';

/// lucide "eye-off"
pub const EYE_OFF: char = '\u{e0bf}';

/// lucide "square-mouse-pointer"
pub const PICK: char = '\u{e202}';

/// lucide "mouse-pointer-click"
pub const PICK_CLICK: char = '\u{e124}';

/// lucide "save"
pub const SAVE: char = '\u{e151}';

/// lucide "file-output"
pub const EXPORT: char = '\u{e0cc}';

/// lucide "file-input"
pub const IMPORT: char = '\u{e0c9}';

/// lucide "command"
pub const COMMAND: char = '\u{e09e}';

/// lucide "activity"
pub const ACTIVITY: char = '\u{e038}';

/// lucide "gauge"
pub const FPS: char = '\u{e1bf}';

/// lucide "triangle"
pub const TRIANGLE: char = '\u{e192}';

/// lucide "boxes"
pub const BODIES: char = '\u{e2d0}';

/// lucide "timer"
pub const TIMER: char = '\u{e1e0}';

/// lucide "list-tree"
pub const FEATURES: char = '\u{e40d}';

/// lucide "variable"
pub const PARAMS: char = '\u{e478}';

/// lucide "info"
pub const INFO: char = '\u{e0ff}';

/// lucide "circle-alert"
pub const ALERT: char = '\u{e07b}';

/// lucide "triangle-alert"
pub const ERROR: char = '\u{e193}';

/// lucide "ellipsis-vertical"
pub const MENU: char = '\u{e0bb}';

/// lucide "ellipsis"
pub const ELLIPSIS: char = '\u{e0ba}';

/// lucide "trash-2"
pub const DELETE: char = '\u{e18e}';

/// lucide "x"
pub const CLOSE: char = '\u{e1b2}';

/// lucide "check"
pub const CHECK: char = '\u{e070}';

/// lucide "circle-check"
pub const CHECK_CIRCLE: char = '\u{e226}';

/// lucide "search"
pub const SEARCH: char = '\u{e155}';

/// lucide "copy"
pub const COPY: char = '\u{e0a2}';

/// lucide "target"
pub const TARGET: char = '\u{e184}';

/// lucide "zap"
pub const ZAP: char = '\u{e1b4}';

/// lucide "settings-2"
pub const SETTINGS: char = '\u{e245}';

/// lucide "sliders-horizontal"
pub const SLIDERS: char = '\u{e29a}';

/// lucide "layers"
pub const LOFT: char = '\u{e52e}';

/// lucide "spline"
pub const SWEEP: char = '\u{e38f}';

/// lucide "chevron-down"
pub const CHEVRON_DOWN: char = '\u{e071}';

/// lucide "chevron-up"
pub const CHEVRON_UP: char = '\u{e074}';

/// Every curated glyph, for tests that walk the whole set.
pub const ALL: &[char] = &[
    HAMMER,
    UNDO,
    REDO,
    BOX,
    SPHERE,
    CYLINDER,
    CONE,
    TORUS,
    PLUS,
    SKETCH,
    ELLIPSE,
    SKETCH_ON_FACE,
    EXTRUDE,
    DRILL,
    COMBINE,
    UNION,
    SUBTRACT,
    INTERSECT,
    EXCLUDE,
    MOVE_3D,
    ROTATE_3D,
    REVOLVE,
    MIRROR,
    LINEAR_PATTERN,
    CIRCULAR_PATTERN,
    DATUM,
    IMPORTED_MESH,
    AXIS_3D,
    FIT,
    CAMERA,
    GRID,
    EDGES,
    SHADED,
    WIREFRAME,
    XRAY,
    SECTION,
    MEASURE,
    EYE,
    EYE_OFF,
    PICK,
    PICK_CLICK,
    SAVE,
    EXPORT,
    IMPORT,
    COMMAND,
    ACTIVITY,
    FPS,
    TRIANGLE,
    BODIES,
    TIMER,
    FEATURES,
    PARAMS,
    INFO,
    ALERT,
    ERROR,
    MENU,
    ELLIPSIS,
    DELETE,
    CLOSE,
    CHECK,
    CHECK_CIRCLE,
    SEARCH,
    COPY,
    TARGET,
    ZAP,
    SETTINGS,
    SLIDERS,
    LOFT,
    SWEEP,
    CHEVRON_DOWN,
    CHEVRON_UP,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph constant must match the codepoint of its documented
    /// lucide slug in the shipped `assets/lucide.css`, and `ALL` must
    /// cover the whole curated set. A font/CSS swap without
    /// regenerating this file fails here (and in the theme test with
    /// missing glyphs).
    #[test]
    fn constants_match_the_shipped_font_css() {
        let css = include_str!("../assets/lucide.css");
        let src = include_str!("icons.rs");
        let mut pairs = 0;
        let mut lines = src.lines().peekable();
        while let Some(line) = lines.next() {
            let Some(rest) = line.strip_prefix("/// lucide \"") else {
                continue;
            };
            let Some(slug) = rest.strip_suffix('"') else {
                continue;
            };
            let const_line = lines.next().expect("const line follows its doc comment");
            let needle = format!(".icon-{slug}::before {{ content: \"\\");
            let Some(pos) = css.find(&needle) else {
                panic!("lucide css is missing the \"{slug}\" icon");
            };
            let hex: String = css[pos + needle.len()..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            let want = char::from_u32(u32::from_str_radix(&hex, 16).expect("hex codepoint"))
                .expect("codepoint is a valid char");
            assert!(
                const_line.contains(&format!("\\u{{{:x}}}", want as u32)),
                "constant for {slug} does not match the css codepoint: {const_line}"
            );
            pairs += 1;
        }
        assert!(pairs > 50, "expected the full curated set, got {pairs}");
        assert_eq!(pairs, ALL.len(), "ALL array is stale");
    }
}
