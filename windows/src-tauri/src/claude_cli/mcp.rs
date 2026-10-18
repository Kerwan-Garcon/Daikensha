// The MCP server « OK Coucou » hands to its `claude` process in the Claude
// subscription mode, so that Claude Code can use the voice's five tools
// (voice/brain.rs) and nothing else. It only runs while that process does.
//
// Streamable HTTP, answered with plain JSON: one POST per JSON-RPC message.
// Locked down on every request:
//   - 127.0.0.1 only, on a port the system picks;
//   - a bearer token of 32 random bytes, new at every start, compared in
//     constant time; it reaches the CLI through its environment
//     (COUCOU_MCP_TOKEN), never its command line, which other users can read;
//   - a Host that is not this address and port, or any Origin at all (a page
//     in a browser, DNS rebinding), is refused;
//   - only the five tools' names are served; they run voice::run_tool, which
//     never sends, approves or deletes anything.
// No new dependency: the little HTTP it needs is read by hand, with limits.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// The environment variable the CLI reads the token from (`--mcp-config`).
pub const TOKEN_VAR: &str = "COUCOU_MCP_TOKEN";
/// The server's name for Claude Code: its tools are `mcp__coucou__<name>`.
pub const NAME: &str = "coucou";

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;
/// A request must arrive whole within this; a tool may then take its time.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Runs one tool: (result text, whether it changed something).
pub type Runner = Arc<dyn Fn(&str, &Value) -> (String, bool) + Send + Sync>;

pub struct Server {
    pub port: u16,
    pub token: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    /// What `--mcp-config` gets: the address, and the token as a variable.
    pub fn config(&self) -> String {
        json!({
            "mcpServers": {
                NAME: {
                    "type": "http",
                    "url": format!("http://127.0.0.1:{}/mcp", self.port),
                    "headers": { "Authorization": format!("Bearer ${{{TOKEN_VAR}}}") },
                }
            }
        })
        .to_string()
    }
}

fn new_token() -> std::io::Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).map_err(|_| std::io::Error::other("no secure random source"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Starts the server with `tools` (MCP tool definitions) and `run` for them.
pub async fn start(tools: Vec<Value>, run: Runner) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = new_token()?;
    let state = Arc::new(State { port, token: token.clone(), tools, run });
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let state = state.clone();
            tokio::spawn(async move {
                let _ = serve(stream, &state).await;
            });
        }
    });
    Ok(Server { port, token, task })
}

struct State {
    port: u16,
    token: String,
    tools: Vec<Value>,
    run: Runner,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = (&mut reader).take((MAX_HEAD - head.len()) as u64 + 1).read_until(b'\n', &mut line).await.ok()?;
        if n == 0 || head.len() + n > MAX_HEAD {
            return None;
        }
        let end = line == b"\r\n" || line == b"\n";
        head.extend_from_slice(&line);
        if end {
            break;
        }
    }
    let text = String::from_utf8(head).ok()?;
    let mut lines = text.lines();
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length = match headers.iter().find(|(k, _)| k == "content-length") {
        Some((_, v)) => v.parse::<usize>().ok()?,
        None => 0,
    };
    if length > MAX_BODY || headers.iter().any(|(k, _)| k == "transfer-encoding") {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await.ok()?;
    Some(Request { method, path, headers, body })
}

async fn respond(stream: &mut TcpStream, status: &str, body: Option<&Value>) -> std::io::Result<()> {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let kind = if body.is_empty() { "" } else { "Content-Type: application/json\r\n" };
    let allow = if status.starts_with("405") { "Allow: POST\r\n" } else { "" };
    let head = format!("HTTP/1.1 {status}\r\n{kind}{allow}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

async fn serve(mut stream: TcpStream, state: &State) -> std::io::Result<()> {
    let Ok(Some(req)) = tokio::time::timeout(READ_TIMEOUT, read_request(&mut stream)).await else {
        return respond(&mut stream, "400 Bad Request", None).await;
    };
    if let Err(status) = admit(&req, state.port, &state.token) {
        return respond(&mut stream, status, None).await;
    }
    if req.method != "POST" {
        return respond(&mut stream, "405 Method Not Allowed", None).await;
    }
    let Ok(message) = serde_json::from_slice::<Value>(&req.body) else {
        return respond(&mut stream, "400 Bad Request", Some(&rpc_error(Value::Null, -32700, "parse error"))).await;
    };
    match handle(&message, state).await {
        Some(answer) => respond(&mut stream, "200 OK", Some(&answer)).await,
        None => respond(&mut stream, "202 Accepted", None).await,
    }
}

/// The checks every request goes through before anything else is read of it.
fn admit(req: &Request, port: u16, token: &str) -> Result<(), &'static str> {
    let host = req.header("host").unwrap_or_default();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return Err("403 Forbidden");
    }
    if req.header("origin").is_some() {
        return Err("403 Forbidden");
    }
    let given = req.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).unwrap_or_default();
    if !same(given.as_bytes(), token.as_bytes()) {
        return Err("401 Unauthorized");
    }
    if req.path != "/mcp" {
        return Err("404 Not Found");
    }
    Ok(())
}

/// Constant-time equality (the length of a token is no secret).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The answer to one JSON-RPC message, or None for a notification.
async fn handle(message: &Value, state: &State) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": NAME, "version": env!("CARGO_PKG_VERSION") },
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": state.tools }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            if !state.tools.iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name.as_str())) {
                return Some(rpc_error(id, -32602, "unknown tool"));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let run = state.run.clone();
            let call = tokio::task::spawn_blocking(move || run(&name, &args));
            match tokio::time::timeout(TOOL_TIMEOUT, call).await {
                Ok(Ok((text, _))) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
                _ => json!({ "content": [{ "type": "text", "text": "the tool did not answer" }], "isError": true }),
            }
        }
        _ => return Some(rpc_error(id, -32601, "method not found")),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// The voice's tools in MCP's shape (`inputSchema`).
pub fn tool_definitions(voice_tools: &[Value]) -> Vec<Value> {
    voice_tools
        .iter()
        .filter_map(|t| {
            Some(json!({
                "name": t.get("name")?.as_str()?,
                "description": t.get("description").and_then(Value::as_str).unwrap_or_default(),
                "inputSchema": t.get("input_schema")?.clone(),
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn post(port: u16, headers: &[(&str, String)], body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut head = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        s.write_all(head.as_bytes()).await.unwrap();
        s.write_all(body.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        let status = out[9..12].parse().unwrap();
        let body = out.split("\r\n\r\n").nth(1).unwrap_or_default();
        (status, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    fn tools() -> Vec<Value> {
        tool_definitions(&[json!({"name":"pills","description":"d","input_schema":{"type":"object"}})])
    }

    async fn server() -> Server {
        let run: Runner = Arc::new(|name: &str, args: &Value| (format!("{name} ran with {args}"), true));
        start(tools(), run).await.unwrap()
    }

    fn good(s: &Server) -> Vec<(&'static str, String)> {
        vec![("Host", format!("127.0.0.1:{}", s.port)), ("Authorization", format!("Bearer {}", s.token))]
    }

    const LIST: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

    #[tokio::test]
    async fn the_token_is_new_long_and_kept_out_of_the_config() {
        let a = server().await;
        let b = server().await;
        assert_eq!(a.token.len(), 64);
        assert_ne!(a.token, b.token);
        assert!(!a.config().contains(&a.token));
        assert!(a.config().contains("${COUCOU_MCP_TOKEN}"));
        assert!(a.config().contains(&format!("http://127.0.0.1:{}/mcp", a.port)));
    }

    #[tokio::test]
    async fn no_token_a_wrong_one_an_origin_or_another_host_is_refused() {
        let s = server().await;
        let host = ("Host", format!("127.0.0.1:{}", s.port));
        assert_eq!(post(s.port, std::slice::from_ref(&host), LIST).await.0, 401);
        assert_eq!(post(s.port, &[host.clone(), ("Authorization", "Bearer nope".into())], LIST).await.0, 401);
        let mut wrong = s.token.clone();
        wrong.replace_range(0..1, if wrong.starts_with('a') { "b" } else { "a" });
        assert_eq!(post(s.port, &[host.clone(), ("Authorization", format!("Bearer {wrong}"))], LIST).await.0, 401);
        let mut with_origin = good(&s);
        with_origin.push(("Origin", "http://evil.example".into()));
        assert_eq!(post(s.port, &with_origin, LIST).await.0, 403);
        let rebound = vec![("Host", format!("evil.example:{}", s.port)), ("Authorization", format!("Bearer {}", s.token))];
        assert_eq!(post(s.port, &rebound, LIST).await.0, 403);
        assert_eq!(post(s.port, &good(&s), LIST).await.0, 200);
    }

    #[tokio::test]
    async fn it_speaks_mcp_and_serves_only_its_tools() {
        let s = server().await;
        let (status, init) = post(s.port, &good(&s), r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).await;
        assert_eq!(status, 200);
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["name"], "coucou");
        assert_eq!(post(s.port, &good(&s), r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await.0, 202);
        let (_, list) = post(s.port, &good(&s), LIST).await;
        assert_eq!(list["result"]["tools"][0]["name"], "pills");
        assert!(list["result"]["tools"][0].get("inputSchema").is_some());
        let (_, call) = post(s.port, &good(&s), r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"pills","arguments":{"action":"list"}}}"#).await;
        assert_eq!(call["result"]["content"][0]["text"], r#"pills ran with {"action":"list"}"#);
        let (_, bad) = post(s.port, &good(&s), r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"bash","arguments":{}}}"#).await;
        assert_eq!(bad["error"]["code"], -32602);
        let (_, unknown) = post(s.port, &good(&s), r#"{"jsonrpc":"2.0","id":4,"method":"server/discover"}"#).await;
        assert_eq!(unknown["error"]["code"], -32601);
        assert_eq!(post(s.port, &good(&s), "{").await.0, 400);
    }

    #[test]
    fn equality_is_exact() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
        assert!(!same(b"", b"a"));
    }
}
