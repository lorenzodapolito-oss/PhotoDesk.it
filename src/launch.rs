//! Choosing between DarkRaw and LightRaw, finding their programs and starting them.

use crate::files::Kind;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    DarkRaw,
    LightRaw,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::DarkRaw => "DarkRaw",
            Tool::LightRaw => "LightRaw",
        }
    }
    /// File names to look for (lower case program names; Windows adds `.exe`).
    fn program_names(self) -> [&'static str; 2] {
        match self {
            Tool::DarkRaw => ["DarkRaw", "darkraw"],
            Tool::LightRaw => ["LightRaw", "lightraw"],
        }
    }
    fn folder_hints(self) -> [&'static str; 4] {
        match self {
            Tool::DarkRaw => ["DarkRaw", "DarkRaw-portable", "darkraw", "darkraw-portable"],
            Tool::LightRaw => [
                "LightRaw",
                "LightRaw-portable",
                "lightraw",
                "lightraw-portable",
            ],
        }
    }
}

/// What the user picked in "Apri con".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Choice {
    /// Raw files go to LightRaw (the photo developer), everything else to DarkRaw (the editor).
    Auto,
    DarkRaw,
    LightRaw,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::Auto => "Automatico",
            Choice::DarkRaw => "DarkRaw",
            Choice::LightRaw => "LightRaw",
        }
    }
}

/// The tool for one file under a choice.
pub fn tool_for(choice: Choice, kind: Kind) -> Tool {
    match choice {
        Choice::DarkRaw => Tool::DarkRaw,
        Choice::LightRaw => Tool::LightRaw,
        Choice::Auto => match kind {
            Kind::Raw => Tool::LightRaw,
            Kind::Image => Tool::DarkRaw,
        },
    }
}

fn exe_file_names(tool: Tool) -> Vec<String> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    tool.program_names()
        .iter()
        .map(|n| format!("{n}{ext}"))
        .collect()
}

/// Look for a program: the configured path first, then next to PhotoDesk (also in a folder named
/// after the tool, or its `-portable` folder, one level up too), then on PATH.
pub fn find_tool(tool: Tool, configured: &str, exe_dir: Option<&Path>) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.is_file().then_some(p);
    }
    let names = exe_file_names(tool);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        dirs.push(d.to_path_buf());
        for h in tool.folder_hints() {
            dirs.push(d.join(h));
        }
        if let Some(parent) = d.parent() {
            for h in tool.folder_hints() {
                dirs.push(parent.join(h));
            }
        }
    }
    let direct = dirs
        .iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file());
    if direct.is_some() {
        return direct;
    }
    if let Some(found) = exe_dir.and_then(|d| nearby(tool, d)) {
        return Some(found);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// Look around PhotoDesk's own folder: in each of its parent folders (up to six levels), for a
/// folder named after the tool ("DarkRaw", "DarkRaw-main", "darkraw.it"...), and inside it for the
/// program, down to three levels ("DarkRaw/dist/DarkRaw-portable/DarkRaw.exe").
fn nearby(tool: Tool, exe_dir: &Path) -> Option<PathBuf> {
    let names = exe_file_names(tool);
    let needle = tool.name().to_lowercase();
    for ancestor in exe_dir.ancestors().take(7) {
        let Ok(rd) = std::fs::read_dir(ancestor) else {
            continue;
        };
        let mut candidates: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&needle)
            })
            .map(|e| e.path())
            .take(40)
            .collect();
        candidates.sort();
        for dir in candidates {
            if let Some(p) = find_exe_in(&dir, &names, 3) {
                return Some(p);
            }
        }
    }
    None
}

/// `names` in `dir` or, going down at most `depth` levels, in a folder below it (shallowest first).
fn find_exe_in(dir: &Path, names: &[String], depth: u8) -> Option<PathBuf> {
    if let Some(p) = names.iter().map(|n| dir.join(n)).find(|p| p.is_file()) {
        return Some(p);
    }
    if depth == 0 {
        return None;
    }
    let rd = std::fs::read_dir(dir).ok()?;
    let mut subs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            !n.starts_with('.')
                && !["src", "node_modules", "deps", "build", "incremental"].contains(&n.as_str())
        })
        .map(|e| e.path())
        .take(60)
        .collect();
    subs.sort();
    subs.into_iter()
        .find_map(|d| find_exe_in(&d, names, depth - 1))
}

/// The command that opens `files` in `exe`. The program's own folder is its working directory, so
/// a portable copy finds its settings.
pub fn command(exe: &Path, files: &[PathBuf]) -> Command {
    let mut c = Command::new(exe);
    c.args(files);
    if let Some(dir) = exe.parent().filter(|d| !d.as_os_str().is_empty()) {
        c.current_dir(dir);
    }
    c
}

/// Start `tool` on `files`. The error text is ready to show to the user.
pub fn open(
    tool: Tool,
    configured: &str,
    exe_dir: Option<&Path>,
    files: &[PathBuf],
) -> Result<(), String> {
    let exe = find_tool(tool, configured, exe_dir).ok_or_else(|| {
        format!("{} non trovato: copia PhotoDesk nella sua cartella o indica il programma in Impostazioni.", tool.name())
    })?;
    command(&exe, files)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_sends_raw_to_lightraw_and_pictures_to_darkraw() {
        assert_eq!(tool_for(Choice::Auto, Kind::Raw), Tool::LightRaw);
        assert_eq!(tool_for(Choice::Auto, Kind::Image), Tool::DarkRaw);
        assert_eq!(tool_for(Choice::DarkRaw, Kind::Raw), Tool::DarkRaw);
        assert_eq!(tool_for(Choice::LightRaw, Kind::Image), Tool::LightRaw);
    }

    #[test]
    fn finds_the_program_next_to_photodesk_and_in_a_tool_folder() {
        let d = std::env::temp_dir().join(format!("photodesk-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("DarkRaw-portable")).unwrap();
        let names = exe_file_names(Tool::DarkRaw);
        let dark = d.join("DarkRaw-portable").join(&names[0]);
        std::fs::write(&dark, b"x").unwrap();
        assert_eq!(find_tool(Tool::DarkRaw, "", Some(&d)), Some(dark));
        assert_eq!(
            find_tool(Tool::LightRaw, "", Some(&d)).filter(|p| p.starts_with(&d)),
            None
        );
        let light = d.join(&exe_file_names(Tool::LightRaw)[0]);
        std::fs::write(&light, b"x").unwrap();
        assert_eq!(find_tool(Tool::LightRaw, "", Some(&d)), Some(light));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn finds_a_portable_build_in_a_sibling_project_folder() {
        let root = std::env::temp_dir().join(format!("photodesk-nearby-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let here = root
            .join("bin")
            .join("PhotoDesk.it")
            .join("dist")
            .join("PhotoDesk-portable");
        let dark = root
            .join("bin")
            .join("DarkRaw")
            .join("dist")
            .join("DarkRaw-portable");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::create_dir_all(&dark).unwrap();
        let exe = dark.join(&exe_file_names(Tool::DarkRaw)[0]);
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(nearby(Tool::DarkRaw, &here), Some(exe));
        assert_eq!(nearby(Tool::LightRaw, &here), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_configured_path_wins_and_must_exist() {
        let d = std::env::temp_dir().join(format!("photodesk-launch2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("custom-tool");
        assert_eq!(
            find_tool(Tool::DarkRaw, p.to_str().unwrap_or(""), None),
            None
        );
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(
            find_tool(Tool::DarkRaw, p.to_str().unwrap_or(""), None),
            Some(p)
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_command_carries_the_files_and_the_program_folder() {
        let c = command(
            Path::new("/opt/x/darkraw"),
            &[PathBuf::from("/p/a.jpg"), PathBuf::from("/p/b.jpg")],
        );
        let args: Vec<_> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["/p/a.jpg", "/p/b.jpg"]);
        assert_eq!(c.get_current_dir(), Some(Path::new("/opt/x")));
    }

    #[test]
    fn opening_without_the_program_gives_a_readable_error() {
        let e = open(Tool::LightRaw, "/definitely/not/here", None, &[]).unwrap_err();
        assert!(e.contains("LightRaw"));
    }
}
