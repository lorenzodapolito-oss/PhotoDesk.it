//! Drive names for the folder tree, as Explorer shows them: "Dati (D:)", "Disco locale (C:)".

use std::path::{Path, PathBuf};

/// What kind of drive a root is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Other,
}

impl DriveKind {
    fn generic_name(self) -> &'static str {
        match self {
            DriveKind::Fixed => "Disco locale",
            DriveKind::Removable => "Disco rimovibile",
            DriveKind::Network => "Unità di rete",
            DriveKind::Optical => "Unità CD/DVD",
            DriveKind::Other => "Unità",
        }
    }
}

/// "Dati (D:)": the volume name (or the kind of drive when it has none) and the drive letter.
pub fn display_name(root: &Path, label: &str, kind: DriveKind) -> String {
    let text = root.to_string_lossy();
    let letter: String = text.trim_end_matches(['\\', '/']).to_string();
    let label = label.trim();
    let name = if label.is_empty() {
        kind.generic_name()
    } else {
        label
    };
    if letter.is_empty() {
        name.to_string()
    } else {
        format!("{name} ({letter})")
    }
}

/// The names of all `roots`, read from the system. Reading a disconnected network drive can take a
/// while, so call this off the UI thread.
pub fn names(roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    roots
        .iter()
        .filter_map(|r| volume(r).map(|(label, kind)| (r.clone(), display_name(r, &label, kind))))
        .collect()
}

#[cfg(windows)]
fn volume(root: &Path) -> Option<(String, DriveKind)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW};

    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call.
    let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
        2 => DriveKind::Removable,
        3 => DriveKind::Fixed,
        4 => DriveKind::Network,
        5 => DriveKind::Optical,
        _ => DriveKind::Other,
    };
    let mut name = [0u16; 261];
    // SAFETY: `wide` is NUL-terminated; `name` is a writable buffer of the length passed; the
    // other out-parameters are optional and passed as null.
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            name.as_mut_ptr(),
            name.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    // A drive without a disk (an empty card reader) has no name to read; it keeps its kind.
    let label = if ok != 0 {
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(name.get(..end).unwrap_or(&[]))
    } else {
        String::new()
    };
    Some((label, kind))
}

#[cfg(not(windows))]
fn volume(_root: &Path) -> Option<(String, DriveKind)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_drive_shows_its_name_and_letter() {
        assert_eq!(
            display_name(Path::new("D:\\"), "Dati", DriveKind::Fixed),
            "Dati (D:)"
        );
        assert_eq!(
            display_name(Path::new("E:\\"), "  ", DriveKind::Removable),
            "Disco rimovibile (E:)"
        );
    }

    #[test]
    fn an_unnamed_drive_shows_its_kind() {
        assert_eq!(
            display_name(Path::new("C:\\"), "", DriveKind::Fixed),
            "Disco locale (C:)"
        );
        assert_eq!(
            display_name(Path::new("Z:\\"), "", DriveKind::Network),
            "Unità di rete (Z:)"
        );
        // no drive letter (a Unix root): just the name
        assert_eq!(display_name(Path::new("/"), "", DriveKind::Other), "Unità");
    }
}
