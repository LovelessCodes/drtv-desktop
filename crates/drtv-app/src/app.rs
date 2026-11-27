//! The eframe shell: routing, workers, and the frame loop that ties the screens together.
//!
//! The shape is deliberately plain — one struct owns the state, one `ui` pass per frame, screens are
//! `impl` blocks on it. Anything slow happens on a worker thread and arrives as a [`worker::Msg`];
//! the UI thread never blocks, so a slow network only ever shows a spinner.

use std::sync::Arc;

use drtv_api::auth;
use drtv_api::card::{Card, Detail, Shelf};
use egui::{Align, Layout, RichText, Stroke, Ui};
use std::time::{Duration, Instant};

use crate::images::ImageCache;
use crate::worker::{Cmd, Msg, PollState, Workers};
use drtv_ui::icons;
use drtv_ui::theme::{self, ACCENT, BG, BORDER, SURFACE, TEXT, TEXT_DIM, TEXT_FAINT};

/// Where the app is. `Detail` carries the id so a reload and a navigation are the same request.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Route {
    Home,
    Search,
    Detail(String),
    Player,
    Account,
}

/// A screen's data: exactly one of loading, ready or failed. There is no "empty but fine", because
/// an empty front page is an error DR itself would call a bug.
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
}

pub struct SearchState {
    pub term: String,
    pub load: Load<Vec<Shelf>>,
    /// Set while the box has focus and the term changed; the request is debounced so a term is not
    /// asked for once per keystroke.
    pub due: Option<Instant>,
    pub focus: bool,
}

pub struct AccountState {
    pub signed_in: bool,
    pub profile_name: String,
    pub code: Option<auth::DeviceCode>,
    pub polling: bool,
    pub error: Option<String>,
    pub profiles: Option<Vec<auth::Profile>>,
    pub busy: bool,
}

/// The HUD's open menu, at most one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Subtitles,
    Audio,
}

pub struct Playback {
    pub card: Card,
    pub duration: f64,
    pub markers: Vec<(bool, i64, i64)>,
    pub menu: Option<Menu>,
    /// Position the viewer asked for with a scrub that has not been committed to mpv yet.
    pub scrubbing: Option<f64>,
    pub hud_until: Option<Instant>,
    pub last_reported: Instant,
    pub last_position: f64,
    pub error: Option<String>,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
}

pub struct DrtvApp {
    pub(crate) route: Route,
    pub(crate) history: Vec<Route>,
    pub(crate) workers: Workers,
    pub(crate) images: ImageCache,
    pub(crate) home: Load<Vec<Shelf>>,
    pub(crate) detail: Load<Detail>,
    /// The other episodes of the season an episode belongs to. Empty for everything else.
    pub(crate) siblings: Load<Detail>,
    pub(crate) search: SearchState,
    pub(crate) account: AccountState,
    pub(crate) playback: Option<Playback>,
    pub(crate) player: Option<drtv_player::VideoPlayer>,
    pub(crate) gl: Option<Arc<glow::Context>>,
    pub(crate) loader:
        Option<Arc<dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void + Send + Sync>>,
    /// Bumped on every navigation the screen should respond to; a message carrying an older
    /// generation belongs to a screen the viewer already left.
    pub(crate) generation: u64,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) fullscreen: bool,
    /// A fullscreen change to send once the frame has the context to send it with.
    pub(crate) pending_fullscreen: Option<bool>,
    /// Set by `DRTV_START=play:<id>`: open the title and start it as soon as its detail loads. A
    /// smoke test needs a way past the mouse; nothing else sets this.
    pub(crate) autoplay: bool,
    pub(crate) timeout: Duration,
    /// How far the viewer got, per title, on this machine. Written while playing and read when a
    /// title opens, so the player resumes even for a viewer who never signed in.
    pub(crate) progress: crate::progress::Progress,
}

impl DrtvApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let stored = auth::load();
        let mut app = Self {
            route: Route::Home,
            history: Vec::new(),
            workers: Workers::new(&cc.egui_ctx),
            images: ImageCache::new(),
            home: Load::Loading,
            detail: Load::Loading,
            siblings: Load::Loading,
            search: SearchState {
                term: String::new(),
                load: Load::Loading,
                due: None,
                focus: false,
            },
            account: AccountState {
                signed_in: stored.is_some(),
                profile_name: String::new(),
                code: None,
                polling: false,
                error: None,
                profiles: None,
                busy: false,
            },
            playback: None,
            player: None,
            gl: cc.gl.clone(),
            loader: cc.get_proc_address.clone().map(|f| f as _),
            generation: 0,
            toast: None,
            fullscreen: false,
            pending_fullscreen: None,
            autoplay: false,
            timeout: Duration::from_secs(4),
            progress: crate::progress::Progress::load(&crate::config::progress_path()),
        };
        app.load(Route::Home);
        if app.account.signed_in {
            let _ = app.workers.api().send(Cmd::Profiles);
        }
        if let Ok(start) = std::env::var("DRTV_START") {
            if let Some(id) = start.strip_prefix("detail:") {
                app.navigate(Route::Detail(id.to_string()));
            } else if let Some(id) = start.strip_prefix("play:") {
                app.autoplay = true;
                app.navigate(Route::Detail(id.to_string()));
            } else if start == "account" {
                app.navigate(Route::Account);
            } else if let Some(term) = start.strip_prefix("search:") {
                app.navigate(Route::Search);
                app.search.term = term.to_string();
                app.search.focus = true;
                app.load(Route::Search);
            }
        }
        app
    }

    /// Put this machine's own positions onto a freshly loaded detail page.
    ///
    /// DR's watch marks only exist for a signed-in profile; these exist for everyone, and the later
    /// of the two wins, so signing in tomorrow does not undo a session watched tonight.
    fn resume_marks(&self, detail: &mut Detail) {
        let local = |card: &mut drtv_api::card::Card| {
            if let Some(seconds) = self.progress.get(&card.id)
                && seconds * 1000 > card.resume_ms
            {
                card.resume_ms = seconds * 1000;
            }
        };
        local(&mut detail.card);
        for episode in &mut detail.episodes {
            local(episode);
        }
    }

    /// Ask the workers for whatever the route needs. Called on entry to a route, never per frame.
    pub(crate) fn load(&mut self, route: Route) {
        self.generation += 1;
        let generation = self.generation;
        let api = self.workers.api();
        match route {
            Route::Home => {
                self.home = Load::Loading;
                let _ = api.send(Cmd::Home { generation });
            }
            Route::Search => {
                if !self.search.term.trim().is_empty() {
                    self.search.load = Load::Loading;
                    let term = self.search.term.trim().to_string();
                    let _ = api.send(Cmd::Search { term, generation });
                } else {
                    self.search.load = Load::Ready(Vec::new());
                }
            }
            Route::Detail(id) => {
                self.detail = Load::Loading;
                self.siblings = Load::Loading;
                let _ = api.send(Cmd::Detail { id, generation });
            }
            Route::Account => {
                if self.account.profiles.is_none() && self.account.signed_in {
                    let _ = api.send(Cmd::Profiles);
                }
            }
            Route::Player => {}
        }
    }

    pub(crate) fn navigate(&mut self, route: Route) {
        if self.route == route {
            return;
        }
        self.history.push(self.route.clone());
        self.route = route.clone();
        self.load(route);
    }

    pub(crate) fn back(&mut self) {
        // Leaving the player is not the same as going back a page: it should stop playback and
        // return where the title was opened from.
        if self.route == Route::Player {
            self.stop_playback(true);
            self.pending_fullscreen = Some(false);
        }
        let route = self.history.pop().unwrap_or(Route::Home);
        if route != self.route {
            self.route = route.clone();
            self.load(route);
        }
    }

    pub(crate) fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }
}

impl eframe::App for DrtvApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(want) = self.pending_fullscreen.take() {
            self.fullscreen = want;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(want));
        }
        self.drain(&ctx);
        self.shortcuts(&ctx);

        // The player owns the window while it is open: no bar, no page margins, no theme chrome.
        if self.route == Route::Player {
            self.player_ui(ui);
        } else {
            self.top_bar(ui);
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(
                ui,
                |ui| match self.route.clone() {
                    Route::Home => self.home_ui(ui),
                    Route::Search => self.search_ui(ui),
                    Route::Detail(id) => self.detail_ui(ui, &id),
                    Route::Account => self.account_ui(ui),
                    Route::Player => {}
                },
            );
        }

        self.toast_ui(&ctx);
        if self.route == Route::Player {
            self.playback_tick(&ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        if let Some(playback) = &self.playback {
            let position = self
                .player
                .as_ref()
                .and_then(|p| drtv_player::Backend::position(p))
                .unwrap_or(playback.last_position);
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
            drtv_player::Backend::stop(player);
        }
    }
}

impl DrtvApp {
    // ---------------------------------------------------------------- messages

    fn drain(&mut self, ctx: &egui::Context) {
        for msg in self.workers.drain() {
            match msg {
                Msg::Home { generation, result } if generation == self.generation => {
                    self.home = match result {
                        Ok(shelves) => Load::Ready(shelves),
                        Err(why) => Load::Failed(why),
                    };
                }
                Msg::Search { generation, result } if generation == self.generation => {
                    self.search.load = match result {
                        Ok(shelves) => Load::Ready(shelves),
                        Err(why) => Load::Failed(why),
                    };
                }
                Msg::Detail { generation, result } if generation == self.generation => {
                    self.detail = match result {
                        Ok(mut detail) => {
                            self.resume_marks(&mut detail);
                            Load::Ready(detail)
                        }
                        Err(why) => Load::Failed(why),
                    };
                    if let Load::Ready(detail) = &self.detail
                        && !detail.card.season_id.is_empty()
                    {
                        // An episode page is otherwise the only page in the app with nothing under
                        // its hero: the episode has no children of its own, its season does.
                        let id = detail.card.season_id.clone();
                        let _ = self.workers.api().send(Cmd::Siblings { id, generation });
                    }
                    if self.autoplay
                        && let Load::Ready(detail) = &self.detail
                    {
                        self.autoplay = false;
                        if let Some(card) = crate::screens::detail::playable(detail) {
                            self.play(card);
                        }
                    }
                }
                Msg::Siblings { generation, result } if generation == self.generation => {
                    self.siblings = match result {
                        Ok(mut detail) => {
                            self.resume_marks(&mut detail);
                            Load::Ready(detail)
                        }
                        Err(_) => Load::Ready(Detail::default()),
                    };
                }
                Msg::Stream { generation, result } if generation == self.generation => match result
                {
                    Ok(stream) => self.start_playback(stream, ctx),
                    Err(why) => {
                        self.playback = None;
                        self.route = self.history.pop().unwrap_or(Route::Home);
                        let route = self.route.clone();
                        self.load(route);
                        self.toast(why);
                    }
                },
                Msg::Image { url, result } => match result {
                    Ok(image) => self.images.insert(ctx, &url, image),
                    Err(_) => self.images.failed(&url),
                },
                Msg::SignIn { result, .. } => {
                    self.account.busy = false;
                    match result {
                        Ok(code) => {
                            self.account.code = Some(code);
                            self.account.error = None;
                            self.account.polling = true;
                        }
                        Err(why) => self.account.error = Some(why),
                    }
                }
                Msg::SignInPoll { state, .. } => match state {
                    PollState::Pending => {}
                    PollState::Approved => {
                        self.account.polling = false;
                        self.account.code = None;
                        self.account.signed_in = true;
                        self.account.busy = true;
                        let _ = self.workers.api().send(Cmd::Profiles);
                        let _ = self.workers.api().send(Cmd::Home { generation: self.generation });
                    }
                    PollState::Failed(why) => {
                        self.account.polling = false;
                        self.account.code = None;
                        self.account.error = Some(why);
                    }
                },
                Msg::Profiles(result) => {
                    self.account.busy = false;
                    match result {
                        Ok(profiles) => {
                            self.account.profiles = Some(profiles);
                            self.account.signed_in = true;
                        }
                        Err(why) => {
                            if !self.account.signed_in {
                                self.account.error = Some(why);
                            }
                        }
                    }
                    if let Some(stored) = auth::load() {
                        self.account.profile_name = stored.name.clone();
                    }
                }
                Msg::Seated { result, .. } => {
                    self.account.busy = false;
                    match result {
                        Ok(stored) => {
                            self.account.signed_in = true;
                            self.account.profile_name = stored.name.clone();
                            self.account.error = None;
                            self.load(Route::Home);
                        }
                        Err(why) => self.account.error = Some(why),
                    }
                }
                Msg::SignedOut => {
                    self.account.signed_in = false;
                    self.account.profile_name.clear();
                    self.account.profiles = None;
                    self.account.code = None;
                    self.account.error = None;
                    self.home = Load::Loading;
                    self.load(Route::Home);
                }
                Msg::Progress(result) => {
                    if let Err(status) = result {
                        tracing::debug!("progress write refused with HTTP {status}");
                    }
                }
                // A message for a screen the viewer already left: dropping it is the point of the
                // generation counter.
                _ => {}
            }
        }
    }

    // ---------------------------------------------------------------- chrome

    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("nav")
            .exact_size(64.0)
            .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::symmetric(28, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let mark =
                        ui.label(RichText::new("DRTV").font(theme::semibold(19.0)).color(ACCENT));
                    mark.on_hover_text("Uofficiel desktop-klient");
                    ui.label(RichText::new("desktop").font(theme::regular(13.0)).color(TEXT_FAINT));
                    ui.add_space(22.0);

                    // Back sits before the sections, where a browser puts it: after them it reads as
                    // a fifth page.
                    if !self.history.is_empty() && self.route != Route::Home {
                        if icons::icon_button(ui, icons::Icon::Back, 26.0, TEXT_DIM)
                            .on_hover_text("Tilbage")
                            .clicked()
                        {
                            self.back();
                        }
                        ui.add_space(10.0);
                    }

                    for (label, route) in [("Hjem", Route::Home), ("Søg", Route::Search)] {
                        if nav_item(ui, label, self.route == route).clicked() {
                            self.navigate(route);
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.account.signed_in {
                            if self.account.profile_name.is_empty() {
                                "Konto".to_string()
                            } else {
                                self.account.profile_name.clone()
                            }
                        } else {
                            "Log ind".to_string()
                        };
                        let active = self.route == Route::Account;
                        if nav_item(ui, &label, active).clicked() {
                            self.navigate(Route::Account);
                        }
                    });
                });
            });
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((message, shown)) = &self.toast else {
            return;
        };
        if shown.elapsed() > self.timeout {
            self.toast = None;
            return;
        }
        let alpha = ((self.timeout.as_secs_f32() - shown.elapsed().as_secs_f32()) / 0.6).min(1.0);
        egui::Area::new(egui::Id::unique(egui::IdSalt::new("drtv-toast")))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(SURFACE.gamma_multiply(alpha.clamp(0.35, 1.0)))
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .show(ui, |ui| {
                        ui.label(RichText::new(message).color(TEXT).size(13.5));
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    // ---------------------------------------------------------------- keyboard

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                if self.route == Route::Player {
                    self.back();
                } else if self.route != Route::Home {
                    self.back();
                }
            }
            if typing {
                return;
            }
            // **Any of the three.** Which modifier bit arrives for ⌘K depends on the backend and on
            // what sits between the keyboard and the app — a remote desktop session or a keyboard
            // layout can deliver it as `mac_cmd` or as `ctrl`, and a shortcut that works only under
            // one of them is a shortcut that does not work. `/` is the same action without a
            // modifier, for anyone who never finds out.
            let command = i.modifiers.command || i.modifiers.mac_cmd || i.modifiers.ctrl;
            if command && i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::Slash) {
                self.navigate(Route::Search);
                self.search.focus = true;
            }
            if self.route == Route::Player {
                if i.key_pressed(egui::Key::Space)
                    && let Some(player) = &self.player
                {
                    drtv_player::Backend::toggle_pause(player);
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.seek_by(10.0);
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.seek_by(-10.0);
                }
                if i.key_pressed(egui::Key::F) {
                    self.fullscreen = !self.fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
                // The track menus are the one part of the HUD with no keyboard of its own; a viewer
                // watching with the pointer parked should not have to find a 16-point icon.
                if i.key_pressed(egui::Key::S) {
                    self.toggle_menu(Menu::Subtitles);
                }
                if i.key_pressed(egui::Key::A) {
                    self.toggle_menu(Menu::Audio);
                }
                if i.key_pressed(egui::Key::M) {
                    self.toggle_mute();
                }
            }
        });
    }
}

/// A nav label: dim until it is the current page, with a red rule under the word when it is.
fn nav_item(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let font = theme::semibold(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, TEXT);
    let size = egui::vec2(galley.size().x + 8.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = if active {
        TEXT
    } else if response.hovered() {
        TEXT_DIM
    } else {
        TEXT_FAINT
    };
    let painter = ui.painter();
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    if active {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + 4.0, rect.bottom() - 3.0),
                egui::pos2(rect.right() - 4.0, rect.bottom()),
            ),
            1.0,
            ACCENT,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
