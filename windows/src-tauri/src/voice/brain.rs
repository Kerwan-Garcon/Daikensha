// « OK Coucou »'s brain: Claude with the user's key and Coucou's tools —
// pills, music, the user's own data, opening an app, preparing an email (the
// mail app opens with it filled in; the user sends it) and Anthropic's web
// search. The conversation lives in memory only and is forgotten 90 s after
// the last turn. Port of ClaudeVoiceBrain.swift. With the Claude subscription
// on, the same prompt and tools go through the user's Claude Code instead
// (claude_cli/voice.rs), and never to the API.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const FAST_MODEL: &str = "claude-haiku-4-5";
const FORGET_AFTER: Duration = Duration::from_secs(90);

pub struct Brain {
    messages: Vec<Value>,
    last: Instant,
    /// The subscription's kept `claude` process and its conversation.
    cli: crate::claude_cli::voice::Voice,
    /// Why the last turn had no answer, when the subscription says (shown in the pill).
    error: Option<String>,
}

impl Default for Brain {
    fn default() -> Self {
        Self { messages: Vec::new(), last: Instant::now(), cli: crate::claude_cli::voice::Voice::default(), error: None }
    }
}

pub struct Reply {
    pub text: String,
    pub acted: bool,
}

fn tools() -> Vec<Value> {
    let mut tools = coucou_tools();
    tools.push(json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": 3 }));
    tools
}

/// Coucou's own tools, which run_tool carries out: the five of the voice, then
/// Coucou Shell's (shell/voice_tools.rs, which refuse while the panel is off).
/// The subscription's MCP server (claude_cli/mcp.rs) serves exactly these,
/// with the same schemas.
pub(crate) fn coucou_tools() -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "pills",
            "description": "Add, remove, set as main or list the pills shown in the island at the top of the screen. At most 4 pills besides the main one: remove one first if needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["add", "remove", "set_main", "list"] },
                    "names": { "type": "array", "items": { "type": "string" }, "description": "Pill names as listed (e.g. Stripe, GitHub, Cursor). Empty for list." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "music",
            "description": "Control Spotify or YouTube Music (through Pear Desktop): play or pause, next, previous, set the volume, or search a song, an artist or a playlist. On YouTube Music, search plays the top result; on Spotify it opens the search for the user to pick.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["play_pause", "next", "previous", "set_volume", "search"] },
                    "player": { "type": "string", "enum": ["spotify", "youtube_music"], "description": "Only when the user names one. Otherwise the one playing, else YouTube Music when it is connected, else Spotify." },
                    "query": { "type": "string", "description": "What to search, for search." },
                    "volume": { "type": "integer", "description": "0-100 for set_volume." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "service_info",
            "description": "Read the user's real data that Coucou already has, as JSON: the active pills, the agent sessions (Claude Code, Codex…) and what they are doing, Claude or Codex plan usage, Spotify and YouTube Music now playing, and the services' latest data (Stripe, GitHub, Vercel, Resend, n8n, Notion, Cal.com).",
            "input_schema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "open_app",
            "description": "Open an app on this computer by name (Firefox, VS Code, Notion, Spotify…).",
            "input_schema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
        }),
        json!({
            "name": "prepare_email",
            "description": "Open the user's mail app with an email filled in, for them to check and send (it is never sent automatically). The recipient must be an email address.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "recipient": { "type": "string", "description": "An email address." },
                    "subject": { "type": "string" },
                    "body": { "type": "string", "description": "The full email text, written for the user." }
                },
                "required": ["recipient", "subject", "body"]
            }
        }),
    ];
    tools.extend(crate::shell::voice_tools::definitions());
    tools
}

/// What the prompt says of Coucou Shell's tools, when the panel is on.
fn shell_prompt(on: bool) -> &'static str {
    if !on {
        return "";
    }
    "\nThe computer: system_stats, list_windows, list_apps, clipboard_recent read it; set_volume, toggle_wifi, \
toggle_bluetooth, focus_window, snap_window, apply_layout, set_wallpaper and web_open act right away: say in a few \
words what you did. web_open takes the words to search, never a link. For something to do every week or once \
later, call create_routine or schedule_action with the steps as those same tools and their inputs. power_action, \
type_text, create_routine and schedule_action only put a card in the island for the user to click: say so, never \
that it is done, and a spoken yes is not a click.\n"
}

fn shell_on(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().shell_panel)
}

fn system_prompt(pills: &str, shell: bool) -> String {
    let t = crate::platform::local_time();
    let os = if cfg!(windows) { "Windows" } else { "Linux" };
    format!(
        "You are Coucou, a voice assistant living at the top of the screen of the user's {os} computer, a bit like Jarvis: \
quick, warm, a little playful. It is {:04}-{:02}-{:02} {:02}:{:02}.\n\
Everything you write is read aloud: answer in the language the user spoke, in one or two short sentences, \
no markdown, no lists, no emojis, no links.\n\
Act with the tools instead of describing: pills, music, service_info for the user's own data, open_app, \
prepare_email, and web_search for anything current (weather, news, scores, prices, facts you are not sure of). \
Never invent the user's data.\n\
Email: gather the recipient's address, the subject and what to say (write the text yourself from what the user \
wants to say, in the language they used), asking for what is missing one short question at a time, then call \
prepare_email. The email is never sent by you: the user checks it in their mail app and sends it.\n\
When you need an answer from the user, end with one short question. Otherwise never end with a question.\n\
{shell}Pills: {pills}",
        t.year, t.month, t.day, t.hour, t.minute,
        shell = shell_prompt(shell)
    )
}

impl Brain {
    /// « OK Coucou » was heard: with the subscription, start Claude Code now
    /// so it is ready when the question comes.
    pub fn warm(&mut self, app: &AppHandle) {
        if !crate::claude_cli::enabled() {
            return;
        }
        let system = system_prompt(&super::ask_island(app, "pills", json!({ "action": "list" })).unwrap_or_default(), shell_on(app));
        let tools = crate::claude_cli::voice::runner(app);
        if let Err(f) = tauri::async_runtime::block_on(self.cli.warm(&tools, &system)) {
            crate::log::line(format!("voice: Claude Code did not start: {f:?}"));
        }
    }

    /// Called on every tick of the voice loop: lets the subscription's process
    /// go once the conversation is forgotten.
    pub fn idle(&mut self) {
        self.cli.idle();
    }

    /// Why the last turn had no answer, if the subscription said.
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn respond_cli(&mut self, app: &AppHandle, transcript: &str) -> Option<Reply> {
        let pills = super::ask_island(app, "pills", json!({ "action": "list" })).unwrap_or_default();
        let system = system_prompt(&pills, shell_on(app));
        let tools = crate::claude_cli::voice::runner(app);
        match tauri::async_runtime::block_on(self.cli.ask(&tools, &system, transcript)) {
            Ok((text, acted)) => Some(Reply { text: spoken_text(&text), acted }),
            Err(f) => {
                crate::log::line(format!("voice: Claude Code turn failed: {f:?}"));
                self.error = Some(f.message());
                None
            }
        }
    }

    pub fn respond(&mut self, app: &AppHandle, transcript: &str) -> Option<Reply> {
        if crate::claude_cli::enabled() {
            return self.respond_cli(app, transcript);
        }
        if self.last.elapsed() > FORGET_AFTER {
            self.messages.clear();
        }
        self.last = Instant::now();
        self.messages.push(json!({ "role": "user", "content": transcript }));
        self.trim();
        let pills = super::ask_island(app, "pills", json!({ "action": "list" })).unwrap_or_default();
        let system = system_prompt(&pills, shell_on(app));
        // The Shell's tools refuse while the panel is off: no need to describe them then.
        let shell = shell_on(app);
        let tools: Vec<Value> = tools().into_iter().filter(|t| shell || t["name"].as_str().and_then(crate::shell::voice_tools::level).is_none()).collect();
        let chat_model = app
            .try_state::<crate::Shared>()
            .map(|s| s.settings.lock().model.clone())
            .unwrap_or_default();
        let mut acted = false;
        let mut spoken = String::new();
        for _ in 0..5 {
            let turn = tauri::async_runtime::block_on(async {
                match crate::claude::voice_turn(FAST_MODEL, &system, &self.messages, &tools).await {
                    Ok(v) => Ok(v),
                    Err(err) if !chat_model.is_empty() => {
                        crate::log::line(format!("voice: {err}; the chat's model instead"));
                        crate::claude::voice_turn(&chat_model, &system, &self.messages, &tools).await
                    }
                    Err(err) => Err(err),
                }
            });
            let turn = match turn {
                Ok(t) => t,
                Err(err) => {
                    crate::log::line(format!("voice: Claude turn failed: {err}"));
                    if self.messages.last().is_some_and(|m| m["content"].is_string()) {
                        self.messages.pop();
                    }
                    return acted.then(|| Reply { text: String::new(), acted });
                }
            };
            let content = turn["content"].as_array().cloned().unwrap_or_default();
            self.messages.push(json!({ "role": "assistant", "content": content.clone() }));
            let text: String = content.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ");
            if !text.trim().is_empty() {
                spoken = text;
            }
            let stop = turn["stop_reason"].as_str().unwrap_or("end_turn");
            if stop == "pause_turn" {
                continue;
            }
            let uses: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
            if stop != "tool_use" || uses.is_empty() {
                break;
            }
            let mut results = Vec::new();
            for u in uses {
                let (result, did) = super::run_tool(app, u["name"].as_str().unwrap_or(""), &u["input"]);
                acted |= did;
                results.push(json!({ "type": "tool_result", "tool_use_id": u["id"], "content": result }));
            }
            self.messages.push(json!({ "role": "user", "content": results }));
            spoken.clear();
        }
        self.repair();
        Some(Reply { text: spoken_text(&spoken), acted })
    }

    /// Keeps the last exchanges, never cutting between a tool call and its result.
    fn trim(&mut self) {
        if self.messages.len() <= 24 {
            return;
        }
        let mut cut = self.messages.len() - 24;
        while cut < self.messages.len() && !(self.messages[cut]["role"] == "user" && self.messages[cut]["content"].is_string()) {
            cut += 1;
        }
        self.messages.drain(..cut.min(self.messages.len() - 1));
    }

    /// The API refuses a history ending on a tool call with no result after it.
    fn repair(&mut self) {
        let Some(last) = self.messages.last() else { return };
        if last["role"] != "assistant" {
            return;
        }
        let calls = last["content"].as_array().is_some_and(|c| c.iter().any(|b| b["type"] == "tool_use"));
        if calls {
            self.messages.pop();
        }
    }
}

/// What Claude wrote, ready to be said: citations and markdown left out.
pub fn spoken_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip = 0;
    for c in text.chars() {
        match c {
            '[' => skip += 1,
            ']' if skip > 0 => skip -= 1,
            '*' | '#' | '`' | '_' if skip == 0 => {}
            _ if skip == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_and_citations_are_not_read_aloud() {
        assert_eq!(spoken_text("**Done** — Stripe [1] is in the island."), "Done — Stripe is in the island.");
        assert_eq!(spoken_text("  Il fait  20 °C à Bordeaux. "), "Il fait 20 °C à Bordeaux.");
    }

    #[test]
    fn every_tool_has_a_schema() {
        for tool in tools() {
            assert!(tool["name"].is_string());
            assert!(tool.get("input_schema").is_some() || tool["type"] == "web_search_20250305");
        }
    }

    #[test]
    fn the_mcp_server_serves_exactly_coucous_tools() {
        let names: Vec<String> = coucou_tools().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        let mut expected_names = vec!["pills", "music", "service_info", "open_app", "prepare_email"];
        expected_names.extend(crate::shell::voice_tools::LEVELS.iter().map(|(n, _)| *n));
        assert_eq!(names, expected_names);
        // And the guard's list names each of them, and web search, and nothing else.
        let mut expected: Vec<String> = names.iter().map(|n| format!("mcp__coucou__{n}")).collect();
        expected.insert(0, "WebSearch".into());
        assert_eq!(crate::claude_cli::Mode::Voice.tools(), expected.as_slice());
        let served = crate::claude_cli::mcp::tool_definitions(&coucou_tools());
        for (mcp, api) in served.iter().zip(coucou_tools()) {
            assert_eq!(mcp["inputSchema"], api["input_schema"]);
        }
    }
}
