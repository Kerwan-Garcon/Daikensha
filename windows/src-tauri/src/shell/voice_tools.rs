// « Coucou Shell »'s tools for « OK Coucou » (docs/WINDOWS_SHELL.md). Their
// risk level is fixed here, in this table, never by the model:
//   0 — reading: runs.
//   1 — reversible: runs, and the answer says what was done so Coucou says it aloud.
//   2 — confirmation: never runs. A card goes up in the island (shell/confirm.rs)
//       and the result tells Claude so, without ever saying it is done; only
//       the user's click on the card runs it. A spoken "yes" does not click.
// They work only while the panel is on in Settings. The phone gets none of them
// (claude_cli::Mode::Phone has no tool), and every name is in
// hook/src/tool_lists.rs, which feeds the four guards of the subscription mode.

// The pure parts serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use super::power::PowerAction;
use super::windows_mgr::{Snap, WindowInfo};

/// Every Shell tool and its level. A name missing here is not a Shell tool.
pub const LEVELS: &[(&str, u8)] = &[
    ("system_stats", 0),
    ("list_windows", 0),
    ("list_apps", 0),
    ("clipboard_recent", 0),
    ("set_volume", 1),
    ("toggle_wifi", 1),
    ("toggle_bluetooth", 1),
    ("focus_window", 1),
    ("snap_window", 1),
    ("apply_layout", 1),
    ("set_wallpaper", 1),
    ("web_open", 1),
    ("power_action", 2),
    ("type_text", 2),
];

pub fn level(name: &str) -> Option<u8> {
    LEVELS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// What a level 2 tool hands back to Claude: never that it is done.
pub const CARD_SHOWN: &str = "A confirmation card is now shown in the island. Nothing has happened yet: it happens only if the user clicks the card's button. Tell the user to check the card and click it; never say it is done, and a spoken yes does not confirm it.";

/// How many clipboard entries « OK Coucou » may read, and how much of each.
pub const CLIPBOARD_ENTRIES: usize = 5;
pub const CLIPBOARD_CHARS: usize = 500;

/// The tools' definitions, in the voice's (API) shape.
pub fn definitions() -> Vec<Value> {
    let none = json!({ "type": "object", "properties": {} });
    let window = json!({ "type": "string", "description": "The app or part of the window's title, as list_windows gives them." });
    vec![
        json!({ "name": "system_stats", "description": "Read the computer's CPU, memory, GPU and network use, and the programs using the most.", "input_schema": none }),
        json!({ "name": "list_windows", "description": "List the open windows: app, title and screen.", "input_schema": none }),
        json!({
            "name": "list_apps",
            "description": "Search the apps installed on this computer (Start menu and Store apps). To open one, use open_app.",
            "input_schema": { "type": "object", "properties": { "query": { "type": "string" } } }
        }),
        json!({
            "name": "clipboard_recent",
            "description": "Read the last few texts the user copied (only when they turned the clipboard history and voice access on in Settings).",
            "input_schema": none
        }),
        json!({
            "name": "set_volume",
            "description": "Set the computer's sound volume, or mute or unmute it.",
            "input_schema": { "type": "object", "properties": { "level": { "type": "integer", "description": "0-100" }, "mute": { "type": "boolean" } } }
        }),
        json!({
            "name": "toggle_wifi",
            "description": "Turn Wi-Fi on or off.",
            "input_schema": { "type": "object", "properties": { "on": { "type": "boolean" } }, "required": ["on"] }
        }),
        json!({
            "name": "toggle_bluetooth",
            "description": "Turn Bluetooth on or off.",
            "input_schema": { "type": "object", "properties": { "on": { "type": "boolean" } }, "required": ["on"] }
        }),
        json!({
            "name": "focus_window",
            "description": "Bring an open window to the front.",
            "input_schema": { "type": "object", "properties": { "window": window }, "required": ["window"] }
        }),
        json!({
            "name": "snap_window",
            "description": "Place an open window: left or right half, a quarter, full screen or centred.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "position": { "type": "string", "enum": ["left", "right", "full", "center", "topLeft", "topRight", "bottomLeft", "bottomRight"] }
                },
                "required": ["window", "position"]
            }
        }),
        json!({
            "name": "apply_layout",
            "description": "Put the windows back as in one of the user's saved layouts.",
            "input_schema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
        }),
        json!({
            "name": "set_wallpaper",
            "description": "Set one of the user's own wallpapers (the images they dropped in the panel). Without a number, a random one.",
            "input_schema": { "type": "object", "properties": { "number": { "type": "integer", "description": "1 for the first image, and so on." } } }
        }),
        json!({
            "name": "web_open",
            "description": "Open a search in the user's browser on one of these sites. Give the words to search, never a link.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "site": { "type": "string", "enum": ["google", "youtube", "wikipedia", "amazon", "maps"] },
                    "query": { "type": "string" }
                },
                "required": ["site", "query"]
            }
        }),
        json!({
            "name": "power_action",
            "description": "Ask to lock, sleep, sign out, restart or shut down the computer. This only shows a card the user must click.",
            "input_schema": {
                "type": "object",
                "properties": { "action": { "type": "string", "enum": ["lock", "sleep", "signOut", "restart", "shutDown"] } },
                "required": ["action"]
            }
        }),
        json!({
            "name": "type_text",
            "description": "Ask to type a text into the window in front (a message, a reply). This only shows a card the user must click; Enter is never pressed, the user sends it.",
            "input_schema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }
        }),
    ]
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// The window `query` names: the app's name first, then the title.
pub fn find_window<'a>(list: &'a [WindowInfo], query: &str) -> Option<&'a WindowInfo> {
    let q = fold(query.trim());
    if q.is_empty() {
        return None;
    }
    let score = |w: &WindowInfo| {
        let (app, title) = (fold(&w.app), fold(&w.title));
        if app == q {
            Some(0)
        } else if app.starts_with(&q) {
            Some(1)
        } else if app.contains(&q) {
            Some(2)
        } else if title.contains(&q) {
            Some(3)
        } else {
            None
        }
    };
    list.iter().filter_map(|w| score(w).map(|s| (s, w))).min_by_key(|(s, _)| *s).map(|(_, w)| w)
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The search URL Coucou builds for `site`; None for any other site. The
/// model only ever gives words.
pub fn search_url(site: &str, query: &str, lang: &str) -> Option<String> {
    let q: String = query.chars().filter(|c| !c.is_control()).take(200).collect();
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let lang = lang.split('-').next().unwrap_or("en");
    let e = encode(q);
    Some(match site {
        "google" => format!("https://www.google.com/search?q={e}"),
        "youtube" => format!("https://www.youtube.com/results?search_query={e}"),
        "wikipedia" => {
            let wiki = if lang.chars().all(|c| c.is_ascii_lowercase()) && (2..=3).contains(&lang.len()) { lang } else { "en" };
            format!("https://{wiki}.wikipedia.org/w/index.php?search={e}")
        }
        "amazon" => {
            let domain = match lang {
                "fr" => "amazon.fr",
                "es" => "amazon.es",
                "pt" => "amazon.com.br",
                "hi" => "amazon.in",
                _ => "amazon.com",
            };
            format!("https://www.{domain}/s?k={e}")
        }
        "maps" => format!("https://www.google.com/maps/search/{}", e.replace('+', "%20")),
        _ => return None,
    })
}

fn panel_on(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().unwrap().shell_panel)
}

fn clipboard_voice_on(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| {
        let s = s.settings.lock().unwrap();
        s.shell_panel && s.shell_clipboard && s.shell_clipboard_voice
    })
}

/// Puts a level 2 card up in the island.
fn card(app: &AppHandle, action: super::confirm::Action) -> (String, bool) {
    match super::confirm::prepare(action) {
        Ok(card) => {
            let _ = app.emit_to(crate::island::WINDOW_LABEL, "shell-confirm", &card);
            (CARD_SHOWN.into(), false)
        }
        Err(e) => (e, false),
    }
}

/// Runs a Shell tool: what goes back to Claude, and whether something changed.
/// None when `name` is not a Shell tool.
pub fn run(app: &AppHandle, name: &str, input: &Value) -> Option<(String, bool)> {
    level(name)?;
    if !panel_on(app) {
        return Some(("The Coucou Shell panel is off: tell the user to turn it on in Settings first.".into(), false));
    }
    let s = |v: &str| input[v].as_str().unwrap_or("").trim().to_string();
    let fail = |e: String| (e, false);
    Some(match name {
        "system_stats" => (stats_text(), false),
        "list_windows" => match super::windows_mgr::list() {
            Ok(list) if list.is_empty() => ("No window is open.".into(), false),
            Ok(list) => (list.iter().map(|w| format!("{} — {} (screen {})", w.app, w.title, w.screen + 1)).collect::<Vec<_>>().join("\n"), false),
            Err(e) => fail(e),
        },
        "list_apps" => match super::apps::list() {
            Ok(apps) => {
                let q = fold(&s("query"));
                let names: Vec<String> = apps.into_iter().map(|a| a.name).filter(|n| q.is_empty() || fold(n).contains(&q)).take(30).collect();
                if names.is_empty() {
                    ("No app matches.".into(), false)
                } else {
                    (names.join(", "), false)
                }
            }
            Err(e) => fail(e),
        },
        "clipboard_recent" => {
            if !clipboard_voice_on(app) {
                ("The clipboard history is not shared with the voice: the user can turn it on in Settings.".into(), false)
            } else {
                let recent = super::clipboard::history().recent(CLIPBOARD_ENTRIES, CLIPBOARD_CHARS);
                if recent.is_empty() {
                    ("Nothing copied yet.".into(), false)
                } else {
                    (recent.iter().enumerate().map(|(i, t)| format!("{}. {t}", i + 1)).collect::<Vec<_>>().join("\n"), false)
                }
            }
        }
        "set_volume" => {
            let mut said = Vec::new();
            if let Some(m) = input["mute"].as_bool() {
                if let Err(e) = super::controls::set_mute(m) {
                    return Some(fail(e));
                }
                said.push(if m { "Muted." } else { "Unmuted." }.to_string());
            }
            if let Some(l) = input["level"].as_f64() {
                let l = l.clamp(0.0, 100.0);
                if let Err(e) = super::controls::set_volume((l / 100.0) as f32) {
                    return Some(fail(e));
                }
                said.push(format!("Volume set to {} %.", l.round()));
            }
            if said.is_empty() {
                ("Say a level or mute.".into(), false)
            } else {
                (said.join(" "), true)
            }
        }
        "toggle_wifi" | "toggle_bluetooth" => {
            let on = input["on"].as_bool().unwrap_or(true);
            let (kind, label) = if name == "toggle_wifi" { ("wifi", "Wi-Fi") } else { ("bluetooth", "Bluetooth") };
            match super::controls::set_radio(kind, on) {
                Ok(()) => (format!("{label} turned {}.", if on { "on" } else { "off" }), true),
                Err(e) => fail(e),
            }
        }
        "focus_window" | "snap_window" => {
            let list = match super::windows_mgr::list() {
                Ok(l) => l,
                Err(e) => return Some(fail(e)),
            };
            let Some(w) = find_window(&list, &s("window")) else {
                return Some(("No open window matches: use list_windows.".into(), false));
            };
            let action = if name == "focus_window" {
                super::windows_mgr::Action::Focus
            } else {
                match serde_json::from_value::<Snap>(json!(s("position"))) {
                    Ok(to) => super::windows_mgr::Action::Snap { to },
                    Err(_) => return Some(("Unknown position.".into(), false)),
                }
            };
            match super::windows_mgr::act(&w.id, action) {
                Ok(()) if name == "focus_window" => (format!("{} is in front.", w.app), true),
                Ok(()) => (format!("{} placed: {}.", w.app, s("position")), true),
                Err(e) => fail(e),
            }
        }
        "apply_layout" => {
            let wanted = fold(&s("name"));
            let layouts = super::windows_mgr::load_layouts();
            let Some(l) = layouts.iter().find(|l| fold(&l.name) == wanted).or_else(|| layouts.iter().find(|l| fold(&l.name).contains(&wanted))) else {
                let names: Vec<&str> = layouts.iter().map(|l| l.name.as_str()).collect();
                return Some((format!("No layout by that name. Saved layouts: {}.", if names.is_empty() { "none".into() } else { names.join(", ") }), false));
            };
            match super::windows_mgr::apply(&l.name) {
                Ok(n) => (format!("Layout {} applied to {n} windows.", l.name), n > 0),
                Err(e) => fail(e),
            }
        }
        "set_wallpaper" => {
            let images = super::wallpaper::list();
            if images.is_empty() {
                return Some(("The user has no wallpapers yet: they drop images in the panel's Wallpapers tab.".into(), false));
            }
            let index = match input["number"].as_u64() {
                Some(n) if n >= 1 && (n as usize) <= images.len() => n as usize - 1,
                Some(_) => return Some((format!("There are {} images.", images.len()), false)),
                None => (super::clipboard::now_ms() as usize) % images.len(),
            };
            match super::wallpaper::apply(&images[index].id, None) {
                Ok(()) => (format!("Wallpaper {} of {} set.", index + 1, images.len()), true),
                Err(e) => fail(e),
            }
        }
        "web_open" => match search_url(&s("site"), &s("query"), crate::i18n::current()) {
            Some(url) => {
                crate::platform::open_url(&url);
                (format!("The browser opened a {} search for {}.", s("site"), s("query")), true)
            }
            None => ("That site is not one Coucou opens, or the search is empty.".into(), false),
        },
        "power_action" => match serde_json::from_value::<PowerAction>(json!(s("action"))) {
            Ok(a) => card(app, super::confirm::Action::Power(a)),
            Err(_) => ("Unknown action.".into(), false),
        },
        "type_text" => {
            let text = super::confirm::clean_typed(&s("text"));
            if text.is_empty() {
                return Some(("Nothing to type.".into(), false));
            }
            #[cfg(windows)]
            let front = super::win_desk::foreground();
            #[cfg(not(windows))]
            let front: Option<(isize, String)> = None;
            match front {
                Some((window, app_name)) => card(app, super::confirm::Action::Type { text, window, app: app_name }),
                None => ("No window to type into: the user should click in the app first.".into(), false),
            }
        }
        _ => return None,
    })
}

fn stats_text() -> String {
    let sampler = super::stats::Sampler::default();
    let _ = sampler.sample(true);
    std::thread::sleep(std::time::Duration::from_millis(600));
    let s = sampler.sample(true);
    let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
    let mut parts = vec![format!("CPU {:.0} %", s.cpu), format!("memory {:.1} of {:.1} GB", gb(s.mem_used), gb(s.mem_total))];
    if let Some(g) = s.gpu {
        parts.push(format!("GPU {g:.0} %"));
    }
    parts.push(format!("network down {:.0} KB/s, up {:.0} KB/s", s.net_rx as f64 / 1024.0, s.net_tx as f64 / 1024.0));
    if !s.top.is_empty() {
        parts.push(format!("busiest: {}", s.top.iter().map(|p| format!("{} {:.0} %", p.name, p.cpu)).collect::<Vec<_>>().join(", ")));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_fixed_level_and_a_schema() {
        let defs = definitions();
        assert_eq!(defs.len(), LEVELS.len());
        for d in &defs {
            let name = d["name"].as_str().unwrap();
            assert!(level(name).is_some(), "{name} has no level");
            assert!(d["input_schema"].is_object());
        }
        assert_eq!(level("power_action"), Some(2));
        assert_eq!(level("type_text"), Some(2));
        assert_eq!(level("bash"), None);
        // The only level 2 tools are the two that put a card up.
        let two: Vec<&str> = LEVELS.iter().filter(|(_, l)| *l == 2).map(|(n, _)| *n).collect();
        assert_eq!(two, ["power_action", "type_text"]);
    }

    #[test]
    fn a_card_never_says_it_is_done() {
        assert!(CARD_SHOWN.contains("Nothing has happened yet"));
        assert!(CARD_SHOWN.contains("only if the user clicks"));
    }

    #[test]
    fn the_search_url_is_built_by_coucou_on_a_known_site() {
        assert_eq!(search_url("youtube", "lofi & chill", "fr").unwrap(), "https://www.youtube.com/results?search_query=lofi+%26+chill");
        assert_eq!(search_url("amazon", "papier cadeau", "fr").unwrap(), "https://www.amazon.fr/s?k=papier+cadeau");
        assert_eq!(search_url("wikipedia", "Mochi", "pt-BR").unwrap(), "https://pt.wikipedia.org/w/index.php?search=Mochi");
        assert_eq!(search_url("maps", "Bordeaux gare", "en").unwrap(), "https://www.google.com/maps/search/Bordeaux%20gare");
        assert!(search_url("evil.example", "x", "en").is_none());
        assert!(search_url("google", "  ", "en").is_none());
        // Words cannot turn into another host or path.
        let u = search_url("google", "a/../@evil.com?x=1#y", "en").unwrap();
        assert!(u.starts_with("https://www.google.com/search?q="));
        assert!(!u[31..].contains(['/', '@', '?', '#']));
    }

    #[test]
    fn a_window_is_found_by_app_then_title() {
        let w = |app: &str, title: &str| WindowInfo { id: app.into(), title: title.into(), app: app.into(), screen: 0, minimized: false, focused: false };
        let list = [w("Code", "main.rs — Daikensha"), w("Discord", "#général"), w("Firefox", "Discord — web")];
        assert_eq!(find_window(&list, "discord").unwrap().app, "Discord");
        assert_eq!(find_window(&list, "daikensha").unwrap().app, "Code");
        assert_eq!(find_window(&list, "fire").unwrap().app, "Firefox");
        assert!(find_window(&list, "unity").is_none());
        assert!(find_window(&list, " ").is_none());
    }
}
