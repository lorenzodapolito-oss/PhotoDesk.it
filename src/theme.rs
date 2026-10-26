//! PhotoDesk's look: a pastel indigo, the same whatever the system theme says.

use egui::{Color32, CornerRadius, Margin, Stroke};

/// Text and strokes: the deep midnight blue of the icon, never pure black.
pub const INK: Color32 = Color32::from_rgb(0x18, 0x18, 0x38);
/// Secondary text.
pub const MUTED: Color32 = Color32::from_rgb(0x5b, 0x58, 0x90);
/// Small headings and labels.
pub const FAINT: Color32 = Color32::from_rgb(0x8a, 0x86, 0xb8);
/// Side panels.
pub const PANEL: Color32 = Color32::from_rgb(0xfa, 0xf9, 0xfe);
/// Top and bottom bars.
pub const BAR: Color32 = Color32::WHITE;
/// The thumbnail area.
pub const STAGE: Color32 = Color32::from_rgb(0xf3, 0xf4, 0xfc);
/// A thumbnail card.
pub const CARD: Color32 = Color32::WHITE;
/// What sits behind a picture.
pub const MAT: Color32 = Color32::from_rgb(0xec, 0xee, 0xfb);
/// A selected card: still white, the outline does the work.
pub const CARD_SELECTED: Color32 = Color32::WHITE;
/// Hairlines between areas.
pub const LINE: Color32 = Color32::from_rgb(0xe3, 0xe0, 0xf2);
/// The active row in a list.
pub const ROW_ACTIVE: Color32 = Color32::from_rgb(0xe3, 0xe3, 0xfb);
/// The accent: indigo from the icon (buttons, selection outline, folders).
pub const ACCENT: Color32 = Color32::from_rgb(0x68, 0x68, 0xc8);
/// Lighter accent: hover.
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x88, 0x88, 0xf8);
/// Blue of the icon (DarkRaw).
pub const BLUE: Color32 = Color32::from_rgb(0x48, 0x88, 0xc8);
/// Violet of the icon (the RAW badge).
pub const VIOLET: Color32 = Color32::from_rgb(0x88, 0x48, 0xc8);
/// Stars.
pub const STAR: Color32 = Color32::from_rgb(0xf0, 0xb4, 0x29);
/// Stars that are not lit.
pub const STAR_OFF: Color32 = Color32::from_rgb(0xde, 0xda, 0xf0);

/// The folder icon: indigo (back, front).
const FOLDER_BACK: Color32 = Color32::from_rgb(0x58, 0x58, 0xb8);
const FOLDER_FRONT: Color32 = Color32::from_rgb(0x88, 0x88, 0xe0);

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
        .stroke(Stroke::new(1.0, LINE))
}

pub fn panel_frame() -> egui::Frame {
    frame(PANEL, 12)
}
pub fn bar_frame() -> egui::Frame {
    frame(BAR, 10)
}
pub fn stage_frame() -> egui::Frame {
    frame(STAGE, 14)
}

/// Paint a soft shadow under `rect`, then the white card itself.
pub fn card(painter: &egui::Painter, rect: egui::Rect, radius: u8, selected: bool, hovered: bool) {
    let r = CornerRadius::same(radius);
    let shadow = egui::epaint::Shadow {
        offset: [0, if selected { 5 } else { 2 }],
        blur: if selected { 16 } else { 8 },
        spread: 0,
        color: if selected {
            Color32::from_rgba_unmultiplied(0x68, 0x68, 0xc8, 70)
        } else {
            Color32::from_rgba_unmultiplied(0x3b, 0x32, 0x7a, if hovered { 34 } else { 20 })
        },
    };
    painter.add(shadow.as_shape(rect, r));
    painter.rect_filled(rect, r, CARD);
    if selected {
        painter.rect_stroke(rect, r, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    } else if hovered {
        painter.rect_stroke(
            rect,
            r,
            Stroke::new(1.0, ACCENT.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
    }
}

/// Small spaced capitals for a panel heading ("CARTELLE").
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(FAINT),
    );
    ui.add_space(2.0);
}

/// A button with the main (filled indigo) look.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).strong().color(Color32::WHITE))
        .fill(ACCENT)
        .corner_radius(9)
        .min_size(egui::vec2(0.0, 30.0))
}

/// A button with an outline in `color`.
pub fn outline_button(text: &str, color: Color32) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).strong().color(color))
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.5, color))
        .corner_radius(9)
        .min_size(egui::vec2(0.0, 30.0))
}

/// Make the light theme indigo and use it always.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = egui::Visuals::light();
    v.override_text_color = Some(INK);
    v.panel_fill = PANEL;
    v.window_fill = Color32::WHITE;
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(0xf1, 0xf0, 0xfb);
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0xd5, 0xd5, 0xf6);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.bg_fill = Color32::from_rgb(0xec, 0xec, 0xfa);
    w.inactive.weak_bg_fill = Color32::from_rgb(0xee, 0xee, 0xfb);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd8, 0xd6, 0xef));
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.hovered.bg_fill = Color32::from_rgb(0xdc, 0xdc, 0xfa);
    w.hovered.weak_bg_fill = Color32::from_rgb(0xe0, 0xe0, 0xfb);
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT_LIGHT);
    w.hovered.fg_stroke = Stroke::new(1.5, INK);
    w.active.bg_fill = Color32::from_rgb(0xc8, 0xc8, 0xf4);
    w.active.weak_bg_fill = Color32::from_rgb(0xcc, 0xcc, 0xf5);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(2.0, INK);
    w.open.bg_fill = Color32::from_rgb(0xe0, 0xe0, 0xfb);
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
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(12.0, 5.0);
        s.spacing.interact_size.y = 28.0;
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
        for bg in [PANEL, BAR, STAGE, CARD, CARD_SELECTED, MAT, ROW_ACTIVE] {
            assert!(ratio(INK, bg) >= 6.5, "ink on {bg:?}: {}", ratio(INK, bg));
        }
        for bg in [PANEL, STAGE, CARD] {
            assert!(
                ratio(MUTED, bg) >= 4.5,
                "muted on {bg:?}: {}",
                ratio(MUTED, bg)
            );
        }
        // white text on the filled buttons and badges
        for bg in [ACCENT, VIOLET] {
            assert!(
                ratio(Color32::WHITE, bg) >= 3.9,
                "white on {bg:?}: {}",
                ratio(Color32::WHITE, bg)
            );
        }
    }
}
