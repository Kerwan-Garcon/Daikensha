// « Coucou Shell » (docs/WINDOWS_SHELL.md): the island's fold-down panel and
// what its tabs read and change on the computer. Off until Settings turns it
// on; every command refuses while it is off. Nothing here runs in the
// background: each tab asks while it is on screen.

pub mod appbar;
pub mod apps;
pub mod capture;
pub mod clipboard;
pub mod confirm;
pub mod controls;
pub mod power;
pub mod routines;
pub mod stats;
pub mod today;
pub mod voice_tools;
pub mod wallpaper;
pub mod windows_mgr;
#[cfg(windows)]
pub(crate) mod win;
#[cfg(windows)]
mod win_desk;

use std::sync::OnceLock;

use serde::Serialize;
use tauri::State;

use crate::Shared;

fn sampler() -> &'static stats::Sampler {
    static SAMPLER: OnceLock<stats::Sampler> = OnceLock::new();
    SAMPLER.get_or_init(stats::Sampler::default)
}

fn enabled(shared: &State<'_, Shared>) -> Result<(), String> {
    if shared.settings.lock().shell_panel {
        Ok(())
    } else {
        Err(crate::i18n::t("Turn on the panel in Settings first."))
    }
}

/// Runs `f` off the async runtime: these calls enumerate processes, windows or
/// the Start menu, or wait on COM.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Which tabs this system can fill.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    today: bool,
    stats: bool,
    controls: bool,
    apps: bool,
    windows: bool,
    clipboard: bool,
    wallpapers: bool,
    power: bool,
}

#[tauri::command]
pub fn shell_capabilities() -> Capabilities {
    let win = cfg!(windows);
    Capabilities { today: true, stats: true, controls: win, apps: win, windows: win, clipboard: win, wallpapers: win, power: win }
}

/// Settings were saved, or the app started: the clipboard listens only while
/// both the panel and its history are on.
pub fn settings_saved(app: &tauri::AppHandle, settings: &crate::settings::Settings) {
    clipboard::sync(app, settings.shell_panel && settings.shell_clipboard);
    appbar::sync(settings);
}

/// The emergency shortcut, or the island's "show the taskbar" button: the
/// taskbar comes back at once, and the « top bar » mode goes off.
#[tauri::command]
pub fn shell_top_bar_off(app: tauri::AppHandle, shared: State<'_, Shared>) {
    appbar::restore();
    let settings = {
        let mut s = shared.settings.lock();
        s.shell_top_bar = false;
        s.clone()
    };
    if let Err(err) = crate::settings::save(&settings) {
        crate::log::line(format!("could not save settings: {err}"));
    }
    settings_saved(&app, &settings);
    crate::shortcuts::apply(&app, &appbar::bindings(&settings));
    use tauri::Emitter;
    let _ = app.emit("settings-changed", settings);
}

#[tauri::command]
pub async fn shell_stats(shared: State<'_, Shared>, top: bool) -> Result<stats::Sample, String> {
    enabled(&shared)?;
    blocking(move || Ok(sampler().sample(top))).await
}

/// The Stats tab closed: the next sample starts from scratch.
#[tauri::command]
pub fn shell_stats_reset() {
    sampler().reset();
}

#[tauri::command]
pub async fn shell_controls(shared: State<'_, Shared>) -> Result<controls::Controls, String> {
    enabled(&shared)?;
    blocking(controls::read).await
}

#[tauri::command]
pub async fn shell_set_volume(shared: State<'_, Shared>, level: f32) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || controls::set_volume(level)).await
}

#[tauri::command]
pub async fn shell_set_mute(shared: State<'_, Shared>, muted: bool) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || controls::set_mute(muted)).await
}

#[tauri::command]
pub async fn shell_set_radio(shared: State<'_, Shared>, kind: String, on: bool) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || controls::set_radio(&kind, on)).await
}

#[tauri::command]
pub async fn shell_set_brightness(shared: State<'_, Shared>, index: usize, value: u32) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || controls::set_brightness(index, value)).await
}

#[tauri::command]
pub async fn shell_open_settings(shared: State<'_, Shared>, page: controls::SettingsPage) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || controls::open_settings(page)).await
}

#[tauri::command]
pub async fn shell_windows(shared: State<'_, Shared>) -> Result<Vec<windows_mgr::WindowInfo>, String> {
    enabled(&shared)?;
    blocking(windows_mgr::list).await
}

#[tauri::command]
pub async fn shell_window_action(shared: State<'_, Shared>, id: String, action: windows_mgr::Action) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || windows_mgr::act(&id, action)).await
}

#[tauri::command]
pub fn shell_layouts(shared: State<'_, Shared>) -> Result<Vec<windows_mgr::Layout>, String> {
    enabled(&shared)?;
    Ok(windows_mgr::load_layouts())
}

#[tauri::command]
pub async fn shell_layout_save(shared: State<'_, Shared>, name: String) -> Result<Vec<windows_mgr::Layout>, String> {
    enabled(&shared)?;
    blocking(move || windows_mgr::save_current(&name)).await
}

#[tauri::command]
pub async fn shell_layout_apply(shared: State<'_, Shared>, name: String) -> Result<usize, String> {
    enabled(&shared)?;
    blocking(move || windows_mgr::apply(&name)).await
}

#[tauri::command]
pub fn shell_layout_delete(shared: State<'_, Shared>, name: String) -> Result<Vec<windows_mgr::Layout>, String> {
    enabled(&shared)?;
    windows_mgr::delete(&name)
}

#[tauri::command]
pub async fn shell_apps(shared: State<'_, Shared>) -> Result<Vec<apps::App>, String> {
    enabled(&shared)?;
    blocking(apps::list).await
}

#[tauri::command]
pub async fn shell_app_launch(shared: State<'_, Shared>, id: String) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || apps::launch(&id)).await
}

// ── Today ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn shell_todos(shared: State<'_, Shared>) -> Result<Vec<today::Todo>, String> {
    enabled(&shared)?;
    Ok(today::load_todos())
}

#[tauri::command]
pub fn shell_todo_change(shared: State<'_, Shared>, change: today::TodoChange) -> Result<Vec<today::Todo>, String> {
    enabled(&shared)?;
    today::change_todos(change)
}

/// The weather for the city in Settings; None when there is none (and then no call is made).
#[tauri::command]
pub async fn shell_weather(shared: State<'_, Shared>) -> Result<Option<today::Weather>, String> {
    enabled(&shared)?;
    let city = shared.settings.lock().shell_weather_city.clone();
    today::weather(&city).await
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

fn clipboard_on(shared: &State<'_, Shared>) -> Result<(), String> {
    enabled(shared)?;
    if shared.settings.lock().shell_clipboard {
        Ok(())
    } else {
        Err(crate::i18n::t("Turn on the clipboard history in Settings first."))
    }
}

#[tauri::command]
pub fn shell_clipboard(shared: State<'_, Shared>) -> Result<Vec<clipboard::Entry>, String> {
    clipboard_on(&shared)?;
    Ok(clipboard::history().list())
}

#[tauri::command]
pub async fn shell_clipboard_copy(shared: State<'_, Shared>, id: u64) -> Result<(), String> {
    clipboard_on(&shared)?;
    blocking(move || clipboard::copy(id)).await
}

#[tauri::command]
pub fn shell_clipboard_remove(shared: State<'_, Shared>, id: u64) -> Result<Vec<clipboard::Entry>, String> {
    clipboard_on(&shared)?;
    let mut h = clipboard::history();
    h.remove(id);
    Ok(h.list())
}

#[tauri::command]
pub fn shell_clipboard_clear() {
    clipboard::history().clear();
}

// ── Wallpapers ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpapers {
    images: Vec<wallpaper::Wallpaper>,
    screens: usize,
    /// Dropped files that were not taken (not an image, too large, the selection full).
    refused: usize,
}

#[tauri::command]
pub async fn shell_wallpapers(shared: State<'_, Shared>) -> Result<Wallpapers, String> {
    enabled(&shared)?;
    blocking(|| Ok(Wallpapers { images: wallpaper::list(), screens: wallpaper::screens(), refused: 0 })).await
}

#[tauri::command]
pub async fn shell_wallpaper_add(shared: State<'_, Shared>, paths: Vec<String>) -> Result<Wallpapers, String> {
    enabled(&shared)?;
    blocking(move || {
        let (images, refused) = wallpaper::add(&paths)?;
        Ok(Wallpapers { images, screens: wallpaper::screens(), refused })
    })
    .await
}

#[tauri::command]
pub async fn shell_wallpaper_delete(shared: State<'_, Shared>, id: String) -> Result<Wallpapers, String> {
    enabled(&shared)?;
    blocking(move || Ok(Wallpapers { images: wallpaper::delete(&id)?, screens: wallpaper::screens(), refused: 0 })).await
}

#[tauri::command]
pub async fn shell_wallpaper_apply(shared: State<'_, Shared>, id: String, screen: Option<usize>) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || wallpaper::apply(&id, screen)).await
}

// ── Power ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn shell_battery(shared: State<'_, Shared>) -> Result<power::Battery, String> {
    enabled(&shared)?;
    Ok(power::battery())
}

/// Puts a confirmation card up; nothing runs until its button is clicked.
#[tauri::command]
pub async fn shell_power_prepare(shared: State<'_, Shared>, action: power::PowerAction) -> Result<confirm::Card, String> {
    enabled(&shared)?;
    blocking(move || confirm::prepare(confirm::Action::Power(action))).await
}

/// A level 2 card's button (the Power tab's, or the one « OK Coucou » put up).
#[tauri::command]
pub async fn shell_confirm(shared: State<'_, Shared>, token: String) -> Result<(), String> {
    enabled(&shared)?;
    blocking(move || confirm::confirm(&token)).await
}

#[tauri::command]
pub fn shell_confirm_cancel() {
    confirm::cancel();
}

// ── Routines ──────────────────────────────────────────────────────────────────

/// A routine as Settings and the Today tab show it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineView {
    id: u64,
    name: String,
    when: routines::When,
    steps: Vec<String>,
    /// The next run, in local minutes since 1970-01-01.
    next: Option<i64>,
}

fn routine_views() -> Vec<RoutineView> {
    let now = routines::now_minutes();
    routines::list()
        .into_iter()
        .map(|r| RoutineView {
            id: r.id,
            next: routines::next_due(&r.when, now),
            steps: r.steps.iter().map(routines::step_summary).collect(),
            name: r.name,
            when: r.when,
        })
        .collect()
}

/// Listed and deleted even while the panel is off: they are the user's.
#[tauri::command]
pub fn shell_routines() -> Vec<RoutineView> {
    routine_views()
}

#[tauri::command]
pub fn shell_routine_delete(id: u64) -> Result<Vec<RoutineView>, String> {
    routines::delete(id)?;
    Ok(routine_views())
}
