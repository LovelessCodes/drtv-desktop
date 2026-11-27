//! The five screens. Each is an `impl DrtvApp` block: they share the app's state, and a screen that
//! needed its own struct would need a place to put it that is not the app anyway.

mod account;
mod detail;
mod home;
mod player;
mod search;

use drtv_api::card::Shelf;

use crate::images::ImageCache;
use crate::worker::Workers;

/// The width a portrait tile takes at this window size: six across at 1440, never wider than a
/// poster's native 2:3 crop, never so narrow that a Danish title wraps into three lines.
pub fn tile_width(available: f32) -> f32 {
    const GAP: f32 = 12.0;
    let columns = if available < 900.0 { 4.0 } else { 6.0 };
    let width = (available - GAP * (columns - 1.0)) / columns;
    width.clamp(132.0, 226.0)
}

/// Ask for every poster on the page, once. The cache dedupes, so this is a hash lookup per card per
/// frame — cheap enough that the UI needs no notion of visibility.
fn prefetch(images: &mut ImageCache, workers: &Workers, shelves: &[Shelf], tile: f32) {
    let (w, h) = ((tile * 2.0) as u32, (tile * 3.0) as u32);
    for shelf in shelves {
        for card in &shelf.items {
            let url = if !card.poster.is_empty() {
                &card.poster
            } else {
                &card.still
            };
            images.want(workers, url, w, h);
        }
    }
}

fn prefetch_wide(images: &mut ImageCache, workers: &Workers, cards: &[drtv_api::card::Card], width: f32) {
    let (w, h) = ((width * 2.0) as u32, (width * 2.0 / 16.0 * 9.0) as u32);
    for card in cards {
        let url = if !card.still.is_empty() {
            &card.still
        } else {
            &card.poster
        };
        images.want(workers, url, w, h);
    }
}
