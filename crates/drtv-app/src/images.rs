//! Artwork, by URL.
//!
//! DR's CDN resizes server-side, so the cache asks for the pixels a tile will actually draw and then
//! keeps the decoded texture. Eviction is by insertion order: the app is a browser with one screen
//! in front at a time, and a texture that scrolled off a shelf five minutes ago is the one to drop.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::worker::{Cmd, Workers};

pub struct ImageCache {
    textures: HashMap<String, egui::TextureHandle>,
    started: HashSet<String>,
    /// URLs that already failed once. Retrying them every frame would be a request storm.
    dead: HashSet<String>,
    /// Insertion order, oldest first. The bound is generous: a shelf is 24 posters, and four
    /// screens of history is still less than 30 MB of texture.
    order: VecDeque<String>,
    limit: usize,
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            started: HashSet::new(),
            dead: HashSet::new(),
            order: VecDeque::new(),
            limit: 300,
        }
    }

    /// The texture for `url`, if it has arrived. Cheap: a handle clone is an `Arc` bump.
    pub fn get(&self, url: &str) -> Option<egui::TextureHandle> {
        if url.is_empty() {
            return None;
        }
        self.textures.get(url).cloned()
    }

    /// Ask a worker for `url` at the pixels the tile will draw, unless it is already here or on its
    /// way. Called for every visible card every frame, so it must stay a hash lookup.
    pub fn want(&mut self, workers: &Workers, url: &str, width: u32, height: u32) {
        if url.is_empty()
            || self.textures.contains_key(url)
            || self.started.contains(url)
            || self.dead.contains(url)
        {
            return;
        }
        if workers
            .images()
            .send(Cmd::Image {
                url: url.to_string(),
                width,
                height,
            })
            .is_ok()
        {
            self.started.insert(url.to_string());
        }
    }

    /// A decoded image from a worker. Uploading is the UI thread's job; that is the only place the
    /// egui context may be touched.
    pub fn insert(&mut self, ctx: &egui::Context, url: &str, image: egui::ColorImage) {
        self.started.remove(url);
        let name = format!("art:{url}");
        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
        self.textures.insert(url.to_string(), texture);
        self.order.push_back(url.to_string());
        while self.order.len() > self.limit {
            if let Some(evicted) = self.order.pop_front()
                && self.textures.remove(&evicted).is_some()
            {
                self.started.remove(&evicted);
            }
        }
    }

    /// A URL that failed stays failed for this session: retrying a 404 on every frame is how a UI
    /// turns into a request storm. The tile draws its placeholder instead.
    pub fn failed(&mut self, url: &str) {
        self.started.remove(url);
        self.textures.remove(url);
        self.dead.insert(url.to_string());
        tracing::debug!("artwork unavailable: {url}");
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}
