//! File operations: rename (one or many, with a pattern), copy, move and the recycle bin.
//!
//! Everything here returns a message for the user instead of panicking: a locked file or a full
//! disk must never take the program down.

use crate::fsutil::unique_in;
use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};

/// What a rename pattern can use about one file.
#[derive(Clone, Debug)]
pub struct RenameItem {
    pub path: PathBuf,
    /// When it was taken (camera) or last changed.
    pub date: Option<NaiveDateTime>,
    pub camera: String,
    /// The number this file counts as: a RAW and its JPEG share one, so they stay a pair.
    pub group: usize,
}

const BAD_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'];

/// A file name with the characters Windows does not allow replaced by "_", and no trailing dots
/// or spaces.
pub fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if BAD_CHARS.contains(&c) || c.is_control() { '_' } else { c })
        .collect();
    s.trim_end_matches(['.', ' ']).trim_start().to_string()
}

/// Expand `pattern` for one file. Tokens (in braces):
/// `{originale}` the old name without extension, `{n}` or `{n:4}` a counter (zero padded),
/// `{data}` (or `{data:%Y-%m-%d}`), `{ora}`, `{camera}`, `{cartella}`.
pub fn expand(pattern: &str, item: &RenameItem, index: usize, start: u32) -> String {
    let stem = item
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = item
        .path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let token = &after[..close];
        let (name, arg) = match token.split_once(':') {
            Some((n, a)) => (n, Some(a)),
            None => (token, None),
        };
        match name.trim().to_lowercase().as_str() {
            "originale" | "nome" => out.push_str(&stem),
            "n" | "contatore" => {
                let width = arg.and_then(|a| a.trim().parse::<usize>().ok()).unwrap_or(1);
                let n = u64::from(start) + index as u64;
                out.push_str(&format!("{n:0width$}", width = width.min(10)));
            }
            "data" => {
                let fmt = arg.unwrap_or("%Y%m%d");
                match item.date {
                    Some(d) => out.push_str(&format_date(&d, fmt)),
                    None => out.push_str("senzadata"),
                }
            }
            "ora" => match item.date {
                Some(d) => out.push_str(&d.format("%H%M%S").to_string()),
                None => out.push_str("000000"),
            },
            "camera" => out.push_str(&item.camera.replace(' ', "")),
            "cartella" => out.push_str(&folder),
            _ => {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Format a date, falling back to the default when the format text is not valid.
fn format_date(d: &NaiveDateTime, fmt: &str) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    if write!(s, "{}", d.format(fmt)).is_err() {
        return d.format("%Y%m%d").to_string();
    }
    s
}

/// One planned rename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub from: PathBuf,
    pub to_name: String,
    /// Why this one can not be done ("" = fine).
    pub problem: String,
}

/// The new name of every item for `pattern` (the extension is kept), and the problems: an empty
/// result, two files ending up with the same name, or a name another file in the folder has.
pub fn plan_rename(items: &[RenameItem], pattern: &str, start: u32) -> Vec<Planned> {
    let mut out: Vec<Planned> = items
        .iter()
        .enumerate()
        .map(|(_, it)| {
            let ext = it
                .path
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let stem = clean_name(&expand(pattern, it, it.group, start));
            let to_name = if stem.is_empty() {
                String::new()
            } else {
                format!("{stem}{ext}")
            };
            Planned {
                from: it.path.clone(),
                to_name,
                problem: String::new(),
            }
        })
        .collect();
    let moving: Vec<&PathBuf> = items.iter().map(|i| &i.path).collect();
    for i in 0..out.len() {
        if out[i].to_name.is_empty() {
            out[i].problem = "nome vuoto".into();
            continue;
        }
        let dir = out[i].from.parent().map(Path::to_path_buf).unwrap_or_default();
        let target = dir.join(&out[i].to_name);
        let same_as_other = out
            .iter()
            .enumerate()
            .any(|(j, o)| j != i && o.from.parent() == out[i].from.parent() && o.to_name.eq_ignore_ascii_case(&out[i].to_name));
        if same_as_other {
            out[i].problem = "stesso nome di un'altra foto".into();
        } else if target != out[i].from
            && target.exists()
            && !moving.iter().any(|m| m.as_path() == target.as_path())
        {
            out[i].problem = "esiste già un file con questo nome".into();
        }
    }
    out
}

/// Do the renames of a plan. Names that collide with a file that is itself being renamed are
/// handled by going through temporary names first. Returns the (old, new) paths done.
pub fn apply_rename(plan: &[Planned]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if let Some(p) = plan.iter().find(|p| !p.problem.is_empty()) {
        return Err(format!("{}: {}", p.from.display(), p.problem));
    }
    let todo: Vec<&Planned> = plan
        .iter()
        .filter(|p| p.from.file_name().is_some_and(|n| n.to_string_lossy() != p.to_name))
        .collect();
    let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new(); // from, temp, final
    for (i, p) in todo.iter().enumerate() {
        let dir = p.from.parent().map(Path::to_path_buf).unwrap_or_default();
        let temp = dir.join(format!(".photodesk-rename-{}-{i}", std::process::id()));
        std::fs::rename(&p.from, &temp).map_err(|e| format!("{}: {e}", p.from.display()))?;
        staged.push((p.from.clone(), temp, dir.join(&p.to_name)));
    }
    let mut done = Vec::new();
    for (from, temp, to) in staged {
        match std::fs::rename(&temp, &to) {
            Ok(()) => done.push((from, to)),
            Err(e) => {
                // put the file back under its old name rather than leaving a temporary one
                let _ = std::fs::rename(&temp, &from);
                return Err(format!("{}: {e}", to.display()));
            }
        }
    }
    Ok(done)
}

/// Copy files into `dir` (a name that exists gets " (2)"). Returns the new paths.
pub fn copy_into(dir: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{}: nome non valido", f.display()))?;
        let to = unique_in(dir, &name);
        std::fs::copy(f, &to).map_err(|e| format!("{}: {e}", f.display()))?;
        out.push(to);
    }
    Ok(out)
}

/// Move files into `dir`: a rename, or copy and delete across disks.
pub fn move_into(dir: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for f in files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{}: nome non valido", f.display()))?;
        if f.parent() == Some(dir) {
            continue;
        }
        let to = unique_in(dir, &name);
        if std::fs::rename(f, &to).is_err() {
            std::fs::copy(f, &to).map_err(|e| format!("{}: {e}", f.display()))?;
            std::fs::remove_file(f).map_err(|e| format!("{}: {e}", f.display()))?;
        }
        out.push(to);
    }
    Ok(out)
}

/// Send files to the system recycle bin.
pub fn trash(files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    ::trash::delete_all(files).map_err(|e| e.to_string())
}

/// A new sub-folder named `name` in `dir`.
pub fn new_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = clean_name(name);
    if name.is_empty() {
        return Err("nome vuoto".into());
    }
    let path = dir.join(&name);
    if path.exists() {
        return Err("esiste già".into());
    }
    std::fs::create_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "photodesk-ops-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn item(path: &Path) -> RenameItem {
        RenameItem {
            path: path.to_path_buf(),
            date: NaiveDate::from_ymd_opt(2026, 10, 26).and_then(|d| d.and_hms_opt(9, 5, 7)),
            camera: "Nikon Z6".into(),
            group: 0,
        }
    }

    #[test]
    fn tokens_expand() {
        let it = item(Path::new("/foto/Roma/DSC_0042.NEF"));
        assert_eq!(expand("{data}_{cartella}_{n:3}", &it, 4, 1), "20261026_Roma_005");
        assert_eq!(expand("{originale}-{camera}", &it, 0, 1), "DSC_0042-NikonZ6");
        assert_eq!(expand("{data:%Y-%m-%d}_{ora}", &it, 0, 1), "2026-10-26_090507");
        assert_eq!(expand("{sconosciuto}{", &it, 0, 1), "{sconosciuto}{");
    }

    #[test]
    fn illegal_characters_are_replaced() {
        assert_eq!(clean_name("a:b/c?.jpg."), "a_b_c_.jpg");
        assert_eq!(clean_name("..."), "");
    }

    #[test]
    fn a_plan_keeps_extensions_and_flags_clashes() {
        let d = temp("plan");
        let (a, b) = (d.join("a.jpg"), d.join("b.jpg"));
        std::fs::write(&a, b"1").unwrap();
        std::fs::write(&b, b"2").unwrap();
        let mut items = [item(&a), item(&b)];
        items[1].group = 1;
        let ok = plan_rename(&items, "foto_{n:2}", 1);
        assert_eq!(ok[0].to_name, "foto_01.jpg");
        assert_eq!(ok[1].to_name, "foto_02.jpg");
        assert!(ok.iter().all(|p| p.problem.is_empty()));
        let same = plan_rename(&items, "uguale", 1);
        assert!(same.iter().all(|p| !p.problem.is_empty()));
        let empty = plan_rename(&items, "", 1);
        assert!(empty.iter().all(|p| p.problem == "nome vuoto"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn renaming_can_swap_names_through_temporaries() {
        let d = temp("swap");
        let (a, b) = (d.join("a.jpg"), d.join("b.jpg"));
        std::fs::write(&a, b"A").unwrap();
        std::fs::write(&b, b"B").unwrap();
        let plan = vec![
            Planned { from: a.clone(), to_name: "b.jpg".into(), problem: String::new() },
            Planned { from: b.clone(), to_name: "a.jpg".into(), problem: String::new() },
        ];
        apply_rename(&plan).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"B");
        assert_eq!(std::fs::read(&b).unwrap(), b"A");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plan_with_a_problem_changes_nothing() {
        let d = temp("refuse");
        let a = d.join("a.jpg");
        std::fs::write(&a, b"A").unwrap();
        let plan = vec![Planned { from: a.clone(), to_name: String::new(), problem: "nome vuoto".into() }];
        assert!(apply_rename(&plan).is_err());
        assert!(a.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn copy_and_move_do_not_overwrite() {
        let d = temp("copymove");
        let sub = d.join("dest");
        std::fs::create_dir_all(&sub).unwrap();
        let a = d.join("a.jpg");
        std::fs::write(&a, b"A").unwrap();
        std::fs::write(sub.join("a.jpg"), b"old").unwrap();
        let copied = copy_into(&sub, std::slice::from_ref(&a)).unwrap();
        assert_eq!(copied, vec![sub.join("a (2).jpg")]);
        assert_eq!(std::fs::read(sub.join("a.jpg")).unwrap(), b"old");
        let moved = move_into(&sub, std::slice::from_ref(&a)).unwrap();
        assert_eq!(moved, vec![sub.join("a (3).jpg")]);
        assert!(!a.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn new_folder_refuses_bad_or_existing_names() {
        let d = temp("newfolder");
        assert!(new_folder(&d, "").is_err());
        assert!(new_folder(&d, "Viaggio").is_ok());
        assert!(new_folder(&d, "Viaggio").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
