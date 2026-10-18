// The Power tab (docs/WINDOWS_SHELL.md): lock, sleep, sign out, restart, shut
// down. Every one of them is level 2: asking for it only puts a confirmation
// card up (`prepare`), and only the click on that card's button runs it
// (`confirm`, with the card's one-time token). Windows is never forced: apps
// with unsaved work still get to ask.

// The pure helpers serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long a card's token stays good.
const CARD_VALID_FOR: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerAction {
    Lock,
    Sleep,
    SignOut,
    Restart,
    ShutDown,
}

/// What the confirmation card shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub token: String,
    pub action: PowerAction,
    /// Apps with a window open, which may hold unsaved work.
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0–100; None on a desktop without a battery.
    pub percent: Option<u8>,
    pub plugged_in: bool,
}

struct Pending {
    token: String,
    action: PowerAction,
    at: Instant,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

fn token() -> Result<String, String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| "no secure random source".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
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

/// Puts a card up for `action`; nothing runs. A newer card replaces an older one.
pub fn prepare(action: PowerAction) -> Result<Card, String> {
    let token = token()?;
    #[cfg(windows)]
    let apps = open_apps(&super::windows_mgr::list().unwrap_or_default().into_iter().map(|w| w.app).collect::<Vec<_>>(), 6);
    #[cfg(not(windows))]
    let apps = Vec::new();
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending { token: token.clone(), action, at: Instant::now() });
    Ok(Card { token, action, apps })
}

/// The card's action, once, if `token` is the card's and still fresh.
pub fn take(token: &str) -> Result<PowerAction, String> {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    match pending.take() {
        Some(p) if p.token == token && p.at.elapsed() < CARD_VALID_FOR => Ok(p.action),
        _ => Err(crate::i18n::t("This card has expired. Ask again.")),
    }
}

pub fn cancel() {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Runs the card's action: only the card's button calls this.
pub fn confirm(token: &str) -> Result<(), String> {
    let action = take(token)?;
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
    fn only_the_cards_own_token_runs_it_and_only_once() {
        let card = prepare(PowerAction::ShutDown).unwrap();
        assert_eq!(card.token.len(), 32);
        assert!(take("not-the-token").is_err());
        // A wrong guess spends the card.
        assert!(take(&card.token).is_err());
        let card = prepare(PowerAction::Lock).unwrap();
        assert_eq!(take(&card.token).unwrap(), PowerAction::Lock);
        assert!(take(&card.token).is_err());
        let old = prepare(PowerAction::Restart).unwrap();
        let new = prepare(PowerAction::Sleep).unwrap();
        assert!(take(&old.token).is_err());
        let _ = new;
        let card = prepare(PowerAction::SignOut).unwrap();
        cancel();
        assert!(take(&card.token).is_err());
    }

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
