//! The look of the app: one dark palette, one type scale and the two or three widgets every screen
//! composes.
//!
//! Keeping this crate free of app state is what lets a screen stay a layout function: a tile takes a
//! card and a texture, not a store.

pub mod icon;
pub mod icons;
pub mod theme;
pub mod widgets;

pub use icons::{Icon, icon_button};
pub use theme::{ACCENT, BG, BORDER, TEXT, TEXT_DIM, TEXT_FAINT, install};

/// Inter, under the SIL Open Font Licence (see `assets/fonts/OFL.txt`). Three weights: the tile
/// titles and screen headings need a semibold, and egui's built-in font has one weight only.
pub(crate) const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
pub(crate) const FONT_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
pub(crate) const FONT_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
