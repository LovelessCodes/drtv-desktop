//! The handful of glyphs the app draws itself.
//!
//! A font's symbols are not on the list: egui's fallback fonts disagree about which glyphs exist, a
//! missing one renders as a box, and a play triangle that is a hair off-centre looks broken. These
//! are painted from primitives in a 24×24 space, so they scale with the hinting off and stay crisp.

use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2, vec2};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Close,
    Back,
    ChevronLeft,
    ChevronRight,
    Search,
    Volume,
    Muted,
    Subtitles,
    Next,
    Fullscreen,
    Audio,
    Check,
    Settings,
}

/// A 24-unit design space, mapped onto whatever rectangle the caller allocated.
struct Grid {
    min: Pos2,
    scale: f32,
}

impl Grid {
    fn new(rect: Rect) -> Self {
        let scale = rect.width().min(rect.height()) / 24.0;
        let origin = rect.center() - vec2(12.0 * scale, 12.0 * scale);
        Self { min: origin, scale }
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.min + vec2(x * self.scale, y * self.scale)
    }

    fn w(&self, width: f32) -> f32 {
        width * self.scale
    }

    fn line(&self, points: &[(f32, f32)], width: f32, color: Color32, painter: &egui::Painter) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        painter.add(Shape::line(points, Stroke::new(self.w(width), color)));
    }

    fn poly(&self, points: &[(f32, f32)], color: Color32, painter: &egui::Painter) {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| self.p(*x, *y)).collect();
        painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
    }
}

pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let g = Grid::new(rect);
    match icon {
        Icon::Play => g.poly(&[(8.0, 5.0), (8.0, 19.0), (19.0, 12.0)], color, painter),
        Icon::Pause => {
            for x in [7.5, 14.0] {
                painter.rect_filled(
                    Rect::from_min_size(g.p(x, 5.0), vec2(g.w(2.6), g.w(14.0))),
                    g.w(0.8),
                    color,
                );
            }
        }
        Icon::Close => {
            g.line(&[(6.0, 6.0), (18.0, 18.0)], 2.0, color, painter);
            g.line(&[(18.0, 6.0), (6.0, 18.0)], 2.0, color, painter);
        }
        Icon::Back => {
            g.line(&[(19.0, 12.0), (5.0, 12.0)], 2.0, color, painter);
            g.line(&[(11.0, 5.0), (4.5, 12.0), (11.0, 19.0)], 2.0, color, painter);
        }
        Icon::ChevronLeft => g.line(&[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)], 2.2, color, painter),
        Icon::ChevronRight => g.line(&[(9.0, 5.0), (16.0, 12.0), (9.0, 19.0)], 2.2, color, painter),
        Icon::Search => {
            painter.circle_stroke(g.p(10.5, 10.5), g.w(6.2), Stroke::new(g.w(2.0), color));
            g.line(&[(15.0, 15.0), (20.0, 20.0)], 2.0, color, painter);
        }
        Icon::Volume => {
            g.poly(
                &[(4.0, 9.5), (8.0, 9.5), (13.0, 5.0), (13.0, 19.0), (8.0, 14.5), (4.0, 14.5)],
                color,
                painter,
            );
            for (radius, width) in [(5.5, 1.8), (9.0, 1.6)] {
                let points: Vec<(f32, f32)> = (0..=8)
                    .map(|i| {
                        let t = -0.9 + 1.8 * (i as f32 / 8.0);
                        (16.0 + radius * t.cos() * 0.55, 12.0 + radius * t.sin())
                    })
                    .collect();
                g.line(&points, width, color, painter);
            }
        }
        Icon::Muted => {
            g.poly(
                &[(4.0, 9.5), (8.0, 9.5), (13.0, 5.0), (13.0, 19.0), (8.0, 14.5), (4.0, 14.5)],
                color,
                painter,
            );
            g.line(&[(16.0, 9.0), (21.0, 15.0)], 1.9, color, painter);
            g.line(&[(21.0, 9.0), (16.0, 15.0)], 1.9, color, painter);
        }
        Icon::Subtitles => {
            painter.rect_stroke(
                Rect::from_min_max(g.p(3.0, 6.0), g.p(21.0, 18.0)),
                g.w(2.0),
                Stroke::new(g.w(1.8), color),
                egui::StrokeKind::Inside,
            );
            for (x0, x1, y) in [(6.5, 13.0, 11.5), (10.0, 17.5, 14.5)] {
                g.line(&[(x0, y), (x1, y)], 1.8, color, painter);
            }
        }
        Icon::Next => {
            g.poly(&[(5.0, 5.0), (5.0, 19.0), (16.0, 12.0)], color, painter);
            painter.rect_filled(
                Rect::from_min_size(g.p(17.0, 5.0), vec2(g.w(2.2), g.w(14.0))),
                g.w(0.6),
                color,
            );
        }
        Icon::Audio => {
            for (i, height) in [8.0f32, 14.0, 6.0].into_iter().enumerate() {
                let x = 7.0 + i as f32 * 5.0;
                g.line(&[(x, 12.0 - height / 2.0), (x, 12.0 + height / 2.0)], 2.2, color, painter);
            }
        }
        Icon::Fullscreen => {
            for (corner, dx, dy) in [
                ((4.0, 4.0), 5.0, 5.0),
                ((20.0, 4.0), -5.0, 5.0),
                ((4.0, 20.0), 5.0, -5.0),
                ((20.0, 20.0), -5.0, -5.0),
            ] {
                g.line(&[corner, (corner.0 + dx, corner.1)], 2.0, color, painter);
                g.line(&[corner, (corner.0, corner.1 + dy)], 2.0, color, painter);
            }
        }
        Icon::Check => g.line(&[(5.0, 12.5), (10.0, 17.5), (19.0, 6.5)], 2.4, color, painter),
        Icon::Settings => {
            painter.circle_stroke(g.p(12.0, 12.0), g.w(4.0), Stroke::new(g.w(1.9), color));
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let (sin, cos) = angle.sin_cos();
                g.line(
                    &[(12.0 + 6.5 * cos, 12.0 + 6.5 * sin), (12.0 + 9.0 * cos, 12.0 + 9.0 * sin)],
                    1.9,
                    color,
                    painter,
                );
            }
        }
    }
}

/// A square icon button: a hit area, a hover disc and the glyph. `size` is the whole square.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.circle_filled(rect.center(), size * 0.5, theme::SURFACE_HI.gamma_multiply(0.9));
    }
    // The glyph occupies 60% of the button, which leaves the hover disc visible around it.
    paint(painter, Rect::from_center_size(rect.center(), Vec2::splat(size * 0.6)), icon, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
