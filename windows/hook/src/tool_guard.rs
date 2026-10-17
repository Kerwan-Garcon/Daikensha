//! `coucou-hook --tool-guard <mode>`: the PreToolUse hook Coucou passes to the
//! `claude` processes it starts for the Claude subscription mode (claude_cli.rs,
//! docs/SUBSCRIPTION_MODE.md). It is one of four independent layers that keep
//! those processes away from any tool outside the mode's list (tool_lists.rs).
//!
//! Unlike the relay, this one fails closed: anything it cannot read, an unknown
//! mode, another event, a tool not on the list, or stdin that does not end in
//! time is denied (exit code 2, the reason on stderr). An allowed tool gets no
//! output at all, so the permission rules Coucou passed still decide: the guard
//! can only take a tool away, never grant one.
//!
//! Claude Code itself lets the tool run when a hook command is missing or times
//! out, so the app checks the relay is there before it starts `claude`, and this
//! answers well within the hook timeout it is given.

use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use super::tool_lists;

/// The hook input is a small JSON object; anything bigger is not one.
const MAX_INPUT: u64 = 1024 * 1024;
/// Claude Code closes stdin right after writing. Past this, deny and go.
const READ_BUDGET: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// No opinion: Claude Code's own rules (already limited to the list) decide.
    Pass,
    Deny(String),
}

pub fn run(mode: Option<&str>) -> ! {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut raw = Vec::new();
        let ok = std::io::stdin().take(MAX_INPUT + 1).read_to_end(&mut raw).is_ok();
        let _ = tx.send(ok.then_some(raw));
    });
    let decision = match rx.recv_timeout(READ_BUDGET) {
        Ok(Some(raw)) => decide(mode, &raw),
        _ => Decision::Deny("Coucou tool guard: could not read the hook input".into()),
    };
    match decision {
        Decision::Pass => std::process::exit(0),
        Decision::Deny(reason) => {
            eprintln!("{reason}");
            std::process::exit(2);
        }
    }
}

pub fn decide(mode: Option<&str>, raw: &[u8]) -> Decision {
    let deny = |why: &str| Decision::Deny(format!("Coucou tool guard: {why}"));
    let Some(list) = mode.and_then(tool_lists::allowed) else { return deny("unknown mode") };
    if raw.len() as u64 > MAX_INPUT {
        return deny("hook input too large");
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw) else { return deny("unreadable hook input") };
    if value.get("hook_event_name").and_then(|v| v.as_str()) != Some("PreToolUse") {
        return deny("not a PreToolUse event");
    }
    let Some(tool) = value.get("tool_name").and_then(|v| v.as_str()) else { return deny("no tool name") };
    if list.contains(&tool) {
        Decision::Pass
    } else {
        deny(&format!("{tool} is not allowed here"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(tool: &str) -> Vec<u8> {
        serde_json::json!({ "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": {} }).to_string().into_bytes()
    }

    #[test]
    fn only_the_modes_tools_pass() {
        assert_eq!(decide(Some("voice"), &input("WebSearch")), Decision::Pass);
        assert_eq!(decide(Some("voice"), &input("mcp__coucou__prepare_email")), Decision::Pass);
        for tool in ["Bash", "PowerShell", "Read", "Write", "Edit", "WebFetch", "Agent", "Task", "mcp__other__x", "mcp__coucou__send_email", "websearch", ""] {
            assert!(matches!(decide(Some("voice"), &input(tool)), Decision::Deny(_)), "{tool}");
        }
        // The text modes have no tool at all.
        for tool in ["WebSearch", "Bash", "mcp__coucou__pills"] {
            assert!(matches!(decide(Some("text"), &input(tool)), Decision::Deny(_)), "{tool}");
        }
    }

    #[test]
    fn anything_unexpected_is_denied() {
        assert!(matches!(decide(None, &input("WebSearch")), Decision::Deny(_)));
        assert!(matches!(decide(Some("admin"), &input("WebSearch")), Decision::Deny(_)));
        assert!(matches!(decide(Some("voice"), b"not json"), Decision::Deny(_)));
        assert!(matches!(decide(Some("voice"), b""), Decision::Deny(_)));
        assert!(matches!(decide(Some("voice"), br#"{"tool_name":"WebSearch"}"#), Decision::Deny(_)));
        assert!(matches!(decide(Some("voice"), br#"{"hook_event_name":"PreToolUse","tool_name":7}"#), Decision::Deny(_)));
        let big = vec![b' '; MAX_INPUT as usize + 1];
        assert!(matches!(decide(Some("voice"), &big), Decision::Deny(_)));
    }

    #[test]
    fn the_denial_names_the_tool() {
        assert_eq!(decide(Some("text"), &input("Bash")), Decision::Deny("Coucou tool guard: Bash is not allowed here".into()));
    }
}
