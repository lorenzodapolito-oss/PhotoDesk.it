//! The PhotoDesk window: folders on the left, thumbnails in the middle, preview and details on the
//! right. Opening a picture hands it to DarkRaw or LightRaw.

use crate::files::{self, Entry, Kind, Sort};
use crate::launch::{self, Choice, Tool};
use crate::meta::{FolderMeta, Label};
use crate::settings::{self, Settings};
use crate::theme;
use crate::thumbs::{self, Pixels};
use egui::{Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const THUMB_PX: u32 = 320;
const PREVIEW_PX: u32 = 1800;

struct Job {
    generation: u64,
    path: PathBuf,
    kind: Kind,
    max: u32,
}

struct Done {
    generation: u64,
    path: PathBuf,
    preview: bool,
    pixels: Option<Pixels>,
}

/// Fit `item` inside `bounds`, centred, keeping the aspect ratio.
fn fit(item: Vec2, bounds: Rect) -> Rect {
    if item.x <= 0.0 || item.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / item.x).min(bounds.height() / item.y);
    Rect::from_center_size(bounds.center(), item * scale)
}

/// A name that fits `max_chars`, keeping the end (the extension) visible: "IMG_12…3.NEF".
pub fn elide(name: &str, max_chars: usize) -> String {
    let n = name.chars().count();
    if n <= max_chars || max_chars < 6 {
        return name.to_string();
    }
    let tail = 6.min(max_chars / 2);
    let head = max_chars - tail - 1;
    let h: String = name.chars().take(head).collect();
    let t: String = name.chars().skip(n - tail).collect();
    format!("{h}…{t}")
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

fn draw_star(painter: &egui::Painter, c: Pos2, r: f32, filled: bool, color: Color32) {
    let pts = star_points(c, r);
    if filled {
        painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
    } else {
        painter.add(egui::Shape::closed_line(
            pts,
            egui::Stroke::new(1.0, theme::MUTED),
        ));
    }
}

fn label_color(l: Label) -> Color32 {
    let [r, g, b] = l.rgb();
    Color32::from_rgb(r, g, b)
}

pub struct PhotoDesk {
    settings: Settings,
    settings_file: PathBuf,
    exe_dir: Option<PathBuf>,
    folder: PathBuf,
    entries: Vec<Entry>,
    meta: FolderMeta,
    sort: Sort,
    descending: bool,
    min_rating: u8,
    label_filter: Option<Label>,
    search: String,
    selection: HashSet<PathBuf>,
    anchor: Option<PathBuf>,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    pending: HashSet<PathBuf>,
    preview: Option<(PathBuf, Option<TextureHandle>)>,
    preview_wanted: Option<PathBuf>,
    generation: u64,
    thumb_jobs: Sender<Job>,
    preview_jobs: Sender<Job>,
    results: Receiver<Done>,
    subdir_cache: HashMap<PathBuf, Vec<PathBuf>>,
    reveal: bool,
    status: String,
    status_error: bool,
    show_settings: bool,
    columns: usize,
}

impl PhotoDesk {
    pub fn new(ctx: &egui::Context, start: Option<PathBuf>) -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let settings_file = settings::settings_path(exe_dir.as_deref());
        let settings = Settings::load(&settings_file);

        let (res_tx, results) = channel::<Done>();
        let (thumb_jobs, thumb_rx) = channel::<Job>();
        let (preview_jobs, preview_rx) = channel::<Job>();
        let thumb_rx = Arc::new(Mutex::new(thumb_rx));
        for _ in 0..2 {
            spawn_worker(ctx.clone(), thumb_rx.clone(), res_tx.clone(), false);
        }
        spawn_worker(ctx.clone(), Arc::new(Mutex::new(preview_rx)), res_tx, true);

        theme::apply(ctx);
        let mut app = PhotoDesk {
            settings,
            settings_file,
            exe_dir,
            folder: PathBuf::new(),
            entries: Vec::new(),
            meta: FolderMeta::default(),
            sort: Sort::Name,
            descending: false,
            min_rating: 0,
            label_filter: None,
            search: String::new(),
            selection: HashSet::new(),
            anchor: None,
            thumbs: HashMap::new(),
            pending: HashSet::new(),
            preview: None,
            preview_wanted: None,
            generation: 0,
            thumb_jobs,
            preview_jobs,
            results,
            subdir_cache: HashMap::new(),
            reveal: true,
            status: String::new(),
            status_error: false,
            show_settings: false,
            columns: 1,
        };
        let (folder, select) = match start {
            Some(p) if p.is_dir() => (p, None),
            Some(p) if p.is_file() => (
                p.parent().map(Path::to_path_buf).unwrap_or_default(),
                Some(p),
            ),
            _ => {
                let last = PathBuf::from(&app.settings.last_folder);
                let f = if last.is_dir() {
                    last
                } else {
                    home_dir()
                        .map(|h| h.join("Pictures"))
                        .filter(|p| p.is_dir())
                        .or_else(home_dir)
                        .unwrap_or_else(|| PathBuf::from("."))
                };
                (f, None)
            }
        };
        app.navigate(folder);
        if let Some(p) = select {
            app.anchor = Some(p.clone());
            app.selection.insert(p);
        }
        app
    }

    fn navigate(&mut self, folder: PathBuf) {
        self.generation += 1;
        self.thumbs.clear();
        self.pending.clear();
        self.selection.clear();
        self.anchor = None;
        self.preview = None;
        self.preview_wanted = None;
        self.reveal = true;
        match files::scan_dir(&folder) {
            Ok(mut v) => {
                files::sort_entries(&mut v, self.sort, self.descending);
                self.status = format!("{} immagini", v.len());
                self.entries = v;
            }
            Err(e) => {
                self.entries.clear();
                self.status = format!("Cartella non leggibile: {e}");
            }
        }
        self.meta = FolderMeta::load(&folder);
        self.settings.last_folder = folder.to_string_lossy().into_owned();
        self.folder = folder;
    }

    fn refresh(&mut self) {
        self.subdir_cache.clear();
        let keep = self.selection.clone();
        let f = self.folder.clone();
        self.navigate(f);
        self.selection = keep
            .into_iter()
            .filter(|p| self.entries.iter().any(|e| &e.path == p))
            .collect();
    }

    fn resort(&mut self) {
        files::sort_entries(&mut self.entries, self.sort, self.descending);
    }

    /// Indices into `entries` that pass the filters, in display order.
    fn view(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let m = self.meta.get(&e.name);
                m.rating >= self.min_rating
                    && self.label_filter.is_none_or(|l| m.label == Some(l))
                    && (q.is_empty() || e.name.to_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selected_entries(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| self.selection.contains(&e.path))
            .collect()
    }

    fn save_meta(&mut self) {
        if let Err(e) = self.meta.save(&self.folder) {
            self.status = format!("Valutazioni non salvate: {e}");
        }
    }

    fn rate_selection(&mut self, rating: u8) {
        let names: Vec<String> = self
            .selected_entries()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        for n in &names {
            self.meta.set_rating(n, rating);
        }
        if !names.is_empty() {
            self.save_meta();
        }
    }

    fn label_selection(&mut self, label: Option<Label>) {
        let names: Vec<String> = self
            .selected_entries()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        for n in &names {
            self.meta.set_label(n, label);
        }
        if !names.is_empty() {
            self.save_meta();
        }
    }

    fn open_selection(&mut self, choice: Choice) {
        let mut groups: Vec<(Tool, Vec<PathBuf>)> = Vec::new();
        for e in self.selected_entries() {
            let tool = launch::tool_for(choice, e.kind);
            match groups.iter_mut().find(|(t, _)| *t == tool) {
                Some((_, v)) => v.push(e.path.clone()),
                None => groups.push((tool, vec![e.path.clone()])),
            }
        }
        if groups.is_empty() {
            self.status = "Seleziona prima una o più immagini.".into();
            return;
        }
        let mut msgs = Vec::new();
        let mut failed = false;
        for (tool, files) in groups {
            if launch::find_tool(tool, self.configured(tool), self.exe_dir.as_deref()).is_none() {
                // not found next to PhotoDesk: ask where it is, and remember the answer
                let mut dialog =
                    rfd::FileDialog::new().set_title(format!("Dove si trova {}?", tool.name()));
                if cfg!(windows) {
                    dialog = dialog.add_filter("Programma", &["exe"]);
                }
                match dialog.pick_file() {
                    Some(p) => {
                        *self.configured_mut(tool) = p.to_string_lossy().into_owned();
                        self.save_settings();
                    }
                    None => {
                        msgs.push(format!(
                            "{} non trovato: indica dove si trova il programma.",
                            tool.name()
                        ));
                        failed = true;
                        continue;
                    }
                }
            }
            match launch::open(tool, self.configured(tool), self.exe_dir.as_deref(), &files) {
                Ok(()) => msgs.push(format!("{}: {} file aperti", tool.name(), files.len())),
                Err(e) => {
                    msgs.push(e);
                    failed = true;
                }
            }
        }
        self.status = msgs.join(" · ");
        self.status_error = failed;
    }

    fn configured(&self, tool: Tool) -> &str {
        match tool {
            Tool::DarkRaw => &self.settings.darkraw_path,
            Tool::LightRaw => &self.settings.lightraw_path,
        }
    }

    fn configured_mut(&mut self, tool: Tool) -> &mut String {
        match tool {
            Tool::DarkRaw => &mut self.settings.darkraw_path,
            Tool::LightRaw => &mut self.settings.lightraw_path,
        }
    }

    fn request(&mut self, e: &Entry, preview: bool) {
        let job = Job {
            generation: self.generation,
            path: e.path.clone(),
            kind: e.kind,
            max: if preview { PREVIEW_PX } else { THUMB_PX },
        };
        let _ = if preview {
            self.preview_jobs.send(job)
        } else {
            self.thumb_jobs.send(job)
        };
    }

    fn collect_results(&mut self, ctx: &egui::Context) {
        while let Ok(d) = self.results.try_recv() {
            if d.generation != self.generation {
                continue;
            }
            let tex = d.pixels.map(|p| {
                let img = egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba);
                ctx.load_texture(d.path.to_string_lossy(), img, TextureOptions::LINEAR)
            });
            if d.preview {
                if self.preview_wanted.as_ref() == Some(&d.path) {
                    self.preview = Some((d.path, tex));
                }
            } else {
                self.pending.remove(&d.path);
                self.thumbs.insert(d.path, tex);
            }
        }
    }

    fn update_preview_target(&mut self) {
        let single = (self.selection.len() == 1)
            .then(|| self.selection.iter().next().cloned())
            .flatten()
            .or_else(|| self.anchor.clone().filter(|a| self.selection.contains(a)));
        if single == self.preview_wanted {
            return;
        }
        self.preview_wanted = single.clone();
        let Some(p) = single else {
            self.preview = None;
            return;
        };
        if self.preview.as_ref().is_some_and(|(q, _)| q != &p) {
            self.preview = None;
        }
        if let Some(e) = self.entries.iter().find(|e| e.path == p).cloned() {
            self.request(&e, true);
        }
    }

    fn select_click(&mut self, view: &[usize], pos: usize, shift: bool, ctrl: bool) {
        let Some(&idx) = view.get(pos) else { return };
        let path = self.entries[idx].path.clone();
        if shift {
            let anchor_pos = self
                .anchor
                .as_ref()
                .and_then(|a| view.iter().position(|&i| &self.entries[i].path == a))
                .unwrap_or(pos);
            let (a, b) = (anchor_pos.min(pos), anchor_pos.max(pos));
            if !ctrl {
                self.selection.clear();
            }
            for &i in view.iter().skip(a).take(b - a + 1) {
                self.selection.insert(self.entries[i].path.clone());
            }
        } else if ctrl {
            if !self.selection.remove(&path) {
                self.selection.insert(path.clone());
            }
            self.anchor = Some(path);
        } else {
            self.selection.clear();
            self.selection.insert(path.clone());
            self.anchor = Some(path);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context, view: &[usize]) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (keys, shift, ctrl) = ctx.input(|i| {
            let mut ks = Vec::new();
            for (k, n) in [
                (egui::Key::Num0, 0u8),
                (egui::Key::Num1, 1),
                (egui::Key::Num2, 2),
                (egui::Key::Num3, 3),
                (egui::Key::Num4, 4),
                (egui::Key::Num5, 5),
            ] {
                if i.key_pressed(k) {
                    ks.push(Cmd::Rate(n));
                }
            }
            for (k, c) in [
                (egui::Key::ArrowLeft, -1i64),
                (egui::Key::ArrowRight, 1),
                (egui::Key::ArrowUp, -(self.columns as i64)),
                (egui::Key::ArrowDown, self.columns as i64),
            ] {
                if i.key_pressed(k) {
                    ks.push(Cmd::Move(c));
                }
            }
            if i.key_pressed(egui::Key::Enter) {
                ks.push(Cmd::Open);
            }
            if i.key_pressed(egui::Key::F5) {
                ks.push(Cmd::Refresh);
            }
            if i.modifiers.command && i.key_pressed(egui::Key::A) {
                ks.push(Cmd::All);
            }
            (ks, i.modifiers.shift, i.modifiers.command)
        });
        for k in keys {
            match k {
                Cmd::Rate(n) => self.rate_selection(n),
                Cmd::Open => self.open_selection(self.settings.open_with),
                Cmd::Refresh => self.refresh(),
                Cmd::All => {
                    self.selection = view.iter().map(|&i| self.entries[i].path.clone()).collect();
                }
                Cmd::Move(delta) => {
                    if view.is_empty() {
                        continue;
                    }
                    let cur = self
                        .anchor
                        .as_ref()
                        .and_then(|a| view.iter().position(|&i| &self.entries[i].path == a));
                    let next = match cur {
                        Some(c) => (c as i64 + delta).clamp(0, view.len() as i64 - 1) as usize,
                        None => 0,
                    };
                    self.select_click(view, next, shift, ctrl && shift);
                    if !shift {
                        // plain arrows move a single selection
                        let p = self.entries[view[next]].path.clone();
                        self.selection.clear();
                        self.selection.insert(p.clone());
                        self.anchor = Some(p);
                    }
                }
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            if p.is_dir() {
                self.navigate(p);
            } else if let Some(parent) = p.parent() {
                self.navigate(parent.to_path_buf());
                self.anchor = Some(p.clone());
                self.selection.insert(p);
            }
        }
    }

    // ---- panels ----

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut go: Option<PathBuf> = None;
        let mut resort = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Apri cartella…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .set_directory(&self.folder)
                    .pick_folder()
                {
                    go = Some(p);
                }
            }
            if ui
                .button("Su")
                .on_hover_text("Cartella superiore")
                .clicked()
            {
                go = self.folder.parent().map(Path::to_path_buf);
            }
            if ui.button("Aggiorna").on_hover_text("F5").clicked() {
                self.refresh();
            }
            ui.separator();
            ui.label("Ordina:");
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
            ui.label("Stelle da:");
            egui::ComboBox::from_id_salt("minrating")
                .selected_text(if self.min_rating == 0 {
                    "tutte".to_string()
                } else {
                    format!("{}+", self.min_rating)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.min_rating, 0, "tutte");
                    for n in 1..=5u8 {
                        ui.selectable_value(&mut self.min_rating, n, format!("{n}+"));
                    }
                });
            ui.label("Etichetta:");
            egui::ComboBox::from_id_salt("labelfilter")
                .selected_text(self.label_filter.map_or("tutte", Label::name))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.label_filter, None, "tutte");
                    for l in Label::ALL {
                        ui.selectable_value(&mut self.label_filter, Some(l), l.name());
                    }
                });
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Cerca per nome")
                    .desired_width(150.0),
            );
            ui.add(
                egui::Slider::new(&mut self.settings.thumb_size, 90.0..=300.0).text("Miniature"),
            );
            if ui.button("Impostazioni").clicked() {
                self.show_settings = true;
            }
        });
        if resort {
            self.resort();
        }
        if let Some(p) = go {
            self.navigate(p);
        }
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let mut go: Option<PathBuf> = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong("Posizioni");
                if let Some(h) = home_dir() {
                    for (label, sub) in [
                        ("Home", ""),
                        ("Immagini", "Pictures"),
                        ("Desktop", "Desktop"),
                        ("Documenti", "Documents"),
                    ] {
                        let p = if sub.is_empty() {
                            h.clone()
                        } else {
                            h.join(sub)
                        };
                        if p.is_dir() && ui.selectable_label(self.folder == p, label).clicked() {
                            go = Some(p);
                        }
                    }
                }
                ui.add_space(6.0);
                ui.strong("Preferiti");
                let mut remove: Option<usize> = None;
                for (i, f) in self.settings.favorites.iter().enumerate() {
                    let p = PathBuf::from(f);
                    let name = p
                        .file_name()
                        .map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned());
                    let r = ui
                        .selectable_label(self.folder == p, name)
                        .on_hover_text(f.as_str());
                    if r.clicked() {
                        go = Some(p);
                    }
                    r.context_menu(|ui| {
                        if ui.button("Togli dai preferiti").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                }
                if let Some(i) = remove {
                    self.settings.favorites.remove(i);
                }
                let here = self.folder.to_string_lossy().into_owned();
                if !self.settings.favorites.contains(&here)
                    && ui.small_button("+ Aggiungi questa cartella").clicked()
                {
                    self.settings.favorites.push(here);
                }
                ui.add_space(6.0);
                ui.strong("Cartelle");
                for root in roots() {
                    self.tree_node(ui, &root, &mut go);
                }
            });
        self.reveal = false;
        if let Some(p) = go {
            self.navigate(p);
        }
    }

    fn tree_node(&mut self, ui: &mut egui::Ui, path: &Path, go: &mut Option<PathBuf>) {
        let name = path.file_name().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let id = ui.make_persistent_id(("tree", path));
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if self.reveal && self.folder.starts_with(path) && self.folder != path {
            state.set_open(true);
        }
        let selected = self.folder == path;
        let open = state.is_open();
        state
            .show_header(ui, |ui| {
                if ui.selectable_label(selected, name).clicked() {
                    *go = Some(path.to_path_buf());
                }
            })
            .body(|ui| {
                let children = self
                    .subdir_cache
                    .entry(path.to_path_buf())
                    .or_insert_with(|| files::subdirs(path))
                    .clone();
                for c in children {
                    self.tree_node(ui, &c, go);
                }
            });
        let _ = open;
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        let sel: Vec<Entry> = self.selected_entries().into_iter().cloned().collect();
        ui.add_space(4.0);
        match sel.len() {
            0 => {
                ui.label("Nessuna immagine selezionata.");
            }
            1 => {
                let e = &sel[0];
                let avail = ui.available_width();
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(avail, avail * 0.75), Sense::hover());
                ui.painter().rect_filled(rect, 8.0, theme::MAT);
                let tex = self
                    .preview
                    .as_ref()
                    .filter(|(p, _)| p == &e.path)
                    .and_then(|(_, t)| t.clone());
                let fallback = self.thumbs.get(&e.path).and_then(|t| t.clone());
                match tex.or(fallback) {
                    Some(t) => {
                        ui.painter().image(
                            t.id(),
                            fit(t.size_vec2(), rect.shrink(2.0)),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Anteprima non disponibile",
                            egui::FontId::proportional(13.0),
                            theme::MUTED,
                        );
                    }
                }
                ui.add_space(6.0);
                ui.strong(&e.name);
                ui.label(format!(
                    "{} · {}",
                    if e.kind == Kind::Raw {
                        "Raw"
                    } else {
                        "Immagine"
                    },
                    files::human_size(e.size)
                ));
                ui.label(files::format_time(e.modified));
            }
            n => {
                ui.strong(format!("{n} immagini selezionate"));
            }
        }
        if !sel.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            ui.label("Valutazione (tasti 0-5)");
            let cur = if sel.len() == 1 {
                self.meta.get(&sel[0].name).rating
            } else {
                0
            };
            ui.horizontal(|ui| {
                for n in 1..=5u8 {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    draw_star(ui.painter(), rect.center(), 9.0, n <= cur, theme::STAR);
                    if resp.clicked() {
                        self.rate_selection(if n == cur { 0 } else { n });
                    }
                }
                if ui
                    .small_button("0")
                    .on_hover_text("Nessuna stella")
                    .clicked()
                {
                    self.rate_selection(0);
                }
            });
            ui.label("Etichetta");
            let cur_label = if sel.len() == 1 {
                self.meta.get(&sel[0].name).label
            } else {
                None
            };
            ui.horizontal(|ui| {
                for l in Label::ALL {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    ui.painter()
                        .rect_filled(rect.shrink(3.0), 3.0, label_color(l));
                    if cur_label == Some(l) {
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            4.0,
                            egui::Stroke::new(2.0, Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if resp.on_hover_text(l.name()).clicked() {
                        self.label_selection(if cur_label == Some(l) { None } else { Some(l) });
                    }
                }
                if ui.small_button("Nessuna").clicked() {
                    self.label_selection(None);
                }
            });
            ui.add_space(8.0);
            ui.separator();
            ui.label("Apri con");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("DarkRaw")
                    .on_hover_text("Editor di immagini")
                    .clicked()
                {
                    self.open_selection(Choice::DarkRaw);
                }
                if ui
                    .button("LightRaw")
                    .on_hover_text("Sviluppo raw e libreria")
                    .clicked()
                {
                    self.open_selection(Choice::LightRaw);
                }
                if ui
                    .button("Automatico")
                    .on_hover_text("Raw in LightRaw, il resto in DarkRaw (Invio)")
                    .clicked()
                {
                    self.open_selection(Choice::Auto);
                }
            });
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui) {
        let view = self.view();
        if view.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(if self.entries.is_empty() {
                    "Nessuna immagine in questa cartella."
                } else {
                    "Nessuna immagine corrisponde ai filtri."
                });
            });
            return;
        }
        let size = self.settings.thumb_size;
        let cell = Vec2::new(size + 12.0, size + 46.0);
        let cols = ((ui.available_width() / cell.x).floor() as usize).max(1);
        self.columns = cols;
        let rows = view.len().div_ceil(cols);
        let mut clicked: Option<(usize, bool, bool)> = None;
        let mut double: Option<usize> = None;
        let mut want: Vec<usize> = Vec::new();
        let mods = ui.input(|i| i.modifiers);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, cell.y, rows, |ui, range| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for row in range {
                    ui.horizontal(|ui| {
                        for col in 0..cols {
                            let pos = row * cols + col;
                            let Some(&idx) = view.get(pos) else { break };
                            let e = &self.entries[idx];
                            let (rect, resp) = ui.allocate_exact_size(cell, Sense::click());
                            let selected = self.selection.contains(&e.path);
                            let painter = ui.painter().with_clip_rect(rect);
                            let card = rect.shrink(3.0);
                            if selected {
                                painter.rect_filled(card, 10.0, theme::CARD_SELECTED);
                                painter.rect_stroke(
                                    card,
                                    10.0,
                                    egui::Stroke::new(2.0, theme::ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                            } else {
                                painter.rect_filled(card, 10.0, theme::CARD);
                                if resp.hovered() {
                                    painter.rect_stroke(
                                        card,
                                        10.0,
                                        egui::Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.6)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                            let img_rect = Rect::from_min_size(
                                rect.min + Vec2::new(6.0, 6.0),
                                Vec2::splat(size),
                            );
                            painter.rect_filled(img_rect, 6.0, theme::MAT);
                            match self.thumbs.get(&e.path) {
                                Some(Some(t)) => {
                                    painter.image(
                                        t.id(),
                                        fit(t.size_vec2(), img_rect),
                                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                }
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
                                    if self.pending.insert(e.path.clone()) {
                                        want.push(idx);
                                    }
                                }
                            }
                            let m = self.meta.get(&e.name);
                            if let Some(l) = m.label {
                                painter.rect_filled(
                                    Rect::from_min_size(
                                        img_rect.left_bottom() + Vec2::new(0.0, 4.0),
                                        Vec2::new(size, 4.0),
                                    ),
                                    2.0,
                                    label_color(l),
                                );
                            }
                            let text_color = ui.visuals().text_color();
                            painter.text(
                                Pos2::new(rect.center().x, img_rect.bottom() + 10.0),
                                egui::Align2::CENTER_TOP,
                                elide(&e.name, ((size + 8.0) / 6.5) as usize),
                                egui::FontId::proportional(12.0),
                                text_color,
                            );
                            for s in 0..5u8 {
                                draw_star(
                                    &painter,
                                    Pos2::new(
                                        rect.center().x - 24.0 + s as f32 * 12.0,
                                        img_rect.bottom() + 36.0,
                                    ),
                                    5.0,
                                    s < m.rating,
                                    theme::STAR,
                                );
                            }
                            if e.kind == Kind::Raw {
                                let badge = Rect::from_min_size(
                                    img_rect.right_top() + Vec2::new(-34.0, 5.0),
                                    Vec2::new(30.0, 15.0),
                                );
                                painter.rect_filled(badge, 7.0, theme::ACCENT);
                                painter.text(
                                    badge.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "RAW",
                                    egui::FontId::proportional(10.0),
                                    Color32::WHITE,
                                );
                            }
                            if resp.clicked() {
                                clicked = Some((pos, mods.shift, mods.command));
                            }
                            if resp.double_clicked() {
                                double = Some(pos);
                            }
                        }
                    });
                }
            });
        for idx in want {
            let e = self.entries[idx].clone();
            self.request(&e, false);
        }
        if let Some((pos, shift, ctrl)) = clicked {
            self.select_click(&view, pos, shift, ctrl);
        }
        if let Some(pos) = double {
            if let Some(&idx) = view.get(pos) {
                let p = self.entries[idx].path.clone();
                if !self.selection.contains(&p) {
                    self.selection.clear();
                    self.selection.insert(p);
                }
                self.open_selection(self.settings.open_with);
            }
        }
        self.keyboard(ui.ctx(), &view);
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
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
                    ui.label(format!(
                        "Programma {name} (vuoto = cerca accanto a PhotoDesk)"
                    ));
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
                            Color32::from_rgb(28, 120, 68),
                            format!("Trovato: {}", p.display()),
                        ),
                        None => ui.colored_label(Color32::from_rgb(180, 70, 30), "Non trovato"),
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
                if ui.button("Salva").clicked() {
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

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_file) {
            self.status = format!("Impostazioni non salvate: {e}");
        }
    }
}

enum Cmd {
    Rate(u8),
    Move(i64),
    Open,
    Refresh,
    All,
}

fn spawn_worker(
    ctx: egui::Context,
    jobs: Arc<Mutex<Receiver<Job>>>,
    out: Sender<Done>,
    preview: bool,
) {
    let _ = std::thread::Builder::new()
        .name("photodesk-thumbs".into())
        .spawn(move || loop {
            let job = {
                let guard = jobs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.recv()
            };
            let Ok(job) = job else { return };
            let pixels = thumbs::load(&job.path, job.kind, job.max);
            if out
                .send(Done {
                    generation: job.generation,
                    path: job.path,
                    preview,
                    pixels,
                })
                .is_err()
            {
                return;
            }
            ctx.request_repaint();
        });
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        ('A'..='Z')
            .map(|c| PathBuf::from(format!("{c}:\\")))
            .filter(|p| p.exists())
            .collect()
    } else {
        vec![PathBuf::from("/")]
    }
}

impl eframe::App for PhotoDesk {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.collect_results(&ctx);
        self.handle_drops(&ctx);
        self.update_preview_target();

        egui::Panel::top("toolbar")
            .frame(theme::bar_frame())
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(theme::bar_frame())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let shown = self.view().len();
                    ui.label(format!(
                        "{} — {} di {} mostrate, {} selezionate",
                        self.folder.display(),
                        shown,
                        self.entries.len(),
                        self.selection.len()
                    ));
                    ui.separator();
                    ui.colored_label(
                        if self.status_error {
                            Color32::from_rgb(170, 40, 40)
                        } else {
                            theme::INK
                        },
                        &self.status,
                    );
                });
            });
        egui::Panel::left("folders")
            .default_size(230.0)
            .resizable(true)
            .frame(theme::panel_frame())
            .show(ui, |ui| self.left_panel(ui));
        egui::Panel::right("details")
            .default_size(320.0)
            .resizable(true)
            .frame(theme::panel_frame())
            .show(ui, |ui| self.right_panel(ui));
        egui::CentralPanel::default()
            .frame(theme::stage_frame())
            .show(ui, |ui| self.grid(ui));
        self.settings_window(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elide_keeps_the_end_of_a_long_name() {
        assert_eq!(elide("short.jpg", 20), "short.jpg");
        let e = elide("DSC_0001234567890.NEF", 14);
        assert_eq!(e.chars().count(), 14);
        assert!(e.ends_with("90.NEF"));
        assert!(e.contains('…'));
        assert_eq!(elide("abcdefghij", 3), "abcdefghij");
    }

    #[test]
    fn fit_keeps_the_aspect_ratio_inside_the_bounds() {
        let b = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        let r = fit(Vec2::new(200.0, 100.0), b);
        assert_eq!((r.width(), r.height()), (100.0, 50.0));
        assert_eq!(fit(Vec2::ZERO, b), b);
    }

    #[test]
    fn a_star_has_ten_points_around_its_centre() {
        let p = star_points(Pos2::new(10.0, 10.0), 5.0);
        assert_eq!(p.len(), 10);
        assert!(p
            .iter()
            .all(|q| (q.x - 10.0).abs() <= 5.01 && (q.y - 10.0).abs() <= 5.01));
    }
}
