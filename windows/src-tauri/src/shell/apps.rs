// The Apps tab (docs/WINDOWS_SHELL.md): the apps of the Start menu and the
// Store, as shell:AppsFolder lists them, and launching one. An app is only
// ever launched by an id this list gave: never a path, never a command line.

// The pure helpers here serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
    /// shell:AppsFolder's parsing name: an AppUserModelID or a known-folder path.
    pub id: String,
    pub name: String,
}

/// The last list, the only ids `launch` accepts.
static LISTED: Mutex<Vec<App>> = Mutex::new(Vec::new());

/// Sorted by name, without duplicates or entries with no name or id.
pub fn tidy(mut apps: Vec<App>) -> Vec<App> {
    apps.retain(|a| !a.id.trim().is_empty() && !a.name.trim().is_empty() && !a.id.chars().any(char::is_control));
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

#[cfg(windows)]
pub fn list() -> Result<Vec<App>, String> {
    let apps = tidy(super::win::list_apps()?);
    (*LISTED.lock()).clone_from(&apps);
    Ok(apps)
}

#[cfg(not(windows))]
pub fn list() -> Result<Vec<App>, String> {
    Err(crate::i18n::t("Not available on this system."))
}

/// True when `id` is one the last list gave.
pub fn known(id: &str) -> bool {
    LISTED.lock().iter().any(|a| a.id == id)
}

pub fn launch(id: &str) -> Result<(), String> {
    if !known(id) {
        return Err("unknown app".into());
    }
    #[cfg(windows)]
    return super::win::launch_app(id);
    #[cfg(not(windows))]
    Err(crate::i18n::t("Not available on this system."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into() }
    }

    #[test]
    fn the_list_is_sorted_and_cleaned() {
        let apps = tidy(vec![app("b", "spotify"), app("a", "Edge"), app("", "Ghost"), app("c", "  "), app("a", "Edge"), app("x\ny", "Bad")]);
        assert_eq!(apps, vec![app("a", "Edge"), app("b", "spotify")]);
    }

    #[test]
    fn only_a_listed_id_may_be_launched() {
        *LISTED.lock() = vec![app("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App", "Calculator")];
        assert!(known("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!known("C:\\Windows\\System32\\cmd.exe"));
        assert!(launch("C:\\Windows\\System32\\cmd.exe").is_err());
    }
}
