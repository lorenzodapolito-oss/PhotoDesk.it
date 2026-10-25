//! PhotoDesk's look: a pastel indigo, the same whatever the system theme says.

use egui::{Color32, CornerRadius, Margin, Stroke};

/// Text and strokes: a deep indigo, never pure black.
pub const INK: Color32 = Color32::from_rgb(0x1f, 0x23, 0x55);
/// Secondary text.
pub const MUTED: Color32 = Color32::from_rgb(0x5d, 0x63, 0x9c);
/// Side panels.
pub const PANEL: Color32 = Color32::from_rgb(0xcd, 0xd2, 0xf3);
/// Top and bottom bars: a little deeper than the panels.
pub const BAR: Color32 = Color32::from_rgb(0xb9, 0xc0, 0xee);
/// The thumbnail area.
pub const STAGE: Color32 = Color32::from_rgb(0xdf, 0xe3, 0xf8);
/// A thumbnail card.
pub const CARD: Color32 = Color32::from_rgb(0xed, 0xef, 0xfc);
/// What sits behind a picture.
pub const MAT: Color32 = Color32::from_rgb(0xf7, 0xf8, 0xfe);
/// A selected card.
pub const CARD_SELECTED: Color32 = Color32::from_rgb(0xa5, 0xb0, 0xe8);
/// The accent: selection outline and focus.
pub const ACCENT: Color32 = Color32::from_rgb(0x52, 0x60, 0xc4);
/// Stars.
pub const STAR: Color32 = Color32::from_rgb(0xe8, 0xa3, 0x1c);

/// The folder icon: indigo (back, front).
const FOLDER_BACK: Color32 = Color32::from_rgb(0x4a, 0x57, 0xb5);
const FOLDER_FRONT: Color32 = Color32::from_rgb(0x8f, 0x9b, 0xe0);

/// An indigo folder drawn to fit `rect` (the icon keeps a 4:3 shape, centred). `open` shows the
/// front flap tilted, as an expanded folder in a tree.
pub fn draw_folder(painter: &egui::Painter, rect: egui::Rect, open: bool) {
    let w = rect.width().min(rect.height() * 4.0 / 3.0);
    let h = w * 0.75;
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h));
    let at = |fx: f32, fy: f32| egui::pos2(r.left() + fx * r.width(), r.top() + fy * r.height());
    let round = CornerRadius::same((w * 0.1).round().clamp(1.0, 255.0) as u8);
    // the tab and the back
    painter.rect_filled(
        egui::Rect::from_min_max(at(0.0, 0.0), at(0.45, 0.35)),
        round,
        FOLDER_BACK,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(at(0.0, 0.14), at(1.0, 1.0)),
        round,
        FOLDER_BACK,
    );
    if open {
        let flap = vec![at(0.14, 0.38), at(1.0, 0.38), at(0.86, 1.0), at(0.0, 1.0)];
        painter.add(egui::Shape::convex_polygon(
            flap,
            FOLDER_FRONT,
            Stroke::NONE,
        ));
    } else {
        painter.rect_filled(
            egui::Rect::from_min_max(at(0.0, 0.3), at(1.0, 1.0)),
            round,
            FOLDER_FRONT,
        );
    }
}

fn frame(fill: Color32, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(Margin::same(margin))
}

pub fn panel_frame() -> egui::Frame {
    frame(PANEL, 10)
}
pub fn bar_frame() -> egui::Frame {
    frame(BAR, 8)
}
pub fn stage_frame() -> egui::Frame {
    frame(STAGE, 10)
}

/// Make the light theme indigo and use it always.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = egui::Visuals::light();
    v.override_text_color = Some(INK);
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgb(0xe3, 0xe6, 0xfa);
    v.extreme_bg_color = Color32::from_rgb(0xf3, 0xf4, 0xfd);
    v.faint_bg_color = Color32::from_rgb(0xc3, 0xc9, 0xf0);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x9a, 0xa4, 0xe0));
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0x9b, 0xa7, 0xe6);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    let radius = CornerRadius::same(7);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xa9, 0xb2, 0xe6));
    w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.bg_fill = Color32::from_rgb(0xae, 0xb7, 0xea);
    w.inactive.weak_bg_fill = Color32::from_rgb(0xb4, 0xbd, 0xed);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x95, 0xa0, 0xe0));
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.hovered.bg_fill = Color32::from_rgb(0x9b, 0xa7, 0xe6);
    w.hovered.weak_bg_fill = Color32::from_rgb(0xa3, 0xaf, 0xe8);
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = Stroke::new(1.5, INK);
    w.active.bg_fill = Color32::from_rgb(0x84, 0x93, 0xdd);
    w.active.weak_bg_fill = Color32::from_rgb(0x8c, 0x9a, 0xe0);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(2.0, INK);
    w.open.bg_fill = Color32::from_rgb(0xa3, 0xaf, 0xe8);
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = radius;
    }
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.style_mut_of(egui::Theme::Light, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 26.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_readable_on_every_surface() {
        // WCAG-ish contrast ratio of the ink against each background it is drawn on.
        fn lum(c: Color32) -> f32 {
            let f = |u: u8| {
                let s = f32::from(u) / 255.0;
                if s <= 0.03928 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        let ratio = |a: Color32, b: Color32| {
            let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
            x.max(y) / x.min(y)
        };
        for bg in [PANEL, BAR, STAGE, CARD, CARD_SELECTED, MAT] {
            assert!(ratio(INK, bg) >= 6.5, "ink on {bg:?}: {}", ratio(INK, bg));
        }
        for bg in [PANEL, STAGE, CARD] {
            assert!(
                ratio(MUTED, bg) >= 3.5,
                "muted on {bg:?}: {}",
                ratio(MUTED, bg)
            );
        }
    }
}
