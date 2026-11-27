//! The threads that talk to DR, and the messages they send back.
//!
//! Two pools, because the two kinds of work have different shapes: a handful of API calls that must
//! not queue behind anything, and a burst of a hundred image downloads that must not block them.
//! Everything is blocking `std` threads — the work is I/O and the counts are small.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use drtv_api::auth;
use drtv_api::{api, card};

/// A request from the UI thread.
#[derive(Debug)]
pub enum Cmd {
    Home {
        generation: u64,
    },
    Detail {
        id: String,
        generation: u64,
    },
    Search {
        term: String,
        generation: u64,
    },
    Stream {
        id: String,
        generation: u64,
    },
    Image {
        url: String,
        width: u32,
        height: u32,
    },
    SignIn {
        generation: u64,
    },
    PollSignIn {
        code: auth::DeviceCode,
        interval: u64,
        generation: u64,
    },
    Profiles,
    Seat {
        id: String,
        segments: Vec<String>,
        generation: u64,
    },
    SignOut,
    /// Write playback progress, in SECONDS. A position at or past the item's length is what makes
    /// DR mark it watched; there is no separate "mark watched" call.
    Progress { item_id: String, position: i64 },
}

/// What a thread sends back.
#[derive(Debug)]
pub enum Msg {
    Home {
        generation: u64,
        result: Result<Vec<card::Shelf>, String>,
    },
    Detail {
        generation: u64,
        result: Result<card::Detail, String>,
    },
    Search {
        generation: u64,
        result: Result<Vec<card::Shelf>, String>,
    },
    Stream {
        generation: u64,
        result: Result<Stream, String>,
    },
    Image {
        url: String,
        result: Result<egui::ColorImage, String>,
    },
    SignIn {
        generation: u64,
        result: Result<auth::DeviceCode, String>,
    },
    SignInPoll {
        generation: u64,
        state: PollState,
    },
    Profiles(Result<Vec<auth::Profile>, String>),
    Seated {
        generation: u64,
        result: Result<auth::Stored, String>,
    },
    SignedOut,
    Progress(Result<(), i32>),
}

#[derive(Debug, Clone)]
pub enum PollState {
    Pending,
    Approved,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Stream {
    pub url: String,
    /// Skip-intro and end-credit windows, as `(is_credits, start_s, end_s)`.
    pub markers: Vec<(bool, i64, i64)>,
    /// The item's own length, when DR stated one; the player prefers this to nothing.
    pub duration_s: i64,
}

pub struct Workers {
    api: Sender<Cmd>,
    images: Sender<Cmd>,
    pub rx: Receiver<Msg>,
    threads: Vec<JoinHandle<()>>,
}

impl Workers {
    pub fn new(ctx: &egui::Context) -> Self {
        let (api_tx, api_rx) = channel::<Cmd>();
        let (image_tx, image_rx) = channel::<Cmd>();
        let (msg_tx, msg_rx) = channel::<Msg>();
        let api_rx = std::sync::Arc::new(std::sync::Mutex::new(api_rx));
        let image_rx = std::sync::Arc::new(std::sync::Mutex::new(image_rx));

        let mut threads = Vec::new();
        // Two API threads: a screen change should not wait behind the previous screen's read.
        for n in 0..2 {
            let rx = std::sync::Arc::clone(&api_rx);
            let tx = msg_tx.clone();
            let ctx = ctx.clone();
            threads.push(
                std::thread::Builder::new()
                    .name(format!("drtv-api-{n}"))
                    .spawn(move || loop {
                        let Ok(cmd) = rx.lock().expect("api queue").recv() else {
                            return;
                        };
                        let Some(msg) = api_command(cmd) else { continue };
                        wake(&tx, &ctx, msg);
                    })
                    .expect("spawn api thread"),
            );
        }
        // Three image threads: a poster is ~40 KB and a shelf is 24 of them.
        for n in 0..3 {
            let rx = std::sync::Arc::clone(&image_rx);
            let tx = msg_tx.clone();
            let ctx = ctx.clone();
            threads.push(
                std::thread::Builder::new()
                    .name(format!("drtv-image-{n}"))
                    .spawn(move || loop {
                        let Ok(cmd) = rx.lock().expect("image queue").recv() else {
                            return;
                        };
                        if let Some(msg) = image_command(cmd) {
                            wake(&tx, &ctx, msg);
                        }
                    })
                    .expect("spawn image thread"),
            );
        }

        Self {
            api: api_tx,
            images: image_tx,
            rx: msg_rx,
            threads,
        }
    }

    pub fn api(&self) -> &Sender<Cmd> {
        &self.api
    }

    pub fn images(&self) -> &Sender<Cmd> {
        &self.images
    }

    /// Everything that arrived since last frame.
    pub fn drain(&self) -> Vec<Msg> {
        self.rx.try_iter().collect()
    }

    /// Ask every thread to finish once its current request is done, then join. Called on exit so a
    /// request in flight cannot outlive the window it was made for.
    pub fn shutdown(self) {
        drop(self.api);
        drop(self.images);
        drop(self.rx);
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

fn wake(tx: &Sender<Msg>, ctx: &egui::Context, msg: Msg) {
    if tx.send(msg).is_ok() {
        ctx.request_repaint();
    }
}

/// A usable session, refreshed if it is close to expiring. `None` means "act signed out": either
/// nobody is signed in, or the refresh token is dead and only a new code will help.
fn session() -> Option<auth::Stored> {
    auth::load()?;
    match auth::ensure_fresh() {
        auth::Freshness::Ok(stored) | auth::Freshness::Refreshed(stored) => Some(stored),
        auth::Freshness::NeedsSignIn(why) => {
            tracing::info!("session needs a new sign-in: {why}");
            None
        }
    }
}

fn token(stored: &Option<auth::Stored>) -> String {
    stored.as_ref().map(|s| s.profile.clone()).unwrap_or_default()
}

fn api_command(cmd: Cmd) -> Option<Msg> {
    match cmd {
        Cmd::Home { generation } => {
            let stored = session();
            let universe = auth::universe();
            let watches = api::watched(&token(&stored));
            let Some(page) = api::home(&universe) else {
                return Some(Msg::Home {
                    generation,
                    result: Err("DR svarede ikke på forsiden".to_string()),
                });
            };
            let mut shelves = card::shelves(&page, &watches);
            let pending = card::pending_list_ids(&shelves);
            if !pending.is_empty() {
                let lists = api::lists(&pending, &universe);
                card::fill_lists(&mut shelves, &lists, &watches);
            }
            Some(Msg::Home {
                generation,
                result: Ok(shelves),
            })
        }

        Cmd::Detail { id, generation } => {
            let stored = session();
            let watches = api::watched(&token(&stored));
            let Some(item) = api::expanded(&id) else {
                return Some(Msg::Detail {
                    generation,
                    result: Err("Titlen kunne ikke hentes".to_string()),
                });
            };
            Some(Msg::Detail {
                generation,
                result: Ok(card::detail(&item, &watches)),
            })
        }

        Cmd::Search { term, generation } => {
            let stored = session();
            let universe = auth::universe();
            let watches = api::watched(&token(&stored));
            let Some(reply) = api::search(&term, &token(&stored), &universe) else {
                return Some(Msg::Search {
                    generation,
                    result: Err("Søgningen kunne ikke gennemføres".to_string()),
                });
            };
            // DR answers in named groups; keep the ones that carry items, in its own order, and
            // rename the two that read oddly in a desktop layout.
            let groups = [
                (&reply.playable, "Titler"),
                (&reply.series, "Serier"),
                (&reply.movies, "Film"),
                (&reply.tv, "TV"),
            ];
            let mut shelves = Vec::new();
            for (group, fallback) in groups {
                if group.items.is_empty() {
                    continue;
                }
                let title = if group.title.is_empty() {
                    fallback.to_string()
                } else {
                    group.title.clone()
                };
                shelves.push(card::Shelf {
                    id: format!("search-{title}"),
                    title,
                    items: group.items.iter().map(|i| card::card(i, watches.get(&i.id))).collect(),
                });
            }
            Some(Msg::Search {
                generation,
                result: Ok(shelves),
            })
        }

        Cmd::Stream { id, generation } => {
            let Some(files) = api::videos(&id, api::Resolution::Best) else {
                return Some(Msg::Stream {
                    generation,
                    result: Err("DR havde ingen afspilning til denne titel".to_string()),
                });
            };
            let Some(file) = api::stream_file(&files) else {
                let drm = files.iter().any(|f| !f.playable());
                let message = if drm {
                    "Titlen er DRM-beskyttet (PlayReady) og kan ikke afspilles her"
                } else {
                    "Ingen afspilningsbar fil i svaret"
                };
                return Some(Msg::Stream {
                    generation,
                    result: Err(message.to_string()),
                });
            };
            Some(Msg::Stream {
                generation,
                result: Ok(Stream {
                    url: file.url.clone(),
                    markers: api::markers(file),
                    duration_s: api::item(&id).map(|i| i.duration).unwrap_or(0),
                }),
            })
        }

        Cmd::SignIn { generation } => Some(Msg::SignIn {
            generation,
            result: auth::start().ok_or_else(|| "DR svarede ikke på login".to_string()),
        }),

        Cmd::PollSignIn {
            code,
            interval,
            generation,
        } => {
            std::thread::sleep(std::time::Duration::from_secs(interval.max(1)));
            let state = match auth::poll(&code) {
                auth::Poll::Pending => PollState::Pending,
                auth::Poll::SlowDown => PollState::Pending,
                auth::Poll::Failed(why) => PollState::Failed(why),
                auth::Poll::Approved {
                    access,
                    id,
                    refresh,
                } => match auth::exchange(&access, &id, &refresh) {
                    Some(stored) => {
                        if auth::save(&stored) {
                            PollState::Approved
                        } else {
                            PollState::Failed("sessionen kunne ikke gemmes".to_string())
                        }
                    }
                    None => PollState::Failed("DR afviste login".to_string()),
                },
            };
            Some(Msg::SignInPoll { generation, state })
        }

        Cmd::Profiles => Some(Msg::Profiles(
            auth::profiles().ok_or_else(|| "Profil-listen kunne ikke hentes".to_string()),
        )),

        Cmd::Seat {
            id,
            segments,
            generation,
        } => {
            let result = match auth::select_profile(&id, &segments) {
                Ok(stored) => Ok(stored),
                Err(refusal) => Err(refusal.why()),
            };
            Some(Msg::Seated { generation, result })
        }

        Cmd::SignOut => {
            auth::sign_out();
            Some(Msg::SignedOut)
        }

        Cmd::Progress { item_id, position } => {
            let Some(stored) = session() else {
                return None;
            };
            Some(Msg::Progress(api::set_watched(
                &stored.profile,
                &item_id,
                position.max(0),
            )))
        }

        // Image work is only ever sent on the image queue.
        Cmd::Image { .. } => None,
    }
}

fn image_command(cmd: Cmd) -> Option<Msg> {
    let Cmd::Image {
        url,
        width,
        height,
    } = cmd
    else {
        return None;
    };
    Some(Msg::Image {
        result: fetch_image(&url, width, height),
        url,
    })
}

fn fetch_image(url: &str, width: u32, height: u32) -> Result<egui::ColorImage, String> {
    let url = drtv_api::card::sized(url, width, height);
    let headers = vec!["Accept: image/avif,image/webp,image/jpeg".to_string()];
    let resp = drtv_net::get(&url, &headers).map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status));
    }
    let image = image::load_from_memory(&resp.body).map_err(|e| e.to_string())?;
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}
