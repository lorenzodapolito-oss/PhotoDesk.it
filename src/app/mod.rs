//! The PhotoDesk window: folders on the left, thumbnails in the middle, preview and details on the
//! right. Opening a picture hands it to DarkRaw or LightRaw.
//!
//! This file holds the state and the logic (navigation, selection, ratings, file operations,
//! background work); the drawing lives in the sub-modules.

mod dialogs;
mod grid;
mod panels;
mod review;
mod widgets;

use crate::cache::Cache;
use crate::drives;
use crate::exif::ExifSummary;
use crate::files::{self, Entry, Kind, Sort};
use crate::history::History;
use crate::launch::{self, Choice, Tool};
use crate::meta::{FolderMeta, Label};
use crate::ops;
use crate::settings::{self, Settings};
use crate::theme;
use crate::thumbs::{self, Pixels};
use chrono::NaiveDateTime;
use egui::{Color32, Rect, TextureHandle, TextureOptions, Vec2};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const THUMB_PX: u32 = 320;
const PREVIEW_PX: u32 = 2400;
/// The thumbnail cache is kept under this size.
const CACHE_LIMIT: u64 = 600 * 1024 * 1024;
/// Ratings are written to disk this long after the last change (and always when leaving).
const SAVE_DELAY: Duration = Duration::from_millis(600);

// ---- background work ----

struct Job {
    generation: u64,
    path: PathBuf,
    kind: Kind,
    max: u32,
    /// Cache name, for thumbnails.
    key: Option<String>,
}

struct Done {
    generation: u64,
    path: PathBuf,
    preview: bool,
    pixels: Option<Pixels>,
    exif: Option<ExifSummary>,
}

/// Jobs waiting for a worker. The newest is taken first: it is what is on screen now.
struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
}

impl Queue {
    fn new() -> Arc<Queue> {
        Arc::new(Queue {
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
        })
    }

    fn push(&self, job: Job) {
        let mut q = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        q.push_front(job);
        // a long queue of old requests is never worth keeping
        q.truncate(4000);
        drop(q);
        self.wake.notify_one();
    }

    /// The next job of the current generation (blocks while there is none).
    fn next(&self, current: &AtomicU64) -> Job {
        let mut q = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            while let Some(j) = q.pop_front() {
                if j.generation == current.load(Ordering::Relaxed) {
                    return j;
                }
            }
            q = self
                .wake
                .wait(q)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

fn spawn_worker(
    ctx: egui::Context,
    queue: Arc<Queue>,
    generation: Arc<AtomicU64>,
    out: Sender<Done>,
    preview: bool,
    cache: Option<Cache>,
) {
    let _ = std::thread::Builder::new()
        .name("photodesk-thumbs".into())
        .spawn(move || loop {
            let job = queue.next(&generation);
            let (pixels, exif) = run_job(&job, cache.as_ref());
            if out
                .send(Done {
                    generation: job.generation,
                    path: job.path,
                    preview,
                    pixels,
                    exif,
                })
                .is_err()
            {
                return;
            }
            ctx.request_repaint();
        });
}

fn run_job(job: &Job, cache: Option<&Cache>) -> (Option<Pixels>, Option<ExifSummary>) {
    if let (Some(c), Some(k)) = (cache, job.key.as_deref()) {
        if let Some(hit) = c.get(k) {
            return (Some(hit.pixels), hit.exif);
        }
    }
    match thumbs::load_with_exif(&job.path, job.kind, job.max) {
        Some((px, ex)) => {
            if let (Some(c), Some(k)) = (cache, job.key.as_deref()) {
                c.put(k, &px, ex.as_ref());
            }
            (Some(px), ex)
        }
        None => (None, None),
    }
}

/// What the folder reader found.
struct ScanDone {
    generation: u64,
    entries: Result<Vec<Entry>, String>,
    subfolders: Vec<PathBuf>,
    meta: FolderMeta,
    fav_ok: Vec<bool>,
}

/// A cheap fingerprint of which pictures a folder holds (names only, no file is opened).
fn folder_signature(dir: &Path) -> Option<u64> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for item in rd.flatten() {
        let name = item.file_name();
        if files::classify(Path::new(&name)).is_some() {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in name.to_string_lossy().bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            sum = sum.wrapping_add(h);
            count += 1;
        }
    }
    Some(sum ^ count.rotate_left(48))
}

// ---- filters ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindFilter {
    All,
    Raw,
    Jpeg,
    Other,
}

impl KindFilter {
    pub const ALL: [KindFilter; 4] = [
        KindFilter::All,
        KindFilter::Raw,
        KindFilter::Jpeg,
        KindFilter::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            KindFilter::All => "tutti i tipi",
            KindFilter::Raw => "solo RAW",
            KindFilter::Jpeg => "solo JPEG",
            KindFilter::Other => "PNG, TIFF, altri",
        }
    }
    pub fn matches(self, e: &Entry) -> bool {
        let jpeg = e
            .path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("jpg") || x.eq_ignore_ascii_case("jpeg"));
        match self {
            KindFilter::All => true,
            KindFilter::Raw => e.kind == Kind::Raw,
            KindFilter::Jpeg => jpeg,
            KindFilter::Other => e.kind == Kind::Image && !jpeg,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateFilter {
    All,
    Today,
    Week,
    Month,
    Year,
}

impl DateFilter {
    pub const ALL: [DateFilter; 5] = [
        DateFilter::All,
        DateFilter::Today,
        DateFilter::Week,
        DateFilter::Month,
        DateFilter::Year,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DateFilter::All => "qualsiasi data",
            DateFilter::Today => "oggi",
            DateFilter::Week => "ultimi 7 giorni",
            DateFilter::Month => "ultimi 30 giorni",
            DateFilter::Year => "ultimo anno",
        }
    }
    pub fn matches(self, when: Option<NaiveDateTime>, now: NaiveDateTime) -> bool {
        let days = match self {
            DateFilter::All => return true,
            DateFilter::Today => 0,
            DateFilter::Week => 7,
            DateFilter::Month => 30,
            DateFilter::Year => 365,
        };
        let Some(d) = when else { return false };
        let age = now.date().signed_duration_since(d.date()).num_days();
        (0..=days).contains(&age)
    }
}

// ---- state ----

/// What is being dragged: the files of the selection.
pub struct DragFiles(pub Vec<PathBuf>);

/// A question or a small form shown over the window.
enum Modal {
    Trash(Vec<PathBuf>),
    Rename { path: PathBuf, text: String },
    Batch { pattern: String, start: u32 },
    NewFolder(String),
    Shortcuts,
}

/// Full-screen review (and compare) of the pictures.
struct Review {
    /// 1.0 = the picture fits the screen.
    zoom: f32,
    /// Where the picture is moved to when zoomed, in screen pixels.
    offset: Vec2,
    /// Several pictures side by side (2 to 4).
    compare: Vec<PathBuf>,
    /// The picture the zoom belongs to (a new one starts fitted).
    shown: Option<PathBuf>,
}

pub struct PhotoDesk {
    settings: Settings,
    settings_file: PathBuf,
    exe_dir: Option<PathBuf>,
    folder: PathBuf,
    entries: Vec<Entry>,
    subfolders: Vec<PathBuf>,
    meta: FolderMeta,
    meta_dirty: Option<Instant>,
    sort: Sort,
    descending: bool,
    min_rating: u8,
    label_filter: Option<Label>,
    kind_filter: KindFilter,
    date_filter: DateFilter,
    camera_filter: Option<String>,
    search: String,
    selection: HashSet<PathBuf>,
    /// The picture the preview shows: the last one clicked (the "active" one).
    anchor: Option<PathBuf>,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    pending: HashSet<PathBuf>,
    previews: HashMap<PathBuf, Option<TextureHandle>>,
    preview_pending: HashSet<PathBuf>,
    exifs: HashMap<PathBuf, ExifSummary>,
    /// RAW name -> name of the JPEG with the same name.
    companions: HashMap<String, String>,
    /// The JPEGs that are shown under their RAW.
    hidden: HashSet<String>,
    generation: Arc<AtomicU64>,
    thumb_queue: Arc<Queue>,
    preview_queue: Arc<Queue>,
    results: Receiver<Done>,
    scan_gen: u64,
    scan_tx: Sender<ScanDone>,
    scans: Receiver<ScanDone>,
    change_tx: Sender<u64>,
    changes: Receiver<u64>,
    loading: bool,
    keep_selection: HashSet<PathBuf>,
    select_after: Option<PathBuf>,
    fav_ok: Vec<bool>,
    history: History,
    subdir_cache: HashMap<PathBuf, Vec<PathBuf>>,
    reveal: bool,
    drive_names: Arc<Mutex<HashMap<PathBuf, String>>>,
    status: String,
    status_error: bool,
    show_settings: bool,
    columns: usize,
    rows_visible: usize,
    modal: Option<Modal>,
    review: Option<Review>,
    clipboard: Vec<PathBuf>,
    clipboard_cut: bool,
    /// The address bar is being edited.
    editing_path: Option<String>,
    /// Where the pointer-drag of files started this frame, for the drop targets.
    ctx: egui::Context,
}

impl PhotoDesk {
    pub fn new(ctx: &egui::Context, start: Option<PathBuf>) -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let settings_file = settings::settings_path(exe_dir.as_deref());
        let settings = Settings::load(&settings_file);

        let generation = Arc::new(AtomicU64::new(1));
        let (res_tx, results) = channel::<Done>();
        let thumb_queue = Queue::new();
        let preview_queue = Queue::new();
        let cache_dir = settings::cache_path(exe_dir.as_deref());
        let cache = Cache::new(cache_dir);
        {
            let c = cache.clone();
            let _ = std::thread::Builder::new()
                .name("photodesk-cache-trim".into())
                .spawn(move || c.trim(CACHE_LIMIT));
        }
        for _ in 0..3 {
            spawn_worker(
                ctx.clone(),
                thumb_queue.clone(),
                generation.clone(),
                res_tx.clone(),
                false,
                Some(cache.clone()),
            );
        }
        spawn_worker(
            ctx.clone(),
            preview_queue.clone(),
            generation.clone(),
            res_tx,
            true,
            None,
        );

        theme::apply(ctx);
        let drive_names = spawn_drive_names(ctx);
        let (scan_tx, scans) = channel::<ScanDone>();
        let (change_tx, changes) = channel::<u64>();
        let sort = Sort::from_id(&settings.sort);
        let descending = settings.descending;
        let min_rating = settings.min_rating.min(5);
        let mut app = PhotoDesk {
            settings,
            settings_file,
            exe_dir,
            folder: PathBuf::new(),
            entries: Vec::new(),
            subfolders: Vec::new(),
            meta: FolderMeta::default(),
            meta_dirty: None,
            sort,
            descending,
            min_rating,
            label_filter: None,
            kind_filter: KindFilter::All,
            date_filter: DateFilter::All,
            camera_filter: None,
            search: String::new(),
            selection: HashSet::new(),
            anchor: None,
            thumbs: HashMap::new(),
            pending: HashSet::new(),
            previews: HashMap::new(),
            preview_pending: HashSet::new(),
            exifs: HashMap::new(),
            companions: HashMap::new(),
            hidden: HashSet::new(),
            generation,
            thumb_queue,
            preview_queue,
            results,
            scan_gen: 0,
            scan_tx,
            scans,
            change_tx,
            changes,
            loading: false,
            keep_selection: HashSet::new(),
            select_after: None,
            fav_ok: Vec::new(),
            history: History::default(),
            subdir_cache: HashMap::new(),
            reveal: true,
            drive_names,
            status: String::new(),
            status_error: false,
            show_settings: false,
            columns: 1,
            rows_visible: 4,
            modal: None,
            review: None,
            clipboard: Vec::new(),
            clipboard_cut: false,
            editing_path: None,
            ctx: ctx.clone(),
        };
        let (folder, select) = initial_folder(start, &app.settings);
        app.select_after = select;
        app.go(folder);
        app
    }

    fn gen(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    // ---- navigation and reading folders ----

    /// Open `folder` (remembering where we were, for the back button).
    fn go(&mut self, folder: PathBuf) {
        if folder != self.folder {
            self.history.visit(&self.folder.clone());
        }
        self.navigate(folder);
    }

    fn navigate(&mut self, folder: PathBuf) {
        self.flush_meta();
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.thumbs.clear();
        self.pending.clear();
        self.previews.clear();
        self.preview_pending.clear();
        self.exifs.clear();
        self.selection.clear();
        self.anchor = None;
        self.review = None;
        self.camera_filter = None;
        self.reveal = true;
        self.entries.clear();
        self.companions.clear();
        self.hidden.clear();
        self.subfolders.clear();
        self.meta = FolderMeta::default();
        self.settings.last_folder = folder.to_string_lossy().into_owned();
        self.folder = folder.clone();
        self.start_scan();
        self.spawn_watcher(folder);
    }

    /// Read the current folder on a worker thread; the result arrives in `apply_scan`.
    fn start_scan(&mut self) {
        self.scan_gen += 1;
        self.loading = true;
        self.status = "Lettura cartella…".into();
        self.status_error = false;
        let (tx, ctx) = (self.scan_tx.clone(), self.ctx.clone());
        let (folder, generation) = (self.folder.clone(), self.scan_gen);
        let favs = self.settings.favorites.clone();
        let spawned = std::thread::Builder::new()
            .name("photodesk-scan".into())
            .spawn(move || {
                let entries = files::scan_dir(&folder);
                let subfolders = files::subdirs(&folder);
                let meta = FolderMeta::load(&folder);
                let fav_ok = favs.iter().map(|f| Path::new(f).is_dir()).collect();
                let _ = tx.send(ScanDone {
                    generation,
                    entries,
                    subfolders,
                    meta,
                    fav_ok,
                });
                ctx.request_repaint();
            });
        if spawned.is_err() {
            self.loading = false;
            self.status = "Impossibile leggere la cartella (thread non avviato).".into();
            self.status_error = true;
        }
    }

    /// Watch the folder: when pictures are added, removed or renamed, tell the window.
    fn spawn_watcher(&self, folder: PathBuf) {
        let (current, mine) = (self.generation.clone(), self.gen());
        let (tx, ctx) = (self.change_tx.clone(), self.ctx.clone());
        let _ = std::thread::Builder::new()
            .name("photodesk-watch".into())
            .spawn(move || {
                let mut last = folder_signature(&folder);
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    if current.load(Ordering::Relaxed) != mine {
                        return;
                    }
                    let now = folder_signature(&folder);
                    if now != last {
                        last = now;
                        if tx.send(mine).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            });
    }

    fn poll_background(&mut self) {
        while let Ok(done) = self.scans.try_recv() {
            self.apply_scan(done);
        }
        let mut changed = false;
        while let Ok(g) = self.changes.try_recv() {
            changed |= g == self.gen();
        }
        if changed && !self.loading {
            self.rescan();
        }
    }

    fn apply_scan(&mut self, done: ScanDone) {
        if done.generation != self.scan_gen {
            return;
        }
        self.loading = false;
        self.fav_ok = done.fav_ok;
        self.subfolders = done.subfolders;
        self.meta = done.meta;
        match done.entries {
            Ok(v) => {
                self.entries = v;
                self.sort_now();
                self.status = format!("{} immagini", self.entries.len());
                self.status_error = false;
            }
            Err(e) => {
                self.entries.clear();
                self.status = format!("Cartella non leggibile: {e}");
                self.status_error = true;
            }
        }
        self.pair_up();
        let keep = std::mem::take(&mut self.keep_selection);
        self.selection = keep
            .into_iter()
            .filter(|p| self.entries.iter().any(|e| &e.path == p))
            .collect();
        if let Some(p) = self.select_after.take() {
            if self.entries.iter().any(|e| e.path == p) {
                self.anchor = Some(p.clone());
                self.selection.insert(p);
            }
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|a| !self.selection.contains(a))
        {
            self.anchor = None;
        }
    }

    /// Read the folder again, keeping the selection (F5, and when files appear or go).
    fn rescan(&mut self) {
        self.flush_meta();
        self.subdir_cache.clear();
        self.keep_selection = self.selection.clone();
        self.start_scan();
    }

    fn refresh(&mut self) {
        self.rescan();
    }

    fn go_up(&mut self) {
        if let Some(p) = self.folder.parent().map(Path::to_path_buf) {
            self.go(p);
        }
    }

    fn go_back(&mut self) {
        let cur = self.folder.clone();
        if let Some(p) = self.history.back(&cur) {
            self.navigate(p);
        }
    }

    fn go_forward(&mut self) {
        let cur = self.folder.clone();
        if let Some(p) = self.history.forward(&cur) {
            self.navigate(p);
        }
    }

    fn sort_now(&mut self) {
        let exifs = &self.exifs;
        files::sort_entries(&mut self.entries, self.sort, self.descending, &|e| {
            exifs.get(&e.path).and_then(ExifSummary::taken_at)
        });
    }

    /// Re-sort after the camera data of some pictures arrived (only matters when sorting by it).
    fn resort(&mut self) {
        self.settings.sort = self.sort.id().into();
        self.settings.descending = self.descending;
        self.sort_now();
    }

    fn pair_up(&mut self) {
        self.companions = files::jpeg_companions(&self.entries);
        self.hidden = if self.settings.stack_pairs {
            self.companions.values().cloned().collect()
        } else {
            HashSet::new()
        };
    }

    // ---- what is shown ----

    fn taken_or_modified(&self, e: &Entry) -> Option<NaiveDateTime> {
        self.exifs
            .get(&e.path)
            .and_then(ExifSummary::taken_at)
            .or_else(|| e.modified.map(files::local_naive))
    }

    /// Indices into `entries` that pass the filters, in display order.
    fn view(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        let now = chrono::Local::now().naive_local();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if self.hidden.contains(&e.name) {
                    return false;
                }
                let m = self.meta.get(&e.name);
                m.rating >= self.min_rating
                    && self.label_filter.is_none_or(|l| m.label == Some(l))
                    && (q.is_empty() || e.name.to_lowercase().contains(&q))
                    && self.kind_filter.matches(e)
                    && (self.date_filter == DateFilter::All
                        || self.date_filter.matches(self.taken_or_modified(e), now))
                    && self.camera_filter.as_ref().is_none_or(|c| {
                        self.exifs.get(&e.path).is_some_and(|x| &x.camera == c)
                    })
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn filtering(&self) -> bool {
        self.min_rating > 0
            || self.label_filter.is_some()
            || self.kind_filter != KindFilter::All
            || self.date_filter != DateFilter::All
            || self.camera_filter.is_some()
    }

    /// The sub-folders shown among the pictures (hidden while a filter is on).
    fn visible_folders(&self) -> Vec<PathBuf> {
        if self.filtering() {
            return Vec::new();
        }
        let q = self.search.trim().to_lowercase();
        self.subfolders
            .iter()
            .filter(|p| {
                q.is_empty()
                    || p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    /// The cameras seen in this folder, for the filter list.
    fn cameras(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .exifs
            .values()
            .map(|x| x.camera.clone())
            .filter(|c| !c.is_empty())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        v.sort();
        v
    }

    fn selected_entries(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| self.selection.contains(&e.path))
            .collect()
    }

    /// The selected pictures with the JPEGs that are stacked under a selected RAW.
    fn selected_with_companions(&self) -> Vec<&Entry> {
        let mut names: HashSet<&str> = HashSet::new();
        for e in self.selected_entries() {
            names.insert(e.name.as_str());
            if let Some(j) = self.companions.get(&e.name) {
                if self.settings.stack_pairs {
                    names.insert(j.as_str());
                }
            }
        }
        self.entries
            .iter()
            .filter(|e| names.contains(e.name.as_str()))
            .collect()
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_with_companions()
            .into_iter()
            .map(|e| e.path.clone())
            .collect()
    }

    fn active_entry(&self) -> Option<&Entry> {
        let p = self.anchor.as_ref().filter(|a| self.selection.contains(*a));
        match p {
            Some(p) => self.entries.iter().find(|e| &e.path == p),
            None if self.selection.len() == 1 => self.selected_entries().into_iter().next(),
            None => None,
        }
    }

    // ---- ratings ----

    fn mark_meta(&mut self) {
        self.meta_dirty.get_or_insert_with(Instant::now);
    }

    /// Write the ratings now if there are unsaved changes.
    fn flush_meta(&mut self) {
        if self.meta_dirty.take().is_none() || self.folder.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = self.meta.save(&self.folder) {
            self.status = format!("Valutazioni non salvate: {e}");
            self.status_error = true;
        }
    }

    fn flush_meta_if_due(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.meta_dirty {
            if t.elapsed() >= SAVE_DELAY {
                self.flush_meta();
            } else {
                ctx.request_repaint_after(SAVE_DELAY);
            }
        }
    }

    fn rate_selection(&mut self, rating: u8) {
        let names: Vec<String> = self
            .selected_with_companions()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        for n in &names {
            self.meta.set_rating(n, rating);
        }
        if !names.is_empty() {
            self.mark_meta();
        }
    }

    /// Stars for one picture (and the JPEG stacked under it).
    fn rate_one(&mut self, path: &Path, rating: u8) {
        let Some(name) = self.entries.iter().find(|e| e.path == path).map(|e| e.name.clone()) else {
            return;
        };
        self.meta.set_rating(&name, rating);
        if self.settings.stack_pairs {
            if let Some(j) = self.companions.get(&name).cloned() {
                self.meta.set_rating(&j, rating);
            }
        }
        self.mark_meta();
    }

    fn label_selection(&mut self, label: Option<Label>) {
        let names: Vec<String> = self
            .selected_with_companions()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        for n in &names {
            self.meta.set_label(n, label);
        }
        if !names.is_empty() {
            self.mark_meta();
        }
    }

    /// Give every selected picture the stars and the label of the active one.
    fn copy_meta_from_active(&mut self) {
        let Some(name) = self.active_entry().map(|e| e.name.clone()) else {
            return;
        };
        let m = self.meta.get(&name);
        self.rate_selection(m.rating);
        self.label_selection(m.label);
        self.status = "Stelle ed etichetta copiate sulla selezione".into();
        self.status_error = false;
    }

    // ---- opening ----

    fn open_selection(&mut self, choice: Choice) {
        let mut groups: Vec<(Tool, Vec<PathBuf>)> = Vec::new();
        for e in self.selected_with_companions() {
            let tool = launch::tool_for(choice, e.kind);
            match groups.iter_mut().find(|(t, _)| *t == tool) {
                Some((_, v)) => v.push(e.path.clone()),
                None => groups.push((tool, vec![e.path.clone()])),
            }
        }
        self.open_groups(groups);
    }

    /// Open exactly these files (not the selection), e.g. from a drop on a button.
    fn open_files(&mut self, choice: Choice, paths: &[PathBuf]) {
        let mut groups: Vec<(Tool, Vec<PathBuf>)> = Vec::new();
        for p in paths {
            let Some(kind) = files::classify(p) else {
                continue;
            };
            let tool = launch::tool_for(choice, kind);
            match groups.iter_mut().find(|(t, _)| *t == tool) {
                Some((_, v)) => v.push(p.clone()),
                None => groups.push((tool, vec![p.clone()])),
            }
        }
        self.open_groups(groups);
    }

    fn open_groups(&mut self, groups: Vec<(Tool, Vec<PathBuf>)>) {
        if groups.is_empty() {
            self.status = "Seleziona prima una o più immagini.".into();
            self.status_error = false;
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

    // ---- pictures in the background ----

    fn request(&mut self, e: &Entry, preview: bool) {
        let job = Job {
            generation: self.gen(),
            path: e.path.clone(),
            kind: e.kind,
            max: if preview { PREVIEW_PX } else { THUMB_PX },
            key: (!preview).then(|| Cache::key(&e.path, e.size, e.modified)),
        };
        if preview {
            self.preview_queue.push(job);
        } else {
            self.thumb_queue.push(job);
        }
    }

    /// Ask for the big picture of `path` unless we have it or already asked.
    fn want_preview(&mut self, path: &Path) {
        if self.previews.contains_key(path) || self.preview_pending.contains(path) {
            return;
        }
        if let Some(e) = self.entries.iter().find(|e| e.path == path).cloned() {
            self.preview_pending.insert(path.to_path_buf());
            self.request(&e, true);
        }
    }

    fn preview_tex(&self, path: &Path) -> Option<TextureHandle> {
        self.previews.get(path).and_then(Clone::clone)
    }

    fn collect_results(&mut self, ctx: &egui::Context) {
        let mut taken_arrived = false;
        while let Ok(d) = self.results.try_recv() {
            if d.generation != self.gen() {
                continue;
            }
            if let Some(x) = d.exif {
                taken_arrived |= x.taken.is_some();
                self.exifs.insert(d.path.clone(), x);
            }
            let tex = d.pixels.map(|p| {
                let img = egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba);
                ctx.load_texture(d.path.to_string_lossy(), img, TextureOptions::LINEAR)
            });
            if d.preview {
                self.preview_pending.remove(&d.path);
                self.previews.insert(d.path, tex);
            } else {
                self.pending.remove(&d.path);
                self.thumbs.insert(d.path, tex);
            }
        }
        if taken_arrived && self.sort == Sort::Taken {
            self.sort_now();
        }
        // keep only a few big pictures in memory
        if self.previews.len() > 10 {
            let keep: HashSet<PathBuf> = self
                .selection
                .iter()
                .cloned()
                .chain(self.anchor.clone())
                .collect();
            self.previews.retain(|p, _| keep.contains(p));
        }
    }

    /// Ask for the preview of the active picture (and of those compared).
    fn update_preview_target(&mut self) {
        let mut wanted: Vec<PathBuf> = Vec::new();
        if let Some(e) = self.active_entry() {
            wanted.push(e.path.clone());
        }
        if let Some(r) = &self.review {
            wanted.extend(r.compare.iter().cloned());
        }
        for p in wanted {
            self.want_preview(&p);
        }
    }

    // ---- selection ----

    fn select_click(&mut self, view: &[usize], pos: usize, shift: bool, ctrl: bool) {
        let Some(&idx) = view.get(pos) else { return };
        let Some(path) = self.entries.get(idx).map(|e| e.path.clone()) else {
            return;
        };
        if shift {
            let anchor_pos = self
                .anchor
                .as_ref()
                .and_then(|a| {
                    view.iter()
                        .position(|&i| self.entries.get(i).is_some_and(|e| &e.path == a))
                })
                .unwrap_or(pos);
            let (a, b) = (anchor_pos.min(pos), anchor_pos.max(pos));
            if !ctrl {
                self.selection.clear();
            }
            for &i in view.iter().skip(a).take(b - a + 1) {
                if let Some(e) = self.entries.get(i) {
                    self.selection.insert(e.path.clone());
                }
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

    fn select_only(&mut self, path: PathBuf) {
        self.selection.clear();
        self.selection.insert(path.clone());
        self.anchor = Some(path);
    }

    fn select_all(&mut self, view: &[usize]) {
        self.selection = view
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|e| e.path.clone()))
            .collect();
    }

    fn invert_selection(&mut self, view: &[usize]) {
        let all: HashSet<PathBuf> = view
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|e| e.path.clone()))
            .collect();
        self.selection = all.difference(&self.selection).cloned().collect();
        self.anchor = None;
    }

    /// Select by what a picture is: all RAW, all JPEG, or stars at least `stars`.
    fn select_where(&mut self, view: &[usize], test: impl Fn(&Entry, u8) -> bool) {
        self.selection = view
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .filter(|e| test(e, self.meta.get(&e.name).rating))
            .map(|e| e.path.clone())
            .collect();
        self.anchor = None;
    }

    /// Move the active picture by `delta` places in the view.
    fn step(&mut self, view: &[usize], delta: i64, extend: bool) {
        if view.is_empty() {
            return;
        }
        let cur = self.anchor.as_ref().and_then(|a| {
            view.iter()
                .position(|&i| self.entries.get(i).is_some_and(|e| &e.path == a))
        });
        let next = match cur {
            Some(c) => (c as i64 + delta).clamp(0, view.len() as i64 - 1) as usize,
            None => 0,
        };
        if extend {
            self.select_click(view, next, true, false);
        } else if let Some(p) = view
            .get(next)
            .and_then(|&i| self.entries.get(i))
            .map(|e| e.path.clone())
        {
            self.select_only(p);
        }
    }

    // ---- files ----

    fn ask_trash(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        self.modal = Some(Modal::Trash(paths));
    }

    fn do_trash(&mut self, paths: &[PathBuf]) {
        match ops::trash(paths) {
            Ok(()) => {
                self.status = format!("{} file nel cestino", paths.len());
                self.status_error = false;
                self.selection.clear();
                self.anchor = None;
                self.rescan();
            }
            Err(e) => {
                self.status = format!("Cestino non riuscito: {e}");
                self.status_error = true;
            }
        }
    }

    fn start_rename(&mut self) {
        let sel = self.selected_entries();
        match sel.len() {
            0 => {}
            1 => {
                let e = sel[0];
                self.modal = Some(Modal::Rename {
                    path: e.path.clone(),
                    text: e.name.clone(),
                });
            }
            _ => {
                self.modal = Some(Modal::Batch {
                    pattern: "{data}_{originale}".into(),
                    start: 1,
                });
            }
        }
    }

    /// What a rename pattern needs about the selected pictures (in view order). A RAW and the
    /// JPEG stacked under it share a number.
    fn rename_items(&self) -> Vec<ops::RenameItem> {
        let view = self.view();
        let mut out = Vec::new();
        let mut group = 0usize;
        let push = |s: &Self, e: &Entry, group: usize, out: &mut Vec<ops::RenameItem>| {
            let date = s
                .exifs
                .get(&e.path)
                .and_then(ExifSummary::taken_at)
                .or_else(|| e.modified.map(files::local_naive));
            out.push(ops::RenameItem {
                path: e.path.clone(),
                date,
                camera: s.exifs.get(&e.path).map(|x| x.camera.clone()).unwrap_or_default(),
                group,
            });
        };
        for &i in &view {
            let Some(e) = self.entries.get(i) else { continue };
            if !self.selection.contains(&e.path) {
                continue;
            }
            push(self, e, group, &mut out);
            if self.settings.stack_pairs {
                if let Some(j) = self.companions.get(&e.name) {
                    if let Some(je) = self.entries.iter().find(|x| &x.name == j) {
                        push(self, je, group, &mut out);
                    }
                }
            }
            group += 1;
        }
        out
    }

    /// Carry out renames; ratings follow the files to their new names.
    fn finish_rename(&mut self, plan: &[ops::Planned]) {
        match ops::apply_rename(plan) {
            Ok(done) => {
                for (from, to) in &done {
                    let (Some(a), Some(b)) = (from.file_name(), to.file_name()) else {
                        continue;
                    };
                    let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
                    let m = self.meta.get(&a);
                    self.meta.set_rating(&a, 0);
                    self.meta.set_label(&a, None);
                    self.meta.set_rating(&b, m.rating);
                    self.meta.set_label(&b, m.label);
                }
                self.flush_meta_now();
                self.selection = done.iter().map(|(_, to)| to.clone()).collect();
                self.anchor = done.first().map(|(_, to)| to.clone());
                self.keep_selection = self.selection.clone();
                self.status = format!("{} file rinominati", done.len());
                self.status_error = false;
                self.start_scan();
            }
            Err(e) => {
                self.status = format!("Rinomina non riuscita: {e}");
                self.status_error = true;
            }
        }
    }

    fn flush_meta_now(&mut self) {
        self.meta_dirty = Some(Instant::now());
        self.flush_meta();
    }

    fn copy_selection(&mut self, cut: bool) {
        self.clipboard = self.selected_paths();
        self.clipboard_cut = cut;
        self.status = format!(
            "{} file {}",
            self.clipboard.len(),
            if cut { "tagliati" } else { "copiati" }
        );
        self.status_error = false;
    }

    fn paste_here(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let files = std::mem::take(&mut self.clipboard);
        let cut = self.clipboard_cut;
        let folder = self.folder.clone();
        self.transfer(&folder, &files, cut);
        if !cut {
            self.clipboard = files;
        }
    }

    /// Copy or move `files` into `dir`.
    fn transfer(&mut self, dir: &Path, files: &[PathBuf], mv: bool) {
        let r = if mv {
            ops::move_into(dir, files)
        } else {
            ops::copy_into(dir, files)
        };
        match r {
            Ok(done) => {
                self.status = format!(
                    "{} file {} in {}",
                    done.len(),
                    if mv { "spostati" } else { "copiati" },
                    dir.display()
                );
                self.status_error = false;
                self.rescan();
            }
            Err(e) => {
                self.status = format!("Operazione non riuscita: {e}");
                self.status_error = true;
            }
        }
    }

    fn reveal_in_file_manager(&mut self, path: &Path) {
        let r = reveal(path);
        if let Err(e) = r {
            self.status = format!("Impossibile aprire il file manager: {e}");
            self.status_error = true;
        }
    }

    fn save_settings(&mut self) {
        self.settings.sort = self.sort.id().into();
        self.settings.descending = self.descending;
        self.settings.min_rating = self.min_rating;
        if let Err(e) = self.settings.save(&self.settings_file) {
            self.status = format!("Impostazioni non salvate: {e}");
            self.status_error = true;
        }
    }

    // ---- keyboard and drops ----

    fn keyboard(&mut self, ctx: &egui::Context, view: &[usize]) {
        if ctx.egui_wants_keyboard_input() || self.modal.is_some() {
            return;
        }
        let mut cmds: Vec<Cmd> = Vec::new();
        let (shift, ctrl, alt) = ctx.input(|i| {
            let m = i.modifiers;
            let digits = [
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
            ];
            for (k, n) in digits {
                if !i.key_pressed(k) {
                    continue;
                }
                if m.command && n == 0 {
                    cmds.push(Cmd::ThumbSize(150.0));
                } else if m.command && n <= 2 && m.shift {
                    cmds.push(Cmd::ThumbSize(if n == 1 { 100.0 } else { 220.0 }));
                } else if n <= 5 {
                    cmds.push(Cmd::Rate(n));
                } else {
                    cmds.push(Cmd::Label(n));
                }
            }
            let c = m.command;
            let pressed = |k| i.key_pressed(k);
            if m.alt && pressed(egui::Key::ArrowLeft) {
                cmds.push(Cmd::Back);
            } else if m.alt && pressed(egui::Key::ArrowRight) {
                cmds.push(Cmd::Forward);
            } else {
                for (k, d) in [
                    (egui::Key::ArrowLeft, -1i64),
                    (egui::Key::ArrowRight, 1),
                    (egui::Key::ArrowUp, -(self.columns as i64)),
                    (egui::Key::ArrowDown, self.columns as i64),
                    (egui::Key::PageUp, -((self.columns * self.rows_visible) as i64)),
                    (egui::Key::PageDown, (self.columns * self.rows_visible) as i64),
                    (egui::Key::Home, i64::MIN / 2),
                    (egui::Key::End, i64::MAX / 2),
                ] {
                    if pressed(k) {
                        cmds.push(Cmd::Move(d));
                    }
                }
            }
            if pressed(egui::Key::Plus) || pressed(egui::Key::Equals) {
                cmds.push(Cmd::RateStep(1));
            }
            if pressed(egui::Key::Minus) {
                cmds.push(Cmd::RateStep(-1));
            }
            if pressed(egui::Key::Enter) {
                cmds.push(Cmd::Open);
            }
            if pressed(egui::Key::Space) || pressed(egui::Key::F) && !c {
                cmds.push(Cmd::Review);
            }
            if pressed(egui::Key::C) && !c {
                cmds.push(Cmd::Compare);
            }
            if pressed(egui::Key::Backspace) {
                cmds.push(Cmd::Up);
            }
            if pressed(egui::Key::F5) {
                cmds.push(Cmd::Refresh);
            }
            if pressed(egui::Key::F2) || (c && pressed(egui::Key::R)) {
                cmds.push(Cmd::Rename);
            }
            if pressed(egui::Key::Delete) {
                cmds.push(Cmd::Trash);
            }
            if pressed(egui::Key::Escape) {
                cmds.push(Cmd::Deselect);
            }
            if pressed(egui::Key::Questionmark) || pressed(egui::Key::F1) {
                cmds.push(Cmd::Shortcuts);
            }
            if c && pressed(egui::Key::A) {
                cmds.push(if m.shift { Cmd::Deselect } else { Cmd::All });
            }
            if c && m.shift && pressed(egui::Key::I) {
                cmds.push(Cmd::Invert);
            }
            if c && pressed(egui::Key::C) {
                cmds.push(Cmd::Copy(false));
            }
            if c && pressed(egui::Key::X) {
                cmds.push(Cmd::Copy(true));
            }
            if c && pressed(egui::Key::V) {
                cmds.push(Cmd::Paste);
            }
            if c && pressed(egui::Key::L) {
                cmds.push(Cmd::EditPath);
            }
            if c && m.shift && pressed(egui::Key::N) {
                cmds.push(Cmd::NewFolder);
            }
            (m.shift, m.command, m.alt)
        });
        let _ = alt;
        for k in cmds {
            match k {
                Cmd::Rate(n) => self.rate_selection(n),
                Cmd::RateStep(d) => {
                    let cur = self.active_entry().map_or(0, |e| self.meta.get(&e.name).rating);
                    self.rate_selection((i32::from(cur) + d).clamp(0, 5) as u8);
                }
                Cmd::Label(n) => {
                    let l = match n {
                        6 => Some(Label::Red),
                        7 => Some(Label::Yellow),
                        8 => Some(Label::Green),
                        9 => Some(Label::Blue),
                        _ => None,
                    };
                    let cur = self.active_entry().and_then(|e| self.meta.get(&e.name).label);
                    self.label_selection(if cur == l { None } else { l });
                }
                Cmd::Open => self.open_selection(self.settings.open_with),
                Cmd::Review => self.open_review(),
                Cmd::Compare => self.open_compare(),
                Cmd::Refresh => self.refresh(),
                Cmd::Up => {
                    self.go_up();
                    return;
                }
                Cmd::Back => {
                    self.go_back();
                    return;
                }
                Cmd::Forward => {
                    self.go_forward();
                    return;
                }
                Cmd::All => self.select_all(view),
                Cmd::Deselect => {
                    self.selection.clear();
                    self.anchor = None;
                }
                Cmd::Invert => self.invert_selection(view),
                Cmd::Move(delta) => {
                    self.step(view, delta, shift);
                    let _ = ctrl;
                }
                Cmd::Rename => self.start_rename(),
                Cmd::Trash => self.ask_trash(),
                Cmd::Copy(cut) => self.copy_selection(cut),
                Cmd::Paste => self.paste_here(),
                Cmd::ThumbSize(s) => self.settings.thumb_size = s,
                Cmd::Shortcuts => self.modal = Some(Modal::Shortcuts),
                Cmd::EditPath => {
                    self.editing_path = Some(self.folder.to_string_lossy().into_owned());
                }
                Cmd::NewFolder => self.modal = Some(Modal::NewFolder(String::new())),
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
                self.go(p);
            } else if let Some(parent) = p.parent().map(Path::to_path_buf) {
                self.select_after = Some(p);
                self.go(parent);
            }
        }
    }
}

enum Cmd {
    Rate(u8),
    RateStep(i32),
    Label(u8),
    Move(i64),
    Open,
    Review,
    Compare,
    Refresh,
    Up,
    Back,
    Forward,
    All,
    Deselect,
    Invert,
    Rename,
    Trash,
    Copy(bool),
    Paste,
    ThumbSize(f32),
    Shortcuts,
    EditPath,
    NewFolder,
}

/// The folder to start in: the one given on the command line (or the folder of a given file),
/// else the last one used, else Pictures or the home folder.
fn initial_folder(start: Option<PathBuf>, settings: &Settings) -> (PathBuf, Option<PathBuf>) {
    match start {
        Some(p) if p.is_dir() => (p, None),
        Some(p) if p.is_file() => (
            p.parent().map(Path::to_path_buf).unwrap_or_default(),
            Some(p),
        ),
        _ => {
            let last = PathBuf::from(&settings.last_folder);
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
    }
}

/// Drive names are read off the UI thread: a disconnected network drive can be slow.
fn spawn_drive_names(ctx: &egui::Context) -> Arc<Mutex<HashMap<PathBuf, String>>> {
    let drive_names = Arc::new(Mutex::new(HashMap::new()));
    let (names, ctx) = (drive_names.clone(), ctx.clone());
    let _ = std::thread::Builder::new()
        .name("photodesk-drives".into())
        .spawn(move || {
            let found: HashMap<PathBuf, String> = drives::names(&roots()).into_iter().collect();
            *names
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = found;
            ctx.request_repaint();
        });
    drive_names
}

/// Show `path` in the system file manager, selected when possible.
fn reveal(path: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| e.to_string();
    if cfg!(windows) {
        let arg = if path.is_dir() {
            path.as_os_str().to_owned()
        } else {
            let mut a = std::ffi::OsString::from("/select,");
            a.push(path.as_os_str());
            a
        };
        std::process::Command::new("explorer")
            .arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(err)
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(err)
    } else {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(err)
    }
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

fn label_color(l: Label) -> Color32 {
    let [r, g, b] = l.rgb();
    Color32::from_rgb(r, g, b)
}

impl eframe::App for PhotoDesk {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_background();
        self.collect_results(&ctx);
        self.handle_drops(&ctx);
        self.update_preview_target();
        self.flush_meta_if_due(&ctx);

        if self.review.is_some() {
            self.review_screen(ui);
        } else {
            egui::Panel::top("toolbar")
                .frame(theme::bar_frame())
                .show(ui, |ui| self.toolbar(ui));
            egui::Panel::bottom("status")
                .frame(theme::bar_frame())
                .show(ui, |ui| self.status_bar(ui));
            egui::Panel::left("folders")
                .default_size(240.0)
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
        }
        self.settings_window(&ctx);
        self.modal_windows(&ctx);
        self.drag_hint(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_meta();
        self.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;

    fn day(d: u32) -> Option<NaiveDateTime> {
        chrono::NaiveDate::from_ymd_opt(2026, 10, d).and_then(|x| x.and_hms_opt(12, 0, 0))
    }

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
    fn date_filters_count_days_back_from_now() {
        let now = day(26).unwrap_or_default();
        assert!(DateFilter::All.matches(None, now));
        assert!(!DateFilter::Today.matches(None, now));
        assert!(DateFilter::Today.matches(day(26), now));
        assert!(!DateFilter::Today.matches(day(25), now));
        assert!(DateFilter::Week.matches(day(20), now));
        assert!(!DateFilter::Week.matches(day(18), now));
        assert!(DateFilter::Month.matches(day(1), now));
        assert!(!DateFilter::Month.matches(day(27), now), "the future is not 'recent'");
    }

    #[test]
    fn kind_filters_tell_raw_jpeg_and_the_rest_apart() {
        let mk = |n: &str, kind| Entry {
            path: PathBuf::from(n),
            name: n.into(),
            kind,
            size: 1,
            modified: None,
        };
        let (raw, jpg, png) = (
            mk("a.NEF", Kind::Raw),
            mk("b.JPEG", Kind::Image),
            mk("c.png", Kind::Image),
        );
        assert!(KindFilter::Raw.matches(&raw) && !KindFilter::Raw.matches(&jpg));
        assert!(KindFilter::Jpeg.matches(&jpg) && !KindFilter::Jpeg.matches(&png));
        assert!(KindFilter::Other.matches(&png) && !KindFilter::Other.matches(&jpg));
        assert!(KindFilter::All.matches(&raw));
    }

    #[test]
    fn the_folder_signature_changes_only_when_pictures_come_or_go() {
        let d = std::env::temp_dir().join(format!("photodesk-sig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.jpg"), b"x").unwrap();
        let s1 = folder_signature(&d);
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        assert_eq!(folder_signature(&d), s1, "a text file is not a picture");
        std::fs::write(d.join("b.jpg"), b"x").unwrap();
        assert_ne!(folder_signature(&d), s1);
        assert_eq!(folder_signature(&d.join("missing")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_newest_request_is_served_first_and_stale_ones_are_dropped() {
        let q = Queue::new();
        let job = |g, n: &str| Job {
            generation: g,
            path: PathBuf::from(n),
            kind: Kind::Image,
            max: 10,
            key: None,
        };
        q.push(job(1, "old-generation"));
        q.push(job(2, "first"));
        q.push(job(2, "second"));
        let current = AtomicU64::new(2);
        assert_eq!(q.next(&current).path, PathBuf::from("second"));
        assert_eq!(q.next(&current).path, PathBuf::from("first"));
        assert!(q.jobs.lock().map(|g| g.len()).unwrap_or(1) <= 1);
    }
}
