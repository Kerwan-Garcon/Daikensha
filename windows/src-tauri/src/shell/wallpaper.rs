// The Wallpapers tab (docs/WINDOWS_SHELL.md): the user's own selection of
// images, dropped on the tab. No network: a dropped file is checked (a path a
// real drop delivered, a JPG, PNG or WebP by its first bytes, a capped size),
// copied into %LOCALAPPDATA%\Coucou\wallpapers with a small thumbnail beside
// it, and set with IDesktopWallpaper on one screen or all of them.

// The pure helpers serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};

use serde::Serialize;

pub const MAX_IMAGES: usize = 24;
pub const MAX_BYTES: u64 = 40 * 1024 * 1024;
/// The thumbnail's box, in pixels.
pub const THUMB_W: u32 = 192;
pub const THUMB_H: u32 = 120;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    /// The copy's file name, which is also how the page names it.
    pub id: String,
    /// A data: URL of the thumbnail; empty when there is none.
    pub thumb: String,
}

/// The image kind by its first bytes, never by its name.
pub fn image_kind(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// A name the page may hand back: one of ours, a plain file name, no path.
pub fn valid_id(id: &str) -> bool {
    let ok_ext = [".jpg", ".png", ".webp"].iter().any(|e| id.ends_with(e));
    ok_ext
        && id.len() <= 64
        && id.starts_with("wp-")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !id.contains("..")
}

pub fn dir() -> PathBuf {
    crate::settings::local_dir().join("wallpapers")
}

fn thumb_path(image: &Path) -> PathBuf {
    let mut p = image.as_os_str().to_owned();
    p.push(".thumb.bmp");
    PathBuf::from(p)
}

/// A 32-bit top-down BMP of `pixels` (BGRA rows), the format the webview shows without a decoder of ours.
pub fn bmp(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let data = (width * height * 4) as usize;
    let mut out = Vec::with_capacity(54 + data);
    let le32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(b"BM");
    le32(&mut out, 54 + data as u32);
    le32(&mut out, 0);
    le32(&mut out, 54);
    le32(&mut out, 40);
    le32(&mut out, width);
    le32(&mut out, (-(height as i32)) as u32);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for _ in 0..6 {
        le32(&mut out, 0);
    }
    out.extend_from_slice(&pixels[..data.min(pixels.len())]);
    out.resize(54 + data, 0);
    out
}

fn ids() -> Vec<String> {
    let mut list: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            valid_id(&name).then(|| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), name))
        })
        .collect();
    list.sort_by_key(|b| std::cmp::Reverse(b.0));
    list.into_iter().map(|(_, n)| n).collect()
}

pub fn list() -> Vec<Wallpaper> {
    ids()
        .into_iter()
        .map(|id| {
            let thumb = std::fs::read(thumb_path(&dir().join(&id)))
                .ok()
                .filter(|b| b.len() < 512 * 1024)
                .map(|b| format!("data:image/bmp;base64,{}", crate::voice::base64(&b)))
                .unwrap_or_default();
            Wallpaper { id, thumb }
        })
        .collect()
}

/// Copies the dropped images in. Returns the new list and how many were refused.
pub fn add(paths: &[String]) -> Result<(Vec<Wallpaper>, usize), String> {
    let dir = dir();
    crate::platform::ensure_private_dir(&crate::settings::local_dir()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut count = ids().len();
    let mut refused = 0;
    for (i, source) in paths.iter().take(MAX_IMAGES).enumerate() {
        if count >= MAX_IMAGES || !crate::files::take_dropped(source) {
            refused += 1;
            continue;
        }
        let Some(kind) = check(Path::new(source)) else {
            refused += 1;
            continue;
        };
        let id = format!("wp-{}-{i}.{kind}", super::clipboard::now_ms());
        let dest = dir.join(&id);
        if std::fs::copy(source, &dest).is_err() {
            refused += 1;
            continue;
        }
        #[cfg(windows)]
        if let Some((w, h, pixels)) = super::win_desk::thumbnail(&dest, THUMB_W, THUMB_H) {
            let _ = std::fs::write(thumb_path(&dest), bmp(w, h, &pixels));
        }
        count += 1;
    }
    Ok((list(), refused))
}

/// The kind of the image at `path`, if it is one Coucou takes.
fn check(path: &Path) -> Option<&'static str> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES || meta.len() < 16 {
        return None;
    }
    let mut head = [0u8; 16];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    image_kind(&head)
}

pub fn delete(id: &str) -> Result<Vec<Wallpaper>, String> {
    if !valid_id(id) {
        return Err(crate::i18n::t("That image is gone."));
    }
    let path = dir().join(id);
    let _ = std::fs::remove_file(thumb_path(&path));
    let _ = std::fs::remove_file(&path);
    Ok(list())
}

/// Sets `id` as the wallpaper of screen `screen` (0-based), or of every screen.
pub fn apply(id: &str, screen: Option<usize>) -> Result<(), String> {
    let path = dir().join(id);
    if !valid_id(id) || !path.is_file() {
        return Err(crate::i18n::t("That image is gone."));
    }
    #[cfg(windows)]
    return super::win_desk::set_wallpaper(&path, screen);
    #[cfg(not(windows))]
    {
        let _ = screen;
        Err(crate::i18n::t("Not available on this system."))
    }
}

pub fn screens() -> usize {
    #[cfg(windows)]
    return super::win_desk::wallpaper_screens();
    #[cfg(not(windows))]
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_known_by_their_bytes_not_their_name() {
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(image_kind(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_kind(b"MZ\x90\0 not an image"), None);
        assert_eq!(image_kind(b"<svg"), None);
    }

    #[test]
    fn only_our_own_plain_names_are_taken_back() {
        assert!(valid_id("wp-1729250000000-0.jpg"));
        assert!(!valid_id("../settings.json"));
        assert!(!valid_id("wp-..\\x.jpg"));
        assert!(!valid_id("wp-1.exe"));
        assert!(!valid_id("C:\\Windows\\x.jpg"));
        assert!(!valid_id("photo.jpg"));
    }

    #[test]
    fn a_bmp_has_the_size_it_says() {
        let b = bmp(2, 3, &[7u8; 24]);
        assert_eq!(&b[..2], b"BM");
        assert_eq!(b.len(), 54 + 24);
        assert_eq!(u32::from_le_bytes(b[2..6].try_into().unwrap()), 78);
        assert_eq!(i32::from_le_bytes(b[22..26].try_into().unwrap()), -3);
        assert_eq!(bmp(2, 2, &[]).len(), 54 + 16);
    }
}
