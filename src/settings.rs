//! PhotoDesk's own settings: where DarkRaw and LightRaw are, the last folder, the thumbnail size.
//!
//! Portable mode: a `portable.txt` next to the program keeps the file in a `PhotoDeskData` folder
//! beside it. Otherwise it goes to the user's configuration folder.

use crate::launch::Choice;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Full path of DarkRaw's program; empty = look for it next to PhotoDesk and on PATH.
    pub darkraw_path: String,
    pub lightraw_path: String,
    pub last_folder: String,
    pub thumb_size: f32,
    pub open_with: Choice,
    /// Folders pinned in the left panel.
    pub favorites: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            darkraw_path: String::new(),
            lightraw_path: String::new(),
            last_folder: String::new(),
            thumb_size: 150.0,
            open_with: Choice::Auto,
            favorites: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Where the settings file lives. `exe_dir` is the folder of the running program.
pub fn settings_path(exe_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = exe_dir {
        if dir.join("portable.txt").is_file() || dir.join("PhotoDesk.portable").is_file() {
            return dir.join("PhotoDeskData").join("settings.json");
        }
    }
    config_dir().join("PhotoDesk").join("settings.json")
}

fn config_dir() -> PathBuf {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        env("APPDATA").unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        env("HOME")
            .map(|h| h.join("Library").join("Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_missing_file() {
        let d = std::env::temp_dir().join(format!("photodesk-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("sub").join("settings.json");
        assert_eq!(Settings::load(&p).thumb_size, 150.0);
        let s = Settings {
            darkraw_path: "C:/x/DarkRaw.exe".into(),
            thumb_size: 200.0,
            open_with: Choice::LightRaw,
            ..Settings::default()
        };
        s.save(&p).unwrap();
        let back = Settings::load(&p);
        assert_eq!(back.darkraw_path, "C:/x/DarkRaw.exe");
        assert_eq!(back.thumb_size, 200.0);
        assert_eq!(back.open_with, Choice::LightRaw);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn partial_and_damaged_files_fall_back_to_defaults() {
        let d = std::env::temp_dir().join(format!("photodesk-settings2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("s.json");
        std::fs::write(&p, r#"{"thumb_size": 99.0}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.thumb_size, 99.0);
        assert_eq!(s.open_with, Choice::Auto);
        std::fs::write(&p, "garbage").unwrap();
        assert_eq!(Settings::load(&p).thumb_size, 150.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn portable_marker_moves_the_file_beside_the_program() {
        let d = std::env::temp_dir().join(format!("photodesk-portable-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        assert!(!settings_path(Some(&d)).starts_with(&d));
        std::fs::write(d.join("portable.txt"), b"x").unwrap();
        assert_eq!(
            settings_path(Some(&d)),
            d.join("PhotoDeskData").join("settings.json")
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
