// The « top bar » mode (docs/WINDOWS_SHELL.md), off by default: Coucou as the
// main bar, Windows' taskbar set to hide itself. Only the documented API is
// used — SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE), the same switch as
// Settings → Personalisation → Taskbar — never Shell_TrayWnd itself, never
// Explorer's registry.
//
// The taskbar's own state is written to a file *before* anything changes, and
// put back when the mode is turned off, when Coucou quits, and on the next
// start if Coucou crashed with it on. A global shortcut (Ctrl+Alt+B, only
// registered while the mode is on) brings the taskbar back at once.
//
// "Reserve the top of the screen": the island's band is registered as an
// AppBar (ABM_NEW / ABM_SETPOS) so maximised windows stop below it; it is
// placed again when the screens change and always removed (ABM_REMOVE) when
// the option goes or Coucou quits.

// The pure parts serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// The emergency shortcut's action id (shortcuts.rs).
pub const RESCUE: &str = "taskbarRescue";

/// What the taskbar was before Coucou touched it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub auto_hide: bool,
}

fn state_path() -> std::path::PathBuf {
    crate::settings::config_dir().join("taskbar-state.json")
}

pub fn read_saved() -> Option<Saved> {
    std::fs::read(state_path()).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_saved(s: Saved) -> Result<(), String> {
    let p = state_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::to_vec(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// The mode is on: the panel and its "top bar" option.
pub fn wanted(s: &Settings) -> bool {
    cfg!(windows) && s.shell_panel && s.shell_top_bar
}

/// The shortcuts to register: the emergency one is on while the mode is,
/// unless the user turned it off or moved it in Settings.
pub fn bindings(s: &Settings) -> crate::shortcuts::Bindings {
    let mut out = s.shortcuts.clone();
    if wanted(s) && !out.contains_key(RESCUE) {
        if let Some(def) = crate::shortcuts::find(RESCUE) {
            out.insert(RESCUE.into(), crate::shortcuts::Binding { keys: def.default_keys.into(), enabled: true });
        }
    }
    out
}

/// The auto-hide bit to set: on in the mode, the saved one otherwise.
pub fn target_auto_hide(on: bool, saved: Option<Saved>, current: bool) -> bool {
    if on {
        true
    } else {
        saved.map_or(current, |s| s.auto_hide)
    }
}

/// Settings were saved or the app started: the taskbar and the reserved band follow.
pub fn sync(s: &Settings) {
    #[cfg(windows)]
    {
        let on = wanted(s);
        if on {
            if let Err(e) = hide_taskbar() {
                crate::log::line(format!("top bar: {e}"));
            }
        } else {
            restore();
        }
        win::reserve(on && s.shell_reserve_top);
    }
    #[cfg(not(windows))]
    let _ = s;
}

/// At start: a state file left behind means Coucou did not get to put the
/// taskbar back (a crash). Put it back now; sync() hides it again if the mode is on.
pub fn recover() {
    if read_saved().is_some() {
        crate::log::line("top bar: the taskbar's state was left behind; putting it back");
        restore();
    }
}

/// Coucou quits: the taskbar as it was, and no band reserved.
pub fn on_exit() {
    restore();
    #[cfg(windows)]
    win::reserve(false);
}

#[cfg(windows)]
fn hide_taskbar() -> Result<(), String> {
    let current = win::auto_hide();
    // Written before the change, and only the first time: what the user had.
    if read_saved().is_none() {
        write_saved(Saved { auto_hide: current })?;
    }
    if !current {
        win::set_auto_hide(true)?;
    }
    Ok(())
}

/// Puts the taskbar back as the state file says, then forgets the file.
pub fn restore() {
    let Some(saved) = read_saved() else { return };
    #[cfg(windows)]
    {
        let target = target_auto_hide(false, Some(saved), win::auto_hide());
        if let Err(e) = win::set_auto_hide(target) {
            crate::log::line(format!("top bar: could not put the taskbar back: {e}"));
            return;
        }
    }
    let _ = saved;
    let _ = std::fs::remove_file(state_path());
}

#[cfg(windows)]
#[path = "appbar_win.rs"]
mod win;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_taskbar_goes_back_to_what_the_user_had() {
        assert!(target_auto_hide(true, None, false));
        assert!(!target_auto_hide(false, Some(Saved { auto_hide: false }), true));
        assert!(target_auto_hide(false, Some(Saved { auto_hide: true }), false), "it was hidden before Coucou: it stays so");
        assert!(target_auto_hide(false, None, true), "nothing saved: left alone");
    }

    #[test]
    fn the_rescue_shortcut_is_there_only_with_the_mode() {
        let mut s = Settings::default();
        assert!(!bindings(&s).contains_key(RESCUE));
        s.shell_panel = true;
        s.shell_top_bar = true;
        assert_eq!(bindings(&s).contains_key(RESCUE), cfg!(windows));
        // The user's own choice for it wins.
        s.shortcuts.insert(RESCUE.into(), crate::shortcuts::Binding { keys: String::new(), enabled: false });
        assert!(!bindings(&s)[RESCUE].enabled);
    }

    #[test]
    fn the_saved_state_is_a_small_json() {
        let v = serde_json::to_string(&Saved { auto_hide: false }).unwrap();
        assert_eq!(v, r#"{"autoHide":false}"#);
    }
}
