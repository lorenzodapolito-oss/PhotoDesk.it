//! Listing a folder: which files are pictures, and in which order to show them.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Camera raw extensions (lower case). These open in LightRaw by default.
pub const RAW_EXTS: &[&str] = &[
    "3fr", "arw", "cr2", "cr3", "crw", "dcr", "dng", "erf", "fff", "iiq", "kdc", "mef", "mos",
    "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f",
];

/// Other picture extensions (lower case). These open in DarkRaw by default.
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif", "webp"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Image,
}

/// What a path is, going by its extension; `None` for anything that is not a picture.
pub fn classify(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if RAW_EXTS.contains(&ext.as_str()) {
        Some(Kind::Raw)
    } else if IMAGE_EXTS.contains(&ext.as_str()) {
        Some(Kind::Image)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// The pictures directly inside `dir` (no recursion). Unreadable entries are skipped; an
/// unreadable folder is an error the caller can show.
pub fn scan_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for item in rd.flatten() {
        let path = item.path();
        let Some(kind) = classify(&path) else {
            continue;
        };
        let Ok(md) = item.metadata() else { continue };
        if !md.is_file() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        out.push(Entry {
            path,
            name,
            kind,
            size: md.len(),
            modified: md.modified().ok(),
        });
    }
    Ok(out)
}

/// The sub-folders of `dir`, sorted by name, hidden ones (leading dot) left out.
pub fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Name,
    Date,
    Size,
    Type,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Name, Sort::Date, Sort::Size, Sort::Type];
    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "Nome",
            Sort::Date => "Data",
            Sort::Size => "Dimensione",
            Sort::Type => "Tipo",
        }
    }
}

/// Natural-ish name order: case-insensitive, and digit runs compared as numbers (IMG_2 < IMG_10).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut nx = String::new();
                while let Some(&c) = ai.peek() {
                    if c.is_ascii_digit() {
                        nx.push(c);
                        ai.next();
                    } else {
                        break;
                    }
                }
                let mut ny = String::new();
                while let Some(&c) = bi.peek() {
                    if c.is_ascii_digit() {
                        ny.push(c);
                        bi.next();
                    } else {
                        break;
                    }
                }
                let (tx, ty) = (nx.trim_start_matches('0'), ny.trim_start_matches('0'));
                let o = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn sort_entries(v: &mut [Entry], by: Sort, descending: bool) {
    v.sort_by(|a, b| {
        let o = match by {
            Sort::Name => natural_cmp(&a.name, &b.name),
            Sort::Date => a
                .modified
                .cmp(&b.modified)
                .then_with(|| natural_cmp(&a.name, &b.name)),
            Sort::Size => a
                .size
                .cmp(&b.size)
                .then_with(|| natural_cmp(&a.name, &b.name)),
            Sort::Type => {
                let ka = (
                    a.kind == Kind::Image,
                    a.path.extension().map(|e| e.to_ascii_lowercase()),
                );
                let kb = (
                    b.kind == Kind::Image,
                    b.path.extension().map(|e| e.to_ascii_lowercase()),
                );
                ka.cmp(&kb).then_with(|| natural_cmp(&a.name, &b.name))
            }
        };
        if descending {
            o.reverse()
        } else {
            o
        }
    });
}

/// "2026-10-25 11:12" in the local time zone; empty when the time is unknown.
pub fn format_time(t: Option<SystemTime>) -> String {
    match t {
        Some(t) => chrono::DateTime::<chrono::Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

/// A human size: "12 KB", "3.4 MB".
pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("photodesk-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn classifies_by_extension_ignoring_case() {
        assert_eq!(classify(Path::new("a/IMG_1.NEF")), Some(Kind::Raw));
        assert_eq!(classify(Path::new("a/x.dng")), Some(Kind::Raw));
        assert_eq!(classify(Path::new("a/x.JPG")), Some(Kind::Image));
        assert_eq!(classify(Path::new("a/x.txt")), None);
        assert_eq!(classify(Path::new("a/noext")), None);
    }

    #[test]
    fn scan_lists_only_pictures_and_reports_a_missing_folder() {
        let d = tmp("scan");
        std::fs::write(d.join("a.jpg"), b"x").unwrap();
        std::fs::write(d.join("b.cr3"), b"xx").unwrap();
        std::fs::write(d.join("c.txt"), b"x").unwrap();
        std::fs::create_dir(d.join("sub.jpg")).unwrap();
        let mut v = scan_dir(&d).unwrap();
        sort_entries(&mut v, Sort::Name, false);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.cr3"]);
        assert!(scan_dir(&d.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn subdirs_skip_hidden_and_sort() {
        let d = tmp("subdirs");
        for n in ["b", "A", ".hidden", "c"] {
            std::fs::create_dir(d.join(n)).unwrap();
        }
        std::fs::write(d.join("file"), b"x").unwrap();
        let names: Vec<_> = subdirs(&d)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["A", "b", "c"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn natural_order_compares_numbers_as_numbers() {
        assert_eq!(natural_cmp("IMG_2.jpg", "IMG_10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("img_010", "IMG_10"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "B"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
    }

    #[test]
    fn sorts_descending_and_by_size() {
        let mk = |n: &str, s| Entry {
            path: PathBuf::from(n),
            name: n.into(),
            kind: Kind::Image,
            size: s,
            modified: None,
        };
        let mut v = vec![mk("a.jpg", 30), mk("b.jpg", 10), mk("c.jpg", 20)];
        sort_entries(&mut v, Sort::Size, false);
        assert_eq!(v.iter().map(|e| e.size).collect::<Vec<_>>(), [10, 20, 30]);
        sort_entries(&mut v, Sort::Size, true);
        assert_eq!(v.iter().map(|e| e.size).collect::<Vec<_>>(), [30, 20, 10]);
    }

    #[test]
    fn formats_a_time_and_an_unknown_time() {
        assert_eq!(format_time(None), "");
        let s = format_time(Some(
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 365),
        ));
        assert_eq!(s.len(), 16);
        assert!(s.starts_with("1971-01-0"));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(3 * 1024 * 1024 + 400 * 1024), "3.4 MB");
    }
}
