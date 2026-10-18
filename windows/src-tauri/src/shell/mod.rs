// « Coucou Shell » (docs/WINDOWS_SHELL.md): the island's fold-down panel and
// what its tabs read and change on the computer. Off until Settings turns it
// on; every command refuses while it is off. Nothing here runs in the
// background: each tab asks while it is on screen.

pub mod apps;
pub mod controls;
pub mod stats;
pub mod windows_mgr;
#[cfg(windows)]
mod win;

use std::sync::OnceLock;

use serde::Serialize;
use tauri::State;

use crate::Shared;

fn sampler() -> &'static stats::Sampler {
    static SAMPLER: OnceLock<stats::Sampler> = OnceLock::new();
    SAMPLER.get_or_init(stats::Sampler::default)
}

fn enabled(shared: &State<'_, Shared>) -> Result<(), String> {
    if shared.settings.lock().unwrap().shell_panel {
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
    stats: bool,
    controls: bool,
    apps: bool,
    windows: bool,
}

#[tauri::command]
pub fn shell_capabilities() -> Capabilities {
    let win = cfg!(windows);
    Capabilities { stats: true, controls: win, apps: win, windows: win }
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
