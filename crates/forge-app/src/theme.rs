//! Visual identity: fonts (Inter + Lucide icons), a refined dark theme
//! and small layout helpers shared by every panel.
//!
//! Design language: warm-charcoal surfaces with a forge-ember accent
//! (matching the viewport selection tint), Inter typography, 6 px
//! widget rounding, generous spacing, and short (~0.12 s) animated
//! hover fades on tool buttons for a smooth feel.

use crate::icons;
use egui::{
    Color32, Context, CornerRadius, FontDefinitions, FontFamily, FontId, Margin, Response,
    RichText, Stroke, TextStyle, Ui, Visuals,
};

// -- Fonts -------------------------------------------------------------------

const INTER_REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/Inter-SemiBold.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/lucide.ttf");

/// Name of the semibold font family ([`FontFamily::Name`]).
pub const SEMIBOLD: &str = "semibold";

/// Install fonts and the ForgeCAD style on a fresh context (call once
/// from [`crate::ForgeApp::new`]).
pub fn install(ctx: &Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    // ForgeCAD is a dark app: pin the dark style (both theme slots so a
    // stray theme toggle can't flash egui's default light visuals).
    let style = style();
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(INTER_REGULAR).into(),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        egui::FontData::from_static(INTER_SEMIBOLD).into(),
    );
    fonts
        .font_data
        .insert("Lucide".into(), egui::FontData::from_static(LUCIDE).into());

    // Inter replaces Ubuntu-Light as the primary proportional font;
    // the default emoji/unicode fallbacks stay behind it.
    if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
        if let Some(pos) = fam.iter().position(|n| n == "Ubuntu-Light") {
            fam[pos] = "Inter".into();
        } else {
            fam.insert(0, "Inter".into());
        }
    }
    if let Some(fam) = fonts.families.get_mut(&FontFamily::Monospace) {
        if let Some(pos) = fam.iter().position(|n| n == "Ubuntu-Light") {
            fam[pos] = "Inter".into();
        }
    }
    // Real semibold weight: egui's `strong()` only recolors, so headings
    // and emphasis opt into this family explicitly.
    fonts.families.insert(
        FontFamily::Name(SEMIBOLD.into()),
        vec!["Inter-SemiBold".into()],
    );
    fonts.families.insert(
        FontFamily::Name(icons::FAMILY.into()),
        vec!["Lucide".into()],
    );
    ctx.set_fonts(fonts);
}

// -- Palette -------------------------------------------------------------------

/// Warm charcoal of panels, toolbars and the status bar.
pub const PANEL: Color32 = Color32::from_rgb(21, 21, 27);
/// Slightly raised surfaces (window bodies, hover cards).
pub const SURFACE: Color32 = Color32::from_rgb(27, 27, 35);
/// Wells: text fields, combo boxes.
pub const FIELD: Color32 = Color32::from_rgb(13, 13, 18);
/// Hairline borders.
pub const BORDER: Color32 = Color32::from_rgb(42, 42, 54);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(230, 230, 238);
/// Secondary text.
pub const MUTED: Color32 = Color32::from_rgb(140, 140, 155);
/// Forge ember accent — matches the viewport selection tint
/// (`Scene::SELECTED`).
pub const ACCENT: Color32 = Color32::from_rgb(250, 168, 41);
/// Accent pressed on dark text (buttons in active state).
pub const ON_ACCENT: Color32 = Color32::from_rgb(26, 17, 4);
/// Subtle accent-tinted surfaces (selected tool buttons).
pub const ACCENT_DIM: Color32 = Color32::from_rgb(63, 45, 20);
/// Success (fully-constrained sketches, saved state).
pub const OK: Color32 = Color32::from_rgb(126, 211, 153);
/// Errors (feature failures, over-constrained sketches).
pub const ERR: Color32 = Color32::from_rgb(240, 113, 103);

/// Semibold [`RichText`] at body size.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(FontFamily::Name(SEMIBOLD.into()))
        .color(TEXT)
}

/// Linear interpolation of two colors (per-channel, alpha included).
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let c = |x: [u8; 4], y: [u8; 4]| {
        Color32::from_rgba_unmultiplied(
            (x[0] as f32 + (y[0] as f32 - x[0] as f32) * t).round() as u8,
            (x[1] as f32 + (y[1] as f32 - x[1] as f32) * t).round() as u8,
            (x[2] as f32 + (y[2] as f32 - x[2] as f32) * t).round() as u8,
            (x[3] as f32 + (y[3] as f32 - x[3] as f32) * t).round() as u8,
        )
    };
    c(a.to_array(), b.to_array())
}

// -- Style -------------------------------------------------------------------

/// The ForgeCAD [`egui::Style`]: dark charcoal surfaces, ember accent,
/// Inter text, 6 px rounding, roomier spacing.
pub fn style() -> egui::Style {
    let default = egui::Style::default();
    egui::Style {
        text_styles: [
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(15.5, FontFamily::Name(SEMIBOLD.into())),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
            (
                TextStyle::Name(SEMIBOLD.into()),
                FontId::new(13.5, FontFamily::Name(SEMIBOLD.into())),
            ),
        ]
        .into(),
        spacing: spacing(default.spacing),
        visuals: visuals(),
        ..default
    }
}

fn spacing(mut sp: egui::style::Spacing) -> egui::style::Spacing {
    sp.item_spacing = egui::vec2(8.0, 6.0);
    sp.button_padding = egui::vec2(9.0, 5.0);
    sp.interact_size = egui::vec2(22.0, 22.0);
    sp.slider_width = 140.0;
    sp.window_margin = Margin::same(10);
    sp.menu_margin = Margin::same(6);
    sp
}

fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = SURFACE;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = SURFACE;
    v.text_edit_bg_color = Some(FIELD);
    v.hyperlink_color = ACCENT;
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(MUTED);
    v.error_fg_color = ERR;
    v.warn_fg_color = Color32::from_rgb(255, 193, 77);
    v.selection = egui::style::Selection {
        bg_fill: Color32::from_rgb(97, 69, 30),
        stroke: Stroke::new(1.0, ACCENT),
    };

    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);

    let r = CornerRadius::same(6);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = Color32::from_rgb(35, 35, 44);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(46, 46, 58));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = Color32::from_rgb(48, 48, 60);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 64, 80));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.corner_radius = r;

    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.5, ON_ACCENT);
    v.widgets.active.corner_radius = r;

    v.widgets.open.bg_fill = Color32::from_rgb(31, 31, 39);
    v.widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.open.corner_radius = r;
    v
}

// -- Shared layout helpers ------------------------------------------------------

/// Small-caps section header with a leading icon (panel titles).
pub fn section_header(ui: &mut Ui, icon: char, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(icons::colored(icon, 13.0, ACCENT));
        ui.label(
            RichText::new(title.to_uppercase())
                .size(11.0)
                .color(MUTED)
                .family(FontFamily::Name(SEMIBOLD.into())),
        );
    });
    ui.add_space(3.0);
}

/// A compact stat chip for the status bar: icon + text on a raised,
/// rounded surface.
pub fn chip(ui: &mut Ui, icon: char, text: String, tooltip: &str) -> Response {
    let frame = egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(5)
        .inner_margin(Margin::symmetric(7, 2));
    frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(icons::colored(icon, 11.5, MUTED));
                ui.label(RichText::new(text).size(11.5).color(MUTED));
            });
        })
        .response
        .on_hover_text(tooltip)
}

/// Tooltip for tool buttons: bold title + weaker hint line. The hint
/// is the text after the first `\n` (shortcuts go there). Consumes and
/// returns the response (hover attachments chain).
pub fn tool_tooltip(response: Response, tooltip: &str, title: &str) -> Response {
    response
        .on_hover_ui(|ui| {
            ui.set_max_width(260.0);
            ui.horizontal(|ui| {
                ui.label(semibold(title));
            });
            if !tooltip.is_empty() {
                ui.label(RichText::new(tooltip).small().color(MUTED));
            }
        })
        .on_disabled_hover_text(tooltip)
}

/// An animated icon button for the toolbar.
///
/// Hover fades the fill in over ~0.12 s (egui repaints while the
/// animation runs); `selected` pins an ember-tinted active look. Pass
/// `label` to render `icon label` (mixed-font job), or `None` for an
/// icon-only button. `tooltip` is `"title"` or `"title\nshortcut"`
/// (the second line renders as a weaker hint).
pub fn tool_button(
    ui: &mut Ui,
    id: &str,
    icon: char,
    label: Option<&str>,
    tooltip: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    let id = ui.id().with(("tool_button", id));

    // Hover state from the previous frame drives the fade target —
    // the classic egui animation pattern.
    let was_hot = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(id))
        .unwrap_or(false);
    let t = ui
        .ctx()
        .animate_value_with_time(id.with("anim"), f32::from(was_hot), 0.12);

    let visuals = ui.visuals();
    let base = visuals.widgets.inactive.bg_fill;
    let hover = visuals.widgets.hovered.bg_fill;
    let (fill, icon_color) = if selected {
        (
            lerp_color(ACCENT_DIM, lerp_color(ACCENT_DIM, hover, 0.55), t),
            lerp_color(ACCENT, Color32::from_rgb(255, 198, 120), t),
        )
    } else {
        (lerp_color(base, hover, t), TEXT)
    };

    let icon_size = 16.0;
    // Labeled buttons bake the (animated) icon color into the job's
    // first section; the label section stays PLACEHOLDER so widget
    // fg styling applies.
    let text: egui::WidgetText = match label {
        Some(l) => {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &icon.to_string(),
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(icon_size, icons::family()),
                    color: icon_color,
                    ..Default::default()
                },
            );
            job.append(
                "  ",
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(13.5, FontFamily::Proportional),
                    color: Color32::PLACEHOLDER,
                    ..Default::default()
                },
            );
            job.append(
                l,
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(13.5, FontFamily::Proportional),
                    color: Color32::PLACEHOLDER,
                    ..Default::default()
                },
            );
            job.into()
        }
        None => icons::colored(icon, icon_size, icon_color).into(),
    };

    let button = egui::Button::new(text)
        .fill(fill)
        .stroke(Stroke::NONE)
        .min_size(egui::vec2(if label.is_some() { 0.0 } else { 30.0 }, 26.0));
    let response = ui.add_enabled(enabled, button);

    ui.ctx().data_mut(|d| d.insert_temp(id, response.hovered()));

    let (title, hint) = match tooltip.split_once('\n') {
        Some((t, rest)) => (t, rest.trim()),
        None => (tooltip, ""),
    };
    tool_tooltip(response, hint, label.unwrap_or(title).trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After `install`, the Lucide family must cover every curated
    /// glyph and Inter must cover basic Latin in both weights — a
    /// font swap that drops coverage fails here instead of rendering
    /// tofu in the toolbar.
    #[test]
    fn fonts_cover_icons_and_latin() {
        let ctx = Context::default();
        install(&ctx);
        // Fonts materialize on the first pass; headless passes produce
        // texture deltas nobody applies, so clear them (Drop asserts).
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();

        let icons_id = FontId::new(14.0, icons::family());
        let proportional = FontId::new(14.0, FontFamily::Proportional);
        let semibold_id = FontId::new(14.0, FontFamily::Name(SEMIBOLD.into()));

        ctx.fonts_mut(|fonts| {
            for glyph in icons::ALL {
                assert!(
                    fonts.has_glyph(&icons_id, *glyph),
                    "lucide font is missing glyph \\u{{{:x}}} — regenerate \
                     crates/forge-app/src/icons.rs (scripts/gen_icons.py)",
                    *glyph as u32,
                );
            }
            // Basic Latin must resolve in the proportional family.
            assert!(fonts.has_glyphs(&proportional, "The quick brown fox 0123456789"));
            // `has_glyph` is unreliable for single-font families (the
            // replacement-char face is the only face, so every lookup
            // compares against itself). A non-zero advance width proves
            // the semibold font parsed and its faces resolve — a broken
            // TTF yields the invisible glyph and width 0.
            assert!(fonts.glyph_width(&semibold_id, 'T') > 0.0);
        });
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
}
