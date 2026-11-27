//! The player screen: the video on a black field, and a HUD that gets out of the way.
//!
//! The video itself is drawn by mpv through a paint callback; everything else here is egui on top of
//! it. The HUD shows on movement and hides after a few seconds, and while it is hidden the pointer is
//! hidden too — a cursor parked in the middle of a film is the one thing a desktop player must not
//! do.

use std::sync::Arc;
use std::time::{Duration, Instant};

use drtv_player::{Backend, TrackKind, clock};
use drtv_ui::theme::{self, ACCENT, BG, TEXT, TEXT_DIM, TEXT_FAINT};
use drtv_ui::{icons, widgets};
use egui::{
    Align, Color32, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::app::{DrtvApp, Menu, Route};
use crate::worker::{Cmd, Stream};

/// How long the HUD lingers after the last movement.
const HUD_LINGER: Duration = Duration::from_secs(3);
/// How often progress is written back to DR while playing.
const REPORT_EVERY: Duration = Duration::from_secs(15);

impl DrtvApp {
    pub(crate) fn player_ui(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);

        let Some((duration, paused, error)) =
            self.playback.as_ref().map(|p| (p.duration, p.paused, p.error.clone()))
        else {
            // A stream request that is still in flight, or one that failed and already went back.
            widgets::loading(ui, "Starter afspilning …");
            return;
        };

        // Letterbox: the callback stretches into whatever rectangle it is given, so the rectangle is
        // the one place where the aspect ratio is honoured.
        let aspect = self.video_aspect();
        let video_rect = aspect.map(|a| fit(rect, a)).unwrap_or(rect);

        if let Some(player) = &mut self.player {
            let ppp = ui.ctx().pixels_per_point();
            let size = (
                (video_rect.width() * ppp).round() as i32,
                (video_rect.height() * ppp).round() as i32,
            );
            if let Err(error) = player.render(size) {
                tracing::warn!("render failed: {error}");
            }
        }

        if let Some((framebuffer, size)) = self.player.as_ref().and_then(|p| p.framebuffer()) {
            let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
                drtv_player::blit(painter.gl(), &info, framebuffer, size);
            });
            ui.painter()
                .add(egui::PaintCallback { rect: video_rect, callback: Arc::new(callback) });
        }

        // One full-surface click target under the HUD: a click toggles playback, a double click goes
        // full screen, and either one wakes the HUD.
        let surface = ui.allocate_rect(rect, Sense::click());
        if surface.clicked() {
            if let Some(player) = &self.player {
                player.toggle_pause();
            }
            self.wake_hud();
        }
        if surface.double_clicked() {
            self.toggle_fullscreen(ui.ctx());
        }

        self.hud(ui, rect);

        if let Some(error) = error {
            let width = rect.width().min(520.0);
            let area = Rect::from_center_size(rect.center(), vec2(width, 140.0));
            ui.scope_builder(
                UiBuilder::new().max_rect(area).layout(Layout::top_down(Align::Center)),
                |ui| {
                    ui.label(
                        RichText::new("Afspilningen stoppede")
                            .font(theme::semibold(19.0))
                            .color(TEXT),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new(&error).color(TEXT_DIM).size(13.5));
                    ui.add_space(14.0);
                    if widgets::ghost_button(ui, "Tilbage", Some(icons::Icon::Back)).clicked() {
                        self.leave_player();
                    }
                },
            );
            return;
        }

        let buffering =
            duration <= 0.0 && self.player.as_ref().and_then(|p| p.position()).is_none() && !paused;
        if buffering {
            ui.painter().add(egui::Shape::circle_stroke(
                rect.center(),
                20.0,
                Stroke::new(2.0, ACCENT),
            ));
            ui.ctx().request_repaint_after(Duration::from_millis(120));
        }
    }

    fn video_aspect(&self) -> Option<f32> {
        let player = self.player.as_ref()?;
        let width: i64 = player.mpv().get_property("width").ok()?;
        let height: i64 = player.mpv().get_property("height").ok()?;
        if width <= 0 || height <= 0 {
            return None;
        }
        Some(width as f32 / height as f32)
    }

    fn hud(&mut self, ui: &mut Ui, rect: Rect) {
        let (paused, menu_open, hud_until) = match &self.playback {
            Some(p) => {
                (self.player.as_ref().is_some_and(|p| p.paused()), p.menu.is_some(), p.hud_until)
            }
            None => return,
        };
        let visible = paused || menu_open || hud_until.is_some_and(|until| until > Instant::now());
        if !visible {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            return;
        }

        // Geometry: a scrubber with a generous hit area, and a row of controls under it.
        let margin = 44.0;
        let bottom = rect.bottom() - 30.0;
        let scrub = Rect::from_min_max(
            pos2(rect.left() + margin, bottom - 54.0),
            pos2(rect.right() - margin, bottom - 24.0),
        );
        let controls = Rect::from_min_max(
            pos2(rect.left() + margin - 12.0, bottom - 20.0),
            pos2(rect.right() - margin + 12.0, bottom + 18.0),
        );
        let top = Rect::from_min_max(
            pos2(rect.left() + 24.0, rect.top() + 18.0),
            pos2(rect.right() - 24.0, rect.top() + 74.0),
        );

        widgets::gradient(
            ui,
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - 220.0), rect.max),
            Color32::TRANSPARENT,
            BG.gamma_multiply(0.92),
        );
        widgets::gradient(
            ui,
            Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 140.0)),
            BG.gamma_multiply(0.8),
            Color32::TRANSPARENT,
        );

        // Top bar: back and what is playing.
        ui.scope_builder(
            UiBuilder::new().max_rect(top).layout(Layout::left_to_right(Align::Center)),
            |ui| {
                if icons::icon_button(ui, icons::Icon::Back, 34.0, TEXT).clicked() {
                    self.leave_player();
                }
                ui.add_space(6.0);
                if let Some(playback) = &self.playback {
                    ui.label(
                        RichText::new(&playback.card.title).font(theme::semibold(17.0)).color(TEXT),
                    );
                    let kicker = playback.card.kicker();
                    if !kicker.is_empty() {
                        ui.label(RichText::new(kicker).color(TEXT_DIM).size(13.0));
                    }
                }
            },
        );

        let position = self.player.as_ref().and_then(|p| p.position()).unwrap_or(0.0);
        let duration = self
            .player
            .as_ref()
            .and_then(|p| p.duration())
            .filter(|d| *d > 0.0)
            .or_else(|| self.playback.as_ref().map(|p| p.duration))
            .unwrap_or(0.0);
        let markers = self.playback.as_ref().map(|p| p.markers.clone()).unwrap_or_default();
        let shown = self.playback.as_ref().and_then(|p| p.scrubbing).unwrap_or(position);

        ui.scope_builder(UiBuilder::new().max_rect(scrub), |ui| {
            let width = ui.available_width();
            if let Some(target) = scrubber(ui, width, shown, duration, &markers)
                && let Some(playback) = self.playback.as_mut()
            {
                playback.scrubbing = Some(target);
            }
        });

        // Commit a finished scrub, and keep the HUD awake while one is in progress.
        let scrubbing = self.playback.as_ref().and_then(|p| p.scrubbing);
        let pointer_down = ui.ctx().input(|i| i.pointer.any_down());
        if let Some(target) = scrubbing {
            self.wake_hud();
            if !pointer_down {
                if let Some(player) = &self.player {
                    player.seek_to(target);
                }
                if let Some(playback) = self.playback.as_mut() {
                    playback.scrubbing = None;
                    playback.last_reported = Instant::now() - REPORT_EVERY;
                }
            }
        }

        ui.scope_builder(
            UiBuilder::new().max_rect(controls).layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icons::icon_button(
                    ui,
                    if paused { icons::Icon::Play } else { icons::Icon::Pause },
                    38.0,
                    TEXT,
                )
                .clicked()
                {
                    if let Some(player) = &self.player {
                        player.toggle_pause();
                    }
                    self.wake_hud();
                }
                if icons::icon_button(ui, icons::Icon::ChevronLeft, 30.0, TEXT_DIM)
                    .on_hover_text("10 sekunder tilbage")
                    .clicked()
                {
                    self.seek_by(-10.0);
                }
                if icons::icon_button(ui, icons::Icon::ChevronRight, 30.0, TEXT_DIM)
                    .on_hover_text("10 sekunder frem")
                    .clicked()
                {
                    self.seek_by(10.0);
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("{} / {}", clock(shown), clock(duration)))
                        .color(TEXT_DIM)
                        .size(12.5),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::icon_button(ui, icons::Icon::Fullscreen, 30.0, TEXT_DIM).clicked() {
                        self.toggle_fullscreen(ui.ctx());
                    }
                    let audio_open =
                        self.playback.as_ref().is_some_and(|p| p.menu == Some(Menu::Audio));
                    if icons::icon_button(
                        ui,
                        icons::Icon::Audio,
                        30.0,
                        if audio_open { TEXT } else { TEXT_DIM },
                    )
                    .on_hover_text("Lydspor")
                    .clicked()
                    {
                        self.toggle_menu(Menu::Audio);
                    }
                    let subs_open =
                        self.playback.as_ref().is_some_and(|p| p.menu == Some(Menu::Subtitles));
                    if icons::icon_button(
                        ui,
                        icons::Icon::Subtitles,
                        30.0,
                        if subs_open { TEXT } else { TEXT_DIM },
                    )
                    .on_hover_text("Undertekster")
                    .clicked()
                    {
                        self.toggle_menu(Menu::Subtitles);
                    }
                    // The day-to-day control is the slider; the icons above open the track lists.
                    let mut volume = self.player.as_ref().map(|p| p.volume()).unwrap_or(100.0);
                    let muted = self.player.as_ref().is_some_and(|p| p.muted());
                    if icons::icon_button(
                        ui,
                        if muted || volume <= 0.0 {
                            icons::Icon::Muted
                        } else {
                            icons::Icon::Volume
                        },
                        30.0,
                        TEXT_DIM,
                    )
                    .clicked()
                        && let Some(player) = &self.player
                    {
                        player.set_muted(!muted);
                    }
                    ui.spacing_mut().slider_width = 84.0;
                    if ui
                        .add(
                            egui::Slider::new(&mut volume, 0.0..=100.0)
                                .show_value(false)
                                .trailing_fill(true),
                        )
                        .changed()
                        && let Some(player) = &self.player
                    {
                        player.set_volume(volume);
                        player.set_muted(volume <= 0.0);
                    }
                });
            },
        );

        self.menu_panel(ui, rect);
    }

    /// Mute from the keyboard. The HUD's icon and this are the same rule: flip the flag, leave the
    /// slider where the viewer put it.
    pub(crate) fn toggle_mute(&mut self) {
        if let Some(player) = &self.player {
            let muted = player.muted();
            player.set_muted(!muted);
        }
        self.wake_hud();
    }

    pub(crate) fn toggle_menu(&mut self, menu: Menu) {
        if let Some(playback) = self.playback.as_mut() {
            playback.menu = if playback.menu == Some(menu) { None } else { Some(menu) };
        }
        self.wake_hud();
    }

    fn menu_panel(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(menu) = self.playback.as_ref().and_then(|p| p.menu) else {
            return;
        };
        let kind = match menu {
            Menu::Subtitles => TrackKind::Subtitle,
            Menu::Audio => TrackKind::Audio,
        };
        let tracks: Vec<_> = self
            .player
            .as_ref()
            .map(|p| p.tracks())
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.kind == kind)
            .collect();

        let width = 240.0;
        let height = 46.0 + tracks.len().max(1) as f32 * 30.0 + 36.0;
        let anchor = Rect::from_min_size(
            pos2(rect.right() - 44.0 - width, rect.bottom() - 100.0 - height),
            vec2(width, height),
        );
        ui.painter().rect_filled(anchor, theme::RADIUS, Color32::from_rgb(0x12, 0x12, 0x16));
        ui.painter().rect_stroke(
            anchor,
            theme::RADIUS,
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );

        let mut picked: Option<Option<i64>> = None;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(anchor.shrink2(vec2(12.0, 10.0)))
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.label(
                    RichText::new(match menu {
                        Menu::Subtitles => "Undertekster",
                        Menu::Audio => "Lydspor",
                    })
                    .font(theme::semibold(13.0))
                    .color(TEXT),
                );
                ui.add_space(4.0);
                for track in &tracks {
                    if menu_row(ui, &track_label(track), track.selected) {
                        picked = Some(Some(track.id));
                    }
                }
                if tracks.is_empty() {
                    ui.label(RichText::new("Ingen spor").color(TEXT_FAINT).size(12.5));
                }
                ui.add_space(2.0);
                if menu_row(ui, "Fra", !tracks.iter().any(|t| t.selected)) {
                    picked = Some(None);
                }
            },
        );

        if let Some(picked) = picked {
            if let Some(player) = &self.player {
                match picked {
                    Some(id) => player.select_track(kind, id),
                    None => player.set_subtitles_visible(false),
                }
            }
            if let Some(playback) = self.playback.as_mut() {
                playback.menu = None;
            }
        }
    }

    fn wake_hud(&mut self) {
        if let Some(playback) = self.playback.as_mut() {
            playback.hud_until = Some(Instant::now() + HUD_LINGER);
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    /// Leave the player and go back one page, dropping the HUD's full screen on the way out.
    fn leave_player(&mut self) {
        self.pending_fullscreen = Some(false);
        self.back();
    }

    pub(crate) fn seek_by(&mut self, delta: f64) {
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let Some(position) = player.position() else {
            return;
        };
        let duration = player.duration().unwrap_or(f64::MAX);
        player.seek_to((position + delta).clamp(0.0, (duration - 1.0).max(0.0)));
        self.wake_hud();
    }

    /// Create the video surface the first time it is needed, and start the file. The player is kept
    /// alive between titles: reopening libmpv costs a few hundred milliseconds and a GL context.
    pub(crate) fn start_playback(&mut self, stream: Stream, ctx: &egui::Context) {
        if self.route != Route::Player {
            return;
        }
        let Some(playback) = self.playback.as_ref() else {
            return;
        };
        let resume = playback.card.resume_seconds() as f64;
        let known_duration = playback.duration;

        if self.player.is_none() {
            let (Some(gl), Some(loader)) = (self.gl.clone(), self.loader.clone()) else {
                self.toast("Afspilleren kunne ikke få en OpenGL-kontekst");
                self.back();
                return;
            };
            let wake = ctx.clone();
            let fonts = crate::config::fonts_dir();
            match drtv_player::VideoPlayer::with_fonts(
                gl,
                loader,
                move || wake.request_repaint(),
                fonts.as_deref(),
            ) {
                Ok(player) => self.player = Some(player),
                Err(error) => {
                    self.toast(format!("Afspilleren kunne ikke starte: {error}"));
                    self.back();
                    return;
                }
            }
        }

        // A resume point inside the last minute is not a resume point; DR's own client starts such a
        // title from the beginning.
        let start = (resume > 5.0 && (known_duration <= 0.0 || resume < known_duration - 60.0))
            .then_some(resume);
        if let (Some(player), Some(playback)) = (self.player.as_mut(), self.playback.as_mut()) {
            player.load(&stream.url, start);
            player.set_volume(playback.volume);
            playback.duration =
                if stream.duration_s > 0 { stream.duration_s as f64 } else { playback.duration };
            playback.markers = stream.markers;
            playback.last_position = start.unwrap_or(0.0);
        }
        let title = self.playback.as_ref().map(|p| p.card.title.clone()).unwrap_or_default();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{title} — DRTV")));
    }

    /// Leave the player: report where the viewer got to, then release the file. The next title reuses
    /// the same mpv instance.
    pub(crate) fn stop_playback(&mut self, report: bool) {
        let position = self
            .player
            .as_ref()
            .and_then(|p| p.position())
            .unwrap_or_else(|| self.playback.as_ref().map_or(0.0, |p| p.last_position));
        if let Some(playback) = &self.playback
            && report
        {
            if position > 5.0 {
                let _ = self.workers.api().send(Cmd::Progress {
                    item_id: playback.card.id.clone(),
                    position: position.round() as i64,
                });
            }
            self.progress.set(&playback.card.id, position, playback.duration);
            self.progress.save(&crate::config::progress_path());
        }
        if let Some(player) = &self.player {
            player.stop();
        }
        self.playback = None;
    }

    /// Per-frame bookkeeping while the player is open: mpv events, progress writes, HUD timing.
    pub(crate) fn playback_tick(&mut self, ctx: &egui::Context) {
        self.wake_on_input(ctx);

        let mut ended = false;
        let mut error: Option<String> = None;
        if let Some(player) = self.player.as_mut() {
            for event in player.poll() {
                match event {
                    drtv_player::Event::Loaded => {}
                    drtv_player::Event::VideoReconfig => ctx.request_repaint(),
                    drtv_player::Event::Ended { eof, error: why } => {
                        if eof {
                            ended = true;
                        } else {
                            error = why;
                        }
                    }
                }
            }
            player.apply_pending_seek();
        }

        let (position, duration, paused) = match self.player.as_ref() {
            Some(player) => (
                player.position().unwrap_or(0.0),
                player.duration().filter(|d| *d > 0.0),
                player.paused(),
            ),
            None => (0.0, None, false),
        };
        if let Some(playback) = self.playback.as_mut() {
            playback.last_position = position;
            playback.paused = paused;
            playback.error = error;
            if let Some(duration) = duration {
                playback.duration = duration;
            }
        }

        if ended {
            // The film reached its end: tell DR it is watched, then hand the viewer back their page.
            if let Some(playback) = &self.playback {
                let length = playback.duration.max(1.0) as i64;
                let _ = self
                    .workers
                    .api()
                    .send(Cmd::Progress { item_id: playback.card.id.clone(), position: length });
                // Finished, so there is nothing to resume — the page should offer the next
                // episode, not the last two minutes of this one.
                self.progress.clear(&playback.card.id);
                self.progress.save(&crate::config::progress_path());
                self.toast("Markeret som set");
            }
            self.stop_playback(false);
            let route = self.history.pop().unwrap_or(Route::Home);
            self.route = route.clone();
            self.load(route);
            return;
        }

        if let Some(playback) = self.playback.as_mut()
            && !paused
            && playback.last_reported.elapsed() >= REPORT_EVERY
            && position > 5.0
        {
            playback.last_reported = Instant::now();
            let _ = self.workers.api().send(Cmd::Progress {
                item_id: playback.card.id.clone(),
                position: position.round() as i64,
            });
            self.progress.set(&playback.card.id, position, playback.duration);
            self.progress.save(&crate::config::progress_path());
        }

        // The clock in the HUD has to tick even when mpv has no new frame to show.
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn wake_on_input(&mut self, ctx: &egui::Context) {
        let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_click());
        if moved {
            self.wake_hud();
        }
    }
}

/// Draw the scrubber and return a target while the pointer drags it. The hit area is the whole
/// 26-point band; the track is 4 points of it.
fn scrubber(
    ui: &mut Ui,
    width: f32,
    position: f64,
    duration: f64,
    markers: &[(bool, i64, i64)],
) -> Option<f64> {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click_and_drag());
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - 2.0),
        pos2(rect.right(), rect.center().y + 2.0),
    );
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, Color32::from_white_alpha(40));

    // Intro and credit windows, so a skip is visible before the viewer scrubs to find it.
    if duration > 0.0 {
        for (credits, start, end) in markers {
            let (start, end) = (*start as f64, *end as f64);
            if end <= start {
                continue;
            }
            let x0 = rect.left() + width * (start / duration).clamp(0.0, 1.0) as f32;
            let x1 = rect.left() + width * (end / duration).clamp(0.0, 1.0) as f32;
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, track.top()), pos2(x1.max(x0 + 2.0), track.bottom())),
                1.0,
                if *credits { TEXT_FAINT } else { TEXT_DIM }.gamma_multiply(0.55),
            );
        }
    }

    let frac = if duration > 0.0 { (position / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let head = rect.left() + width * frac;
    painter.rect_filled(
        Rect::from_min_max(track.left_top(), pos2(head, track.bottom())),
        2.0,
        ACCENT,
    );

    if response.hovered() || response.dragged() {
        painter.circle_filled(pos2(head, rect.center().y), 6.0, ACCENT);
    }

    let dragging = response.dragged() || response.clicked();
    if !dragging || duration <= 0.0 {
        if response.hovered()
            && let Some(pointer) = ui.ctx().pointer_latest_pos()
        {
            let frac = ((pointer.x - rect.left()) / width).clamp(0.0, 1.0);
            ui.painter().text(
                pos2(pointer.x, rect.top() - 4.0),
                egui::Align2::CENTER_BOTTOM,
                clock(duration * frac as f64),
                theme::regular(11.5),
                TEXT,
            );
        }
        return None;
    }

    let pointer = ui.ctx().pointer_latest_pos()?;
    let frac = ((pointer.x - rect.left()) / width).clamp(0.0, 1.0) as f64;
    Some((duration * frac).clamp(0.0, duration))
}

fn menu_row(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 6.0, theme::SURFACE_HI);
    }
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::regular(13.0),
        if selected { TEXT } else { TEXT_DIM },
    );
    if selected {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(14.0)),
            icons::Icon::Check,
            ACCENT,
        );
    }
    response.clicked()
}

fn track_label(track: &drtv_player::Track) -> String {
    if !track.title.is_empty() {
        return track.title.clone();
    }
    match track.lang.as_str() {
        "da" | "dan" => "Dansk".to_string(),
        "en" | "eng" => "Engelsk".to_string(),
        "" => format!("Spor {}", track.id),
        other => other.to_uppercase(),
    }
}

fn fit(rect: Rect, aspect: f32) -> Rect {
    let available = rect.width() / rect.height().max(1.0);
    if available > aspect {
        Rect::from_center_size(rect.center(), vec2(rect.height() * aspect, rect.height()))
    } else {
        Rect::from_center_size(rect.center(), vec2(rect.width(), rect.width() / aspect))
    }
}
