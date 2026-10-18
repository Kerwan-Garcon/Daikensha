// Level 2 (docs/WINDOWS_SHELL.md): what Coucou never does on its own. Asking
// for it — from the Power tab or by voice — only puts a card up, with a
// one-time token; the action exists nowhere else. Only the card's button,
// clicked by the user, runs it. One card at a time: a newer one replaces the
// older, a wrong token spends it, and a card goes stale after 90 seconds.

// The pure parts serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::power::PowerAction;

const CARD_VALID_FOR: Duration = Duration::from_secs(90);
/// The most `type_text` types in one card.
pub const MAX_TYPED_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Power(PowerAction),
    /// Text typed into the window that was in front when the card went up.
    Type { text: String, window: isize, app: String },
}

/// What the card shows.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CardBody {
    Power { action: PowerAction, apps: Vec<String> },
    Type { text: String, app: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub token: String,
    #[serde(flatten)]
    pub body: CardBody,
}

struct Pending {
    token: String,
    action: Action,
    at: Instant,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

fn token() -> Result<String, String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| "no secure random source".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Text as `type_text` may type it: no control characters (a line break
/// becomes a space, so Enter is never pressed), cut to MAX_TYPED_CHARS.
pub fn clean_typed(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(MAX_TYPED_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Puts a card up for `action`; nothing runs.
pub fn prepare(action: Action) -> Result<Card, String> {
    let token = token()?;
    let body = match &action {
        Action::Power(a) => CardBody::Power { action: *a, apps: super::power::apps_open() },
        Action::Type { text, app, .. } => CardBody::Type { text: text.clone(), app: app.clone() },
    };
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending { token: token.clone(), action, at: Instant::now() });
    Ok(Card { token, body })
}

/// The card's action, once, if `token` is the card's and still fresh.
pub fn take(token: &str) -> Result<Action, String> {
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
    match take(token)? {
        Action::Power(a) => super::power::run(a),
        Action::Type { text, window, .. } => type_into(window, &text),
    }
}

fn type_into(window: isize, text: &str) -> Result<(), String> {
    #[cfg(windows)]
    return super::win_desk::type_text(window, text);
    #[cfg(not(windows))]
    {
        let _ = (window, text);
        Err(crate::i18n::t("Not available on this system."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_cards_own_token_runs_it_and_only_once() {
        let card = prepare(Action::Power(PowerAction::ShutDown)).unwrap();
        assert_eq!(card.token.len(), 32);
        // A wrong guess spends the card.
        assert!(take("not-the-token").is_err());
        assert!(take(&card.token).is_err());
        let card = prepare(Action::Power(PowerAction::Lock)).unwrap();
        assert_eq!(take(&card.token).unwrap(), Action::Power(PowerAction::Lock));
        assert!(take(&card.token).is_err());
        // A newer card replaces the older.
        let old = prepare(Action::Power(PowerAction::Restart)).unwrap();
        let new = prepare(Action::Type { text: "hi".into(), window: 1, app: "Notepad".into() }).unwrap();
        assert!(take(&old.token).is_err());
        assert!(take(&new.token).is_err(), "spent by the wrong guess");
        let card = prepare(Action::Power(PowerAction::SignOut)).unwrap();
        cancel();
        assert!(take(&card.token).is_err());
    }

    #[test]
    fn typed_text_never_holds_enter_or_control_keys() {
        assert_eq!(clean_typed("Merci pour hier,\r\non se voit jeudi !\n"), "Merci pour hier,  on se voit jeudi !");
        assert_eq!(clean_typed("a\u{1b}[31mb\u{7}\u{0}c"), "a[31mbc");
        assert_eq!(clean_typed(&"x".repeat(5000)).chars().count(), MAX_TYPED_CHARS);
        assert_eq!(clean_typed("\n\n"), "");
    }

    #[test]
    fn the_card_says_what_it_does_and_nothing_else() {
        let card = Card { token: "t".into(), body: CardBody::Type { text: "hi".into(), app: "Discord".into() } };
        let v = serde_json::to_value(&card).unwrap();
        assert_eq!(v, serde_json::json!({ "token": "t", "kind": "type", "text": "hi", "app": "Discord" }));
        let card = Card { token: "t".into(), body: CardBody::Power { action: PowerAction::ShutDown, apps: vec![] } };
        assert_eq!(serde_json::to_value(&card).unwrap()["action"], "shutDown");
    }
}
