//! The middle of the window: the pictures as cards (or as a list with columns), the sub-folders,
//! the menu on the right mouse button and dragging files onto folders and buttons.

use super::widgets::{draw_star, draw_stars, pill};
use super::{elide, fit, label_color, DragFiles, PhotoDesk};
use crate::files::{self, Entry, Kind, Sort};
use crate::launch::Choice;
use crate::meta::Label;
use crate::theme;
use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};

/// What a context menu asked for.
pub(super) enum Act {
    Open(Choice),
    Review,
    Reveal(PathBuf),
    CopyPath,
    Rename,
    Trash,
    Copy(bool),
    Rate(u8),
    Label(Option<Label>),
    CopyMeta,
    OpenFolder(PathBuf),
    Favorite(PathBuf),
    NewFolder,
}

/// What the user did with the items this frame.
#[derive(Default)]
struct Out {
    clicked: Option<(usize, bool, bool)>,
    double: Option<usize>,
    open_folder: Option<PathBuf>,
    menu_on: Option<usize>,
    act: Option<Act>,
    drag: Option<(usize, bool)>,
    drop: Option<(PathBuf, Vec<PathBuf>, bool)>,
}

fn menu_item(ui: &mut Ui, act: &mut Option<Act>, text: &str, a: Act) {
    if ui.button(text).clicked() {
        *act = Some(a);
        ui.close();
    }
}

/// The menu of a picture.
pub(super) fn item_menu(ui: &mut Ui, many: bool) -> Option<Act> {
    let mut act = None;
    menu_item(ui, &mut act, "Apri con DarkRaw", Act::Open(Choice::DarkRaw));
    menu_item(ui, &mut act, "Apri con LightRaw", Act::Open(Choice::LightRaw));
    menu_item(ui, &mut act, "Apri (automatico)", Act::Open(Choice::Auto));
    menu_item(ui, &mut act, "Schermo intero (Spazio)", Act::Review);
    ui.separator();
    ui.menu_button("Stelle", |ui| {
        for n in 0..=5u8 {
            let text = match n {
                0 => "Nessuna".to_string(),
                1 => "1 stella".to_string(),
                n => format!("{n} stelle"),
            };
            menu_item(ui, &mut act, &text, Act::Rate(n));
        }
    });
    ui.menu_button("Etichetta", |ui| {
        menu_item(ui, &mut act, "Nessuna", Act::Label(None));
        for l in Label::ALL {
            menu_item(ui, &mut act, l.name(), Act::Label(Some(l)));
        }
    });
    if many {
        menu_item(ui, &mut act, "Stelle ed etichetta dell'attiva a tutte", Act::CopyMeta);
    }
    ui.separator();
    menu_item(ui, &mut act, "Rinomina (F2)", Act::Rename);
    menu_item(ui, &mut act, "Copia (Ctrl+C)", Act::Copy(false));
    menu_item(ui, &mut act, "Taglia (Ctrl+X)", Act::Copy(true));
    menu_item(ui, &mut act, "Copia il percorso", Act::CopyPath);
    menu_item(ui, &mut act, "Mostra nel file manager", Act::Reveal(PathBuf::new()));
    ui.separator();
    menu_item(ui, &mut act, "Sposta nel cestino (Canc)", Act::Trash);
    act
}

fn folder_menu(ui: &mut Ui, path: &Path) -> Option<Act> {
    let mut act = None;
    if ui.button("Apri").clicked() {
        act = Some(Act::OpenFolder(path.to_path_buf()));
        ui.close();
    }
    if ui.button("Aggiungi ai preferiti").clicked() {
        act = Some(Act::Favorite(path.to_path_buf()));
        ui.close();
    }
    if ui.button("Mostra nel file manager").clicked() {
        act = Some(Act::Reveal(path.to_path_buf()));
        ui.close();
    }
    if ui.button("Nuova cartella…").clicked() {
        act = Some(Act::NewFolder);
        ui.close();
    }
    act
}

/// A spinning arc, drawn while a thumbnail is being made.
fn spinner(ui: &Ui, painter: &egui::Painter, c: Pos2) {
    let t = ui.input(|i| i.time) as f32;
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = t * 4.0 + i as f32 * 0.17;
            Pos2::new(c.x + 9.0 * a.cos(), c.y + 9.0 * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(pts, egui::Stroke::new(2.4, theme::ACCENT_LIGHT)));
}

/// Draw `tex` rounded, fitted inside `bounds`.
fn paint_photo(painter: &egui::Painter, tex: &egui::TextureHandle, bounds: Rect, radius: u8) {
    let r = fit(tex.size_vec2(), bounds);
    painter.add(
        egui::epaint::RectShape::filled(r, radius, Color32::WHITE).with_texture(
            tex.id(),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        ),
    );
}

impl PhotoDesk {
    pub(super) fn grid(&mut self, ui: &mut Ui) {
        if self.loading && self.entries.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Lettura cartella…").color(theme::MUTED).size(16.0));
            });
            return;
        }
        let view = self.view();
        let folders = self.visible_folders();
        if view.is_empty() && folders.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new(if self.entries.is_empty() {
                        "Nessuna immagine in questa cartella."
                    } else {
                        "Nessuna immagine corrisponde ai filtri."
                    })
                    .color(theme::MUTED)
                    .size(15.0),
                );
            });
            self.keyboard(ui.ctx(), &view);
            return;
        }
        let out = if self.settings.list_view {
            self.list(ui, &view, &folders)
        } else {
            self.cards(ui, &view, &folders)
        };
        self.apply_out(ui, out, &view);
        self.keyboard(ui.ctx(), &view);
    }

    fn apply_out(&mut self, ui: &Ui, out: Out, view: &[usize]) {
        if let Some(p) = out.open_folder {
            self.go(p);
            return;
        }
        if let Some((target, files, copy)) = out.drop {
            self.transfer(&target, &files, !copy);
            return;
        }
        let path_at = |s: &Self, pos: usize| view.get(pos).and_then(|&i| s.entries.get(i)).map(|e| e.path.clone());
        if let Some(pos) = out.menu_on {
            if let Some(p) = path_at(self, pos) {
                if !self.selection.contains(&p) {
                    self.select_only(p);
                }
            }
        }
        if let Some((pos, started)) = out.drag {
            if let Some(p) = path_at(self, pos) {
                if !self.selection.contains(&p) {
                    self.select_only(p);
                }
                if started {
                    let paths = self.selected_paths();
                    egui::DragAndDrop::set_payload(ui.ctx(), DragFiles(paths));
                }
            }
        }
        if let Some((pos, shift, ctrl)) = out.clicked {
            self.select_click(view, pos, shift, ctrl);
        }
        if let Some(pos) = out.double {
            if let Some(p) = path_at(self, pos) {
                if !self.selection.contains(&p) {
                    self.select_only(p);
                }
                self.open_selection(self.settings.open_with);
            }
        }
        if let Some(a) = out.act {
            self.run_act(ui.ctx(), a);
        }
    }

    pub(super) fn run_act(&mut self, ctx: &egui::Context, a: Act) {
        match a {
            Act::Open(c) => self.open_selection(c),
            Act::Review => self.open_review(),
            Act::Reveal(p) => {
                let target = if p.as_os_str().is_empty() {
                    self.active_entry().map(|e| e.path.clone())
                } else {
                    Some(p)
                };
                if let Some(t) = target {
                    self.reveal_in_file_manager(&t);
                }
            }
            Act::CopyPath => {
                let text = self
                    .selected_paths()
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\n");
                ctx.copy_text(text);
                self.status = "Percorso copiato".into();
                self.status_error = false;
            }
            Act::Rename => self.start_rename(),
            Act::Trash => self.ask_trash(),
            Act::Copy(cut) => self.copy_selection(cut),
            Act::Rate(n) => self.rate_selection(n),
            Act::Label(l) => self.label_selection(l),
            Act::CopyMeta => self.copy_meta_from_active(),
            Act::OpenFolder(p) => self.go(p),
            Act::Favorite(p) => {
                let s = p.to_string_lossy().into_owned();
                if !self.settings.favorites.contains(&s) {
                    self.settings.favorites.push(s);
                    self.fav_ok.push(true);
                }
            }
            Act::NewFolder => self.modal = Some(super::Modal::NewFolder(String::new())),
        }
    }

    /// The pictures as white cards.
    fn cards(&mut self, ui: &mut Ui, view: &[usize], folders: &[PathBuf]) -> Out {
        let size = self.settings.thumb_size;
        let img_h = (size * 0.75).round();
        let cell = Vec2::new(size + 28.0, img_h + 66.0);
        let cols = ((ui.available_width() / cell.x).floor() as usize).max(1);
        self.columns = cols;
        self.rows_visible = ((ui.available_height() / cell.y).floor() as usize).max(1);
        let nf = folders.len();
        let total = nf + view.len();
        let rows = total.div_ceil(cols);
        let mut out = Out::default();
        let mut want_visible: Vec<usize> = Vec::new();
        let mut want_near: Vec<usize> = Vec::new();
        let mods = ui.input(|i| i.modifiers);
        let many = self.selection.len() > 1;
        let mut shown = 0..0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, cell.y, rows, |ui, range| {
                shown = range.clone();
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for row in range {
                    ui.horizontal(|ui| {
                        for col in 0..cols {
                            let pos = row * cols + col;
                            if pos >= total {
                                break;
                            }
                            if let Some(f) = folders.get(pos) {
                                self.folder_card(ui, f, cell, size, img_h, &mut out);
                                continue;
                            }
                            let vpos = pos - nf;
                            let Some(&idx) = view.get(vpos) else { break };
                            let Some(e) = self.entries.get(idx).cloned() else { break };
                            let (rect, resp) =
                                ui.allocate_exact_size(cell, Sense::click_and_drag());
                            let selected = self.selection.contains(&e.path);
                            let active = self.anchor.as_ref() == Some(&e.path);
                            let painter = ui.painter().with_clip_rect(rect.expand(24.0));
                            let card = rect.shrink(8.0);
                            theme::card(&painter, card, 14, selected, resp.hovered());
                            if selected && active && self.selection.len() > 1 {
                                painter.rect_stroke(
                                    card.shrink(-3.0),
                                    17,
                                    egui::Stroke::new(1.0, theme::ACCENT_LIGHT),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            let img_rect = Rect::from_min_size(
                                card.min + Vec2::new(10.0, 10.0),
                                Vec2::new(card.width() - 20.0, img_h),
                            );
                            painter.rect_filled(img_rect, 9, theme::MAT);
                            match self.thumbs.get(&e.path) {
                                Some(Some(t)) => paint_photo(&painter, t, img_rect, 9),
                                Some(None) => {
                                    painter.text(
                                        img_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        if e.kind == Kind::Raw { "RAW" } else { "?" },
                                        egui::FontId::proportional(16.0),
                                        theme::MUTED,
                                    );
                                }
                                None => {
                                    spinner(ui, &painter, img_rect.center());
                                    if self.pending.insert(e.path.clone()) {
                                        want_visible.push(idx);
                                    }
                                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                                }
                            }
                            let m = self.meta.get(&e.name);
                            let mut x = img_rect.left() + 6.0;
                            if e.kind == Kind::Raw {
                                x += pill(&painter, Pos2::new(x, img_rect.top() + 6.0), "RAW", theme::VIOLET) + 4.0;
                            }
                            if self.settings.stack_pairs && self.companions.contains_key(&e.name) {
                                pill(&painter, Pos2::new(x, img_rect.top() + 6.0), "RAW+JPG", theme::ACCENT);
                            }
                            let name_y = img_rect.bottom() + 8.0;
                            painter.text(
                                Pos2::new(card.center().x, name_y),
                                egui::Align2::CENTER_TOP,
                                elide(&e.name, ((card.width() - 8.0) / 6.6) as usize),
                                egui::FontId::proportional(12.0),
                                theme::MUTED,
                            );
                            let star_y = name_y + 24.0;
                            draw_stars(&painter, Pos2::new(card.center().x, star_y), 5.0, m.rating);
                            if let Some(l) = m.label {
                                painter.circle_filled(
                                    Pos2::new(card.right() - 14.0, star_y),
                                    5.0,
                                    label_color(l),
                                );
                            }
                            let resp = match self.exifs.get(&e.path) {
                                Some(x) if !x.is_empty() => {
                                    resp.on_hover_text(format!("{}\n{}", e.name, x.tooltip()))
                                }
                                _ => resp,
                            };
                            self.item_response(&resp, vpos, &e, &mods, many, &mut out);
                        }
                    });
                }
            });
        // thumbnails a little beyond the screen are made too, but after the visible ones
        let first = shown.start.saturating_sub(2) * cols;
        let last = ((shown.end + 2) * cols).min(total);
        for pos in first..last {
            let Some(&idx) = pos.checked_sub(nf).and_then(|v| view.get(v)) else { continue };
            let Some(e) = self.entries.get(idx) else { continue };
            if !self.thumbs.contains_key(&e.path) && self.pending.insert(e.path.clone()) {
                want_near.push(idx);
            }
        }
        for idx in want_near.into_iter().chain(want_visible) {
            if let Some(e) = self.entries.get(idx).cloned() {
                self.request(&e, false);
            }
        }
        out
    }

    fn folder_card(&mut self, ui: &mut Ui, f: &Path, cell: Vec2, size: f32, img_h: f32, out: &mut Out) {
        let (rect, resp) = ui.allocate_exact_size(cell, Sense::click());
        let painter = ui.painter().with_clip_rect(rect.expand(24.0));
        let card = rect.shrink(8.0);
        let dropping = resp.dnd_hover_payload::<DragFiles>().is_some();
        theme::card(&painter, card, 14, dropping, resp.hovered());
        let img_rect = Rect::from_min_size(
            card.min + Vec2::new(10.0, 10.0),
            Vec2::new(card.width() - 20.0, img_h),
        );
        theme::draw_folder(
            &painter,
            Rect::from_center_size(img_rect.center(), Vec2::splat(size * 0.5)),
            resp.hovered(),
        );
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        painter.text(
            Pos2::new(card.center().x, img_rect.bottom() + 12.0),
            egui::Align2::CENTER_TOP,
            elide(&name, ((card.width() - 8.0) / 6.6) as usize),
            egui::FontId::proportional(12.0),
            theme::INK,
        );
        if resp.double_clicked() {
            out.open_folder = Some(f.to_path_buf());
        }
        if let Some(files) = resp.dnd_release_payload::<DragFiles>() {
            let copy = ui.input(|i| i.modifiers.command);
            out.drop = Some((f.to_path_buf(), files.0.clone(), copy));
        }
        resp.context_menu(|ui| {
            if let Some(a) = folder_menu(ui, f) {
                out.act = Some(a);
            }
        });
    }

    /// Clicks, double clicks, drags and the menu of one picture (card or row).
    fn item_response(
        &mut self,
        resp: &egui::Response,
        vpos: usize,
        e: &Entry,
        mods: &egui::Modifiers,
        many: bool,
        out: &mut Out,
    ) {
        if resp.clicked() {
            out.clicked = Some((vpos, mods.shift, mods.command));
        }
        if resp.double_clicked() {
            out.double = Some(vpos);
        }
        if resp.drag_started() {
            out.drag = Some((vpos, true));
        }
        if resp.secondary_clicked() {
            out.menu_on = Some(vpos);
        }
        resp.context_menu(|ui| {
            if self.selection.contains(&e.path) || self.selection.is_empty() || !many {
                if let Some(a) = item_menu(ui, many) {
                    out.act = Some(a);
                }
            } else if let Some(a) = item_menu(ui, many) {
                out.act = Some(a);
            }
        });
    }

    /// The pictures as rows with columns; the header sorts.
    fn list(&mut self, ui: &mut Ui, view: &[usize], folders: &[PathBuf]) -> Out {
        let row_h = 32.0;
        let width = ui.available_width();
        // fixed widths on the right, the name takes the rest
        let cols: [(&str, f32, Option<Sort>); 6] = [
            ("Data scatto", 140.0, Some(Sort::Taken)),
            ("Pixel", 100.0, None),
            ("Dimensione", 90.0, Some(Sort::Size)),
            ("Tipo", 60.0, Some(Sort::Type)),
            ("Stelle", 80.0, None),
            ("Etichetta", 40.0, None),
        ];
        let fixed: f32 = cols.iter().map(|c| c.1).sum();
        let name_w = (width - fixed - 56.0).max(120.0);
        let mut out = Out::default();
        let mut resort = None;
        // header
        let (hrect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
        {
            let painter = ui.painter();
            painter.rect_filled(hrect, 8, Color32::WHITE);
            let mut x = hrect.left() + 48.0;
            let mut heads: Vec<(&str, f32, Option<Sort>)> = vec![("Nome", name_w, Some(Sort::Name))];
            heads.extend(cols.iter().copied());
            for (title, w, sort) in heads {
                let r = Rect::from_min_size(Pos2::new(x, hrect.top()), Vec2::new(w, 26.0));
                let id = ui.id().with(("head", title));
                let resp = ui.interact(r, id, if sort.is_some() { Sense::click() } else { Sense::hover() });
                let arrow = match sort {
                    Some(s) if s == self.sort => {
                        if self.descending { " ▼" } else { " ▲" }
                    }
                    _ => "",
                };
                painter.text(
                    Pos2::new(x + 4.0, hrect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{}{arrow}", title.to_uppercase()),
                    egui::FontId::proportional(10.5),
                    if resp.hovered() && sort.is_some() { theme::ACCENT } else { theme::FAINT },
                );
                if resp.clicked() {
                    resort = sort;
                }
                x += w;
            }
        }
        if let Some(s) = resort {
            if self.sort == s {
                self.descending = !self.descending;
            } else {
                self.sort = s;
                self.descending = false;
            }
            self.resort();
        }
        ui.add_space(4.0);
        let nf = folders.len();
        let total = nf + view.len();
        self.columns = 1;
        self.rows_visible = ((ui.available_height() / row_h).floor() as usize).max(1);
        let mods = ui.input(|i| i.modifiers);
        let many = self.selection.len() > 1;
        let mut want: Vec<usize> = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, total, |ui, range| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for pos in range {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::click_and_drag());
                    let painter = ui.painter().with_clip_rect(rect);
                    if let Some(f) = folders.get(pos) {
                        if resp.hovered() {
                            painter.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 8, Color32::from_rgb(0xee, 0xee, 0xfb));
                        }
                        theme::draw_folder(&painter, Rect::from_center_size(Pos2::new(rect.left() + 24.0, rect.center().y), Vec2::splat(20.0)), false);
                        painter.text(
                            Pos2::new(rect.left() + 52.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            egui::FontId::proportional(13.0),
                            theme::INK,
                        );
                        if resp.double_clicked() {
                            out.open_folder = Some(f.clone());
                        }
                        if let Some(files) = resp.dnd_release_payload::<DragFiles>() {
                            let copy = ui.input(|i| i.modifiers.command);
                            out.drop = Some((f.clone(), files.0.clone(), copy));
                        }
                        resp.context_menu(|ui| {
                            if let Some(a) = folder_menu(ui, f) {
                                out.act = Some(a);
                            }
                        });
                        continue;
                    }
                    let vpos = pos - nf;
                    let Some(&idx) = view.get(vpos) else { continue };
                    let Some(e) = self.entries.get(idx).cloned() else { continue };
                    let selected = self.selection.contains(&e.path);
                    if selected {
                        painter.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 8, theme::ROW_ACTIVE);
                    } else if resp.hovered() {
                        painter.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 8, Color32::from_rgb(0xf1, 0xf0, 0xfb));
                    }
                    let thumb = Rect::from_center_size(Pos2::new(rect.left() + 24.0, rect.center().y), Vec2::new(32.0, 24.0));
                    match self.thumbs.get(&e.path) {
                        Some(Some(t)) => paint_photo(&painter, t, thumb, 4),
                        Some(None) => {}
                        None => {
                            if self.pending.insert(e.path.clone()) {
                                want.push(idx);
                            }
                        }
                    }
                    let x0 = rect.left() + 48.0;
                    let ex = self.exifs.get(&e.path);
                    let m = self.meta.get(&e.name);
                    let text = |p: Pos2, s: String, c: Color32| {
                        painter.text(p, egui::Align2::LEFT_CENTER, s, egui::FontId::proportional(12.5), c);
                    };
                    let cy = rect.center().y;
                    text(Pos2::new(x0 + 4.0, cy), elide(&e.name, (name_w / 7.0) as usize), theme::INK);
                    let mut x = x0 + name_w;
                    let when = ex
                        .and_then(|x| x.taken_at())
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| files::format_time(e.modified));
                    text(Pos2::new(x + 4.0, cy), when, theme::MUTED);
                    x += 140.0;
                    text(Pos2::new(x + 4.0, cy), ex.map(|x| x.size_text()).unwrap_or_default(), theme::MUTED);
                    x += 100.0;
                    text(Pos2::new(x + 4.0, cy), files::human_size(e.size), theme::MUTED);
                    x += 90.0;
                    let ext = e.path.extension().map(|x| x.to_string_lossy().to_uppercase()).unwrap_or_default();
                    text(Pos2::new(x + 4.0, cy), ext, theme::MUTED);
                    x += 60.0;
                    for s in 0..5u8 {
                        draw_star(&painter, Pos2::new(x + 10.0 + f32::from(s) * 13.0, cy), 5.0, s < m.rating);
                    }
                    x += 80.0;
                    if let Some(l) = m.label {
                        painter.circle_filled(Pos2::new(x + 12.0, cy), 5.0, label_color(l));
                    }
                    self.item_response(&resp, vpos, &e, &mods, many, &mut out);
                }
            });
        for idx in want {
            if let Some(e) = self.entries.get(idx).cloned() {
                self.request(&e, false);
            }
        }
        out
    }
}
