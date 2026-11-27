//! The account page: sign in with a code, then choose who is watching.
//!
//! DR has no userpassword box for television clients. Sign-in is an OIDC device flow: the app
//! asks DR for a short code, the viewer types it at `dr.dk/aktiver` on a phone, and the app polls
//! until DR says yes. That is why this screen's job is to make one code unmissable, and to show how
//! long the wait is expected to be.

use drtv_api::auth;
use drtv_ui::theme::{self, ACCENT, BORDER, SURFACE, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT};
use drtv_ui::{icons, widgets};
use egui::{RichText, Ui, pos2, vec2};

use crate::app::DrtvApp;
use crate::worker::Cmd;

impl DrtvApp {
    pub(crate) fn account_ui(&mut self, ui: &mut Ui) {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 24)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("account").auto_shrink([false, false]).show(
                ui,
                |ui| {
                    ui.set_max_width(560.0);
                    if self.account.signed_in {
                        self.profile_picker(ui);
                    } else {
                        self.sign_in(ui);
                    }
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(
                            "Uofficiel DRTV-klient. Denne app er ikke lavet af, godkendt af \
                                 eller tilknyttet DR.",
                        )
                        .font(theme::regular(11.5))
                        .color(TEXT_FAINT),
                    );
                },
            );
        });
    }

    fn sign_in(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Log ind").font(theme::semibold(30.0)).color(TEXT));
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Log ind for at se dine egne lister, fortsætte hvor du stoppede og bruge \
                 børneprofiler. Du kan søge og se DRTV uden at være logget ind.",
            )
            .font(theme::regular(14.0))
            .color(TEXT_DIM),
        );
        ui.add_space(20.0);

        if let Some(error) = &self.account.error {
            ui.label(RichText::new(error).color(ACCENT).size(13.0));
            ui.add_space(10.0);
        }

        let code = self.account.code.clone();
        let polling = self.account.polling;
        match &code {
            Some(code) => {
                // The code is prefilled into the link DR hands out, so the page opens with the
                // eight characters already in the boxes; typing them is only for another device.
                let activate = code.activate_url();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(
                        RichText::new("Indtast koden på ").font(theme::medium(15.0)).color(TEXT),
                    );
                    if let Some(url) = &activate {
                        ui.scope(|ui| {
                            // A link in body colour: red is the wordmark's, the play button's and
                            // the progress fill's, and nothing else's.
                            ui.visuals_mut().hyperlink_color = TEXT;
                            let link = ui.add(egui::Link::new(
                                RichText::new("dr.dk/aktiver").font(theme::medium(15.0)),
                            ));
                            if link.clicked() {
                                self.open_activate(url.clone());
                            }
                        });
                    } else {
                        ui.label(
                            RichText::new("dr.dk/aktiver").font(theme::medium(15.0)).color(TEXT),
                        );
                    }
                });
                ui.add_space(10.0);

                let (rect, _) = ui.allocate_exact_size(vec2(280.0, 74.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, theme::RADIUS, SURFACE);
                ui.painter().rect_stroke(
                    rect,
                    theme::RADIUS,
                    egui::Stroke::new(1.0, BORDER),
                    egui::StrokeKind::Inside,
                );
                // Space the code out by hand: it is read off a screen and typed on a phone, and
                // Inter's default tracking turns "8" and "B" into the same shape at a glance.
                let spaced: String =
                    code.user_code.chars().map(|c| c.to_string()).collect::<Vec<_>>().join(" ");
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    spaced,
                    theme::semibold(30.0),
                    TEXT,
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if widgets::primary_button(ui, "Åbn dr.dk/aktiver", None).clicked()
                        && let Some(url) = &activate
                    {
                        self.open_activate(url.clone());
                    }
                    if polling {
                        ui.spinner();
                        ui.label(
                            RichText::new("Venter på godkendelse …").color(TEXT_DIM).size(13.0),
                        );
                    }
                });

                if !polling {
                    ui.add_space(10.0);
                    if widgets::ghost_button(ui, "Prøv igen", None).clicked() {
                        self.request_code();
                    }
                }
            }
            None => {
                let label = if self.account.busy { "Henter kode …" } else { "Log ind med DR" };
                if widgets::primary_button(ui, label, Some(icons::Icon::Play)).clicked()
                    && !self.account.busy
                {
                    self.request_code();
                }
            }
        }

        ui.add_space(26.0);
        ui.label(
            RichText::new(
                "Koden vises her og skal godkendes i en browser. Appen gemmer kun din \
                 DR-session lokalt — aldrig dit password.",
            )
            .font(theme::regular(12.5))
            .color(TEXT_FAINT),
        );
    }

    /// Open DR's activation page, and say so if the machine has no browser to open it with.
    fn open_activate(&mut self, url: String) {
        if webbrowser::open(&url).is_err() {
            self.toast(format!("Kunne ikke åbne browseren. Gå til {url}"));
        }
    }

    fn request_code(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        self.account.busy = true;
        self.account.error = None;
        self.account.code = None;
        let _ = self.workers.api().send(Cmd::SignIn { generation });
    }

    fn profile_picker(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Hvem ser med?").font(theme::semibold(30.0)).color(TEXT));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Profiler bestemmer, hvilket katalog DRTV viser — også børnenes.")
                .font(theme::regular(14.0))
                .color(TEXT_DIM),
        );
        ui.add_space(18.0);

        let Some(profiles) = self.account.profiles.clone() else {
            ui.spinner();
            ui.label(RichText::new("Henter profiler …").color(TEXT_DIM).size(13.0));
            return;
        };

        if let Some(error) = &self.account.error {
            ui.label(RichText::new(error).color(ACCENT).size(13.0));
            ui.add_space(8.0);
        }

        let mut chosen: Option<auth::Profile> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for profile in &profiles {
                if profile_tile(ui, profile, profile.name == self.account.profile_name).clicked() {
                    chosen = Some(profile.clone());
                }
            }
        });

        ui.add_space(22.0);
        if widgets::ghost_button(ui, "Log ud", None).clicked() {
            let _ = self.workers.api().send(Cmd::SignOut);
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new("Log ud sletter den lokale session. Din DR-konto er urørt.")
                .font(theme::regular(12.0))
                .color(TEXT_FAINT),
        );

        if let Some(profile) = chosen
            && profile.name != self.account.profile_name
        {
            self.account.busy = true;
            self.generation += 1;
            let generation = self.generation;
            let _ = self.workers.api().send(Cmd::Seat {
                id: profile.id.clone(),
                segments: profile.segments.clone(),
                generation,
            });
        }
    }
}

/// One viewer. DR gives each member a colour; the tile uses it when there is no avatar, which is what
/// makes the picker readable at a glance on a television account.
fn profile_tile(ui: &mut Ui, profile: &auth::Profile, active: bool) -> egui::Response {
    let size = vec2(150.0, 138.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() { theme::SURFACE_HI } else { SURFACE };
    let painter = ui.painter();
    painter.rect_filled(rect, theme::RADIUS, fill);
    painter.rect_stroke(
        rect,
        theme::RADIUS,
        egui::Stroke::new(if active { 2.0 } else { 1.0 }, if active { ACCENT } else { BORDER }),
        egui::StrokeKind::Inside,
    );

    let avatar =
        egui::Rect::from_center_size(pos2(rect.center().x, rect.top() + 48.0), vec2(58.0, 58.0));
    let tint = parse_color(&profile.color).unwrap_or(SURFACE_HI);
    painter.circle_filled(avatar.center(), 29.0, tint);
    painter.text(
        avatar.center(),
        egui::Align2::CENTER_CENTER,
        profile.name.chars().next().unwrap_or('·').to_uppercase().to_string(),
        theme::semibold(24.0),
        TEXT,
    );

    painter.text(
        pos2(rect.center().x, rect.top() + 94.0),
        egui::Align2::CENTER_CENTER,
        &profile.name,
        theme::medium(13.5),
        TEXT,
    );
    if profile.pin {
        painter.text(
            pos2(rect.center().x, rect.top() + 114.0),
            egui::Align2::CENTER_CENTER,
            "PIN",
            theme::regular(11.0),
            TEXT_FAINT,
        );
    }
    if active {
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                pos2(rect.right() - 18.0, rect.top() + 18.0),
                vec2(14.0, 14.0),
            ),
            icons::Icon::Check,
            ACCENT,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn parse_color(raw: &str) -> Option<egui::Color32> {
    let hex = raw.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(egui::Color32::from_rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ))
}
