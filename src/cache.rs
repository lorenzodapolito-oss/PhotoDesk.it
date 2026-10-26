//! The thumbnail cache on disk. A folder opened a second time shows its thumbnails (and camera
//! data) at once instead of decoding every picture again.
//!
//! One small JPEG (PNG when the picture has transparency) per picture, named after a hash of the
//! path, size and modification time: an edited or replaced file gets a new name, so a stale
//! thumbnail is never shown. The camera data sits next to it in a `.json` file.

use crate::exif::ExifSummary;
use crate::thumbs::Pixels;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a cached entry holds.
pub struct Cached {
    pub pixels: Pixels,
    pub exif: Option<ExifSummary>,
}

#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

/// FNV-1a, 64 bit: simple, and the same on every platform and Rust version.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Cache {
    pub fn new(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    /// The cache name of a file in its current state.
    pub fn key(path: &Path, size: u64, modified: Option<SystemTime>) -> String {
        let t = modified
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| (d.as_secs(), d.subsec_nanos()))
            .unwrap_or((0, 0));
        let mut buf = path.to_string_lossy().into_owned().into_bytes();
        buf.extend_from_slice(&size.to_le_bytes());
        buf.extend_from_slice(&t.0.to_le_bytes());
        buf.extend_from_slice(&t.1.to_le_bytes());
        format!("{:016x}", fnv1a(&buf))
    }

    fn picture_path(&self, key: &str, png: bool) -> PathBuf {
        self.dir
            .join(format!("{key}.{}", if png { "png" } else { "jpg" }))
    }

    fn data_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Cached> {
        let bytes = [false, true]
            .iter()
            .find_map(|&png| std::fs::read(self.picture_path(key, png)).ok())?;
        let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let exif = std::fs::read_to_string(self.data_path(key))
            .ok()
            .and_then(|s| serde_json::from_str::<ExifSummary>(&s).ok());
        Some(Cached {
            pixels: Pixels {
                width: img.width() as usize,
                height: img.height() as usize,
                rgba: img.into_raw(),
            },
            exif,
        })
    }

    /// Keep a thumbnail. Failures (a full disk, a read-only folder) are ignored: the cache is
    /// only a speed-up.
    pub fn put(&self, key: &str, pixels: &Pixels, exif: Option<&ExifSummary>) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let (Ok(w), Ok(h)) = (u32::try_from(pixels.width), u32::try_from(pixels.height)) else {
            return;
        };
        let Some(rgba) = image::RgbaImage::from_raw(w, h, pixels.rgba.clone()) else {
            return;
        };
        let transparent = rgba.pixels().any(|p| p.0[3] < 250);
        let mut out = Vec::new();
        let encoded = if transparent {
            image::DynamicImage::ImageRgba8(rgba)
                .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        } else {
            let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
                .encode_image(&rgb)
        };
        if encoded.is_err() {
            return;
        }
        if crate::fsutil::write_atomic(&self.picture_path(key, transparent), &out).is_err() {
            return;
        }
        if let Some(e) = exif {
            if let Ok(json) = serde_json::to_vec(e) {
                let _ = crate::fsutil::write_atomic(&self.data_path(key), &json);
            }
        }
    }

    /// Delete the oldest entries until the cache is under `max_bytes`.
    pub fn trim(&self, max_bytes: u64) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let md = e.metadata().ok()?;
                md.is_file().then(|| {
                    (
                        md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        md.len(),
                        e.path(),
                    )
                })
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= max_bytes {
            return;
        }
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "photodesk-cache-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn pixels(alpha: u8) -> Pixels {
        Pixels {
            width: 4,
            height: 2,
            rgba: [200, 100, 50, alpha].repeat(8),
        }
    }

    #[test]
    fn a_stored_thumbnail_comes_back_with_its_camera_data() {
        let d = temp("roundtrip");
        let c = Cache::new(d.clone());
        let ex = ExifSummary {
            camera: "Nikon Z6".into(),
            iso: Some(200),
            ..Default::default()
        };
        c.put("abc", &pixels(255), Some(&ex));
        let got = c.get("abc").expect("cached");
        assert_eq!((got.pixels.width, got.pixels.height), (4, 2));
        assert_eq!(got.exif, Some(ex));
        assert!(c.get("other").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn transparency_survives_in_the_cache() {
        let d = temp("alpha");
        let c = Cache::new(d.clone());
        c.put("png", &pixels(100), None);
        let got = c.get("png").expect("cached");
        assert!(got.pixels.rgba.chunks(4).all(|p| p[3] < 150));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_name_changes_when_the_file_changes() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let a = Cache::key(Path::new("/p/a.jpg"), 10, Some(t));
        assert_eq!(a, Cache::key(Path::new("/p/a.jpg"), 10, Some(t)));
        assert_ne!(a, Cache::key(Path::new("/p/a.jpg"), 11, Some(t)));
        assert_ne!(
            a,
            Cache::key(Path::new("/p/a.jpg"), 10, Some(t + Duration::from_secs(1)))
        );
        assert_ne!(a, Cache::key(Path::new("/p/b.jpg"), 10, Some(t)));
    }

    #[test]
    fn trimming_keeps_the_cache_under_its_limit() {
        let d = temp("trim");
        let c = Cache::new(d.clone());
        for i in 0..5 {
            c.put(&format!("k{i}"), &pixels(255), None);
        }
        c.trim(0);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_entry_is_just_a_miss() {
        let d = temp("damaged");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("bad.jpg"), b"not a jpeg").unwrap();
        assert!(Cache::new(d.clone()).get("bad").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
