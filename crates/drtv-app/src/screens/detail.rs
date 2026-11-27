//! A title's page: backdrop, the copy, and the episodes.
//!
//! The backdrop is full-bleed; everything after it is inside the same 28-point margin as the rest of
//! the app. The play button is the only red thing on the page, so it is never ambiguous what the
//! primary action is.

use drtv_api::card::{Card, Detail};
use drtv_ui::theme::{self, ACCENT, BG, BORDER, SURFACE, TEXT, TEXT_DIM, TEXT_FAINT};
use drtv_ui::{icons, widgets};
use egui::{
    Align, Color32, Layout, Rect, Response, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::app::{DrtvApp, Load, Route};
use crate::screens::{prefetch_wide, tile_width};
use crate::worker::Cmd;

impl DrtvApp {
    pub(crate) fn detail_ui(&mut self, ui: &mut Ui, id: &str) {
        match &self.detail {
            Load::Loading => widgets::loading(ui, "Henter titlen …"),
            Load::Failed(why) => {
                let why = why.clone();
                widgets::empty(ui, "Titlen kunne ikke hentes", &why);
                if widgets::ghost_button(ui, "Tilbage", Some(icons::Icon::Back)).clicked() {
                    self.back();
                }
            }
            Load::Ready(detail) => {
                // The backdrop is asked for at twice the window width so it stays sharp on a HiDPI
                // display; the CDN clamps it to what it actually has.
                let width = ui.available_width().max(640.0);
                self.images.want(
                    &self.workers,
                    &detail.card.still,
                    (width * 2.0) as u32,
                    (width * 2.0 / 16.0 * 9.0) as u32,
                );

                // An episode has no children of its own, so its page is the one page with nothing
                // under the hero. Its season does, and those are the rows worth having there.
                let (siblings, season_title): (Vec<Card>, String) = match &self.siblings {
                    Load::Ready(season) if !season.episodes.is_empty() => (
                        season
                            .episodes
                            .iter()
                            .filter(|episode| episode.id != detail.card.id)
                            .cloned()
                            .collect(),
                        season.card.title.clone(),
                    ),
                    _ => (Vec::new(), String::new()),
                };

                let mut action: Option<Action> = None;
                egui::ScrollArea::vertical()
                    .id_salt(("detail", id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        hero(ui, detail, &self.images, &mut action);
                        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 8)).show(
                            ui,
                            |ui| {
                                let tile = tile_width(ui.available_width());
                                if !detail.seasons.is_empty() {
                                    prefetch_wide(
                                        &mut self.images,
                                        &self.workers,
                                        &detail.seasons,
                                        (tile * 1.4).clamp(180.0, 300.0),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("Sæsoner")
                                            .font(theme::semibold(19.0))
                                            .color(TEXT),
                                    );
                                    ui.add_space(8.0);
                                    let images = &self.images;
                                    let lookup = |url: &str| images.get(url);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                                        for season in &detail.seasons {
                                            let w = (tile * 1.4).clamp(180.0, 300.0);
                                            if widgets::wide(ui, season, &lookup, w).clicked() {
                                                action = Some(Action::Open(season.id.clone()));
                                            }
                                        }
                                    });
                                    ui.add_space(22.0);
                                }

                                if !siblings.is_empty() {
                                    prefetch_wide(
                                        &mut self.images,
                                        &self.workers,
                                        &siblings,
                                        160.0,
                                    );
                                    ui.label(
                                        RichText::new(if detail.card.season > 0 {
                                            format!("Afsnit i sæson {}", detail.card.season)
                                        } else {
                                            "Andre afsnit".to_string()
                                        })
                                        .font(theme::semibold(19.0))
                                        .color(TEXT),
                                    );
                                    ui.add_space(6.0);
                                    let images = &self.images;
                                    let lookup = |url: &str| images.get(url);
                                    for episode in &siblings {
                                        if episode_row(
                                            ui,
                                            episode,
                                            &season_title,
                                            &lookup,
                                            ui.available_width(),
                                        )
                                        .clicked()
                                        {
                                            action = Some(Action::Play(episode.clone()));
                                        }
                                    }
                                    ui.add_space(20.0);
                                }

                                if !detail.episodes.is_empty() {
                                    prefetch_wide(
                                        &mut self.images,
                                        &self.workers,
                                        &detail.episodes,
                                        160.0,
                                    );
                                    ui.label(
                                        RichText::new("Afsnit")
                                            .font(theme::semibold(19.0))
                                            .color(TEXT),
                                    );
                                    ui.add_space(6.0);
                                    let images = &self.images;
                                    let lookup = |url: &str| images.get(url);
                                    for episode in &detail.episodes {
                                        if episode_row(
                                            ui,
                                            episode,
                                            &detail.card.title,
                                            &lookup,
                                            ui.available_width(),
                                        )
                                        .clicked()
                                        {
                                            action = Some(Action::Play(episode.clone()));
                                        }
                                    }
                                    ui.add_space(20.0);
                                }
                            },
                        );
                    });

                match action {
                    Some(Action::Open(id)) => {
                        self.navigate(Route::Detail(id));
                    }
                    Some(Action::Play(card)) => self.play(card),
                    Some(Action::Restart(card)) => self.play_from(card, 0.0),
                    Some(Action::PlayShow) => {
                        if let Load::Ready(detail) = &self.detail
                            && let Some(card) = playable(detail)
                        {
                            self.play(card);
                        }
                    }
                    None => {}
                }
            }
        }
    }

    /// Open the player for `card` at DR's resume point.
    pub(crate) fn play(&mut self, card: Card) {
        let start = card.resume_seconds() as f64;
        self.play_from(card, start);
    }

    /// Open the player for `card`, asking the worker for a stream first. `start` is where mpv should
    /// begin, in seconds; zero means "from the beginning", which also clears the card's resume mark
    /// so the HUD does not claim a position the viewer just declined.
    pub(crate) fn play_from(&mut self, mut card: Card, start: f64) {
        if start <= 0.0 {
            card.resume_ms = 0;
        }
        self.generation += 1;
        let generation = self.generation;
        self.history.push(self.route.clone());
        self.route = Route::Player;
        self.playback = Some(crate::app::Playback {
            card: card.clone(),
            duration: card.duration_s as f64,
            markers: Vec::new(),
            menu: None,
            scrubbing: None,
            hud_until: Some(std::time::Instant::now() + std::time::Duration::from_secs(3)),
            last_reported: std::time::Instant::now(),
            last_position: start,
            error: None,
            paused: false,
            volume: 100.0,
            muted: false,
        });
        let _ = self.workers.api().send(Cmd::Stream { id: card.id.clone(), generation });
    }
}

enum Action {
    Open(String),
    Play(Card),
    PlayShow,
    /// Play this item from the beginning, ignoring DR's resume point.
    Restart(Card),
}

/// The episode a play button should start: the one in progress, else the first unwatched, else the
/// first. This is what DR's own "Fortsæt" does, and it is why the button's label changes.
pub(crate) fn playable(detail: &Detail) -> Option<Card> {
    match detail.card.kind.as_str() {
        "movie" | "episode" | "program" => Some(detail.card.clone()),
        _ => {
            let episodes = &detail.episodes;
            episodes
                .iter()
                .find(|e| e.resume_ms > 0 && !e.watched)
                .or_else(|| episodes.iter().find(|e| !e.watched))
                .or_else(|| episodes.first())
                .cloned()
        }
    }
}

fn hero(
    ui: &mut Ui,
    detail: &Detail,
    images: &crate::images::ImageCache,
    action: &mut Option<Action>,
) {
    let width = ui.available_width();
    // The art is 400 tall; the hero is 56 taller than that, and the fade spends those extra points
    // reaching the page background. Ending the gradient exactly on the art's last row left a hard
    // horizontal seam right where the first heading sits.
    const ART: f32 = 400.0;
    const TAIL: f32 = 56.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, ART + TAIL), Sense::hover());
    let art = Rect::from_min_size(rect.min, vec2(width, ART));

    // Artwork, cover-cropped. A title with no landscape art falls back to its poster, cropped.
    let texture = images.get(&detail.card.still).or_else(|| images.get(&detail.card.poster));
    if let Some(texture) = texture {
        let uv = widgets::cover_uv(texture.size_vec2(), art.size());
        ui.painter().image(texture.id(), art, uv, egui::Color32::WHITE);
    } else {
        ui.painter().rect_filled(art, 0.0, SURFACE);
    }

    // Two gradients: the top one lets the nav bar sit on the art, the bottom one hands the art back
    // to the page background so the copy does not float on a hard edge.
    widgets::gradient(
        ui,
        Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 120.0)),
        BG.gamma_multiply(0.85),
        Color32::TRANSPARENT,
    );
    // Twice, because one pass of a linear gradient leaves the copy sitting on a bright scene at
    // roughly 40% black; the second pass is what makes white text readable over sunlit footage.
    let fade = Rect::from_min_max(pos2(rect.left(), art.top() + 120.0), rect.max);
    widgets::gradient(ui, fade, Color32::TRANSPARENT, BG);
    widgets::gradient(ui, fade, Color32::TRANSPARENT, BG);

    let content = Rect::from_min_max(
        pos2(rect.left() + 40.0, art.top() + 118.0),
        pos2(rect.right() - 40.0, art.bottom() - 18.0),
    );
    ui.scope_builder(
        UiBuilder::new().max_rect(content).layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.set_max_width(660.0);
            let card = &detail.card;
            ui.label(RichText::new(&card.title).font(theme::semibold(38.0)).color(TEXT));
            if !card.episode_name.is_empty() && card.episode_name != card.title {
                ui.label(
                    RichText::new(&card.episode_name).font(theme::medium(16.0)).color(TEXT_DIM),
                );
            }
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if let Some(year) = card.year {
                    ui.label(RichText::new(year.to_string()).color(TEXT_DIM).size(13.0));
                    dot(ui);
                }
                if card.episode_count > 0 && card.kind != "episode" {
                    ui.label(
                        RichText::new(format!("{} afsnit", card.episode_count))
                            .color(TEXT_DIM)
                            .size(13.0),
                    );
                    dot(ui);
                }
                let length = theme::length(card.duration_s);
                if !length.is_empty() {
                    ui.label(RichText::new(length).color(TEXT_DIM).size(13.0));
                    dot(ui);
                }
                for genre in
                    card.genres.iter().filter(|g| !g.eq_ignore_ascii_case("ukendt")).take(2)
                {
                    ui.label(RichText::new(genre).color(TEXT_DIM).size(13.0));
                    dot(ui);
                }
                if let Some(rating) = &card.rating {
                    widgets::badge(ui, rating);
                }
            });

            if !card.description.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&card.description).font(theme::regular(14.0)).color(TEXT_DIM),
                );
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let target = playable(detail);
                let (label, icon) = if target.as_ref().is_some_and(|c| c.resume_ms > 0) {
                    ("Fortsæt", icons::Icon::Play)
                } else if detail.seasons.is_empty() {
                    ("Afspil", icons::Icon::Play)
                } else {
                    ("Start forfra", icons::Icon::Play)
                };
                if widgets::primary_button(ui, label, Some(icon)).clicked() {
                    *action = match target {
                        Some(card) if detail.seasons.is_empty() || !detail.episodes.is_empty() => {
                            Some(Action::Play(card))
                        }
                        Some(season) => Some(Action::Open(season.id.clone())),
                        None => Some(Action::PlayShow),
                    };
                }
                if let Some(card) = &playable(detail)
                    && card.resume_ms > 0
                {
                    if widgets::ghost_button(ui, "Fra start", None).clicked() {
                        *action = Some(Action::Restart(card.clone()));
                    }
                    ui.label(
                        RichText::new(format!(
                            "{} set",
                            drtv_player::clock(card.resume_seconds() as f64)
                        ))
                        .color(TEXT_FAINT)
                        .size(12.5),
                    );
                }
                if detail.card.kind == "episode"
                    && !detail.card.show_id.is_empty()
                    && widgets::ghost_button(ui, "Serien", None).clicked()
                {
                    *action = Some(Action::Open(detail.card.show_id.clone()));
                }
            });
        },
    );

    // **`scope_builder` hands its child's height back to the cursor**, and the hero's copy is much
    // shorter than the artwork behind it: without this the page resumed 128 points early and the
    // first heading landed inside the backdrop.
    ui.advance_cursor_after_rect(rect);
}

/// DR titles an episode `"Show: Episode name"`, which repeats the heading directly above the row.
fn episode_title<'a>(title: &'a str, show: &str) -> &'a str {
    if show.is_empty() {
        return title;
    }
    title.strip_prefix(show).and_then(|rest| rest.strip_prefix(": ")).unwrap_or(title)
}

fn dot(ui: &mut Ui) {
    ui.label(RichText::new("·").color(TEXT_FAINT).size(13.0));
}

fn episode_row(
    ui: &mut Ui,
    card: &Card,
    show: &str,
    images: &dyn Fn(&str) -> Option<egui::TextureHandle>,
    width: f32,
) -> Response {
    let height = 88.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hovered = response.hovered();

    if hovered {
        ui.painter().rect_filled(rect, theme::RADIUS_SM, SURFACE.gamma_multiply(0.85));
    }

    let thumb =
        Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - 36.0), vec2(128.0, 72.0));
    let url = if !card.still.is_empty() { &card.still } else { &card.poster };
    match images(url) {
        Some(texture) => {
            let uv = widgets::cover_uv(texture.size_vec2(), thumb.size());
            ui.painter().image(texture.id(), thumb, uv, egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(thumb, theme::RADIUS_SM, SURFACE);
        }
    }
    ui.painter().rect_stroke(
        thumb,
        theme::RADIUS_SM,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );

    if card.progress() > 0.0 {
        let track = Rect::from_min_max(
            pos2(thumb.left() + 6.0, thumb.bottom() - 6.0),
            pos2(thumb.right() - 6.0, thumb.bottom() - 3.0),
        );
        ui.painter().rect_filled(track, 2.0, Color32::from_black_alpha(160));
        let filled = Rect::from_min_max(
            track.min,
            pos2(track.left() + track.width() * card.progress(), track.max.y),
        );
        ui.painter().rect_filled(filled, 2.0, ACCENT);
    } else if hovered {
        let play = Rect::from_center_size(thumb.center(), vec2(30.0, 30.0));
        ui.painter().circle_filled(play.center(), 15.0, Color32::from_black_alpha(150));
        icons::paint(ui.painter(), play.shrink(8.0), icons::Icon::Play, Color32::WHITE);
    }

    let text_left = thumb.right() + 14.0;
    let text_width = (rect.right() - text_left - 90.0).max(120.0);
    let mut y = rect.top() + 12.0;
    let painter = ui.painter();

    let kicker = card.kicker();
    if !kicker.is_empty() {
        painter.text(
            pos2(text_left, y),
            egui::Align2::LEFT_TOP,
            kicker,
            theme::medium(11.5),
            TEXT_FAINT,
        );
        y += 16.0;
    }

    let title = episode_title(&card.title, show);
    let galley = painter.layout(title.to_string(), theme::semibold(15.0), TEXT, text_width);
    painter.with_clip_rect(Rect::from_min_size(pos2(text_left, y), vec2(text_width, 22.0))).galley(
        pos2(text_left, y),
        galley,
        TEXT,
    );
    y += 24.0;

    if !card.description.is_empty() {
        let galley =
            painter.layout(card.description.clone(), theme::regular(12.5), TEXT_DIM, text_width);
        painter
            .with_clip_rect(Rect::from_min_size(pos2(text_left, y), vec2(text_width, 18.0)))
            .galley(pos2(text_left, y), galley, TEXT_DIM);
    }

    let length = theme::length(card.duration_s);
    if !length.is_empty() {
        painter.text(
            pos2(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            length,
            theme::regular(12.5),
            TEXT_FAINT,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
