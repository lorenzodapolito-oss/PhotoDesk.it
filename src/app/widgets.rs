//! Small drawn controls: icons are painted with lines so they look the same everywhere.

use crate::theme;
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Back,
    Forward,
    Up,
    Refresh,
    Grid,
    List,
    Edit,
}

fn star_points(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let rad = if i % 2 == 0 { r } else { r * 0.45 };
            Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect()
}

/// A star: lit (yellow) or not (pale lavender).
pub fn draw_star(painter: &egui::Painter, c: Pos2, r: f32, filled: bool) {
    let color = if filled { theme::STAR } else { theme::STAR_OFF };
    painter.add(egui::Shape::convex_polygon(
        star_points(c, r),
        color,
        Stroke::NONE,
    ));
}

/// A row of five stars centred on `centre`.
pub fn draw_stars(painter: &egui::Painter, centre: Pos2, r: f32, lit: u8) {
    let step = r * 2.3;
    for s in 0..5u8 {
        let x = centre.x + (f32::from(s) - 2.0) * step;
        draw_star(painter, Pos2::new(x, centre.y), r, s < lit);
    }
}

fn chevron(painter: &egui::Painter, c: Pos2, size: f32, dir: Vec2, stroke: Stroke) {
    // a "greater than" sign pointing along `dir`
    let perp = Vec2::new(-dir.y, dir.x);
    let tip = c + dir * size;
    let a = c - dir * size * 0.2 + perp * size;
    let b = c - dir * size * 0.2 - perp * size;
    painter.line_segment([a, tip], stroke);
    painter.line_segment([tip, b], stroke);
}

/// A thin chevron between breadcrumb parts.
pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 18.0), Sense::hover());
    chevron(
        ui.painter(),
        rect.center() - Vec2::new(1.0, 0.0),
        3.5,
        Vec2::new(1.0, 0.0),
        Stroke::new(1.3, theme::FAINT),
    );
}

fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = Stroke::new(1.8, color);
    match icon {
        Icon::Back => {
            chevron(painter, c + Vec2::new(-2.0, 0.0), 5.5, Vec2::new(-1.0, 0.0), s);
        }
        Icon::Forward => {
            chevron(painter, c + Vec2::new(2.0, 0.0), 5.5, Vec2::new(1.0, 0.0), s);
        }
        Icon::Up => {
            chevron(painter, c + Vec2::new(0.0, -2.0), 5.5, Vec2::new(0.0, -1.0), s);
            painter.line_segment([c + Vec2::new(0.0, -4.0), c + Vec2::new(0.0, 7.0)], s);
        }
        Icon::Refresh => {
            let r = 6.0;
            let pts: Vec<Pos2> = (0..=18)
                .map(|i| {
                    let a = -0.6 + i as f32 * (std::f32::consts::TAU * 0.78 / 18.0);
                    Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            let end = pts.last().copied().unwrap_or(c);
            painter.add(egui::Shape::line(pts, s));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    end + Vec2::new(-3.5, -4.0),
                    end + Vec2::new(4.0, -2.5),
                    end + Vec2::new(0.0, 3.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Grid => {
            for dx in [-1.0f32, 1.0] {
                for dy in [-1.0f32, 1.0] {
                    let r = Rect::from_center_size(c + Vec2::new(dx * 4.5, dy * 4.5), Vec2::splat(7.0));
                    painter.rect_filled(r, 2.0, color);
                }
            }
        }
        Icon::List => {
            for dy in [-5.5f32, 0.0, 5.5] {
                painter.line_segment(
                    [c + Vec2::new(-8.0, dy), c + Vec2::new(8.0, dy)],
                    Stroke::new(2.4, color),
                );
            }
        }
        Icon::Edit => {
            painter.line_segment([c + Vec2::new(-6.0, 6.0), c + Vec2::new(5.0, -5.0)], Stroke::new(2.6, color));
            painter.line_segment([c + Vec2::new(-7.0, 7.0), c + Vec2::new(-4.5, 6.5)], Stroke::new(1.6, color));
        }
    }
}

/// A square icon button. `on` draws it as the chosen one of a group.
pub fn icon_button(ui: &mut Ui, icon: Icon, enabled: bool, on: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let hovered = resp.hovered() && enabled;
    let fill = if on {
        theme::ROW_ACTIVE
    } else if hovered {
        Color32::from_rgb(0xee, 0xee, 0xfb)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let color = if !enabled {
        theme::STAR_OFF
    } else if on || hovered {
        theme::ACCENT
    } else {
        theme::MUTED
    };
    paint_icon(ui.painter(), rect, icon, color);
    let resp = resp.on_hover_text(tip);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A small coloured pill with white letters ("RAW", "2"); returns its width.
pub fn pill(painter: &egui::Painter, top_left: Pos2, text: &str, color: Color32) -> f32 {
    let w = 10.0 + text.chars().count() as f32 * 6.5;
    let r = Rect::from_min_size(top_left, Vec2::new(w, 16.0));
    painter.rect_filled(r, 8.0, color);
    painter.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(10.0),
        Color32::WHITE,
    );
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_has_ten_points_around_its_centre() {
        let p = star_points(Pos2::new(10.0, 10.0), 5.0);
        assert_eq!(p.len(), 10);
        assert!(p
            .iter()
            .all(|q| (q.x - 10.0).abs() <= 5.01 && (q.y - 10.0).abs() <= 5.01));
    }
}
