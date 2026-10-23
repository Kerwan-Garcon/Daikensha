// The Clipboard tab (docs/WINDOWS_SHELL.md): the last 25 texts copied, held in
// memory only — never written to disk, never logged. Off by default; while it
// is off nothing listens. On Windows the listener is a message-only window
// woken by WM_CLIPBOARDUPDATE (no polling), and it skips what an app marks as
// private (ExcludeClipboardContentFromMonitorProcessing,
// CanIncludeInClipboardHistory = 0) and what password managers copy.

// The history serves the Windows listener; elsewhere only the tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

use parking_lot::Mutex;
use serde::Serialize;

pub const MAX_ENTRIES: usize = 25;
/// Longer copies are kept cut to this many characters.
pub const MAX_CHARS: usize = 10_000;

/// Apps whose copies are never kept (lower-case executable names).
pub const EXCLUDED_APPS: &[&str] = &[
    "keepass.exe",
    "keepassxc.exe",
    "1password.exe",
    "bitwarden.exe",
    "lastpass.exe",
    "dashlane.exe",
    "enpass.exe",
    "nordpass.exe",
    "proton pass.exe",
    "protonpass.exe",
    "roboform.exe",
    "keeper.exe",
    "passwordsafe.exe",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    pub text: String,
    /// The app it was copied from ("Visual Studio Code"), when Windows says.
    pub app: String,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
}

#[derive(Default)]
pub struct History {
    entries: Vec<Entry>,
    next: u64,
}

pub fn excluded(exe: &str) -> bool {
    let exe = exe.to_lowercase();
    EXCLUDED_APPS.iter().any(|x| *x == exe)
}

impl History {
    /// Keeps `text` at the top. The same text again moves up instead of
    /// showing twice; blank copies are ignored.
    pub fn push(&mut self, text: &str, app: &str, at: u64) {
        if text.trim().is_empty() {
            return;
        }
        let text: String = text.chars().take(MAX_CHARS).collect();
        self.entries.retain(|e| e.text != text);
        self.next += 1;
        self.entries.insert(0, Entry { id: self.next, text, app: app.to_string(), at });
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn list(&self) -> Vec<Entry> {
        self.entries.clone()
    }

    #[cfg(test)]
    pub fn get(&self, id: u64) -> Option<String> {
        self.entries.iter().find(|e| e.id == id).map(|e| e.text.clone())
    }

    /// The `n` newest texts cut to `chars` characters (« OK Coucou » reads no more).
    pub fn recent(&self, n: usize, chars: usize) -> Vec<String> {
        self.entries.iter().take(n).map(|e| e.text.chars().take(chars).collect()).collect()
    }

    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

static HISTORY: Mutex<History> = Mutex::new(History { entries: Vec::new(), next: 0 });

pub fn history() -> parking_lot::MutexGuard<'static, History> {
    HISTORY.lock()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Starts or stops listening as the settings say. Turning it off forgets everything.
pub fn sync(app: &tauri::AppHandle, on: bool) {
    #[cfg(windows)]
    {
        use tauri::Emitter;
        let app = app.clone();
        super::win_desk::clipboard_listen(on, move || {
            let _ = app.emit("shell-clipboard", ());
        });
    }
    #[cfg(not(windows))]
    let _ = app;
    if !on {
        history().clear();
    }
}

/// Puts an entry back on the clipboard.
pub fn copy(id: u64) -> Result<(), String> {
    let entry = history().list().into_iter().find(|e| e.id == id).ok_or_else(|| crate::i18n::t("That entry is gone."))?;
    let mut board = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    board.set_text(entry.text.clone()).map_err(|e| e.to_string())?;
    // The listener skips Coucou's own copies: move it up here, from the app it came from.
    history().push(&entry.text, &entry.app, now_ms());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_is_bounded_newest_first_and_without_repeats() {
        let mut h = History::default();
        for i in 0..30 {
            h.push(&format!("copy {i}"), "Notepad", i);
        }
        let list = h.list();
        assert_eq!(list.len(), MAX_ENTRIES);
        assert_eq!(list[0].text, "copy 29");
        h.push("copy 10", "Edge", 99);
        assert_eq!(h.list()[0].text, "copy 10");
        assert_eq!(h.list().iter().filter(|e| e.text == "copy 10").count(), 1);
        h.push("   \n", "Edge", 100);
        assert_eq!(h.list()[0].text, "copy 10");
    }

    #[test]
    fn long_copies_are_cut_and_entries_can_go() {
        let mut h = History::default();
        h.push(&"x".repeat(MAX_CHARS + 50), "", 0);
        assert_eq!(h.list()[0].text.chars().count(), MAX_CHARS);
        let id = h.list()[0].id;
        assert!(h.get(id).is_some());
        h.remove(id);
        assert!(h.get(id).is_none());
        h.push("a", "", 0);
        h.clear();
        assert!(h.list().is_empty());
    }

    #[test]
    fn password_managers_are_never_kept() {
        assert!(excluded("KeePassXC.exe"));
        assert!(excluded("1Password.exe"));
        assert!(!excluded("notepad.exe"));
    }
}
