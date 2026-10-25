//! Ratings and colour labels, kept in a `.photodesk.json` file inside each folder so they travel
//! with the pictures. Nothing is written into the picture files themselves.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE_NAME: &str = ".photodesk.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Label {
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl Label {
    pub const ALL: [Label; 5] = [
        Label::Red,
        Label::Yellow,
        Label::Green,
        Label::Blue,
        Label::Purple,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Label::Red => "Rosso",
            Label::Yellow => "Giallo",
            Label::Green => "Verde",
            Label::Blue => "Blu",
            Label::Purple => "Viola",
        }
    }
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Label::Red => [220, 60, 60],
            Label::Yellow => [225, 190, 40],
            Label::Green => [70, 180, 90],
            Label::Blue => [70, 130, 225],
            Label::Purple => [160, 100, 215],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// 0 = no rating, 1..=5 stars.
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub label: Option<Label>,
}

impl Meta {
    fn is_empty(&self) -> bool {
        self.rating == 0 && self.label.is_none()
    }
}

/// The metadata of one folder, keyed by file name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FolderMeta {
    #[serde(default)]
    files: BTreeMap<String, Meta>,
}

impl FolderMeta {
    /// Read the folder's file. A missing or damaged file gives an empty set (never an error: the
    /// ratings are a convenience, and a bad file must not stop the folder from opening).
    pub fn load(dir: &Path) -> FolderMeta {
        std::fs::read_to_string(dir.join(FILE_NAME))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let path = dir.join(FILE_NAME);
        if self.files.is_empty() {
            // Nothing to keep: don't leave an empty file (or fail on a read-only folder).
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            return Ok(());
        }
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get(&self, name: &str) -> Meta {
        self.files.get(name).cloned().unwrap_or_default()
    }

    pub fn set_rating(&mut self, name: &str, rating: u8) {
        let mut m = self.get(name);
        m.rating = rating.min(5);
        self.put(name, m);
    }

    pub fn set_label(&mut self, name: &str, label: Option<Label>) {
        let mut m = self.get(name);
        m.label = label;
        self.put(name, m);
    }

    fn put(&mut self, name: &str, m: Meta) {
        if m.is_empty() {
            self.files.remove(name);
        } else {
            self.files.insert(name.to_string(), m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("photodesk-meta-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rating_and_label_round_trip() {
        let d = tmp("rt");
        let mut m = FolderMeta::default();
        m.set_rating("a.jpg", 4);
        m.set_label("a.jpg", Some(Label::Green));
        m.set_label("b.jpg", Some(Label::Red));
        m.save(&d).unwrap();
        let back = FolderMeta::load(&d);
        assert_eq!(
            back.get("a.jpg"),
            Meta {
                rating: 4,
                label: Some(Label::Green)
            }
        );
        assert_eq!(back.get("b.jpg").label, Some(Label::Red));
        assert_eq!(back.get("none.jpg"), Meta::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rating_is_capped_and_cleared_entries_are_dropped() {
        let mut m = FolderMeta::default();
        m.set_rating("a.jpg", 9);
        assert_eq!(m.get("a.jpg").rating, 5);
        m.set_rating("a.jpg", 0);
        assert!(m.files.is_empty());
    }

    #[test]
    fn a_damaged_file_gives_an_empty_set() {
        let d = tmp("bad");
        std::fs::write(d.join(FILE_NAME), b"{ not json").unwrap();
        assert_eq!(FolderMeta::load(&d).get("x"), Meta::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_nothing_removes_the_file() {
        let d = tmp("empty");
        let mut m = FolderMeta::default();
        m.set_rating("a.jpg", 2);
        m.save(&d).unwrap();
        assert!(d.join(FILE_NAME).exists());
        m.set_rating("a.jpg", 0);
        m.save(&d).unwrap();
        assert!(!d.join(FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
