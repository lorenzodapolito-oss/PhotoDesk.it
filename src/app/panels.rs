//! The bars and the side panels: navigation, filters, the folder tree and the details of the
//! selected picture.

use super::widgets::{draw_stars, icon_button, separator, Icon};
use super::{
    home_dir, label_color, roots, DateFilter, DragFiles, KindFilter, Modal, PhotoDesk,
};
use crate::files::{self, Sort};
use crate::launch::{self, Choice, Tool};
use crate::meta::Label;
use crate::theme;
use egui::{Align, Color32, Layout, Pos2, Rect, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};

impl PhotoDesk {
    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
        let mut go: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Back, self.history.can_back(), false, "Indietro (Alt+←)").clicked() {
                self.go_back();
            }
            if icon_button(ui, Icon::Forward, self.history.can_forward(), false, "Avanti (Alt+→)").clicked() {
                self.go_forward();
            }
            let has_parent = self.folder.parent().is_some();
            if icon_button(ui, Icon::Up, has_parent, false, "Cartella superiore (Backspace)").clicked() {
                self.go_up();
            }
            if icon_button(ui, Icon::Refresh, true, false, "Aggiorna (F5)").clicked() {
                self.refresh();
            }
            ui.add_space(6.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.open_buttons(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    self.breadcrumb(ui, &mut go);
                });
            });
        });
        ui.add_space(4.0);
        self.filter_row(ui, &mut go);
        if let Some(p) = go {
            self.go(p);
        }
    }

    /// "Apri in DarkRaw" and "Apri in LightRaw": for the selection, or files dropped on them.
    fn open_buttons(&mut self, ui: &mut Ui) {
        let has_sel = !self.selection.is_empty();
        for (choice, text, color, tip) in [
            (Choice::LightRaw, "Apri in LightRaw", theme::ACCENT, "Sviluppo raw e libreria"),
            (Choice::DarkRaw, "Apri in DarkRaw", theme::BLUE, "Editor di immagini"),
        ] {
            let button = if choice == Choice::LightRaw {
                theme::primary_button(text)
            } else {
                theme::outline_button(text, color)
            };
            let resp = ui.add_enabled(has_sel || self.dragging(ui), button);
            let resp = resp.on_hover_text(format!("{tip} — anche trascinandoci sopra le foto"));
            self.drop_target_highlight(ui, &resp, color);
            if resp.clicked() {
                self.open_selection(choice);
            }
            if let Some(files) = resp.dnd_release_payload::<DragFiles>() {
                let files = files.0.clone();
                self.open_files(choice, &files);
            }
        }
    }

    fn dragging(&self, ui: &Ui) -> bool {
        egui::DragAndDrop::has_payload_of_type::<DragFiles>(ui.ctx())
    }

    fn drop_target_highlight(&self, ui: &Ui, resp: &egui::Response, color: Color32) {
        if resp.dnd_hover_payload::<DragFiles>().is_some() {
            ui.painter().rect_stroke(
                resp.rect.expand(2.0),
                11,
                egui::Stroke::new(2.5, color),
                egui::StrokeKind::Outside,
            );
        }
    }

    /// The path as clickable parts; Ctrl+L or the pencil lets you type it.
    fn breadcrumb(&mut self, ui: &mut Ui, go: &mut Option<PathBuf>) {
        if self.editing_path.is_some() {
            let mut text = self.editing_path.take().unwrap_or_default();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width().max(120.0) - 8.0)
                    .hint_text("Scrivi un percorso e premi Invio"),
            );
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if enter {
                let p = PathBuf::from(text.trim());
                if p.is_dir() {
                    *go = Some(p);
                } else {
                    self.status = format!("Cartella non trovata: {}", text.trim());
                    self.status_error = true;
                }
            } else if !escape && !resp.lost_focus() {
                self.editing_path = Some(text);
            }
            return;
        }
        let mut parts: Vec<(String, PathBuf)> = Vec::new();
        let mut acc = PathBuf::new();
        for comp in self.folder.components() {
            acc.push(comp.as_os_str());
            let label = match comp {
                std::path::Component::Prefix(p) => p.as_os_str().to_string_lossy().into_owned(),
                std::path::Component::RootDir => continue,
                other => other.as_os_str().to_string_lossy().into_owned(),
            };
            let target = if matches!(comp, std::path::Component::Prefix(_)) {
                // "D:" alone means the current folder of that drive; "D:\" is its root
                PathBuf::from(format!("{label}\\"))
            } else {
                acc.clone()
            };
            parts.push((label, target));
        }
        if parts.is_empty() {
            parts.push((self.folder.to_string_lossy().into_owned(), self.folder.clone()));
        }
        let max = 5;
        let skipped = parts.len().saturating_sub(max);
        let last = parts.len().saturating_sub(1);
        ui.spacing_mut().item_spacing.x = 2.0;
        if icon_button(ui, Icon::Edit, true, false, "Scrivi il percorso (Ctrl+L)").clicked() {
            self.editing_path = Some(self.folder.to_string_lossy().into_owned());
        }
        if skipped > 0 {
            ui.label(egui::RichText::new("…").color(theme::FAINT));
            separator(ui);
        }
        for (i, (label, target)) in parts.into_iter().enumerate().skip(skipped) {
            let name = self
                .drive_names
                .try_lock()
                .ok()
                .and_then(|m| m.get(&target).cloned())
                .unwrap_or(label);
            let is_last = i == last;
            let text = egui::RichText::new(name)
                .strong()
                .color(if is_last { theme::INK } else { theme::ACCENT });
            let resp = ui.add(egui::Button::new(text).fill(if is_last {
                theme::ROW_ACTIVE
            } else {
                Color32::TRANSPARENT
            })
            .frame(is_last)
            .corner_radius(8));
            if resp.clicked() && !is_last {
                *go = Some(target.clone());
            }
            if let Some(files) = resp.dnd_release_payload::<DragFiles>() {
                let copy = ui.input(|i| i.modifiers.command);
                let files = files.0.clone();
                self.transfer(&target, &files, !copy);
            }
            if !is_last {
                separator(ui);
            }
        }
    }

    fn filter_row(&mut self, ui: &mut Ui, go: &mut Option<PathBuf>) {
        let _ = go;
        let mut resort = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Ordina").color(theme::FAINT));
            egui::ComboBox::from_id_salt("sort")
                .selected_text(self.sort.label())
                .show_ui(ui, |ui| {
                    for s in Sort::ALL {
                        if ui.selectable_value(&mut self.sort, s, s.label()).changed() {
                            resort = true;
                        }
                    }
                });
            if ui.checkbox(&mut self.descending, "inverso").changed() {
                resort = true;
            }
            ui.separator();
            egui::ComboBox::from_id_salt("minrating")
                .selected_text(if self.min_rating == 0 {
                    "tutte le stelle".to_string()
                } else {
                    format!("{}+ stelle", self.min_rating)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.min_rating, 0, "tutte le stelle");
                    for n in 1..=5u8 {
                        ui.selectable_value(&mut self.min_rating, n, format!("{n}+ stelle"));
                    }
                });
            egui::ComboBox::from_id_salt("labelfilter")
                .selected_text(self.label_filter.map_or("ogni etichetta", Label::name))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.label_filter, None, "ogni etichetta");
                    for l in Label::ALL {
                        ui.selectable_value(&mut self.label_filter, Some(l), l.name());
                    }
                });
            egui::ComboBox::from_id_salt("kindfilter")
                .selected_text(self.kind_filter.label())
                .show_ui(ui, |ui| {
                    for k in KindFilter::ALL {
                        ui.selectable_value(&mut self.kind_filter, k, k.label());
                    }
                });
            egui::ComboBox::from_id_salt("datefilter")
                .selected_text(self.date_filter.label())
                .show_ui(ui, |ui| {
                    for d in DateFilter::ALL {
                        ui.selectable_value(&mut self.date_filter, d, d.label());
                    }
                });
            let cams = self.cameras();
            if !cams.is_empty() {
                egui::ComboBox::from_id_salt("camerafilter")
                    .selected_text(self.camera_filter.clone().unwrap_or_else(|| "ogni fotocamera".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.camera_filter, None, "ogni fotocamera");
                        for c in cams {
                            ui.selectable_value(&mut self.camera_filter, Some(c.clone()), c);
                        }
                    });
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Cerca per nome")
                    .desired_width(130.0),
            );
            ui.add(egui::Slider::new(&mut self.settings.thumb_size, 90.0..=300.0).show_value(false))
                .on_hover_text("Dimensione delle miniature (Ctrl+0 per ripristinare)");
            if icon_button(ui, Icon::Grid, true, !self.settings.list_view, "Griglia").clicked() {
                self.settings.list_view = false;
            }
            if icon_button(ui, Icon::List, true, self.settings.list_view, "Elenco con colonne").clicked() {
                self.settings.list_view = true;
            }
            ui.menu_button("Seleziona", |ui| self.select_menu(ui));
            ui.menu_button("Altro", |ui| self.more_menu(ui));
        });
        if resort {
            self.resort();
        }
    }

    fn select_menu(&mut self, ui: &mut Ui) {
        let view = self.view();
        if ui.button("Tutto (Ctrl+A)").clicked() {
            self.select_all(&view);
            ui.close();
        }
        if ui.button("Inverti (Ctrl+Maiusc+I)").clicked() {
            self.invert_selection(&view);
            ui.close();
        }
        if ui.button("Nessuna (Esc)").clicked() {
            self.selection.clear();
            self.anchor = None;
            ui.close();
        }
        ui.separator();
        if ui.button("Solo i RAW").clicked() {
            self.select_where(&view, |e, _| e.kind == files::Kind::Raw);
            ui.close();
        }
        if ui.button("Solo i JPEG").clicked() {
            self.select_where(&view, |e, _| {
                e.path.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg") || x.eq_ignore_ascii_case("jpeg"))
            });
            ui.close();
        }
        for n in [1u8, 3, 4, 5] {
            if ui.button(format!("Con {n} o più stelle")).clicked() {
                self.select_where(&view, move |_, r| r >= n);
                ui.close();
            }
        }
    }

    fn more_menu(&mut self, ui: &mut Ui) {
        if ui.checkbox(&mut self.settings.stack_pairs, "RAW+JPEG come una sola foto").changed() {
            self.pair_up();
            ui.close();
        }
        if ui.button("Rinomina (F2)").clicked() {
            self.start_rename();
            ui.close();
        }
        if ui.button("Nuova cartella…").clicked() {
            self.modal = Some(Modal::NewFolder(String::new()));
            ui.close();
        }
        ui.separator();
        if ui.button("Tasti rapidi (?)").clicked() {
            self.modal = Some(Modal::Shortcuts);
            ui.close();
        }
        if ui.button("Impostazioni").clicked() {
            self.show_settings = true;
            ui.close();
        }
    }

    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        let shown = self.view().len();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} foto · {} mostrate · {} selezionate",
                    self.entries.len(),
                    shown,
                    self.selection.len()
                ))
                .color(theme::FAINT),
            );
            ui.separator();
            ui.colored_label(
                if self.status_error { Color32::from_rgb(0xb0, 0x30, 0x40) } else { theme::MUTED },
                &self.status,
            );
        });
    }

    pub(super) fn left_panel(&mut self, ui: &mut Ui) {
        let mut go: Option<PathBuf> = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::heading(ui, "Posizioni");
                if let Some(h) = home_dir() {
                    for (label, sub) in [
                        ("Home", ""),
                        ("Immagini", "Pictures"),
                        ("Desktop", "Desktop"),
                        ("Documenti", "Documents"),
                    ] {
                        let p = if sub.is_empty() { h.clone() } else { h.join(sub) };
                        if p.is_dir() {
                            self.place_row(ui, &p, label, &mut go);
                        }
                    }
                }
                theme::heading(ui, "Preferiti");
                let mut remove: Option<usize> = None;
                let favs = self.settings.favorites.clone();
                for (i, f) in favs.iter().enumerate() {
                    let p = PathBuf::from(f);
                    let name = p
                        .file_name()
                        .map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned());
                    let ok = self.fav_ok.get(i).copied().unwrap_or(true);
                    let r = if ok {
                        self.place_row(ui, &p, &name, &mut go)
                    } else {
                        ui.add_enabled(false, egui::Button::new(format!("{name} (non disponibile)")).frame(false))
                            .on_hover_text(f.as_str())
                    };
                    r.context_menu(|ui| {
                        if ui.button("Togli dai preferiti").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                }
                if let Some(i) = remove {
                    self.settings.favorites.remove(i);
                    if i < self.fav_ok.len() {
                        self.fav_ok.remove(i);
                    }
                }
                let here = self.folder.to_string_lossy().into_owned();
                if !self.settings.favorites.contains(&here)
                    && ui.small_button("+ Aggiungi questa cartella").clicked()
                {
                    self.settings.favorites.push(here);
                    self.fav_ok.push(true);
                }
                let recent = self.history.recent(6);
                if !recent.is_empty() {
                    theme::heading(ui, "Recenti");
                    for p in recent {
                        let name = p
                            .file_name()
                            .map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
                        self.place_row(ui, &p, &name, &mut go);
                    }
                }
                theme::heading(ui, "Cartelle");
                for root in roots() {
                    self.tree_node(ui, &root, &mut go);
                }
            });
        self.reveal = false;
        if let Some(p) = go {
            self.go(p);
        }
    }

    /// One row of the "Posizioni", "Preferiti" or "Recenti" lists: a click opens it, files can be
    /// dropped on it.
    fn place_row(&mut self, ui: &mut Ui, p: &Path, name: &str, go: &mut Option<PathBuf>) -> egui::Response {
        let selected = self.folder == p;
        let r = ui
            .selectable_label(selected, name)
            .on_hover_text(p.to_string_lossy());
        self.row_drop(ui, &r, p);
        if r.clicked() {
            *go = Some(p.to_path_buf());
        }
        r
    }

    /// Files dropped on a folder row move there (copy with Ctrl held).
    fn row_drop(&mut self, ui: &Ui, r: &egui::Response, target: &Path) {
        self.drop_target_highlight(ui, r, theme::ACCENT);
        if let Some(files) = r.dnd_release_payload::<DragFiles>() {
            let copy = ui.input(|i| i.modifiers.command);
            let files = files.0.clone();
            self.transfer(target, &files, !copy);
        }
    }

    fn tree_node(&mut self, ui: &mut Ui, path: &Path, go: &mut Option<PathBuf>) {
        let drive = self
            .drive_names
            .try_lock()
            .ok()
            .and_then(|m| m.get(path).cloned());
        let name = drive.unwrap_or_else(|| {
            path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
        let id = ui.make_persistent_id(("tree", path));
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if self.reveal && self.folder.starts_with(path) && self.folder != path {
            state.set_open(true);
        }
        let selected = self.folder == path;
        let header = ui
            .horizontal(|ui| {
                // a folder icon instead of the usual triangle: click it to open or close the branch
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(24.0, 18.0), Sense::click());
                theme::draw_folder(ui.painter(), rect.shrink(1.0), state.is_open());
                if resp.clicked() {
                    state.toggle(ui);
                }
                let label = ui.selectable_label(selected, name);
                self.row_drop(ui, &label, path);
                if label.clicked() {
                    *go = Some(path.to_path_buf());
                }
                label.context_menu(|ui| {
                    if ui.button("Mostra nel file manager").clicked() {
                        self.reveal_in_file_manager(path);
                        ui.close();
                    }
                    if ui.button("Aggiungi ai preferiti").clicked() {
                        let s = path.to_string_lossy().into_owned();
                        if !self.settings.favorites.contains(&s) {
                            self.settings.favorites.push(s);
                            self.fav_ok.push(true);
                        }
                        ui.close();
                    }
                });
            })
            .response;
        state.show_body_indented(&header, ui, |ui| {
            let children = self
                .subdir_cache
                .entry(path.to_path_buf())
                .or_insert_with(|| files::subdirs(path))
                .clone();
            for c in children {
                self.tree_node(ui, &c, go);
            }
        });
    }

    pub(super) fn right_panel(&mut self, ui: &mut Ui) {
        let n = self.selection.len();
        let active = self.active_entry().cloned();
        ui.add_space(2.0);
        let Some(e) = active else {
            ui.label(
                egui::RichText::new(if n == 0 {
                    "Nessuna immagine selezionata."
                } else {
                    "Clicca una foto per vederne l'anteprima."
                })
                .color(theme::MUTED),
            );
            if n > 1 {
                ui.strong(format!("{n} immagini selezionate"));
            }
            return;
        };
        let avail = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, avail * 0.72), Sense::click());
        ui.painter().rect_filled(rect, 12, theme::MAT);
        let tex = self.preview_tex(&e.path);
        let fallback = self.thumbs.get(&e.path).and_then(Clone::clone);
        match tex.or(fallback) {
            Some(t) => {
                let r = super::fit(t.size_vec2(), rect.shrink(4.0));
                ui.painter().add(
                    egui::epaint::RectShape::filled(r, 8, Color32::WHITE).with_texture(
                        t.id(),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    ),
                );
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Anteprima in preparazione…",
                    egui::FontId::proportional(13.0),
                    theme::MUTED,
                );
            }
        }
        if resp.on_hover_text("Clic per vederla a schermo intero").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.open_review();
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(&e.name).strong().size(14.0));
        if n > 1 {
            ui.label(egui::RichText::new(format!("{n} immagini selezionate")).color(theme::ACCENT));
        }
        theme::heading(ui, "Dettagli");
        let ex = self.exifs.get(&e.path).cloned().unwrap_or_default();
        let mut rows: Vec<(&str, String)> = vec![
            ("Tipo", format!("{} · {}", if e.kind == files::Kind::Raw { "Raw" } else { "Immagine" }, files::human_size(e.size))),
            ("Modificato", files::format_time(e.modified)),
        ];
        let pairs = [
            ("Fotocamera", ex.camera.clone()),
            ("Obiettivo", ex.lens.clone()),
            ("Esposizione", ex.exposure_line()),
            ("Focale", ex.focal_text()),
            ("Scatto", ex.taken_text()),
            ("Pixel", ex.size_text()),
        ];
        rows.extend(pairs.into_iter().filter(|(_, v)| !v.is_empty()));
        for (k, v) in rows {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [88.0, 18.0],
                    egui::Label::new(egui::RichText::new(k).color(theme::FAINT).size(12.5)),
                );
                ui.add(egui::Label::new(egui::RichText::new(v).size(12.5)).wrap());
            });
            let y = ui.cursor().top();
            ui.painter().hline(ui.min_rect().x_range(), y, egui::Stroke::new(1.0, theme::LINE));
        }
        theme::heading(ui, "Valutazione (tasti 0-5)");
        let m = self.meta.get(&e.name);
        let cur = m.rating;
        ui.horizontal(|ui| {
            for s in 1..=5u8 {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                super::widgets::draw_star(ui.painter(), rect.center(), 9.5, s <= cur);
                if resp.clicked() {
                    self.rate_selection(if s == cur { 0 } else { s });
                }
            }
        });
        theme::heading(ui, "Etichetta (tasti 6-9)");
        ui.horizontal(|ui| {
            for l in Label::ALL {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                ui.painter().circle_filled(rect.center(), 8.5, label_color(l));
                if m.label == Some(l) {
                    ui.painter().circle_stroke(rect.center(), 11.0, egui::Stroke::new(2.0, theme::ACCENT));
                }
                if resp.on_hover_text(l.name()).clicked() {
                    self.label_selection(if m.label == Some(l) { None } else { Some(l) });
                }
            }
            if ui.small_button("Nessuna").clicked() {
                self.label_selection(None);
            }
        });
        if n > 1 && ui.button("Applica stelle ed etichetta a tutte").clicked() {
            self.copy_meta_from_active();
        }
        theme::heading(ui, "Apri con");
        ui.horizontal_wrapped(|ui| {
            for (choice, text, tip) in [
                (Choice::DarkRaw, "DarkRaw", "Editor di immagini"),
                (Choice::LightRaw, "LightRaw", "Sviluppo raw e libreria"),
                (Choice::Auto, "Automatico", "Raw in LightRaw, il resto in DarkRaw (Invio)"),
            ] {
                let resp = ui.button(text).on_hover_text(tip);
                if resp.clicked() {
                    self.open_selection(choice);
                }
            }
        });
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new("Impostazioni")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for tool in [Tool::DarkRaw, Tool::LightRaw] {
                    let (field, name) = match tool {
                        Tool::DarkRaw => (&mut self.settings.darkraw_path, "DarkRaw"),
                        Tool::LightRaw => (&mut self.settings.lightraw_path, "LightRaw"),
                    };
                    ui.label(format!("Programma {name} (vuoto = cerca accanto a PhotoDesk)"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(field).desired_width(360.0));
                        if ui.button("Sfoglia…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                *field = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    let found = launch::find_tool(tool, field, self.exe_dir.as_deref());
                    match found {
                        Some(p) => ui.colored_label(
                            Color32::from_rgb(0x2c, 0x7a, 0x4a),
                            format!("Trovato: {}", p.display()),
                        ),
                        None => ui.colored_label(Color32::from_rgb(0xb0, 0x50, 0x30), "Non trovato"),
                    };
                    ui.add_space(6.0);
                }
                ui.label("Doppio clic e tasto Invio aprono con:");
                ui.horizontal(|ui| {
                    for c in [Choice::Auto, Choice::DarkRaw, Choice::LightRaw] {
                        ui.selectable_value(&mut self.settings.open_with, c, c.label());
                    }
                });
                ui.add_space(6.0);
                if ui.add(theme::primary_button("Salva")).clicked() {
                    save = true;
                }
            });
        if save {
            self.save_settings();
        }
        if !open || save {
            self.show_settings = false;
        }
    }

    /// While files are dragged, show how many next to the pointer.
    pub(super) fn drag_hint(&self, ctx: &egui::Context) {
        let Some(payload) = egui::DragAndDrop::payload::<DragFiles>(ctx) else {
            return;
        };
        let Some(pos) = ctx.pointer_latest_pos() else { return };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("drag-hint"),
        ));
        let text = format!(
            "{} foto — rilascia su una cartella (Ctrl = copia) o su un pulsante",
            payload.0.len()
        );
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.5), Color32::WHITE);
        let r = Rect::from_min_size(pos + Vec2::new(14.0, 14.0), galley.size() + Vec2::new(16.0, 10.0));
        painter.rect_filled(r, 8, theme::ACCENT);
        painter.galley(r.min + Vec2::new(8.0, 5.0), galley, Color32::WHITE);
        let _ = draw_stars;
    }
}
