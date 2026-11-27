//! The application layer: an eframe shell, five screens, worker threads and an image cache.
//!
//! The crates underneath know about DR (the API), the network, the player and the look; this crate
//! is the only one that knows about *the app* — which screen is open, what is loaded, and what the
//! viewer is allowed to see while it loads.

pub mod app;
pub mod config;
pub mod images;
pub mod progress;
pub mod screens;
pub mod worker;

pub use app::DrtvApp;
