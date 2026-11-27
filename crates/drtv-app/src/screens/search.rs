//! Search: one box, then DR's own groups.
//!
//! The term is debounced by a third of a second. Typing "matador" is six keystrokes and DR's search
//! is a full catalogue query; asking six times would make the sixth answer arrive after the first
//! one has been drawn.

use std::time::Duration;

use drtv_api::card::Card;
use drtv_ui::theme::{self, BORDER, SURFACE, TEXT, TEXT_FAINT};
use drtv_ui::{icons, widgets};
use egui::{Align, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::{DrtvApp, Load};
use crate::screens::{prefetch, tile_width};
use crate::worker::Cmd;

const DEBOUNCE: Duration = Duration::from_millis(320);

impl DrtvApp {
    pub(crate) fn search_ui(&mut self, ui: &mut Ui) {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 16)).show(ui, |ui| {
            self.search_box(ui);
            ui.add_space(16.0);

            let mut opened: Option<Card> = None;
            let blank = self.search.term.trim().is_empty();
            match &self.search.load {
                Load::Loading => {
                    if !self.search.term.trim().is_empty() {
                        widgets::loading(ui, "Søger …");
                    } else {
                        hint(ui, "Søg i hele DRTV — film, serier og dokumentarer.");
                    }
                }
                Load::Failed(why) => {
                    let why = why.clone();
                    widgets::empty(ui, "Søgningen fejlede", &why);
                }
                // An empty term is not a search that found nothing; saying so before the first
                // keystroke is a small lie the screen tells every time it opens.
                Load::Ready(_) if blank => {
                    hint(ui, "Søg i hele DRTV — film, serier og dokumentarer.");
                }
                Load::Ready(shelves) if shelves.is_empty() => {
                    hint(ui, "Ingen resultater. Prøv et andet ord.");
                }
                Load::Ready(shelves) => {
                    let tile = tile_width(ui.available_width());
                    prefetch(&mut self.images, &self.workers, shelves, tile);
                    let term = self.search.term.trim().to_string();
                    egui::ScrollArea::vertical()
                        .id_salt("search")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(2.0);
                            for shelf in shelves {
                                let images = &self.images;
                                let lookup = |url: &str| images.get(url);
                                widgets::shelf(ui, shelf, &lookup, tile, &mut |card| {
                                    opened = Some(card.clone());
                                });
                            }
                            let _ = &term;
                            ui.add_space(20.0);
                        });
                }
            }
            if let Some(card) = opened {
                self.open_card(card);
            }
        });

        // The debounce fires here, one frame after the last keystroke, rather than on a timer thread.
        if let Some(due) = self.search.due {
            if std::time::Instant::now() >= due {
                self.search.due = None;
                let term = self.search.term.trim().to_string();
                self.generation += 1;
                let generation = self.generation;
                if term.is_empty() {
                    self.search.load = Load::Ready(Vec::new());
                } else {
                    self.search.load = Load::Loading;
                    let _ = self.workers.api().send(Cmd::Search { term, generation });
                }
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(60));
            }
        }
    }

    fn search_box(&mut self, ui: &mut Ui) {
        let width = ui.available_width().min(620.0);
        let (rect, _) = ui.allocate_exact_size(vec2(width, 46.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, theme::RADIUS, SURFACE);
        painter.rect_stroke(
            rect,
            theme::RADIUS,
            egui::Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
        icons::paint(
            painter,
            Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(18.0, 18.0)),
            icons::Icon::Search,
            TEXT_FAINT,
        );

        let inner = Rect::from_min_max(
            pos2(rect.left() + 42.0, rect.top() + 8.0),
            pos2(rect.right() - 40.0, rect.bottom() - 8.0),
        );
        let mut changed = false;
        ui.scope_builder(
            UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)),
            |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search.term)
                        .hint_text(RichText::new("Søg på DRTV").color(TEXT_FAINT))
                        .font(theme::regular(16.0))
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .margin(vec2(0.0, 0.0))
                        .text_color(TEXT),
                );
                if self.search.focus {
                    response.request_focus();
                    self.search.focus = false;
                }
                changed = response.changed();
            },
        );

        if changed {
            self.search.due = Some(std::time::Instant::now() + DEBOUNCE);
            ui.ctx().request_repaint_after(DEBOUNCE);
        }

        if !self.search.term.is_empty() {
            let close = Rect::from_center_size(
                pos2(rect.right() - 22.0, rect.center().y),
                vec2(24.0, 24.0),
            );
            if ui
                .interact(close, ui.scope_id().with("search-clear"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.search.term.clear();
                self.search.due = Some(std::time::Instant::now());
            }
            icons::paint(ui.painter(), close.shrink(6.0), icons::Icon::Close, TEXT_FAINT);
        }
    }
}

fn hint(ui: &mut Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.24);
        ui.label(RichText::new(text).font(theme::regular(14.0)).color(TEXT_FAINT));
    });
}
