// Mochi looks at a window (WindowLook.swift on the Mac): dragged out of the
// island and dropped on another app's window, he takes that window alone and
// the chat opens with what he sees, a summary and an analysis. Holding Alt
// while dropping keeps the old behaviour: he lands on the desktop there.
//
// Like the circled capture (shell/capture.rs), the picture never touches the
// disk and never reaches the log: a PNG held here, sent with the first question
// of the conversation it starts (to the chat provider the user picked), and
// dropped once that question is answered or another window replaces it.
// Windows only for now: elsewhere no window is ever found under Mochi.

// The pixel helpers serve the Windows capture; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use std::sync::Mutex;

use serde::Serialize;

/// The longest side sent. Smaller than a circled capture's: the PNG is not
/// compressed, and a whole window at 1568 px could pass the 5 MB the APIs take.
pub const MAX_SIDE: u32 = 1280;

/// What the model is told of the picture.
pub const LOOK_LINE: &str = "The image is the window the user just dropped you on.";

static LOOK: Mutex<Option<Vec<u8>>> = Mutex::new(None);

pub fn peek() -> Option<Vec<u8>> {
    LOOK.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn discard() {
    *LOOK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// A window found under Mochi, not captured yet.
pub struct Target {
    #[cfg_attr(not(windows), allow(dead_code))]
    id: isize,
    pub app_name: String,
    pub title: String,
}

/// What the island is told once Mochi has looked (or tried to).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Looked {
    pub app_name: String,
    pub title: String,
    /// Set when the window could not be taken; nothing is kept then.
    pub error: Option<String>,
}

/// The window of another app under `at` (physical desktop pixels), if any.
pub fn window_at(at: (f64, f64)) -> Option<Target> {
    #[cfg(windows)]
    {
        let (id, title, app_name) = crate::shell::win::window_at(at.0.round() as i32, at.1.round() as i32)?;
        Some(Target { id, app_name, title })
    }
    #[cfg(not(windows))]
    {
        let _ = at;
        None
    }
}

/// Takes the window and keeps it for the chat. Blocking: GDI work.
pub fn take(target: Target) -> Looked {
    let result = grab(&target);
    let error = match result {
        Ok(png) => {
            *LOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(png);
            None
        }
        Err(err) => {
            discard();
            crate::log::line(format!("look: the window could not be taken ({err})"));
            Some(crate::i18n::t("Mochi couldn't see that window. Try again."))
        }
    };
    Looked { app_name: target.app_name, title: target.title, error }
}

fn grab(target: &Target) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let (w, h, bgra) = crate::shell::win::capture_window(target.id)?;
        let (w, h, rgb) = crate::shell::capture::fit_rgb(w, h, &bgra, MAX_SIDE);
        Ok(crate::shell::capture::png(w, h, &rgb))
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(crate::i18n::t("Not available on this system."))
    }
}

// ── Pixels ────────────────────────────────────────────────────────────────────

/// The part `(x, y, w, h)` of a `width` × `height` BGRA image, clipped to it.
/// None when nothing of it is inside.
pub fn crop(width: u32, height: u32, bgra: &[u8], (x, y, w, h): (u32, u32, u32, u32)) -> Option<(u32, u32, Vec<u8>)> {
    if x >= width || y >= height || bgra.len() < (width as usize) * (height as usize) * 4 {
        return None;
    }
    let (w, h) = (w.min(width - x), h.min(height - y));
    if w == 0 || h == 0 {
        return None;
    }
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&bgra[start..start + (w * 4) as usize]);
    }
    Some((w, h, out))
}

/// True when every pixel is black: what PrintWindow leaves of a window it
/// could not draw. Only a sample is read, it is enough to tell.
pub fn is_blank(bgra: &[u8]) -> bool {
    let pixels = bgra.len() / 4;
    let step = (pixels / 4096).max(1);
    (0..pixels).step_by(step).all(|i| bgra[i * 4] == 0 && bgra[i * 4 + 1] == 0 && bgra[i * 4 + 2] == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32) -> Vec<u8> {
        // Each pixel's blue byte is its index, so a crop shows where it came from.
        (0..w * h).flat_map(|i| [i as u8, 1, 2, 255]).collect()
    }

    #[test]
    fn a_crop_takes_the_frame_inside_the_window() {
        let px = image(4, 3);
        let (w, h, out) = crop(4, 3, &px, (1, 1, 2, 2)).unwrap();
        assert_eq!((w, h), (2, 2));
        let blues: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        assert_eq!(blues, vec![5, 6, 9, 10]);
    }

    #[test]
    fn a_crop_is_clipped_to_the_image() {
        let px = image(4, 3);
        let (w, h, out) = crop(4, 3, &px, (2, 1, 10, 10)).unwrap();
        assert_eq!((w, h, out.len()), (2, 2, 16));
        assert!(crop(4, 3, &px, (4, 0, 1, 1)).is_none());
        assert!(crop(4, 3, &px, (0, 0, 0, 1)).is_none());
        assert!(crop(4, 3, &px[..8], (0, 0, 1, 1)).is_none());
    }

    #[test]
    fn a_black_picture_is_blank_and_anything_else_is_not() {
        assert!(is_blank(&[0, 0, 0, 255].repeat(10_000)));
        let mut px = [0u8, 0, 0, 255].repeat(10_000);
        px[0] = 40;
        assert!(!is_blank(&px));
        assert!(is_blank(&[]));
    }

    #[test]
    fn the_picture_is_kept_until_discarded() {
        *LOOK.lock().unwrap() = Some(vec![1, 2, 3]);
        assert_eq!(peek(), Some(vec![1, 2, 3]));
        discard();
        assert_eq!(peek(), None);
    }

    #[test]
    fn nothing_is_looked_at_where_windows_cannot_be_read() {
        if cfg!(not(windows)) {
            assert!(window_at((10.0, 10.0)).is_none());
        }
    }
}
