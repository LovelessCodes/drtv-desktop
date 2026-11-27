//! Tiles, shelves and the small parts every screen reuses.
//!
//! A tile is the unit of the app: art, a title and — when the viewer has started it — a progress
//! bar. The two orientations are the two DR itself uses: a 2:3 poster for shelves and search, a 16:9
//! still for episode lists and the detail page.

use drtv_api::card::Card;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Rect, Response, RichText, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::theme::{self, ACCENT, BORDER, RADIUS, SURFACE, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT};

/// How a screen finds the decoded art for a URL. Returning a cloned handle keeps the signature free
/// of the cache's lifetime; a handle is an `Arc` clone.
pub type Lookup<'a> = dyn Fn(&str) -> Option<egui::TextureHandle> + 'a;

const POSTER_ASPECT: f32 = 1.5;
/// Art, then two lines of title and one of kicker. Measured rather than guessed: a two-line Danish
/// title plus its kicker is 36 + 18 points, and a tile that allows less overlaps the shelf below.
const POSTER_TEXT: f32 = 62.0;

/// A portrait tile. Returns the response for the whole tile, art and caption.
pub fn poster(ui: &mut Ui, card: &Card, images: &Lookup<'_>, width: f32) -> Response {
    let art_height = width * POSTER_ASPECT;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, art_height + POSTER_TEXT), Sense::click());
    let art = Rect::from_min_size(rect.min, vec2(width, art_height));

    let url = if !card.poster.is_empty() { &card.poster } else { &card.still };
    art_box(ui, art, images(url.as_str()), &card.title, card);
    caption(ui, rect, art, card);

    response.on_hover_cursor(CursorIcon::PointingHand).on_hover_ui(|ui| tooltip(ui, card))
}

/// A landscape tile, sized by width. `width / 1.78` is the art; the caption sits under it.
pub fn wide(ui: &mut Ui, card: &Card, images: &Lookup<'_>, width: f32) -> Response {
    let art_height = width / 16.0 * 9.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, art_height + POSTER_TEXT - 14.0), Sense::click());
    let art = Rect::from_min_size(rect.min, vec2(width, art_height));

    let url = if !card.still.is_empty() { &card.still } else { &card.poster };
    art_box(ui, art, images(url.as_str()), &card.title, card);
    caption(ui, rect, art, card);

    response.on_hover_cursor(CursorIcon::PointingHand).on_hover_ui(|ui| tooltip(ui, card))
}

/// The art itself: the image (cover-cropped) or, when there is none yet, a placeholder carrying the
/// first letter of the title. Both are drawn inside the same rounded rectangle so a shelf does not
/// reflow as pictures arrive.
fn art_box(ui: &Ui, art: Rect, texture: Option<egui::TextureHandle>, title: &str, card: &Card) {
    let painter = ui.painter();
    let radius = RADIUS;

    match texture {
        Some(texture) => {
            // A raw texture draw, not `egui::Image`: the widget preserves the texture's aspect ratio
            // and would letterbox inside the tile. The uv rectangle is the crop, so the art fills.
            let uv = cover_uv(texture.size_vec2(), art.size());
            painter.image(texture.id(), art, uv, Color32::WHITE);
        }
        None => {
            painter.rect_filled(art, radius, SURFACE_HI);
            let initial = title.chars().next().unwrap_or('·').to_uppercase().to_string();
            painter.text(
                art.center(),
                Align2::CENTER_CENTER,
                initial,
                theme::semibold(art.width() * 0.26),
                TEXT_FAINT,
            );
        }
    }

    // A hairline keeps a dark poster from bleeding into the page background.
    painter.rect_stroke(art, radius, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);

    if card.progress() > 0.0 {
        progress(ui, art, card.progress());
    }

    if ui.rect_contains_pointer(art) {
        painter.rect_filled(art, radius, Color32::from_black_alpha(60));
        painter.circle_filled(art.center(), 22.0, Color32::from_black_alpha(140));
        icons::paint(
            painter,
            Rect::from_center_size(art.center(), Vec2::splat(26.0)),
            Icon::Play,
            Color32::WHITE,
        );
    }
}

/// The two text lines under a tile. Painted rather than laid out as widgets so a long Danish title
/// cannot make one tile taller than its neighbours.
fn caption(ui: &Ui, rect: Rect, art: Rect, card: &Card) {
    let painter = ui.painter();
    let x = rect.left();
    let title_top = art.bottom() + 8.0;
    let title_width = rect.width();

    let galley = painter.layout(card.title.clone(), theme::semibold(13.5), TEXT, title_width);
    // Two lines at most: the caption has a fixed height, and a third line would overlap the kicker.
    let clip =
        Rect::from_min_size(pos2(x, title_top), vec2(title_width, (galley.size().y).min(35.0)));
    painter.with_clip_rect(clip).galley(pos2(x, title_top), galley, TEXT);

    let kicker = kicker_text(card);
    if !kicker.is_empty() {
        painter
            .with_clip_rect(Rect::from_min_size(pos2(x, title_top + 37.0), vec2(title_width, 14.0)))
            .text(
                pos2(x, title_top + 36.0),
                Align2::LEFT_TOP,
                kicker,
                theme::regular(11.5),
                TEXT_DIM,
            );
    }
}

fn kicker_text(card: &Card) -> String {
    let mut parts: Vec<String> = Vec::new();
    let kicker = card.kicker();
    if !kicker.is_empty() {
        parts.push(kicker);
    }
    if card.progress() > 0.0 {
        let left = (card.duration_s - card.resume_seconds()).max(0);
        let left = theme::length(left);
        if !left.is_empty() {
            parts.push(format!("{left} tilbage"));
        }
    } else if card.kind == "program" || card.kind == "episode" {
        let length = theme::length(card.duration_s);
        if !length.is_empty() {
            parts.push(length);
        }
    }
    parts.join(" · ")
}

/// A thin progress bar across the bottom of a tile, inset so it reads as "watched so far" rather
/// than as a border.
fn progress(ui: &Ui, art: Rect, fraction: f32) {
    let painter = ui.painter();
    let track = Rect::from_min_max(
        pos2(art.left() + 8.0, art.bottom() - 9.0),
        pos2(art.right() - 8.0, art.bottom() - 5.0),
    );
    painter.rect_filled(track, CornerRadius::same(2), Color32::from_black_alpha(150));
    let filled = Rect::from_min_max(
        track.min,
        pos2(track.left() + track.width() * fraction.clamp(0.0, 1.0), track.max.y),
    );
    painter.rect_filled(filled, CornerRadius::same(2), ACCENT);
}

/// Cover-crop: the image fills the rectangle and the overflow is cut from the middle, rather than
/// letterboxed. DR's art is already 2:3 or 16:9, so this mostly protects against a stray square.
pub fn cover_uv(texture: Vec2, target: Vec2) -> Rect {
    if texture.x <= 0.0 || texture.y <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let texture_aspect = texture.x / texture.y;
    let target_aspect = target.x / target.y;
    if texture_aspect > target_aspect {
        let keep = target_aspect / texture_aspect;
        let inset = (1.0 - keep) * 0.5;
        Rect::from_min_max(pos2(inset, 0.0), pos2(1.0 - inset, 1.0))
    } else {
        let keep = texture_aspect / target_aspect;
        let inset = (1.0 - keep) * 0.5;
        Rect::from_min_max(pos2(0.0, inset), pos2(1.0, 1.0 - inset))
    }
}

fn tooltip(ui: &mut Ui, card: &Card) {
    ui.set_max_width(320.0);
    ui.label(
        RichText::new(&card.title)
            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
            .size(15.0),
    );
    let mut meta: Vec<String> = Vec::new();
    if let Some(year) = card.year {
        meta.push(year.to_string());
    }
    let length = theme::length(card.duration_s);
    if !length.is_empty() {
        meta.push(length);
    }
    // DR files everything it has no genre for under "ukendt"; a tooltip that says so is noise.
    meta.extend(card.genres.iter().filter(|g| !g.eq_ignore_ascii_case("ukendt")).take(2).cloned());
    if !meta.is_empty() {
        ui.label(RichText::new(meta.join(" · ")).color(TEXT_DIM).size(12.0));
    }
    if !card.description.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&card.description).color(TEXT_DIM).size(12.5));
    }
}

/// One shelf: a heading and a horizontal run of tiles. The run scrolls with the wheel and with a
/// drag, which is what a mouse-driven shelf wants; a scrollbar would eat a row of art.
pub fn shelf(
    ui: &mut Ui,
    shelf: &drtv_api::card::Shelf,
    images: &Lookup<'_>,
    width: f32,
    on_open: &mut dyn FnMut(&Card),
) {
    if shelf.items.is_empty() {
        return;
    }
    ui.add_space(10.0);
    // A hand-picked row on DR's front page has no heading; the art carries it.
    if !shelf.title.is_empty() {
        ui.label(RichText::new(&shelf.title).font(theme::semibold(20.0)).color(TEXT));
        ui.add_space(6.0);
    }
    egui::ScrollArea::horizontal()
        .id_salt(("shelf", &shelf.id))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for card in &shelf.items {
                    if poster(ui, card, images, width).clicked() {
                        on_open(card);
                    }
                }
            });
        });
    ui.add_space(14.0);
}

/// A selectable chip: the season bar, the search filters.
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let font = theme::medium(13.5);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
    let size = vec2(galley.size().x + 26.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let (fill, text) = if selected {
        (TEXT, theme::BG)
    } else if response.hovered() {
        (SURFACE_HI, TEXT)
    } else {
        (SURFACE, TEXT_DIM)
    };
    painter.rect_filled(rect, CornerRadius::same(16), fill);
    painter.galley(
        pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5),
        galley,
        text,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A non-interactive badge: "Sæson 2", "MRAM-11", "8 afsnit".
pub fn badge(ui: &mut Ui, text: &str) {
    let font = theme::medium(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, TEXT_DIM);
    let size = vec2(galley.size().x + 16.0, 24.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), SURFACE);
    ui.painter().galley(
        pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5),
        galley,
        TEXT_DIM,
    );
}

/// A filled primary button, for the one action a page wants ("Afspil").
pub fn primary_button(ui: &mut Ui, label: &str, icon: Option<Icon>) -> Response {
    let font = theme::semibold(14.5);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let icon_width = if icon.is_some() { 26.0 } else { 0.0 };
    let size = vec2(galley.size().x + 40.0 + icon_width, 42.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() { ACCENT.gamma_multiply(0.86) } else { ACCENT };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    let mut x = rect.center().x - (galley.size().x + icon_width) * 0.5;
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(18.0)),
            icon,
            Color32::WHITE,
        );
        x += icon_width;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, Color32::WHITE);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A quiet secondary button: outlined, for "Se senere" and the like.
pub fn ghost_button(ui: &mut Ui, label: &str, icon: Option<Icon>) -> Response {
    let font = theme::medium(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
    let icon_width = if icon.is_some() { 24.0 } else { 0.0 };
    let size = vec2(galley.size().x + 36.0 + icon_width, 42.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), SURFACE);
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.center().x - (galley.size().x + icon_width) * 0.5;
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)),
            icon,
            TEXT,
        );
        x += icon_width;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, TEXT);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A centered "nothing here" state, with an optional action label.
pub fn empty(ui: &mut Ui, title: &str, body: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.28);
        ui.label(RichText::new(title).font(theme::semibold(20.0)));
        ui.add_space(6.0);
        ui.label(RichText::new(body).color(TEXT_DIM).size(13.5));
    });
}

/// A one-line loading state; the art arrives on its own and must not move the layout.
pub fn loading(ui: &mut Ui, what: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.spinner();
        ui.add_space(8.0);
        ui.label(RichText::new(what).color(TEXT_DIM).size(13.5));
    });
}

/// A vertical gradient, for the two places art meets text (the hero and the player's HUD). egui has
/// no gradient shape, but a mesh with per-vertex colours is one.
pub fn gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    let idx = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_top(),
        uv: egui::Pos2::ZERO,
        color: top,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_top(),
        uv: egui::Pos2::ZERO,
        color: top,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.right_bottom(),
        uv: egui::Pos2::ZERO,
        color: bottom,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: rect.left_bottom(),
        uv: egui::Pos2::ZERO,
        color: bottom,
    });
    mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    ui.painter().add(egui::Shape::mesh(mesh));
}
