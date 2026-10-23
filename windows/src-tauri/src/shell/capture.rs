// « Entoure et demande » (circle to ask, docs/WINDOWS_SHELL.md): a shortcut or
// the panel's lasso button lays a full-screen overlay over the screen under the
// cursor; the user circles something; Coucou takes that part of the screen and
// opens the chat with it. The overlay is a window of its own, made for that
// moment and destroyed right after, so nothing stays in memory between uses.
//
// The capture never touches the disk and never reaches the log: it is a PNG
// held here, shown as a preview in the island, sent to Claude as an image block
// with the first question (the API, or the CLI's stream-json input — no file,
// no Read permission), and dropped after that turn or on Cancel. Only Claude
// takes it: the other chat providers refuse it.

// The encoder serves the Windows capture; elsewhere only the tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const LABEL: &str = "circle";
/// The longest side sent: Claude scales larger images down anyway.
pub const MAX_SIDE: u32 = 1568;
const MIN_SIDE: f64 = 8.0;

static CAPTURE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

pub fn peek() -> Option<Vec<u8>> {
    CAPTURE.lock().clone()
}

pub fn discard() {
    *CAPTURE.lock() = None;
}

// ── A PNG, without a crate ────────────────────────────────────────────────────

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// An RGB PNG of `rgb` (rows top-down), its zlib stream made of stored blocks:
/// bigger than a compressed one, but small enough to send, and no dependency.
pub fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0); // no filter
        raw.extend_from_slice(&rgb[y * row..(y + 1) * row]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    if blocks.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
    chunk(&mut out, *b"IHDR", &ihdr);
    chunk(&mut out, *b"IDAT", &z);
    chunk(&mut out, *b"IEND", &[]);
    out
}

/// BGRA rows to RGB, scaled down (averaging) so the longest side is at most `max`.
pub fn fit_rgb(width: u32, height: u32, bgra: &[u8], max: u32) -> (u32, u32, Vec<u8>) {
    let step = width.max(height).div_ceil(max.max(1)).max(1);
    let (w, h) = ((width / step).max(1), (height / step).max(1));
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..step {
                for dx in 0..step {
                    let (sx, sy) = ((x * step + dx).min(width - 1), (y * step + dy).min(height - 1));
                    let i = ((sy * width + sx) * 4) as usize;
                    sum[0] += u32::from(bgra[i + 2]);
                    sum[1] += u32::from(bgra[i + 1]);
                    sum[2] += u32::from(bgra[i]);
                }
            }
            let n = step * step;
            out.extend(sum.iter().map(|s| (s / n) as u8));
        }
    }
    (w, h, out)
}

// ── The overlay ───────────────────────────────────────────────────────────────

fn page_url(app: &AppHandle) -> tauri::WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/circle.html");
        return tauri::WebviewUrl::External(base);
    }
    let _ = app;
    tauri::WebviewUrl::App("circle.html".into())
}

fn panel_on(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().shell_panel)
}

/// Lays the overlay over the screen under the cursor. Nothing while the panel is off.
pub fn start(app: &AppHandle) {
    if !cfg!(windows) || !panel_on(app) {
        return;
    }
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.set_focus();
        return;
    }
    let (cx, cy) = crate::platform::cursor_physical().unwrap_or((0.0, 0.0));
    let monitor = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .find(|m| {
            let (p, s) = (m.position(), m.size());
            cx >= f64::from(p.x) && cy >= f64::from(p.y) && cx < f64::from(p.x) + f64::from(s.width) && cy < f64::from(p.y) + f64::from(s.height)
        })
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let built = tauri::WebviewWindowBuilder::new(app, LABEL, page_url(app))
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Coucou")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .disable_drag_drop_handler()
        .focused(true)
        .visible(false)
        .build();
    match built {
        Ok(win) => {
            let _ = win.set_position(*monitor.position());
            let _ = win.set_size(*monitor.size());
            let _ = win.show();
            let _ = win.set_focus();
        }
        Err(err) => crate::log::line(format!("circle overlay failed: {err}")),
    }
}

fn close(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.destroy();
    }
}

/// The overlay's lasso, as a rectangle in its own CSS pixels: closes the
/// overlay, then takes that part of the screen.
#[tauri::command]
pub async fn circle_capture(app: AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    let win = app.get_webview_window(LABEL).ok_or("no overlay")?;
    let scale = win.scale_factor().map_err(|e| e.to_string())?;
    let origin = win.outer_position().map_err(|e| e.to_string())?;
    close(&app);
    if !(width >= MIN_SIDE && height >= MIN_SIDE && x.is_finite() && y.is_finite()) {
        return Ok(());
    }
    let rect = (
        origin.x + (x * scale).round() as i32,
        origin.y + (y * scale).round() as i32,
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    );
    // Let the compositor repaint the screen without the overlay first.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let png = tauri::async_runtime::spawn_blocking(move || grab(rect)).await.map_err(|e| e.to_string())??;
    *CAPTURE.lock() = Some(png);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "circle-captured", ());
    Ok(())
}

fn grab(rect: (i32, i32, i32, i32)) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let (w, h, bgra) = super::win_desk::capture_screen(rect.0, rect.1, rect.2, rect.3)?;
        let (w, h, rgb) = fit_rgb(w, h, &bgra, MAX_SIDE);
        Ok(png(w, h, &rgb))
    }
    #[cfg(not(windows))]
    {
        let _ = rect;
        Err(crate::i18n::t("Not available on this system."))
    }
}

// Both are `async` on purpose: on Windows, creating or destroying a webview
// window from a synchronous command (which runs on the main thread) deadlocks
// the app, so the panel's lasso froze everything. Tauri's docs ask for async.

#[tauri::command]
pub async fn circle_cancel(app: AppHandle) {
    close(&app);
}

/// Opens the overlay from the island (the panel's lasso button).
#[tauri::command]
pub async fn circle_start(app: AppHandle) {
    start(&app);
}

/// The capture as a data: URL, for the preview in the island.
#[tauri::command]
pub fn circle_preview() -> Option<String> {
    peek().map(|p| format!("data:image/png;base64,{}", crate::voice::base64(&p)))
}

#[tauri::command]
pub fn circle_discard() {
    discard();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_the_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_png_is_well_formed() {
        let p = png(2, 2, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(p[16..20].try_into().unwrap()), 2);
        assert!(p.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
        // A big image spans several stored blocks.
        let big = png(400, 200, &vec![7u8; 400 * 200 * 3]);
        assert!(big.len() > 400 * 200 * 3);
    }

    #[test]
    fn a_big_capture_is_scaled_down_and_colours_swapped() {
        let bgra = [10u8, 20, 30, 255].repeat(4000 * 10);
        let (w, h, rgb) = fit_rgb(4000, 10, &bgra, MAX_SIDE);
        assert!(w <= MAX_SIDE && h >= 1);
        assert_eq!(&rgb[..3], &[30, 20, 10]);
        let (w, h, _) = fit_rgb(100, 50, &vec![0u8; 100 * 50 * 4], MAX_SIDE);
        assert_eq!((w, h), (100, 50));
        // A thin strip never comes out empty.
        let (w, h, rgb) = fit_rgb(4000, 1, &vec![0u8; 4000 * 4], MAX_SIDE);
        assert_eq!((h, rgb.len()), (1, (w * 3) as usize));
    }
}
