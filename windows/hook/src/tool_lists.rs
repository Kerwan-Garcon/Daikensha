//! The tools a `claude` process started by Coucou may use, by mode
//! (docs/SUBSCRIPTION_MODE.md). One list for both sides: the app builds the
//! command line from it (claude_cli.rs, which includes this file with `#[path]`)
//! and `coucou-hook --tool-guard <mode>` denies anything else. Nothing here may
//! run code, read or write files, or fetch a page.

/// The chat, the phone's chat and the one-shot completions: no tool at all.
pub const TEXT: &[&str] = &[];

/// « OK Coucou »: web search, Coucou's own five voice tools (voice/brain.rs)
/// and Coucou Shell's (shell/voice_tools.rs, whose risk levels are fixed
/// there), served by the app's local MCP server named `coucou`. None of them
/// runs code, reads a file or fetches a page; the level 2 ones only put a card
/// up for the user to click.
pub const VOICE: &[&str] = &[
    "WebSearch",
    "mcp__coucou__pills",
    "mcp__coucou__music",
    "mcp__coucou__service_info",
    "mcp__coucou__open_app",
    "mcp__coucou__prepare_email",
    "mcp__coucou__system_stats",
    "mcp__coucou__list_windows",
    "mcp__coucou__list_apps",
    "mcp__coucou__clipboard_recent",
    "mcp__coucou__set_volume",
    "mcp__coucou__toggle_wifi",
    "mcp__coucou__toggle_bluetooth",
    "mcp__coucou__focus_window",
    "mcp__coucou__snap_window",
    "mcp__coucou__apply_layout",
    "mcp__coucou__set_wallpaper",
    "mcp__coucou__web_open",
    "mcp__coucou__power_action",
    "mcp__coucou__type_text",
];

/// The list for a mode name as the guard receives it; None for any other name.
pub fn allowed(mode: &str) -> Option<&'static [&'static str]> {
    match mode {
        "text" => Some(TEXT),
        "voice" => Some(VOICE),
        _ => None,
    }
}
