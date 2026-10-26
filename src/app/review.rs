//! Full-screen review: one picture big (zoom with the wheel, drag to move, arrows to go on, stars
//! and labels from the keyboard), or 2 to 4 side by side to compare, with a strip of thumbnails.

use super::widgets::{draw_star, draw_stars};
use super::{fit, label_color, PhotoDesk, Review};
use crate::launch::Choice;
use crate::meta::Label;
use crate::theme;
use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};

/// Behind the picture while judging it: the icon's midnight blue.
const BACKDROP: Color32 = Color32::from_rgb(0x14, 0x14, 0x30);
const ON_DARK: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xfa);
const MAX_ZOOM: f32 = 8.0;

impl PhotoDesk {
    pub(super) fn open_review(&mut self) {
        if self.active_entry().is_none() {
            self.status = "Seleziona prima una foto.".into();
            self.status_error = false;
            return;
        }
        self.review = Some(Review {
            zoom: 1.0,
            offset: Vec2::ZERO,
            compare: Vec::new(),
            shown: None,
        });
    }

    /// 2 to 4 selected pictures side by side.
    pub(super) fn open_compare(&mut self) {
        let view = self.view();
        let paths: Vec<PathBuf> = view
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .filter(|e| self.selection.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        if !(2..=4).contains(&paths.len()) {
            self.status = "Per confrontare seleziona da 2 a 4 foto.".into();
            self.status_error = false;
            return;
        }
        self.anchor = paths.first().cloned();
        self.review = Some(Review {
            zoom: 1.0,
            offset: Vec2::ZERO,
            compare: paths,
            shown: None,
        });
    }

    fn close_review(&mut self) {
        self.review = None;
    }

    fn tex_for(&self, path: &Path) -> Option<egui::TextureHandle> {
        self.preview_tex(path)
            .or_else(|| self.thumbs.get(path).and_then(Clone::clone))
    }

    pub(super) fn review_screen(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let view = self.view();
        let compare: Vec<PathBuf> = self.review.as_ref().map(|r| r.compare.clone()).unwrap_or_default();
        let comparing = !compare.is_empty();
        if !comparing {
            // the next and the previous picture are made ready
            let cur = self
                .anchor
                .as_ref()
                .and_then(|a| view.iter().position(|&i| self.entries.get(i).is_some_and(|e| &e.path == a)));
            if let Some(c) = cur {
                for n in [c.checked_sub(1), Some(c + 1)].into_iter().flatten() {
                    if let Some(p) = view.get(n).and_then(|&i| self.entries.get(i)).map(|e| e.path.clone()) {
                        self.want_preview(&p);
                    }
                }
            }
            let shown = self.active_entry().map(|e| e.path.clone());
            if let Some(r) = self.review.as_mut() {
                if r.shown != shown {
                    r.shown = shown;
                    r.zoom = 1.0;
                    r.offset = Vec2::ZERO;
                }
            }
        }
        self.review_keys(&ctx, &view, comparing);
        if self.review.is_none() {
            return;
        }
        let frame = egui::Frame::new().fill(BACKDROP);
        egui::Panel::top("review_top")
            .frame(frame.inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ui, |ui| self.review_bar(ui, &view, comparing));
        if !comparing {
            egui::Panel::bottom("review_strip")
                .frame(frame.inner_margin(egui::Margin::symmetric(12, 8)))
                .exact_size(96.0)
                .show(ui, |ui| self.review_strip(ui, &view));
        }
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if comparing {
                self.compare_area(ui, &compare);
            } else {
                self.single_area(ui);
            }
        });
    }

    fn review_keys(&mut self, ctx: &egui::Context, view: &[usize], comparing: bool) {
        if ctx.egui_wants_keyboard_input() || self.modal.is_some() {
            return;
        }
        enum K {
            Close,
            Step(i64),
            Rate(u8),
            RateStep(i32),
            Label(u8),
            Open,
            Compare,
            Trash,
        }
        let mut keys: Vec<K> = Vec::new();
        ctx.input(|i| {
            let p = |k| i.key_pressed(k);
            if p(egui::Key::Escape) || p(egui::Key::Space) || p(egui::Key::F) {
                keys.push(K::Close);
            }
            if p(egui::Key::ArrowLeft) {
                keys.push(K::Step(-1));
            }
            if p(egui::Key::ArrowRight) {
                keys.push(K::Step(1));
            }
            if p(egui::Key::Home) {
                keys.push(K::Step(i64::MIN / 2));
            }
            if p(egui::Key::End) {
                keys.push(K::Step(i64::MAX / 2));
            }
            for (k, n) in [
                (egui::Key::Num0, 0u8),
                (egui::Key::Num1, 1),
                (egui::Key::Num2, 2),
                (egui::Key::Num3, 3),
                (egui::Key::Num4, 4),
                (egui::Key::Num5, 5),
                (egui::Key::Num6, 6),
                (egui::Key::Num7, 7),
                (egui::Key::Num8, 8),
                (egui::Key::Num9, 9),
            ] {
                if p(k) {
                    keys.push(if n <= 5 { K::Rate(n) } else { K::Label(n) });
                }
            }
            if p(egui::Key::Plus) || p(egui::Key::Equals) {
                keys.push(K::RateStep(1));
            }
            if p(egui::Key::Minus) {
                keys.push(K::RateStep(-1));
            }
            if p(egui::Key::Enter) {
                keys.push(K::Open);
            }
            if p(egui::Key::C) {
                keys.push(K::Compare);
            }
            if p(egui::Key::Delete) {
                keys.push(K::Trash);
            }
        });
        for k in keys {
            match k {
                K::Close => self.close_review(),
                K::Step(d) if !comparing => self.step(view, d, false),
                K::Step(_) => {}
                K::Rate(n) => self.rate_active(n),
                K::RateStep(d) => {
                    let cur = self.active_entry().map_or(0, |e| self.meta.get(&e.name).rating);
                    self.rate_active((i32::from(cur) + d).clamp(0, 5) as u8);
                }
                K::Label(n) => {
                    let l = match n {
                        6 => Some(Label::Red),
                        7 => Some(Label::Yellow),
                        8 => Some(Label::Green),
                        _ => Some(Label::Blue),
                    };
                    if let Some(p) = self.active_entry().map(|e| (e.path.clone(), e.name.clone())) {
                        let cur = self.meta.get(&p.1).label;
                        self.select_label_for(&p.0, if cur == l { None } else { l });
                    }
                }
                K::Open => {
                    if comparing {
                        if let Some(p) = self.active_entry().map(|e| e.path.clone()) {
                            self.select_only(p);
                        }
                    }
                    self.open_selection(Choice::Auto);
                }
                K::Compare => {
                    if comparing {
                        let a = self.anchor.clone();
                        self.review = None;
                        if let Some(a) = a {
                            self.select_only(a);
                        }
                        self.open_review();
                    } else {
                        self.open_compare();
                    }
                    return;
                }
                K::Trash => self.ask_trash(),
            }
        }
    }

    /// Stars for the active picture (not the whole selection: in compare mode that is several).
    fn rate_active(&mut self, n: u8) {
        if let Some(p) = self.active_entry().map(|e| e.path.clone()) {
            self.rate_one(&p, n);
        }
    }

    fn select_label_for(&mut self, path: &Path, label: Option<Label>) {
        let Some(name) = self.entries.iter().find(|e| e.path == path).map(|e| e.name.clone()) else {
            return;
        };
        self.meta.set_label(&name, label);
        if self.settings.stack_pairs {
            if let Some(j) = self.companions.get(&name).cloned() {
                self.meta.set_label(&j, label);
            }
        }
        self.mark_meta();
    }

    fn review_bar(&mut self, ui: &mut Ui, view: &[usize], comparing: bool) {
        let active = self.active_entry().cloned();
        ui.horizontal(|ui| {
            if let Some(e) = &active {
                ui.label(egui::RichText::new(&e.name).color(ON_DARK).strong().size(15.0));
                if !comparing {
                    let pos = view
                        .iter()
                        .position(|&i| self.entries.get(i).is_some_and(|x| x.path == e.path))
                        .map_or(0, |p| p + 1);
                    ui.label(egui::RichText::new(format!("{pos} / {}", view.len())).color(theme::FAINT));
                }
                if let Some(x) = self.exifs.get(&e.path) {
                    let line = [x.camera.clone(), x.exposure_line(), x.focal_text()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("  ·  ");
                    ui.label(egui::RichText::new(line).color(theme::FAINT));
                }
                ui.add_space(12.0);
                let m = self.meta.get(&e.name);
                for s in 1..=5u8 {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    draw_star(ui.painter(), rect.center(), 9.0, s <= m.rating);
                    if resp.clicked() {
                        self.rate_one(&e.path, if s == m.rating { 0 } else { s });
                    }
                }
                ui.add_space(6.0);
                for l in Label::ALL {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 7.0, label_color(l));
                    if m.label == Some(l) {
                        ui.painter().circle_stroke(rect.center(), 10.0, egui::Stroke::new(2.0, ON_DARK));
                    }
                    if resp.clicked() {
                        self.select_label_for(&e.path, if m.label == Some(l) { None } else { Some(l) });
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("Chiudi (Esc)")).clicked() {
                    self.close_review();
                }
                let n = self.selection.len();
                if comparing {
                    if ui.add(theme::outline_button("Una sola (C)", Color32::WHITE)).clicked() {
                        let a = self.anchor.clone();
                        self.review = None;
                        if let Some(a) = a {
                            self.select_only(a);
                        }
                        self.open_review();
                    }
                } else if (2..=4).contains(&n)
                    && ui.add(theme::outline_button("Confronta (C)", Color32::WHITE)).clicked()
                {
                    self.open_compare();
                }
                ui.label(
                    egui::RichText::new("rotella = zoom · trascina = sposta · doppio clic = ingrandisci")
                        .color(theme::FAINT)
                        .size(12.0),
                );
            });
        });
    }

    fn review_strip(&mut self, ui: &mut Ui, view: &[usize]) {
        let mut pick: Option<PathBuf> = None;
        let cur = self.anchor.clone();
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                for &i in view {
                    let Some(e) = self.entries.get(i) else { continue };
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(92.0, 70.0), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let here = cur.as_ref() == Some(&e.path);
                    let painter = ui.painter();
                    painter.rect_filled(rect, 8, Color32::from_rgb(0x22, 0x22, 0x44));
                    match self.thumbs.get(&e.path) {
                        Some(Some(t)) => {
                            let r = fit(t.size_vec2(), rect.shrink(3.0));
                            painter.add(
                                egui::epaint::RectShape::filled(r, 5, Color32::WHITE).with_texture(
                                    t.id(),
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                ),
                            );
                        }
                        _ => {
                            if !self.pending.contains(&e.path) && !self.thumbs.contains_key(&e.path) {
                                // not made yet: the grid asks for it when it is shown
                            }
                        }
                    }
                    if here {
                        painter.rect_stroke(rect, 8, egui::Stroke::new(2.5, theme::ACCENT_LIGHT), egui::StrokeKind::Inside);
                        ui.scroll_to_rect(rect, None);
                    }
                    let m = self.meta.get(&e.name);
                    if m.rating > 0 {
                        draw_stars(painter, Pos2::new(rect.center().x, rect.bottom() - 7.0), 3.0, m.rating);
                    }
                    if resp.clicked() {
                        pick = Some(e.path.clone());
                    }
                }
            });
        });
        if let Some(p) = pick {
            self.select_only(p);
        }
    }

    fn single_area(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(area, Sense::click_and_drag());
        let Some(path) = self.active_entry().map(|e| e.path.clone()) else {
            return;
        };
        let tex = self.tex_for(&path);
        self.zoom_and_pan(ui, &resp, area);
        let (zoom, offset) = self.review.as_ref().map_or((1.0, Vec2::ZERO), |r| (r.zoom, r.offset));
        match tex {
            Some(t) => paint_zoomed(ui, &t, area, zoom, offset),
            None => {
                ui.painter().text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    "Foto in preparazione…",
                    egui::FontId::proportional(15.0),
                    theme::FAINT,
                );
            }
        }
        if zoom > 1.01 {
            ui.painter().text(
                area.right_bottom() - Vec2::new(14.0, 12.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{:.1}×", zoom),
                egui::FontId::proportional(13.0),
                theme::FAINT,
            );
        }
    }

    /// Wheel zooms around the pointer, dragging moves, a double click toggles.
    fn zoom_and_pan(&mut self, ui: &Ui, resp: &egui::Response, area: Rect) {
        let Some(r) = self.review.as_mut() else { return };
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (scroll * 0.0035).exp() * pinch;
        let pointer = resp.hover_pos().unwrap_or(area.center());
        let rel = pointer - area.center();
        if (factor - 1.0).abs() > 1e-4 && resp.hovered() {
            let new = (r.zoom * factor).clamp(1.0, MAX_ZOOM);
            let k = new / r.zoom;
            r.offset = rel - (rel - r.offset) * k;
            r.zoom = new;
        }
        if resp.double_clicked() {
            let new = if r.zoom > 1.05 { 1.0 } else { 2.5 };
            let k = new / r.zoom;
            r.offset = rel - (rel - r.offset) * k;
            r.zoom = new;
        }
        if resp.dragged() {
            r.offset += resp.drag_delta();
        }
        if r.zoom <= 1.0 {
            r.offset = Vec2::ZERO;
        }
        // keep the picture from being dragged out of sight
        let limit = area.size() * 0.5 * r.zoom;
        r.offset.x = r.offset.x.clamp(-limit.x, limit.x);
        r.offset.y = r.offset.y.clamp(-limit.y, limit.y);
    }

    fn compare_area(&mut self, ui: &mut Ui, paths: &[PathBuf]) {
        let area = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(area, Sense::click_and_drag());
        let n = paths.len();
        let (cols, rows) = match n {
            0 | 1 => (1, 1),
            2 => (2, 1),
            3 => (3, 1),
            _ => (2, 2),
        };
        let cell = Vec2::new(area.width() / cols as f32, area.height() / rows as f32);
        // one zoom for all: the wheel and the drag work over any picture
        let hover_cell = resp.hover_pos().map(|p| {
            let c = ((p.x - area.left()) / cell.x).floor().max(0.0) as usize;
            let r = ((p.y - area.top()) / cell.y).floor().max(0.0) as usize;
            r * cols + c
        });
        let first_cell = Rect::from_min_size(area.min, cell);
        let mut probe = resp.clone();
        probe.rect = first_cell;
        self.zoom_and_pan_in(ui, &resp, first_cell);
        let (zoom, offset) = self.review.as_ref().map_or((1.0, Vec2::ZERO), |r| (r.zoom, r.offset));
        let mut clicked: Option<PathBuf> = None;
        for (i, p) in paths.iter().enumerate() {
            let (col, row) = (i % cols, i / cols);
            let rect = Rect::from_min_size(
                area.min + Vec2::new(col as f32 * cell.x, row as f32 * cell.y),
                cell,
            )
            .shrink(4.0);
            ui.painter().rect_filled(rect, 8, Color32::from_rgb(0x1c, 0x1c, 0x3c));
            if let Some(t) = self.tex_for(p) {
                paint_zoomed(ui, &t, rect, zoom, offset);
            }
            let active = self.anchor.as_ref() == Some(p);
            if active {
                ui.painter().rect_stroke(rect, 8, egui::Stroke::new(2.5, theme::ACCENT_LIGHT), egui::StrokeKind::Inside);
            }
            if let Some(e) = self.entries.iter().find(|e| &e.path == p) {
                let m = self.meta.get(&e.name);
                ui.painter().text(
                    rect.left_bottom() + Vec2::new(10.0, -10.0),
                    egui::Align2::LEFT_BOTTOM,
                    &e.name,
                    egui::FontId::proportional(12.5),
                    ON_DARK,
                );
                draw_stars(ui.painter(), rect.right_bottom() + Vec2::new(-60.0, -16.0), 5.0, m.rating);
            }
            if resp.clicked() && hover_cell == Some(i) {
                clicked = Some(p.clone());
            }
        }
        if let Some(p) = clicked {
            self.anchor = Some(p);
        }
    }

    fn zoom_and_pan_in(&mut self, ui: &Ui, resp: &egui::Response, area: Rect) {
        self.zoom_and_pan(ui, resp, area);
    }
}

/// Draw `tex` fitted inside `area`, magnified by `zoom` and moved by `offset`.
fn paint_zoomed(ui: &Ui, tex: &egui::TextureHandle, area: Rect, zoom: f32, offset: Vec2) {
    let base = fit(tex.size_vec2(), area.shrink(6.0));
    let r = Rect::from_center_size(base.center() + offset, base.size() * zoom);
    let painter = ui.painter().with_clip_rect(area);
    painter.add(
        egui::epaint::RectShape::filled(r, 0, Color32::WHITE).with_texture(
            tex.id(),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        ),
    );
}
