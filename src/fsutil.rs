//! Small file helpers shared by the settings, the ratings and the file operations.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `data` to `path` safely: first to a sibling temporary file, then rename it over the
/// target. A crash or a full disk in the middle leaves the old file intact instead of a half
/// written one (a damaged ratings file would lose a whole folder's stars).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(e));
    }
    Ok(())
}

/// A path in `dir` for `name` that does not exist yet: "IMG.jpg", then "IMG (2).jpg", ...
pub fn unique_in(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 2..10_000u32 {
        let candidate = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let path = dir.join(candidate);
        if !path.exists() {
            return path;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "photodesk-fsutil-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temporary() {
        let d = temp("atomic");
        let f = d.join("a.json");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(left.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn atomic_write_in_a_missing_folder_is_an_error_not_a_panic() {
        let d = temp("missing");
        assert!(write_atomic(&d.join("no/such/dir/x.json"), b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unique_names_count_up() {
        let d = temp("unique");
        assert_eq!(unique_in(&d, "a.jpg"), d.join("a.jpg"));
        std::fs::write(d.join("a.jpg"), b"x").unwrap();
        assert_eq!(unique_in(&d, "a.jpg"), d.join("a (2).jpg"));
        std::fs::write(d.join("a (2).jpg"), b"x").unwrap();
        assert_eq!(unique_in(&d, "a.jpg"), d.join("a (3).jpg"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
