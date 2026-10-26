//! Camera data (EXIF) for the details panel, the tooltips, sorting by shooting date and filters.
//!
//! Read with the `kamadak-exif` crate from the bytes already in memory (the thumbnail worker has
//! them anyway). Works for JPEG, TIFF and the TIFF based raw files (NEF, CR2, ARW, DNG...); other
//! raw containers simply give no data.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExifSummary {
    pub camera: String,
    pub lens: String,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter: String,
    pub focal: Option<f32>,
    /// When the picture was taken, "2026-05-12T10:11:12" (camera clock, no time zone).
    pub taken: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl ExifSummary {
    pub fn is_empty(&self) -> bool {
        *self == ExifSummary::default()
    }

    pub fn taken_at(&self) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(self.taken.as_deref()?, DATE_FORMAT).ok()
    }

    /// "12 mag 2026 10:11".
    pub fn taken_text(&self) -> String {
        self.taken_at()
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default()
    }

    /// "1/500 · f/5.6 · ISO 200" (the parts that are known).
    pub fn exposure_line(&self) -> String {
        let mut parts = Vec::new();
        if !self.shutter.is_empty() {
            parts.push(self.shutter.clone());
        }
        if let Some(a) = self.aperture {
            parts.push(format!("f/{}", trim_float(a)));
        }
        if let Some(i) = self.iso {
            parts.push(format!("ISO {i}"));
        }
        parts.join(" · ")
    }

    pub fn focal_text(&self) -> String {
        self.focal
            .map(|f| format!("{} mm", trim_float(f)))
            .unwrap_or_default()
    }

    pub fn size_text(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w} × {h}"),
            _ => String::new(),
        }
    }

    /// A short tooltip text for a thumbnail.
    pub fn tooltip(&self) -> String {
        let mut lines = Vec::new();
        if !self.camera.is_empty() {
            lines.push(self.camera.clone());
        }
        if !self.lens.is_empty() {
            lines.push(self.lens.clone());
        }
        let e = self.exposure_line();
        if !e.is_empty() {
            lines.push(e);
        }
        let f = self.focal_text();
        if !f.is_empty() {
            lines.push(f);
        }
        let t = self.taken_text();
        if !t.is_empty() {
            lines.push(t);
        }
        lines.join("\n")
    }
}

fn trim_float(v: f32) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

fn ascii(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let f = exif.get_field(tag, exif::In::PRIMARY)?;
    if let exif::Value::Ascii(v) = &f.value {
        let s = v.first()?;
        let t = String::from_utf8_lossy(s)
            .trim_matches(|c: char| c == '\0' || c.is_whitespace())
            .to_string();
        return (!t.is_empty()).then_some(t);
    }
    None
}

fn uint(exif: &exif::Exif, tag: exif::Tag) -> Option<u32> {
    exif.get_field(tag, exif::In::PRIMARY)?.value.get_uint(0)
}

fn rational(exif: &exif::Exif, tag: exif::Tag) -> Option<f64> {
    match &exif.get_field(tag, exif::In::PRIMARY)?.value {
        exif::Value::Rational(v) => v.first().map(exif::Rational::to_f64),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

/// "1/500" for fast shutters, "2.5 s" or "30 s" for slow ones.
pub fn shutter_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    if seconds < 0.5 {
        format!("1/{}", (1.0 / seconds).round() as u64)
    } else if (seconds - seconds.round()).abs() < 0.05 {
        format!("{} s", seconds.round() as u64)
    } else {
        format!("{seconds:.1} s")
    }
}

/// "2026:05:12 10:11:12" to the sortable form kept in [`ExifSummary::taken`].
pub fn normalize_date(s: &str) -> Option<String> {
    let t = NaiveDateTime::parse_from_str(s.trim(), "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    Some(t.format(DATE_FORMAT).to_string())
}

/// The camera data of a picture file held in memory, `None` when there is none.
pub fn read(data: &[u8]) -> Option<ExifSummary> {
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(data))
        .ok()?;
    let make = ascii(&exif, exif::Tag::Make).unwrap_or_default();
    let model = ascii(&exif, exif::Tag::Model).unwrap_or_default();
    let camera = if model.is_empty() {
        make
    } else if make.is_empty() || model.to_lowercase().starts_with(&make.to_lowercase()) {
        model
    } else {
        format!("{make} {model}")
    };
    let taken = ascii(&exif, exif::Tag::DateTimeOriginal)
        .or_else(|| ascii(&exif, exif::Tag::DateTime))
        .and_then(|s| normalize_date(&s));
    let s = ExifSummary {
        camera,
        lens: ascii(&exif, exif::Tag::LensModel).unwrap_or_default(),
        iso: uint(&exif, exif::Tag::PhotographicSensitivity),
        aperture: rational(&exif, exif::Tag::FNumber).map(|v| v as f32),
        shutter: rational(&exif, exif::Tag::ExposureTime)
            .map(shutter_text)
            .unwrap_or_default(),
        focal: rational(&exif, exif::Tag::FocalLength).map(|v| v as f32),
        taken,
        width: uint(&exif, exif::Tag::PixelXDimension),
        height: uint(&exif, exif::Tag::PixelYDimension),
    };
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_speeds_read_like_a_camera_shows_them() {
        assert_eq!(shutter_text(1.0 / 500.0), "1/500");
        assert_eq!(shutter_text(0.3), "1/3");
        assert_eq!(shutter_text(2.0), "2 s");
        assert_eq!(shutter_text(2.5), "2.5 s");
        assert_eq!(shutter_text(0.0), "");
        assert_eq!(shutter_text(f64::NAN), "");
    }

    #[test]
    fn exif_dates_become_sortable_text() {
        assert_eq!(
            normalize_date("2026:05:12 10:11:12").as_deref(),
            Some("2026-05-12T10:11:12")
        );
        assert_eq!(normalize_date("not a date"), None);
    }

    #[test]
    fn garbage_gives_no_data_and_no_panic() {
        assert!(read(b"").is_none());
        assert!(read(&[0xFF, 0xD8, 0xFF, 0xE1, 0, 5, b'E']).is_none());
        assert!(read(b"II*\0\x08\0\0\0\xff\xff").is_none());
    }

    #[test]
    fn the_summary_lines_skip_unknown_parts() {
        let s = ExifSummary {
            shutter: "1/500".into(),
            aperture: Some(5.6),
            iso: Some(200),
            focal: Some(50.0),
            ..Default::default()
        };
        assert_eq!(s.exposure_line(), "1/500 · f/5.6 · ISO 200");
        assert_eq!(s.focal_text(), "50 mm");
        assert_eq!(ExifSummary::default().exposure_line(), "");
        assert!(ExifSummary::default().is_empty());
    }

    /// A minimal JPEG with an Exif block: Make, Model and DateTimeOriginal, built by hand.
    #[test]
    fn reads_a_small_hand_built_exif_block() {
        let make = b"Nikon\0";
        let model = b"Z6II\0"; // more than 4 bytes: stored out of line
        let date = b"2026:05:12 10:11:12\0";
        // TIFF: header, IFD0 with 3 entries (Make, Model, ExifIFD pointer), then the Exif IFD
        let mut t: Vec<u8> = vec![b'M', b'M', 0, 42, 0, 0, 0, 8];
        let ifd0_len = 2 + 3 * 12 + 4;
        let make_off = 8 + ifd0_len;
        let model_off = make_off + make.len();
        let exif_ifd_off = model_off + model.len();
        t.extend_from_slice(&3u16.to_be_bytes());
        let entry = |tag: u16, typ: u16, count: u32, value: u32| -> Vec<u8> {
            let mut e = Vec::new();
            e.extend_from_slice(&tag.to_be_bytes());
            e.extend_from_slice(&typ.to_be_bytes());
            e.extend_from_slice(&count.to_be_bytes());
            e.extend_from_slice(&value.to_be_bytes());
            e
        };
        t.extend(entry(0x010F, 2, make.len() as u32, make_off as u32));
        t.extend(entry(0x0110, 2, model.len() as u32, model_off as u32));
        t.extend(entry(0x8769, 4, 1, exif_ifd_off as u32));
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(make);
        t.extend_from_slice(model);
        let date_off = exif_ifd_off + 2 + 12 + 4;
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend(entry(0x9003, 2, date.len() as u32, date_off as u32));
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(date);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&t);
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let s = read(&jpeg).expect("exif");
        assert_eq!(s.camera, "Nikon Z6II");
        assert_eq!(s.taken.as_deref(), Some("2026-05-12T10:11:12"));
        assert_eq!(s.taken_text(), "2026-05-12 10:11");
    }
}
