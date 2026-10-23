// Claude through the user's own Claude Code — the `claude` CLI, signed in to
// their Claude subscription — instead of the API with a key. Off unless the user
// picks "Claude subscription" in Settings → Claude; the API stays the default.
// docs/SUBSCRIPTION_MODE.md says why each flag is there and what was measured.
//
// Coucou never sees a token: `claude` is run as published, as a child process,
// and signs itself in. No credentials file, no keychain entry of Claude Code's,
// no `setup-token` is ever read, and the variables that would make the CLI use
// something other than the subscription login are removed from its environment.
//
// The child gets no tool that can run code, read or write files or fetch a page.
// Four layers keep it so, any one of them enough on its own:
//   1. `build_args` is the only place a command line is made, and `check`
//      refuses one with a flag that widens what the child can do or a tool off
//      the mode's list (hook/src/tool_lists.rs);
//   2. the permission rules passed inline with `--settings` deny every such tool
//      and turn off the bypass mode;
//   3. the same inline settings run `coucou-hook --tool-guard <mode>` before any
//      tool, which denies whatever is not on the list (fail closed);
//   4. `Stream` reads the child's output and kills it the moment it reports a
//      tool, an MCP server or an API key it should not have.
// `--restricted` and an empty `--setting-sources` (no user, project or local
// settings, so neither the user's hooks nor their plugins) come on top.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::chat::{self, Chat, ChatContext, ChatReply, ModelInfo};
use crate::i18n::{t, tf};
use crate::{agents, platform, settings};

#[path = "../../hook/src/tool_lists.rs"]
mod tool_lists;

mod job;
pub mod mcp;
pub mod voice;

/// The chat provider id of the subscription (chat.rs, the island's providers.ts).
pub const PROVIDER: &str = "anthropic-subscription";

/// What `--model` takes: Claude Code's aliases, so the plan's own models are used.
pub const MODELS: &[(&str, &str)] = &[("haiku", "Claude Haiku"), ("sonnet", "Claude Sonnet"), ("opus", "Claude Opus")];
pub const DEFAULT_MODEL: &str = "sonnet";

/// Flags that would give the child more than Coucou means it to have: refused
/// anywhere in a command line, even inside another argument.
const FORBIDDEN: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "bypassPermissions",
    "--bare",
    "--add-dir",
    "--plugin-dir",
    "--plugin-url",
    "--agents",
    "--agent",
    "--continue",
    "--fork-session",
    "--chrome",
    "--ide",
    "--file",
    "--worktree",
    "--remote-control",
    "--cloud",
    "--teleport",
    "--background",
    "--from-pr",
];

/// Denied by the inline permission rules whatever `--tools` says (layer 2).
const DENY: &[&str] = &[
    "Bash", "PowerShell", "Edit", "MultiEdit", "Write", "Read", "NotebookEdit", "Glob", "Grep", "WebFetch", "Agent",
    "Task", "Skill",
];

/// Taken out of the child's environment: each would make Claude Code use a key,
/// a token or another endpoint instead of the subscription login (or think it
/// runs inside another Claude Code).
const SCRUBBED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// Set in the child's environment: none of Claude Code's own telemetry, error
/// reports, update checks or other background requests (Coucou sends none
/// either), and the CLI answers about 2 s sooner, a little lighter (measured).
const ADDED_ENV: &[(&str, &str)] = &[("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"), ("DISABLE_AUTOUPDATER", "1")];

/// The guard must answer well within this; past it Claude Code lets the tool run.
const GUARD_TIMEOUT_SECS: u64 = 10;
/// What is kept of the child's stderr, for the log and an error message.
const MAX_STDERR: usize = 16 * 1024;
/// Everything the child prints for one turn. A turn is a few kilobytes of JSON.
const MAX_STDOUT: usize = 32 * 1024 * 1024;
const CHAT_TIMEOUT: Duration = Duration::from_secs(150);
const STATUS_TIMEOUT: Duration = Duration::from_secs(15);

// ── The setting ───────────────────────────────────────────────────────────────

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Mirrors `Settings::claude_subscription`, for the callers that have no
/// settings at hand (`claude::complete`). Set at launch and on every save.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// ── Modes and the command line ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The island's chat.
    Chat,
    /// The phone's chat: text only, whatever the settings say.
    Phone,
    /// One question, one answer: dictation clean-up, the Notetaker's summary.
    Complete,
    /// « OK Coucou »: web search and the voice's tools, through the
    /// local MCP server (mcp.rs), in a process kept for the conversation.
    Voice,
}

impl Mode {
    /// The guard's name for the mode's list.
    fn guard(self) -> &'static str {
        match self {
            Mode::Voice => "voice",
            _ => "text",
        }
    }

    pub fn tools(self) -> &'static [&'static str] {
        tool_lists::allowed(self.guard()).unwrap_or(&[])
    }

    /// Its own working folder, so that one mode's conversations are purged
    /// without touching another's.
    fn dir(self) -> &'static str {
        match self {
            Mode::Chat => "chat",
            Mode::Phone => "phone",
            Mode::Complete => "oneshot",
            Mode::Voice => "voice",
        }
    }
}

pub struct Request<'a> {
    pub mode: Mode,
    pub model: &'a str,
    pub system: &'a str,
    /// The Claude Code session to continue (`--resume`).
    pub resume: Option<&'a str>,
    /// Keep the session on disk so the next turn can resume it.
    pub persist: bool,
    /// The PreToolUse hook command (`coucou-hook --tool-guard <mode>`).
    pub guard: &'a str,
    /// The voice's `--mcp-config` (mcp::Server::config), for `Mode::Voice` only.
    pub mcp_config: Option<&'a str>,
}

/// The inline settings of layers 2 and 3.
fn inline_settings(mode: Mode, guard: &str) -> String {
    json!({
        "permissions": {
            "allow": mode.tools(),
            "deny": DENY,
            "defaultMode": "dontAsk",
            "disableBypassPermissionsMode": "disable",
        },
        "hooks": {
            "PreToolUse": [{
                "matcher": "*",
                "hooks": [{ "type": "command", "command": guard, "timeout": GUARD_TIMEOUT_SECS }],
            }],
        },
    })
    .to_string()
}

/// The only place a `claude` command line is made (layer 1).
pub fn build_args(r: &Request) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--restricted",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--setting-sources",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(["--model".into(), model_alias(r.model).to_string()]);
    args.extend(["--system-prompt".into(), r.system.to_string()]);
    // --tools takes Claude Code's own tools; the MCP ones come with the server.
    let builtin: Vec<&str> = r.mode.tools().iter().copied().filter(|t| !t.starts_with("mcp__")).collect();
    args.extend(["--tools".into(), builtin.join(",")]);
    if !r.mode.tools().is_empty() {
        args.extend(["--allowedTools".into(), r.mode.tools().join(",")]);
    }
    args.extend(["--disallowedTools".into(), DENY.join(",")]);
    args.extend(["--settings".into(), inline_settings(r.mode, r.guard)]);
    if let Some(config) = r.mcp_config {
        args.extend(["--mcp-config".into(), config.to_string()]);
    }
    if let Some(id) = r.resume {
        args.extend(["--resume".into(), id.to_string()]);
    }
    if !r.persist {
        args.push("--no-session-persistence".into());
    }
    check(&args, r.mode)?;
    Ok(args)
}

/// Refuses a command line that would give the child more than `mode` allows.
/// An error, never a warning: nothing is started.
pub fn check(args: &[String], mode: Mode) -> Result<(), String> {
    let refuse = |why: String| Err(format!("claude command refused: {why}"));
    for a in args {
        if let Some(f) = FORBIDDEN.iter().find(|f| a.contains(*f)) {
            return refuse(format!("{f} is not allowed"));
        }
    }
    let allowed = mode.tools();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    for required in ["--restricted", "--strict-mcp-config", "--disable-slash-commands"] {
        if !args.iter().any(|a| a == required) {
            return refuse(format!("{required} is missing"));
        }
    }
    let Some(tools) = value("--tools") else { return refuse("--tools is missing".into()) };
    for tool in tools.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !allowed.contains(&tool) {
            return refuse(format!("tool {tool} is not allowed"));
        }
    }
    for flag in ["--allowedTools", "--allowed-tools"] {
        if let Some(list) = value(flag) {
            for tool in list.split([',', ' ']).map(str::trim).filter(|t| !t.is_empty()) {
                if !allowed.contains(&tool) {
                    return refuse(format!("tool {tool} is not allowed"));
                }
            }
        }
    }
    if value("--permission-mode").map(String::as_str) != Some("dontAsk") {
        return refuse("the permission mode must be dontAsk".into());
    }
    if value("--permission-prompts").map(String::as_str) != Some("none") {
        return refuse("permission prompts must go to nobody".into());
    }
    if value("--setting-sources").map(String::as_str) != Some("") {
        return refuse("no settings file may be loaded".into());
    }
    if !MODELS.iter().any(|(id, _)| Some(*id) == value("--model").map(String::as_str)) {
        return refuse("unknown model".into());
    }
    // A session id is Claude Code's UUID, nothing that could read as a flag.
    if let Some(id) = value("--resume") {
        if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || id.starts_with('-') {
            return refuse("bad session id".into());
        }
    }
    let denied_flag: Vec<&str> = value("--disallowedTools").map(|v| v.split(',').collect()).unwrap_or_default();
    if DENY.iter().any(|d| !denied_flag.contains(d)) {
        return refuse("--disallowedTools is incomplete".into());
    }
    match (mode, value("--mcp-config")) {
        (Mode::Voice, Some(config)) => check_mcp_config(config).map_err(|why| format!("claude command refused: {why}"))?,
        (Mode::Voice, None) => return refuse("the voice needs Coucou's MCP server".into()),
        (_, Some(_)) => return refuse("no MCP server is allowed in this mode".into()),
        (_, None) if args.iter().any(|a| a == "--mcp-config") => return refuse("an empty --mcp-config".into()),
        _ => {}
    }
    let Some(inline) = value("--settings").and_then(|s| serde_json::from_str::<Value>(s).ok()) else {
        return refuse("the inline settings are missing".into());
    };
    let denied: Vec<&str> = inline
        .pointer("/permissions/deny")
        .and_then(Value::as_array)
        .map(|d| d.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if DENY.iter().any(|d| !denied.contains(d)) {
        return refuse("the deny rules are incomplete".into());
    }
    let granted = inline.pointer("/permissions/allow").and_then(Value::as_array).cloned().unwrap_or_default();
    if granted.iter().any(|g| !g.as_str().is_some_and(|g| allowed.contains(&g))) {
        return refuse("an allow rule is not on the list".into());
    }
    if inline.pointer("/permissions/disableBypassPermissionsMode").and_then(Value::as_str) != Some("disable") {
        return refuse("the bypass mode must be off".into());
    }
    let guard = inline.pointer("/hooks/PreToolUse/0/hooks/0/command").and_then(Value::as_str).unwrap_or("");
    if !guard.contains(&format!("--tool-guard {}", mode.guard())) {
        return refuse("the tool guard is missing".into());
    }
    Ok(())
}

/// The only MCP config allowed: Coucou's own server on this machine, its token
/// read from the environment, never written in the command line.
fn check_mcp_config(config: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(config).map_err(|_| "unreadable --mcp-config".to_string())?;
    let servers = v.get("mcpServers").and_then(Value::as_object).ok_or("no mcpServers")?;
    if servers.len() != 1 {
        return Err("only Coucou's MCP server is allowed".into());
    }
    let server = servers.get(mcp::NAME).ok_or("only Coucou's MCP server is allowed")?;
    let url = server.get("url").and_then(Value::as_str).unwrap_or_default();
    let port = url.strip_prefix("http://127.0.0.1:").and_then(|r| r.strip_suffix("/mcp")).unwrap_or_default();
    if server.get("type").and_then(Value::as_str) != Some("http") || port.parse::<u16>().is_err() {
        return Err("the MCP server must be Coucou's, on 127.0.0.1".into());
    }
    let expected = json!({ "Authorization": format!("Bearer ${{{}}}", mcp::TOKEN_VAR) });
    if server.get("headers") != Some(&expected) || server.as_object().is_some_and(|o| o.keys().any(|k| !matches!(k.as_str(), "type" | "url" | "headers"))) {
        return Err("the MCP token must come from the environment".into());
    }
    Ok(())
}

/// The CLI alias for a model: the alias itself, or the family an API model id
/// names ("claude-haiku-4-5" → "haiku"); anything else gets the default.
pub fn model_alias(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    MODELS
        .iter()
        .map(|(id, _)| *id)
        .find(|id| m == *id || m.contains(id))
        .unwrap_or(DEFAULT_MODEL)
}

// ── Where `claude` is ─────────────────────────────────────────────────────────

/// How to start Claude Code: a program and the arguments that go first (the
/// npm build is `node cli.js`, started directly so no shell ever reads the line).
#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    pub program: PathBuf,
    pub pre: Vec<String>,
}

/// Known install places, best first — the native build, then npm's — as
/// InstructionRunner.claudeExecutable() does on the Mac. Never a path the page
/// hands over, and never a search of PATH.
fn candidates(home: &Path, windows: bool, appdata: Option<&Path>, program_files: Option<&Path>) -> Vec<Launcher> {
    let plain = |p: PathBuf| Launcher { program: p, pre: Vec::new() };
    if windows {
        let mut list = vec![plain(home.join(".local").join("bin").join("claude.exe"))];
        if let (Some(appdata), Some(pf)) = (appdata, program_files) {
            let cli = appdata.join("npm").join("node_modules").join("@anthropic-ai").join("claude-code").join("cli.js");
            list.push(Launcher { program: pf.join("nodejs").join("node.exe"), pre: vec![cli.to_string_lossy().to_string()] });
        }
        return list;
    }
    vec![
        plain(home.join(".local/bin/claude")),
        plain(home.join(".claude/local/claude")),
        plain(PathBuf::from("/usr/local/bin/claude")),
        plain(PathBuf::from("/usr/bin/claude")),
        plain(home.join(".npm-global/bin/claude")),
        plain(home.join(".bun/bin/claude")),
    ]
}

pub fn locate() -> Option<Launcher> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    candidates(&platform::home_dir(), cfg!(windows), var("APPDATA").as_deref(), var("ProgramFiles").as_deref())
        .into_iter()
        .find(|l| l.program.is_file() && l.pre.iter().all(|p| Path::new(p).is_file()))
}

/// A `claude` command ready to start: arguments as a list (no shell), no
/// console window, the scrubbed environment, its own folder, killed with us.
fn command(launcher: &Launcher, args: &[String], cwd: &Path) -> tokio::process::Command {
    command_with(launcher, args, cwd, None)
}

/// The same, with the MCP server's token in the child's environment (voice).
fn command_with(launcher: &Launcher, args: &[String], cwd: &Path, token: Option<&str>) -> tokio::process::Command {
    let mut std_cmd = std::process::Command::new(&launcher.program);
    std_cmd.args(&launcher.pre).args(args).current_dir(cwd);
    for var in SCRUBBED_ENV {
        std_cmd.env_remove(var);
    }
    std_cmd.envs(ADDED_ENV.iter().copied());
    match token {
        Some(token) => std_cmd.env(mcp::TOKEN_VAR, token),
        None => std_cmd.env_remove(mcp::TOKEN_VAR),
    };
    platform::no_console(&mut std_cmd);
    let mut cmd = tokio::process::Command::from(std_cmd);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    cmd
}

fn work_dir(mode: Mode) -> Result<PathBuf, String> {
    let dir = settings::local_dir().join("claude-cli").join(mode.dir());
    std::fs::create_dir_all(&dir).map_err(|e| format!("claude-cli folder: {e}"))?;
    Ok(dir)
}

// ── Reading the stream ────────────────────────────────────────────────────────

/// Something the child reported that it should never have: layer 4 kills it.
#[derive(Debug, PartialEq)]
pub enum Violation {
    Tool(String),
    McpServer(String),
    ApiKey(String),
}

/// One turn's `stream-json` output, line by line.
pub struct Stream {
    allowed: &'static [&'static str],
    session_id: Option<String>,
    result: Option<Value>,
    /// The `error` an assistant message carried (authentication_failed, rate_limit…).
    error: Option<String>,
    /// The last `rate_limit_event`'s info: the 5-hour and weekly windows.
    pub limits: Option<Value>,
}

impl Stream {
    pub fn new(mode: Mode) -> Self {
        Self { allowed: mode.tools(), session_id: None, result: None, error: None, limits: None }
    }

    /// The turn ended with a result line (an error one included).
    pub fn has_result(&self) -> bool {
        self.result.is_some()
    }

    /// Takes one line; true once the turn's result is in.
    pub fn feed(&mut self, line: &str) -> Result<bool, Violation> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return Ok(false) };
        if let Some(id) = v.get("session_id").and_then(Value::as_str) {
            self.session_id = Some(id.to_string());
        }
        match v.get("type").and_then(Value::as_str) {
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                for tool in v.get("tools").and_then(Value::as_array).into_iter().flatten() {
                    let name = tool.as_str().unwrap_or_default();
                    if !self.allowed.contains(&name) {
                        return Err(Violation::Tool(name.to_string()));
                    }
                }
                for server in v.get("mcp_servers").and_then(Value::as_array).into_iter().flatten() {
                    let name = server.get("name").and_then(Value::as_str).unwrap_or_default();
                    if !self.allowed.iter().any(|t| t.starts_with(&format!("mcp__{name}__"))) {
                        return Err(Violation::McpServer(name.to_string()));
                    }
                }
                if v.get("permissionMode").and_then(Value::as_str) == Some("bypassPermissions") {
                    return Err(Violation::Tool("bypassPermissions".into()));
                }
                // "none" is the subscription login (measured: docs/SUBSCRIPTION_MODE.md);
                // any other source is an API key, which would be billed.
                match v.get("apiKeySource").and_then(Value::as_str) {
                    None | Some("none" | "oauth" | "claude.ai") => {}
                    Some(source) => return Err(Violation::ApiKey(source.to_string())),
                }
            }
            Some("assistant") => {
                if let Some(e) = v.get("error").and_then(Value::as_str) {
                    self.error = Some(e.to_string());
                }
                for block in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    let kind = block.get("type").and_then(Value::as_str).unwrap_or_default();
                    if matches!(kind, "tool_use" | "server_tool_use" | "mcp_tool_use") {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or_default();
                        if !self.allowed.contains(&name) {
                            return Err(Violation::Tool(name.to_string()));
                        }
                    }
                }
            }
            Some("rate_limit_event") => self.limits = v.get("rate_limit_info").cloned(),
            Some("result") => {
                self.result = Some(v);
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    /// The answer, or why there is none.
    pub fn finish(self) -> Result<Answer, Failure> {
        let rejected = self.limits.as_ref().and_then(|l| l.get("status")).and_then(Value::as_str) == Some("rejected");
        let resets_at = self.limits.as_ref().and_then(|l| l.get("resetsAt")).and_then(Value::as_i64);
        let Some(result) = self.result else {
            return Err(if rejected { Failure::Limit(resets_at) } else { Failure::Other(t("No response text.")) });
        };
        let text = result.get("result").and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let failed = result.get("is_error").and_then(Value::as_bool).unwrap_or(false)
            || result.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success");
        if failed || self.error.is_some() {
            return Err(match self.error.as_deref() {
                Some("authentication_failed") => Failure::NotLoggedIn,
                Some("billing_error") => Failure::Billing,
                Some("rate_limit") => Failure::Limit(resets_at),
                _ if rejected => Failure::Limit(resets_at),
                _ => classify_text(&text),
            });
        }
        if text.is_empty() {
            return Err(Failure::Other(t("No response text.")));
        }
        Ok(Answer { text, session_id: self.session_id })
    }
}

pub struct Answer {
    pub text: String,
    pub session_id: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Failure {
    NotInstalled,
    NotLoggedIn,
    /// The plan's limit, with when it resets (Unix seconds) if known.
    Limit(Option<i64>),
    Billing,
    ApiKey,
    Timeout,
    /// The resumed session is gone (purged, or another machine's).
    ResumeLost,
    /// Layer 4 killed the child: the tool or server it reported.
    Blocked(String),
    /// A kept process (the voice's) ended before its answer.
    Exited,
    Other(String),
}

/// What an error text says, for the failures that carry no `error` field.
fn classify_text(text: &str) -> Failure {
    let lower = text.to_lowercase();
    if lower.contains("no conversation found") {
        Failure::ResumeLost
    } else if lower.contains("not logged in") || lower.contains("/login") || lower.contains("invalid api key") || lower.contains("oauth") {
        Failure::NotLoggedIn
    } else if lower.contains("usage limit") || lower.contains("rate limit") {
        Failure::Limit(None)
    } else {
        Failure::Other(tf("Claude Code: {error}", &[("error", &excerpt(text, 300))]))
    }
}

impl Failure {
    /// What the island shows.
    pub fn message(&self) -> String {
        match self {
            Failure::NotInstalled => t("Claude Code is not installed. Install it, or switch Claude back to an API key in Settings."),
            Failure::NotLoggedIn => t("Claude Code is not signed in. Run claude in a terminal once and sign in with your Claude account."),
            Failure::Limit(Some(at)) => {
                tf("Claude plan limit reached. It resets in {time}.", &[("time", &until(*at, unix_now()))])
            }
            Failure::Limit(None) => t("Claude plan limit reached."),
            Failure::Billing => t("Your Claude subscription needs attention on claude.ai."),
            Failure::ApiKey => t("Claude Code is signed in with an API key, not a subscription. Run claude and sign in with your Claude account."),
            Failure::Timeout => t("Claude Code did not answer in time."),
            Failure::ResumeLost => t("Claude Code lost this conversation. Start a new chat."),
            Failure::Exited => t("Claude Code stopped unexpectedly."),
            Failure::Blocked(tool) => tf("Stopped: Claude Code tried a tool Coucou does not allow ({tool}).", &[("tool", tool)]),
            Failure::Other(text) => text.clone(),
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "2 h 05" or "35 min" from now until `at` (Unix seconds).
fn until(at: i64, now: i64) -> String {
    let mins = ((at - now) / 60).max(1);
    if mins < 60 {
        format!("{mins} min")
    } else {
        format!("{} h {:02}", mins / 60, mins % 60)
    }
}

fn excerpt(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        return one;
    }
    format!("{}…", one.chars().take(max).collect::<String>())
}

// ── The relay's guard ─────────────────────────────────────────────────────────

/// What the relay must answer for a `Bash` call: denied, exit code 2.
const GUARD_PROBE: &str = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"whoami"}}"#;

/// The relay file (modified time, size) that last passed the probe.
static RELAY_CHECKED: parking_lot::Mutex<Option<(std::time::SystemTime, u64)>> = parking_lot::Mutex::new(None);

/// Claude Code lets a tool run when its hook is missing, times out, or is an
/// older relay that does not know `--tool-guard` (it would forward the event
/// and allow it). So before any `claude` starts, the installed relay is asked
/// to judge a `Bash` call and must deny it; the answer is kept until the file
/// changes. No relay, or a relay that does not deny: no start.
pub(crate) async fn relay_ready() -> Result<(), Failure> {
    let path = settings::hook_exe_path();
    let meta = std::fs::metadata(&path).map_err(|_| Failure::Other(t("Coucou's relay is missing. Reinstall Coucou.")))?;
    let stamp = (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len());
    if *RELAY_CHECKED.lock() == Some(stamp) {
        return Ok(());
    }
    if probe_relay(&path).await {
        *RELAY_CHECKED.lock() = Some(stamp);
        Ok(())
    } else {
        crate::log::line("subscription: the installed relay does not deny tools; not starting Claude Code");
        Err(Failure::Other(t("Coucou's relay is out of date and could not guard Claude Code. Quit and restart Coucou to update it.")))
    }
}

/// True when `relay --tool-guard voice` denies `Bash` (exit 2) within 5 s.
async fn probe_relay(relay: &Path) -> bool {
    let mut std_cmd = std::process::Command::new(relay);
    std_cmd.args(["--tool-guard", "voice"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    platform::no_console(&mut std_cmd);
    let mut cmd = tokio::process::Command::from(std_cmd);
    cmd.kill_on_drop(true);
    let Ok(mut child) = cmd.spawn() else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(GUARD_PROBE.as_bytes()).await;
    }
    match tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await {
        Ok(Ok(out)) => guard_denied(out.status.code(), &out.stderr),
        _ => false,
    }
}

/// The probe's answer is a denial from Coucou's guard, and nothing else.
fn guard_denied(code: Option<i32>, stderr: &[u8]) -> bool {
    code == Some(2) && String::from_utf8_lossy(stderr).contains("Coucou tool guard")
}

// ── One turn ──────────────────────────────────────────────────────────────────

/// One user message to a fresh `claude -p`, the answer back.
pub async fn run(req: &Request<'_>, content: Vec<Value>, timeout: Duration) -> Result<Answer, Failure> {
    let launcher = locate().ok_or(Failure::NotInstalled)?;
    relay_ready().await?;
    let args = build_args(req).map_err(Failure::Other)?;
    let cwd = work_dir(req.mode).map_err(Failure::Other)?;
    let mut child = command(&launcher, &args, &cwd).spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Failure::NotInstalled,
        _ => Failure::Other(tf("Claude Code: {error}", &[("error", &e.to_string())])),
    })?;
    let _job = contain(&child);

    let message = json!({
        "type": "user",
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": null,
        "session_id": "",
    });
    let mut stdin = child.stdin.take().ok_or_else(|| Failure::Other("no stdin".into()))?;
    let writer = tokio::spawn(async move {
        let mut line = message.to_string();
        line.push('\n');
        let _ = stdin.write_all(line.as_bytes()).await;
        let _ = stdin.shutdown().await;
        // Dropped here: the CLI sees the end of its input and exits after the turn.
    });
    let mut stderr = child.stderr.take().ok_or_else(|| Failure::Other("no stderr".into()))?;
    let errors = tokio::spawn(async move {
        let mut kept = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if kept.len() < MAX_STDERR {
                kept.extend_from_slice(&buf[..n.min(MAX_STDERR - kept.len())]);
            }
        }
        String::from_utf8_lossy(&kept).to_string()
    });
    let stdout = child.stdout.take().ok_or_else(|| Failure::Other("no stdout".into()))?;

    let mut stream = Stream::new(req.mode);
    let read = tokio::time::timeout(timeout, async {
        let mut lines = BufReader::new(stdout).lines();
        let mut total = 0usize;
        while let Ok(Some(line)) = lines.next_line().await {
            total += line.len();
            if total > MAX_STDOUT {
                return Err(Failure::Other("Claude Code printed too much".into()));
            }
            match stream.feed(&line) {
                Ok(true) => break,
                Ok(false) => {}
                Err(v) => return Err(blocked(v)),
            }
        }
        Ok(())
    })
    .await;
    writer.abort();

    let outcome = match read {
        Err(_) => Err(Failure::Timeout),
        Ok(Err(f)) => Err(f),
        Ok(Ok(())) => Ok(()),
    };
    // After its result the CLI exits by itself; a few seconds, then it is killed.
    let exited = outcome.is_ok() && tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_ok();
    if !exited {
        let _ = child.kill().await;
    }
    let stderr_text = tokio::time::timeout(Duration::from_secs(1), errors).await.ok().and_then(Result::ok).unwrap_or_default();
    outcome?;
    if !stderr_text.trim().is_empty() {
        crate::log::line(format!("claude-cli: {}", excerpt(&stderr_text, 300)));
    }
    // No result at all: the CLI stopped before the turn (an unknown flag in an
    // older version, a session it cannot find…), and only stderr says why.
    if !stream.has_result() && !stderr_text.trim().is_empty() {
        return Err(classify_text(&stderr_text));
    }
    stream.finish()
}

/// Layer 4 fired: say so in the log (the tool's name only) and in the island.
fn blocked(v: Violation) -> Failure {
    match v {
        Violation::ApiKey(source) => {
            crate::log::line(format!("claude-cli: refused, Claude Code uses an API key ({source})"));
            Failure::ApiKey
        }
        Violation::Tool(name) | Violation::McpServer(name) => {
            crate::log::line(format!("claude-cli: killed, it reported a tool or server off the list: {name}"));
            Failure::Blocked(name)
        }
    }
}

/// The child in a Job Object (Windows), or why not in the log.
fn contain(child: &tokio::process::Child) -> Option<job::Job> {
    let job = job::contain(child);
    if job.is_none() {
        crate::log::line("claude-cli: could not put claude in a job object");
    }
    job
}

fn guard_command(mode: Mode) -> String {
    agents::relay_command(agents::Shell::Sh, &format!("--tool-guard {}", mode.guard()))
}

// ── What the app calls ────────────────────────────────────────────────────────

fn not_enabled() -> String {
    t("Turn on the Claude subscription in Settings first.")
}

/// One question, one answer, nothing kept: `claude::complete` in subscription mode.
pub async fn complete(model: &str, system: &str, user: &str, timeout: Duration) -> Result<String, String> {
    if !enabled() {
        return Err(not_enabled());
    }
    let guard = guard_command(Mode::Complete);
    let req = Request { mode: Mode::Complete, model, system, resume: None, persist: false, guard: &guard, mcp_config: None };
    let content = vec![json!({ "type": "text", "text": user })];
    run(&req, content, timeout).await.map(|a| a.text).map_err(|f| f.message())
}

/// One chat turn (the island's, or the phone's with `Mode::Phone`). The
/// conversation goes on with `--resume`; when there is no Claude Code session
/// to resume but earlier turns (another provider answered them, or the
/// session was lost), they ride along as text.
pub async fn chat(chat: &Chat, model: &str, query: String, context: Option<ChatContext>, mode: Mode) -> Result<ChatReply, String> {
    if !enabled() {
        return Err(not_enabled());
    }
    let turn = chat.begin(PROVIDER);
    let system = chat::system_prompt(false);
    let guard = guard_command(mode);
    let context = if mode == Mode::Phone { None } else { context };
    let blocks = crate::claude::user_content(turn.first, context.as_ref(), &query);

    let mut resume = turn.cli_session.clone();
    let answer = loop {
        let mut content = Vec::new();
        if resume.is_none() {
            if let Some(earlier) = transcript(&turn.history) {
                content.push(json!({ "type": "text", "text": earlier }));
            }
        }
        content.extend(blocks.iter().cloned());
        let req = Request { mode, model, system: &system, resume: resume.as_deref(), persist: true, guard: &guard, mcp_config: None };
        match run(&req, content, CHAT_TIMEOUT).await {
            Err(Failure::ResumeLost) if resume.is_some() => resume = None,
            other => break other.map_err(|f| f.message())?,
        }
    };

    let plain = chat::plain_question(turn.first, context.as_ref(), &query);
    chat.commit(&turn, json!({ "role": "user", "content": plain }), json!({ "role": "assistant", "content": answer.text }), &plain, &answer.text);
    if let Some(id) = answer.session_id {
        chat.set_cli_session(&turn, id);
    }
    Ok(ChatReply { text: answer.text })
}

/// Earlier plain turns as one block of text, or None when there are none.
pub(crate) fn transcript(history: &[Value]) -> Option<String> {
    let lines: Vec<String> = history
        .iter()
        .filter_map(|m| {
            let who = match m.get("role")?.as_str()? {
                "user" => "User",
                "assistant" => "Assistant",
                _ => return None,
            };
            let text = match m.get("content")? {
                Value::String(s) => s.clone(),
                Value::Array(blocks) => crate::claude::response_text(blocks)?,
                _ => return None,
            };
            Some(format!("{who}: {text}"))
        })
        .collect();
    (!lines.is_empty()).then(|| format!("Earlier in this conversation:\n\n{}\n\n---", lines.join("\n\n")))
}

/// The models of the subscription, for the picker: fixed, nothing is asked.
pub fn models() -> Vec<ModelInfo> {
    MODELS.iter().map(|(id, label)| ModelInfo { id: id.to_string(), label: label.to_string() }).collect()
}

/// Forgets a mode's conversations: `claude purge` on its folder, in the
/// background (the chat's "New chat", the phone's reset, and at launch).
pub fn forget(mode: Mode) {
    let (Some(launcher), Ok(dir)) = (locate(), work_dir(mode)) else { return };
    tauri::async_runtime::spawn(async move {
        let args = vec!["purge".to_string(), "--yes".to_string(), dir.to_string_lossy().to_string()];
        let mut cmd = command(&launcher, &args, &dir);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Ok(mut child) = cmd.spawn() {
            if tokio::time::timeout(Duration::from_secs(20), child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }
    });
}

// ── Status, for Settings ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CliStatus {
    NotInstalled,
    NotLoggedIn { version: String },
    Subscription { version: String, plan: Option<String> },
    ApiKey { version: String },
    Other { version: String, method: String },
}

/// What `claude auth status` says, reduced to what Settings shows: no email,
/// no organisation, nothing that identifies the account.
pub fn parse_status(version: &str, json: &Value) -> CliStatus {
    let version = version.trim().to_string();
    if json.get("loggedIn").and_then(Value::as_bool) != Some(true) {
        return CliStatus::NotLoggedIn { version };
    }
    let method = json.get("authMethod").and_then(Value::as_str).unwrap_or_default();
    let plan = json.get("subscriptionType").and_then(Value::as_str).map(str::to_string);
    match method {
        "claude.ai" => CliStatus::Subscription { version, plan },
        m if m.contains("api") || m.contains("console") => CliStatus::ApiKey { version },
        m => CliStatus::Other { version, method: m.to_string() },
    }
}

async fn output(launcher: &Launcher, args: &[&str]) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let dir = work_dir(Mode::Complete).ok()?;
    let mut cmd = command(launcher, &args, &dir);
    cmd.stdin(Stdio::null());
    let out = tokio::time::timeout(STATUS_TIMEOUT, cmd.output()).await.ok()?.ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub async fn status() -> CliStatus {
    let Some(launcher) = locate() else { return CliStatus::NotInstalled };
    let version = output(&launcher, &["--version"]).await.unwrap_or_default();
    let json = output(&launcher, &["auth", "status"]).await.and_then(|s| serde_json::from_str::<Value>(&s).ok());
    match json {
        Some(json) => parse_status(&version, &json),
        None => CliStatus::NotLoggedIn { version: version.trim().to_string() },
    }
}

#[cfg(test)]
mod tests;
