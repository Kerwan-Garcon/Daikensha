// The least an MCP server over stdio needs (one JSON-RPC message per line),
// for the spike's stand-in servers.
import readline from "node:readline";

export function serve(name, tools, call) {
  const out = (m) => process.stdout.write(JSON.stringify(m) + "\n");
  readline.createInterface({ input: process.stdin }).on("line", async (line) => {
    let m;
    try { m = JSON.parse(line); } catch { return; }
    if (m.id === undefined) return; // notifications
    const ok = (result) => out({ jsonrpc: "2.0", id: m.id, result });
    if (m.method === "initialize")
      return ok({ protocolVersion: m.params?.protocolVersion ?? "2025-06-18", capabilities: { tools: {} }, serverInfo: { name, version: "0.0.1" } });
    if (m.method === "tools/list") return ok({ tools });
    if (m.method === "tools/call") {
      const text = await call(m.params.name, m.params.arguments ?? {});
      return ok({ content: [{ type: "text", text }] });
    }
    if (m.method === "ping") return ok({});
    out({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "method not found" } });
  });
}
