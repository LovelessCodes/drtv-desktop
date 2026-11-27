//! One dark palette and one type scale.
//!
//! The app is a television client shown on a monitor, at arm's length, with the mouse: the surfaces
//! are flat and close together, the art carries the colour, and red is reserved for the two things
//! that must read instantly — the brand mark and "this is playing".

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

/// The page behind the shelves.
pub const BG: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0c);
/// A raised surface: cards, the top bar, menus.
pub const SURFACE: Color32 = Color32::from_rgb(0x15, 0x15, 0x18);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x24);
/// Hairlines. The only borders in the app.
pub const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x31);
pub const TEXT: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xa2, 0xa2, 0xac);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x78);
/// DR's red, used for the wordmark, the play action and the progress fill. Nothing else.
pub const ACCENT: Color32 = Color32::from_rgb(0xe4, 0x00, 0x2b);

pub const RADIUS: CornerRadius = CornerRadius::same(10);
pub const RADIUS_SM: CornerRadius = CornerRadius::same(6);

/// The family names [`install`] registers. A screen asking for a weight asks by name; `RichText`
/// carries no weight of its own.
pub const SEMIBOLD: &str = "inter-semibold";
pub const MEDIUM: &str = "inter-medium";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Register the fonts and the dark palette. Called once, from the eframe creation hook.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::FONT_REGULAR)),
    );
    fonts.font_data.insert(
        "inter-medium".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::FONT_MEDIUM)),
    );
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::FONT_SEMIBOLD)),
    );

    // Inter first, then egui's own fallbacks — those carry the emoji and the symbol glyphs.
    let proportional = fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default();
    proportional.insert(0, "inter".to_owned());

    for (name, font) in [(SEMIBOLD, "inter-semibold"), (MEDIUM, "inter-medium")] {
        fonts.families.insert(
            FontFamily::Name(name.into()),
            vec![font.to_owned(), "inter".to_owned()],
        );
    }
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Dark);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.text_styles = [
        (TextStyle::Heading, semibold(26.0)),
        (TextStyle::Body, regular(15.0)),
        (TextStyle::Button, medium(15.0)),
        (TextStyle::Small, regular(12.5)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.menu_margin = egui::Margin::same(10);
    style.spacing.interact_size.y = 30.0;

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = Color32::from_rgb(0x08, 0x08, 0x0a);
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = Some(TEXT);
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = RADIUS;
    visuals.menu_corner_radius = RADIUS;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.hyperlink_color = TEXT;
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.bg_fill = SURFACE_HI;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_HI;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x30);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x30);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x42));
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x34, 0x34, 0x3c);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x3c);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.noninteractive.corner_radius = RADIUS_SM;
    visuals.widgets.inactive.corner_radius = RADIUS_SM;
    visuals.widgets.hovered.corner_radius = RADIUS_SM;
    visuals.widgets.active.corner_radius = RADIUS_SM;

    style.visuals = visuals;
    // Both themes get the same style: the app is dark only, and a system light preference must not
    // repaint the menus half-way through a session.
    ctx.all_styles_mut(|target| *target = style.clone());
}

/// A "12 min tilbage" style duration, from seconds. Under an hour, `42 min`; over, `1 t 12 min`.
pub fn length(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    match minutes {
        0 => String::new(),
        m if m < 60 => format!("{m} min"),
        m => {
            let (h, m) = (m / 60, m % 60);
            if m == 0 {
                format!("{h} t")
            } else {
                format!("{h} t {m} min")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_read_like_a_tv_guide_not_a_timestamp() {
        assert_eq!(length(0), "");
        assert_eq!(length(59 * 60 + 59), "59 min");
        assert_eq!(length(3600), "1 t");
        assert_eq!(length(3600 + 12 * 60), "1 t 12 min");
    }
}
