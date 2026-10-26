//! The folders visited, for the back and forward buttons (Alt+Left / Alt+Right).

use std::path::{Path, PathBuf};

const LIMIT: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct History {
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl History {
    /// Leaving `from` for a new folder: it becomes the place "back" returns to, and the forward
    /// list is forgotten (as in a web browser).
    pub fn visit(&mut self, from: &Path) {
        if from.as_os_str().is_empty() || self.back.last().is_some_and(|l| l == from) {
            self.forward.clear();
            return;
        }
        self.back.push(from.to_path_buf());
        if self.back.len() > LIMIT {
            self.back.remove(0);
        }
        self.forward.clear();
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Go back from `current`: the folder to open.
    pub fn back(&mut self, current: &Path) -> Option<PathBuf> {
        let target = self.back.pop()?;
        self.forward.push(current.to_path_buf());
        Some(target)
    }

    /// Go forward from `current`: the folder to open.
    pub fn forward(&mut self, current: &Path) -> Option<PathBuf> {
        let target = self.forward.pop()?;
        self.back.push(current.to_path_buf());
        Some(target)
    }

    /// The recently visited folders, newest first, without repeats.
    pub fn recent(&self, max: usize) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for p in self.back.iter().rev() {
            if !out.contains(p) {
                out.push(p.clone());
            }
            if out.len() >= max {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn back_and_forward_walk_the_visited_folders() {
        let mut h = History::default();
        h.visit(&p("/a"));
        h.visit(&p("/b"));
        assert!(h.can_back() && !h.can_forward());
        assert_eq!(h.back(&p("/c")), Some(p("/b")));
        assert_eq!(h.back(&p("/b")), Some(p("/a")));
        assert_eq!(h.back(&p("/a")), None);
        assert_eq!(h.forward(&p("/a")), Some(p("/b")));
        assert_eq!(h.forward(&p("/b")), Some(p("/c")));
        assert_eq!(h.forward(&p("/c")), None);
    }

    #[test]
    fn a_new_visit_forgets_the_forward_list() {
        let mut h = History::default();
        h.visit(&p("/a"));
        let _ = h.back(&p("/b"));
        assert!(h.can_forward());
        h.visit(&p("/b"));
        assert!(!h.can_forward());
    }

    #[test]
    fn an_empty_start_folder_is_not_remembered() {
        let mut h = History::default();
        h.visit(Path::new(""));
        assert!(!h.can_back());
    }

    #[test]
    fn recent_lists_each_folder_once_newest_first() {
        let mut h = History::default();
        for f in ["/a", "/b", "/a", "/c"] {
            h.visit(&p(f));
        }
        assert_eq!(h.recent(10), vec![p("/c"), p("/a"), p("/b")]);
        assert_eq!(h.recent(2).len(), 2);
    }
}
