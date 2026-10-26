//! Thumbnails and previews. Plain pictures are decoded with the `image` crate; raw files show the
//! JPEG preview most cameras embed in them (no raw decoding here: that is LightRaw's job).

use crate::files::Kind;
use std::path::Path;

/// A decoded picture ready for a texture.
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Raw files bigger than this are not read for a preview.
const MAX_RAW_BYTES: u64 = 400 * 1024 * 1024;
/// Plain pictures bigger than this are not decoded for a thumbnail.
const MAX_IMAGE_BYTES: u64 = 300 * 1024 * 1024;

/// Decode `path` for display, fitted inside `max` pixels. `None` when it can't be shown.
pub fn load(path: &Path, kind: Kind, max: u32) -> Option<Pixels> {
    load_with_exif(path, kind, max).map(|(p, _)| p)
}

/// Like [`load`], and also the camera data of the file (read from the same bytes).
pub fn load_with_exif(
    path: &Path,
    kind: Kind,
    max: u32,
) -> Option<(Pixels, Option<crate::exif::ExifSummary>)> {
    let size = std::fs::metadata(path).ok()?.len();
    let bytes_limit = if kind == Kind::Raw {
        MAX_RAW_BYTES
    } else {
        MAX_IMAGE_BYTES
    };
    if size > bytes_limit {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let mut summary = crate::exif::read(&data);
    let (mut img, orientation) = match kind {
        Kind::Image => {
            let img = image::load_from_memory(&data).ok()?;
            // the real pixel size, whatever the camera data says
            let s = summary.get_or_insert_with(Default::default);
            s.width = Some(img.width());
            s.height = Some(img.height());
            (img, exif_orientation_of_jpeg(&data).unwrap_or(1))
        }
        Kind::Raw => {
            let jpeg = largest_embedded_jpeg(&data)?;
            let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).ok()?;
            if summary.is_none() {
                summary = crate::exif::read(jpeg);
            }
            // The camera's orientation is in the raw file itself (TIFF based), else in the preview.
            let o = exif_orientation_of_tiff(&data)
                .or_else(|| exif_orientation_of_jpeg(jpeg))
                .unwrap_or(1);
            (img, o)
        }
    };
    if img.width() > max || img.height() > max {
        img = img.thumbnail(max, max);
    }
    img = apply_orientation(img, orientation);
    let rgba = img.to_rgba8();
    let pixels = Pixels {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        rgba: rgba.into_raw(),
    };
    Some((pixels, summary.filter(|s| !s.is_empty())))
}

/// The pixel size of the picture, read from the header only (cheap). Raw files: the preview.
pub fn dimensions(path: &Path, kind: Kind) -> Option<(u32, u32)> {
    match kind {
        Kind::Image => image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok(),
        Kind::Raw => None,
    }
}

pub fn apply_orientation(img: image::DynamicImage, orientation: u8) -> image::DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// The biggest JPEG stream inside `data` (a raw file's embedded preview), or `None`.
///
/// It looks for JPEG start markers (FF D8 FF) and ends each candidate at the next start marker
/// or the end of the file; the decoder ignores trailing bytes. Only the few largest candidates
/// are tried, so a file full of tiny thumbnails costs little.
pub fn largest_embedded_jpeg(data: &[u8]) -> Option<&[u8]> {
    let mut starts: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while let Some(rest) = data.get(i..) {
        let Some(pos) = rest.windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) else {
            break;
        };
        starts.push(i + pos);
        i += pos + 3;
        if starts.len() > 64 {
            break;
        }
    }
    let mut spans: Vec<(usize, usize)> = starts
        .iter()
        .enumerate()
        .map(|(n, &s)| (s, starts.get(n + 1).copied().unwrap_or(data.len())))
        .filter(|(s, e)| e.saturating_sub(*s) > 2048)
        .collect();
    spans.sort_by_key(|(s, e)| std::cmp::Reverse(e - s));
    for (s, e) in spans.into_iter().take(3) {
        let Some(slice) = data.get(s..e) else {
            continue;
        };
        if image::load_from_memory_with_format(slice, image::ImageFormat::Jpeg).is_ok() {
            return Some(slice);
        }
    }
    None
}

/// The EXIF orientation (1..=8) of a JPEG, from its APP1 segment.
pub fn exif_orientation_of_jpeg(data: &[u8]) -> Option<u8> {
    if data.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut pos = 2usize;
    // Walk the marker segments at the start of the file; the EXIF block is one of the first.
    for _ in 0..32 {
        if *data.get(pos)? != 0xFF {
            return None;
        }
        let marker = *data.get(pos + 1)?;
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([*data.get(pos + 2)?, *data.get(pos + 3)?]) as usize;
        if marker == 0xE1 {
            let seg = data.get(pos + 4..pos + 2 + len)?;
            if seg.get(0..6)? == b"Exif\0\0" {
                return exif_orientation_of_tiff(seg.get(6..)?);
            }
        }
        pos = pos.checked_add(2)?.checked_add(len)?;
    }
    None
}

/// The orientation tag in IFD0 of a TIFF-structured block (also a TIFF-based raw file).
pub fn exif_orientation_of_tiff(data: &[u8]) -> Option<u8> {
    let le = match data.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = [*data.get(o)?, *data.get(o + 1)?];
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = [
            *data.get(o)?,
            *data.get(o + 1)?,
            *data.get(o + 2)?,
            *data.get(o + 3)?,
        ];
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    for n in 0..count.min(512) {
        let entry = ifd.checked_add(2)?.checked_add(n.checked_mul(12)?)?;
        if u16_at(entry)? == 0x0112 {
            let v = u16_at(entry.checked_add(8)?)?;
            return (1..=8).contains(&v).then_some(v as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 3) as u8, (y * 5) as u8, 90]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    }

    #[test]
    fn finds_the_biggest_jpeg_inside_a_raw_like_blob() {
        let (small, big) = (jpeg(64, 48), jpeg(320, 240));
        let mut blob = vec![0u8; 100];
        blob.extend(&small);
        blob.extend([1u8; 50]);
        blob.extend(&big);
        blob.extend([2u8; 30]);
        let found = largest_embedded_jpeg(&blob).unwrap();
        let img = image::load_from_memory_with_format(found, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (320, 240));
    }

    #[test]
    fn no_jpeg_gives_none_and_garbage_does_not_panic() {
        assert!(largest_embedded_jpeg(&[0u8; 5000]).is_none());
        let mut junk = vec![0xFF, 0xD8, 0xFF];
        junk.extend(vec![7u8; 4000]);
        assert!(largest_embedded_jpeg(&junk).is_none());
        assert!(largest_embedded_jpeg(&[]).is_none());
    }

    fn tiff(le: bool, orientation: u16) -> Vec<u8> {
        let w16 = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let w32 = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut t = Vec::new();
        t.extend(if le { *b"II" } else { *b"MM" });
        t.extend(w16(42));
        t.extend(w32(8));
        t.extend(w16(1)); // one entry
        t.extend(w16(0x0112));
        t.extend(w16(3));
        t.extend(w32(1));
        t.extend(w16(orientation));
        t.extend([0, 0]);
        t.extend(w32(0));
        t
    }

    #[test]
    fn reads_orientation_from_tiff_in_both_byte_orders() {
        assert_eq!(exif_orientation_of_tiff(&tiff(true, 6)), Some(6));
        assert_eq!(exif_orientation_of_tiff(&tiff(false, 8)), Some(8));
        assert_eq!(exif_orientation_of_tiff(&tiff(true, 99)), None);
        assert_eq!(exif_orientation_of_tiff(b"II*"), None);
        assert_eq!(exif_orientation_of_tiff(&[]), None);
    }

    #[test]
    fn reads_orientation_from_a_jpeg_app1_segment() {
        let mut seg = b"Exif\0\0".to_vec();
        seg.extend(tiff(false, 3));
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
        data.extend(((seg.len() + 2) as u16).to_be_bytes());
        data.extend(&seg);
        data.extend([0xFF, 0xD9]);
        assert_eq!(exif_orientation_of_jpeg(&data), Some(3));
        assert_eq!(exif_orientation_of_jpeg(&jpeg(16, 16)), None);
        // truncated segment
        assert_eq!(exif_orientation_of_jpeg(&data[..10]), None);
    }

    #[test]
    fn orientation_6_turns_a_landscape_into_a_portrait() {
        let img = image::DynamicImage::new_rgb8(40, 20);
        let r = apply_orientation(img, 6);
        assert_eq!((r.width(), r.height()), (20, 40));
    }

    #[test]
    fn loads_a_plain_picture_fitted_to_the_requested_size() {
        let d = std::env::temp_dir().join(format!("photodesk-thumb-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("a.jpg");
        std::fs::write(&p, jpeg(400, 200)).unwrap();
        let px = load(&p, Kind::Image, 100).unwrap();
        assert_eq!((px.width, px.height), (100, 50));
        assert_eq!(px.rgba.len(), 100 * 50 * 4);
        assert!(load(&d.join("missing.jpg"), Kind::Image, 100).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
