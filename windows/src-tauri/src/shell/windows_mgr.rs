// The Windows tab (docs/WINDOWS_SHELL.md): the open windows, focus, minimise,
// maximise, halves and quarters, another screen, and saved layouts. Snap
// Layouts have no API, so halves and quarters are computed here from the
// screen's work area and applied with SetWindowPos (shell/win.rs).
//
// A window is named by its handle, as the list gave it; every action checks
// again that the handle is a visible top-level window that is not Coucou's.

// The pure helpers here serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// The window handle, as a string (JavaScript numbers cannot hold every one).
    pub id: String,
    pub title: String,
    /// The program, "Code" for Code.exe: shown, and how layouts find it again.
    pub app: String,
    /// Index of its screen in `screens()`.
    pub screen: usize,
    pub minimized: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Snap {
    Left,
    Right,
    Full,
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Focus,
    Minimize,
    Maximize,
    Restore,
    Snap { to: Snap },
    Screen { index: usize },
}

/// Where `snap` puts a window on a screen whose work area is `work`.
pub fn snap_rect(work: Rect, snap: Snap) -> Rect {
    let (hw, hh) = (work.w / 2, work.h / 2);
    let (x2, y2) = (work.x + hw, work.y + hh);
    match snap {
        Snap::Left => Rect { x: work.x, y: work.y, w: hw, h: work.h },
        Snap::Right => Rect { x: x2, y: work.y, w: work.w - hw, h: work.h },
        Snap::Full => work,
        Snap::Center => {
            let (w, h) = (work.w * 3 / 5, work.h * 7 / 10);
            Rect { x: work.x + (work.w - w) / 2, y: work.y + (work.h - h) / 2, w, h }
        }
        Snap::TopLeft => Rect { x: work.x, y: work.y, w: hw, h: hh },
        Snap::TopRight => Rect { x: x2, y: work.y, w: work.w - hw, h: hh },
        Snap::BottomLeft => Rect { x: work.x, y: y2, w: hw, h: work.h - hh },
        Snap::BottomRight => Rect { x: x2, y: y2, w: work.w - hw, h: work.h - hh },
    }
}

/// The same place on another screen: kept in proportion, and kept inside it.
pub fn move_to_screen(r: Rect, from: Rect, to: Rect) -> Rect {
    let fx = |v: i32, a: i32, b: i32| if b == 0 { 0.0 } else { f64::from(v - a) / f64::from(b) };
    let w = ((f64::from(r.w) / f64::from(from.w.max(1))) * f64::from(to.w)).round() as i32;
    let h = ((f64::from(r.h) / f64::from(from.h.max(1))) * f64::from(to.h)).round() as i32;
    let w = w.clamp(1, to.w.max(1));
    let h = h.clamp(1, to.h.max(1));
    let x = to.x + (fx(r.x, from.x, from.w) * f64::from(to.w)).round() as i32;
    let y = to.y + (fx(r.y, from.y, from.h) * f64::from(to.h)).round() as i32;
    Rect { x: x.clamp(to.x, to.x + to.w - w), y: y.clamp(to.y, to.y + to.h - h), w, h }
}

// ── Layouts ───────────────────────────────────────────────────────────────────

pub const MAX_LAYOUTS: usize = 20;
pub const MAX_LAYOUT_WINDOWS: usize = 16;
pub const MAX_NAME: usize = 40;

/// Where one program's window goes, in fractions of a screen's work area so a
/// layout survives a change of resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub app: String,
    pub screen: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    pub windows: Vec<Placement>,
}

pub fn placement(app: &str, screen: usize, r: Rect, work: Rect) -> Placement {
    let f = |v: i32, len: i32| if len <= 0 { 0.0 } else { (f64::from(v) / f64::from(len)).clamp(0.0, 1.0) };
    Placement {
        app: app.to_string(),
        screen,
        x: f(r.x - work.x, work.w),
        y: f(r.y - work.y, work.h),
        w: f(r.w, work.w),
        h: f(r.h, work.h),
    }
}

pub fn placed(p: &Placement, work: Rect) -> Rect {
    let px = |f: f64, len: i32| (f.clamp(0.0, 1.0) * f64::from(len)).round() as i32;
    Rect { x: work.x + px(p.x, work.w), y: work.y + px(p.y, work.h), w: px(p.w, work.w).max(1), h: px(p.h, work.h).max(1) }
}

/// A layout name as stored: trimmed, no control characters, at most MAX_NAME.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME).collect();
    (!name.is_empty()).then_some(name)
}

/// What a saved file may hold: valid names, a bounded count, and placements
/// within the screen. Anything else is dropped rather than trusted.
pub fn clean_layouts(list: Vec<Layout>) -> Vec<Layout> {
    let mut out: Vec<Layout> = Vec::new();
    for l in list {
        let Some(name) = clean_name(&l.name) else { continue };
        if out.iter().any(|o| o.name == name) || out.len() >= MAX_LAYOUTS {
            continue;
        }
        let windows = l
            .windows
            .into_iter()
            .filter(|p| !p.app.is_empty() && p.app.len() <= 260 && [p.x, p.y, p.w, p.h].iter().all(|v| v.is_finite()))
            .take(MAX_LAYOUT_WINDOWS)
            .collect();
        out.push(Layout { name, windows });
    }
    out
}

fn layouts_path() -> std::path::PathBuf {
    crate::settings::config_dir().join("shell-layouts.json")
}

pub fn load_layouts() -> Vec<Layout> {
    std::fs::read(layouts_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Layout>>(&b).ok())
        .map(clean_layouts)
        .unwrap_or_default()
}

pub fn save_layouts(list: &[Layout]) -> Result<(), String> {
    let path = layouts_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

// ── What the panel calls ──────────────────────────────────────────────────────

#[cfg(windows)]
pub fn list() -> Result<Vec<WindowInfo>, String> {
    Ok(super::win::list_windows())
}

#[cfg(windows)]
pub fn act(id: &str, action: Action) -> Result<(), String> {
    super::win::window_action(id, action)
}

/// Saves where the open windows are now, under `name` (replacing one of that name).
#[cfg(windows)]
pub fn save_current(name: &str) -> Result<Vec<Layout>, String> {
    let name = clean_name(name).ok_or("empty name")?;
    let windows = super::win::current_placements();
    let mut list = load_layouts();
    list.retain(|l| l.name != name);
    list.insert(0, Layout { name, windows: windows.into_iter().take(MAX_LAYOUT_WINDOWS).collect() });
    list.truncate(MAX_LAYOUTS);
    save_layouts(&list)?;
    Ok(list)
}

/// Puts the open windows back where `name` says. Programs that are not open
/// are left alone: a layout moves windows, it never starts anything.
#[cfg(windows)]
pub fn apply(name: &str) -> Result<usize, String> {
    let layout = load_layouts().into_iter().find(|l| l.name == name).ok_or("unknown layout")?;
    Ok(super::win::apply_placements(&layout.windows))
}

#[cfg(not(windows))]
pub fn list() -> Result<Vec<WindowInfo>, String> {
    Err(crate::i18n::t("Not available on this system."))
}

#[cfg(not(windows))]
pub fn act(_id: &str, _action: Action) -> Result<(), String> {
    Err(crate::i18n::t("Not available on this system."))
}

#[cfg(not(windows))]
pub fn save_current(_name: &str) -> Result<Vec<Layout>, String> {
    Err(crate::i18n::t("Not available on this system."))
}

#[cfg(not(windows))]
pub fn apply(_name: &str) -> Result<usize, String> {
    Err(crate::i18n::t("Not available on this system."))
}

pub fn delete(name: &str) -> Result<Vec<Layout>, String> {
    let mut list = load_layouts();
    list.retain(|l| l.name != name);
    save_layouts(&list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };

    #[test]
    fn halves_and_quarters_tile_the_work_area() {
        assert_eq!(snap_rect(WORK, Snap::Left), Rect { x: 0, y: 0, w: 960, h: 1040 });
        assert_eq!(snap_rect(WORK, Snap::Right), Rect { x: 960, y: 0, w: 960, h: 1040 });
        assert_eq!(snap_rect(WORK, Snap::BottomRight), Rect { x: 960, y: 520, w: 960, h: 520 });
        assert_eq!(snap_rect(WORK, Snap::Full), WORK);
        // An odd width: the right half takes the extra pixel, nothing overlaps.
        let odd = Rect { x: 100, y: 40, w: 1281, h: 721 };
        let (l, r) = (snap_rect(odd, Snap::Left), snap_rect(odd, Snap::Right));
        assert_eq!(l.x + l.w, r.x);
        assert_eq!(r.x + r.w, odd.x + odd.w);
        let c = snap_rect(WORK, Snap::Center);
        assert!(c.x > 0 && c.y > 0 && c.x + c.w < 1920 && c.y + c.h < 1040);
    }

    #[test]
    fn another_screen_keeps_the_proportions_and_stays_inside() {
        let laptop = Rect { x: 0, y: 0, w: 1920, h: 1080 };
        let big = Rect { x: 1920, y: -200, w: 2560, h: 1440 };
        let half = Rect { x: 0, y: 0, w: 960, h: 1080 };
        assert_eq!(move_to_screen(half, laptop, big), Rect { x: 1920, y: -200, w: 1280, h: 1440 });
        let off = Rect { x: 1800, y: 900, w: 600, h: 400 };
        let moved = move_to_screen(off, laptop, big);
        assert!(moved.x + moved.w <= big.x + big.w && moved.y + moved.h <= big.y + big.h);
    }

    #[test]
    fn layouts_are_stored_in_fractions_and_come_back_in_pixels() {
        let p = placement("Code", 0, Rect { x: 0, y: 0, w: 960, h: 1040 }, WORK);
        assert_eq!((p.x, p.y, p.w, p.h), (0.0, 0.0, 0.5, 1.0));
        let other = Rect { x: 0, y: 0, w: 2560, h: 1400 };
        assert_eq!(placed(&p, other), Rect { x: 0, y: 0, w: 1280, h: 1400 });
    }

    #[test]
    fn a_saved_file_is_never_trusted_as_is() {
        let p = |app: &str| Placement { app: app.into(), screen: 0, x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let mut many: Vec<Layout> = (0..30).map(|i| Layout { name: format!("L{i}"), windows: vec![p("a")] }).collect();
        many.insert(0, Layout { name: "  \u{7}  ".into(), windows: vec![] });
        many.insert(1, Layout { name: "Code".into(), windows: vec![p(""), Placement { x: f64::NAN, ..p("b") }, p("ok")] });
        many.insert(2, Layout { name: "Code".into(), windows: vec![] });
        let clean = clean_layouts(many);
        assert_eq!(clean.len(), MAX_LAYOUTS);
        assert_eq!(clean[0].name, "Code");
        assert_eq!(clean[0].windows.len(), 1, "empty app and NaN dropped");
        assert_eq!(clean_name(&"x".repeat(100)).unwrap().len(), MAX_NAME);
    }

    #[test]
    fn actions_read_as_the_panel_sends_them() {
        let a: Action = serde_json::from_str(r#"{"kind":"snap","to":"topLeft"}"#).unwrap();
        assert_eq!(a, Action::Snap { to: Snap::TopLeft });
        let a: Action = serde_json::from_str(r#"{"kind":"screen","index":1}"#).unwrap();
        assert_eq!(a, Action::Screen { index: 1 });
        assert!(serde_json::from_str::<Action>(r#"{"kind":"close"}"#).is_err(), "no close: nothing destructive");
    }
}
