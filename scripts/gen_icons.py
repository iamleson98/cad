#!/usr/bin/env python3
"""Generate crates/forge-app/src/icons.rs from the Lucide codepoint CSS.

Usage (from anywhere; paths are resolved relative to the repo root):
    python3 scripts/gen_icons.py

Inputs:  crates/forge-app/assets/lucide.css      (Lucide, ISC license)
Output:  crates/forge-app/src/icons.rs           (generated - do not hand-edit)

The curated ICONS table below maps Rust constant names to lucide icon
slugs. The script fails loudly on unknown slugs so a lucide upgrade can
never silently produce tofu glyphs.
"""
from pathlib import Path
import re
import sys

REPO = Path(__file__).resolve().parent.parent
CSS = REPO / "crates/forge-app/assets/lucide.css"
OUT = REPO / "crates/forge-app/src/icons.rs"

# Rust const name -> lucide icon slug (must exist in lucide.css).
ICONS: dict[str, str] = {
    # brand
    "HAMMER": "hammer",
    # history
    "UNDO": "undo-2",
    "REDO": "redo-2",
    # primitives
    "BOX": "box",
    "SPHERE": "circle",
    "CYLINDER": "cylinder",
    "CONE": "cone",
    "TORUS": "torus",
    # creation
    "PLUS": "plus",
    "SKETCH": "pencil-ruler",
    # lucide has no "ellipse" glyph in this build; "egg" is the
    # ellipse-shaped one (closest semantic match).
    "ELLIPSE": "egg",
    "SKETCH_ON_FACE": "square-pen",
    "EXTRUDE": "arrow-up-from-line",
    "DRILL": "circle-minus",
    # booleans
    "COMBINE": "combine",
    "UNION": "squares-unite",
    "SUBTRACT": "squares-subtract",
    "INTERSECT": "squares-intersect",
    "EXCLUDE": "squares-exclude",
    # transforms & patterns
    "MOVE_3D": "move-3d",
    "ROTATE_3D": "rotate-3d",
    "REVOLVE": "rotate-cw",
    "MIRROR": "flip-horizontal",
    "LINEAR_PATTERN": "columns-3",
    "CIRCULAR_PATTERN": "life-buoy",
    # datums & imported geometry
    "DATUM": "square-dashed",
    "IMPORTED_MESH": "package",
    # view
    "AXIS_3D": "axis-3d",
    "FIT": "maximize",
    "CAMERA": "camera",
    "GRID": "grid-3x3",
    "EDGES": "vector-square",
    "SHADED": "contrast",
    "WIREFRAME": "cuboid",
    "XRAY": "scan",
    "SECTION": "slice",
    "MEASURE": "ruler",
    "EYE": "eye",
    "EYE_OFF": "eye-off",
    # picking
    "PICK": "square-mouse-pointer",
    "PICK_CLICK": "mouse-pointer-click",
    # files & palette
    "SAVE": "save",
    "EXPORT": "file-output",
    "IMPORT": "file-input",
    "COMMAND": "command",
    # status & stats
    "ACTIVITY": "activity",
    "FPS": "gauge",
    "TRIANGLE": "triangle",
    "BODIES": "boxes",
    "TIMER": "timer",
    "FEATURES": "list-tree",
    "PARAMS": "variable",
    "INFO": "info",
    "ALERT": "circle-alert",
    "ERROR": "triangle-alert",
    # row actions & misc
    "MENU": "ellipsis-vertical",
    "ELLIPSIS": "ellipsis",
    "DELETE": "trash-2",
    "CLOSE": "x",
    "CHECK": "check",
    "CHECK_CIRCLE": "circle-check",
    "SEARCH": "search",
    "COPY": "copy",
    "TARGET": "target",
    "ZAP": "zap",
    "SETTINGS": "settings-2",
    "SLIDERS": "sliders-horizontal",
    "LOFT": "layers",
    "SWEEP": "spline",
    # navigation glyphs
    "CHEVRON_DOWN": "chevron-down",
    "CHEVRON_UP": "chevron-up",
}

HEADER = '''//! Lucide icon glyphs (generated file - do not hand-edit).
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
    egui::RichText::new(code).family(family()).size(size).color(color)
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
    job.append(label, 0.0, text(FontId::new(13.0, FontFamily::Proportional)));
    job
}

// ----------------------------------------------------------------------------
// Glyph constants (lucide slug in the comment).
'''


TESTS = r'''
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
            let const_line = lines
                .next()
                .expect("const line follows its doc comment");
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
'''


def main() -> int:
    css = CSS.read_text(encoding="utf-8")
    table = {
        slug: int(cp, 16)
        for slug, cp in re.findall(
            r"\.icon-([a-z0-9-]+)::before\s*\{\s*content:\s*\"\\([0-9a-f]+)\"", css
        )
    }
    missing = [slug for slug in ICONS.values() if slug not in table]
    if missing:
        print(f"ERROR: icons missing from {CSS.name}: {missing}", file=sys.stderr)
        return 1

    lines = [HEADER]
    for const, slug in ICONS.items():
        cp = table[slug]
        lines.append(f"/// lucide \"{slug}\"")
        lines.append(f"pub const {const}: char = '\\u{{{cp:x}}}';")
        lines.append("")
    all_consts = ", ".join(ICONS.keys())
    lines.append("/// Every curated glyph, for tests that walk the whole set.")
    lines.append(f"pub const ALL: &[char] = &[{all_consts}];")
    lines.append("")
    lines.append(TESTS)
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} ({len(ICONS)} icons + ALL + tests)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
