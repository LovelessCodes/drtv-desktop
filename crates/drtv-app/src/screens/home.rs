//! The front page: DR's own shelves, in DR's own order, six posters across.

use drtv_api::card::Card;
use egui::{RichText, Ui};

use crate::app::{DrtvApp, Load, Route};
use crate::screens::{prefetch, tile_width};
use drtv_ui::{theme, widgets};

impl DrtvApp {
    pub(crate) fn home_ui(&mut self, ui: &mut Ui) {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 18)).show(ui, |ui| match &self
            .home
        {
            Load::Loading => widgets::loading(ui, "Henter forsiden …"),
            Load::Failed(why) => {
                let why = why.clone();
                widgets::empty(ui, "Forsiden kunne ikke hentes", &why);
                if widgets::ghost_button(ui, "Prøv igen", None).clicked() {
                    self.load(Route::Home);
                }
            }
            Load::Ready(shelves) => {
                let tile = tile_width(ui.available_width());
                prefetch(&mut self.images, &self.workers, shelves, tile);
                let mut opened: Option<Card> = None;
                egui::ScrollArea::vertical().id_salt("home").auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        ui.add_space(4.0);
                        for shelf in shelves {
                            if shelf.title.is_empty() && shelf.items.is_empty() {
                                continue;
                            }
                            let images = &self.images;
                            let lookup = |url: &str| images.get(url);
                            widgets::shelf(ui, shelf, &lookup, tile, &mut |card| {
                                opened = Some(card.clone());
                            });
                        }
                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("Uofficiel klient. Data og video leveres af DR.")
                                .font(theme::regular(11.5))
                                .color(theme::TEXT_FAINT),
                        );
                        ui.add_space(12.0);
                    },
                );
                if let Some(card) = opened {
                    self.open_card(card);
                }
            }
        });
    }

    /// A click on any tile: the detail page, always — even for an episode. The player opens from the
    /// page, where the resume point and the description are visible together.
    pub(crate) fn open_card(&mut self, card: Card) {
        self.navigate(Route::Detail(card.id));
    }
}
