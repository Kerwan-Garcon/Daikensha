//! The tools a `claude` process started by Coucou may use, by mode
//! (docs/SUBSCRIPTION_MODE.md). One list for both sides: the app builds the
//! command line from it (claude_cli.rs, which includes this file with `#[path]`)
//! and `coucou-hook --tool-guard <mode>` denies anything else. Nothing here may
//! run code, read or write files, or fetch a page.

/// The chat, the phone's chat and the one-shot completions: no tool at all.
pub const TEXT: &[&str] = &[];

/// « OK Coucou »: web search and Coucou's own five voice tools (voice/brain.rs),
/// served by the app's local MCP server named `coucou`.
pub const VOICE: &[&str] = &[
    "WebSearch",
    "mcp__coucou__pills",
    "mcp__coucou__music",
    "mcp__coucou__service_info",
    "mcp__coucou__open_app",
    "mcp__coucou__prepare_email",
];

/// The list for a mode name as the guard receives it; None for any other name.
pub fn allowed(mode: &str) -> Option<&'static [&'static str]> {
    match mode {
        "text" => Some(TEXT),
        "voice" => Some(VOICE),
        _ => None,
    }
}
