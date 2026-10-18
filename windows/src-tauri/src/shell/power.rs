// The Power tab (docs/WINDOWS_SHELL.md): lock, sleep, sign out, restart, shut
// down. Every one of them is level 2: asking for it, from the tab or by voice,
// only puts a confirmation card up (shell/confirm.rs), and only the click on
// that card's button runs it. Windows is never forced: apps with unsaved work
// still get to ask.

// The pure helpers serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerAction {
    Lock,
    Sleep,
    SignOut,
    Restart,
    ShutDown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0–100; None on a desktop without a battery.
    pub percent: Option<u8>,
    pub plugged_in: bool,
}

/// The distinct app names among `windows`, in order, at most `max`.
pub fn open_apps(windows: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in windows {
        if !w.is_empty() && !out.contains(w) {
            out.push(w.clone());
        }
    }
    out.truncate(max);
    out
}

/// Apps with a window open, which may hold unsaved work (shown on the card).
pub fn apps_open() -> Vec<String> {
    #[cfg(windows)]
    return open_apps(&super::windows_mgr::list().unwrap_or_default().into_iter().map(|w| w.app).collect::<Vec<_>>(), 6);
    #[cfg(not(windows))]
    Vec::new()
}

/// Runs `action`: only a confirmation card's button gets here (shell/confirm.rs).
pub fn run(action: PowerAction) -> Result<(), String> {
    #[cfg(windows)]
    return super::win_desk::power(action);
    #[cfg(not(windows))]
    {
        let _ = action;
        Err(crate::i18n::t("Not available on this system."))
    }
}

pub fn battery() -> Battery {
    #[cfg(windows)]
    return super::win_desk::battery();
    #[cfg(not(windows))]
    Battery::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cards_apps_are_distinct_and_few() {
        let w = ["Code", "Edge", "Code", "", "Unity"].map(String::from);
        assert_eq!(open_apps(&w, 2), ["Code", "Edge"]);
    }

    #[test]
    fn actions_come_from_a_fixed_list() {
        assert_eq!(serde_json::from_str::<PowerAction>("\"shutDown\"").unwrap(), PowerAction::ShutDown);
        assert!(serde_json::from_str::<PowerAction>("\"format\"").is_err());
    }
}
