// The browser gate, as a prototype of what Coucou will do in Rust
// (docs/BROWSER_AGENT.md). It sits between Claude and Playwright MCP: a stdio
// MCP server for Claude, a stdio MCP client of `@playwright/mcp`.
//
// The model never decides what is risky. This file does:
//   - only the tools in ALLOWED reach the browser (no evaluate, no code, no
//     file upload, no install);
//   - navigate: http(s) only;
//   - click: the element is looked up in the page's last snapshot by its ref
//     (never by the model's description of it, and never by a CSS selector,
//     which Playwright MCP also accepts: the gate couldn't name what it
//     points at); a button or link whose name
//     buys, pays, sends, posts, deletes or subscribes is not clicked: a card
//     would go up in the island and only the user's click runs it;
//   - type: never into a password or payment field;
//   - Enter (type with submit, or press_key) only submits a search: in any
//     other field (a message, a comment) it may send, so a card goes up.
// Every decision is appended to GATE_LOG (one JSON line each).
//
// Usage (as an MCP server for `claude --mcp-config`):
//   node gate.mjs -- <playwright mcp command and args>

import { spawn } from "node:child_process";
import { appendFileSync } from "node:fs";
import readline from "node:readline";

export const ALLOWED = new Set([
  "browser_navigate",
  "browser_navigate_back",
  "browser_snapshot",
  "browser_click",
  "browser_type",
  "browser_press_key",
  "browser_select_option",
  "browser_hover",
  "browser_wait_for",
  "browser_tabs",
]);

// Names that commit the user to something. French and English, as the user speaks.
export const SENSITIVE = /\b(achet|acheter|buy|commander|commande|order|pay|payer|paiement|checkout|valider la commande|place order|envoyer|send|publier|post|tweet|supprimer|delete|remove account|s'abonner|subscribe|souscrire|transf[ée]rer|transfer|virement|confirmer le paiement|donate|faire un don)/i;
const SECRET_FIELD = /(password|mot de passe|passcode|carte|card number|cvv|cvc|iban|code secret|pin\b)/i;

const SEARCH_FIELD = /(search|recherch|cherch|query|find|trouver)/i;
const isSearch = (el) => el.role === "searchbox" || SEARCH_FIELD.test(el.name);
const FIELD_ROLES = new Set(["textbox", "searchbox", "combobox"]);

export const CARD_SHOWN = "A confirmation card is now shown in the island. Nothing has happened yet: it happens only if the user clicks the card's button. Tell the user to check the card; never say it is done.";

/** ref → { role, name } from a Playwright MCP snapshot (`- button "Acheter" [ref=e12]`). */
export function refsFrom(text) {
  const refs = new Map();
  const line = /-\s+([a-z]+)(?:\s+"((?:[^"\\]|\\.)*)")?[^\n]*?\[ref=([^\]\s]+)\]/g;
  for (const m of text.matchAll(line)) refs.set(m[3], { role: m[1], name: (m[2] ?? "").replace(/\\"/g, '"') });
  return refs;
}

/** The snapshot ref a call points at: `e12`, or the one inside `button "Lecture" [ref=e12]`. */
export function refOf(args) {
  const t = String(args.target ?? args.ref ?? "").trim();
  const inside = t.match(/\[ref=([^\]\s]+)\]/);
  if (inside) return inside[1];
  return /^[a-z]?\d*e\d+$/.test(t) ? t : null;
}

/** What the gate does with one call: { allow } or { deny: text for Claude, card?: summary }.
 *  `state.field` is the last field written in or clicked, for press_key Enter. */
export function decide(tool, args, refs, state = {}) {
  if (!ALLOWED.has(tool)) return { deny: `The tool ${tool} is not available.` };
  if (tool === "browser_navigate") {
    let u;
    try { u = new URL(String(args.url ?? "")); } catch { return { deny: "Not a web address." }; }
    if (u.protocol !== "https:" && u.protocol !== "http:") return { deny: "Only http and https pages can be opened." };
    return { allow: true };
  }
  if (tool === "browser_click" || tool === "browser_type" || tool === "browser_select_option" || tool === "browser_hover") {
    const ref = refOf(args);
    if (!ref) return { deny: "Give the element's ref from browser_snapshot (like e12), not a selector." };
    const el = refs.get(ref);
    if (!el) return { deny: `${ref} is not in the last snapshot: take a browser_snapshot first.` };
    if (tool === "browser_click" && SENSITIVE.test(el.name))
      return { deny: CARD_SHOWN, card: `${el.role} « ${el.name} »` };
    if (tool === "browser_type") {
      if (SECRET_FIELD.test(el.name)) return { deny: "Coucou never types into a password or payment field: the user types it." };
      if (args.submit && !isSearch(el)) return { deny: CARD_SHOWN, card: `send « ${String(args.text ?? "").slice(0, 80)} » in « ${el.name} »` };
    }
    if ((tool === "browser_type" || tool === "browser_click") && FIELD_ROLES.has(el.role)) state.field = el;
  }
  if (tool === "browser_press_key" && /^(enter|return)$/i.test(String(args.key ?? "")) && state.field && !isSearch(state.field))
    return { deny: CARD_SHOWN, card: `press Enter in « ${state.field.name} »` };
  return { allow: true };
}

function main() {
  const sep = process.argv.indexOf("--");
  const [cmd, ...cmdArgs] = process.argv.slice(sep + 1);
  const log = (e) => process.env.GATE_LOG && appendFileSync(process.env.GATE_LOG, JSON.stringify({ t: Date.now(), ...e }) + "\n");
  const child = spawn(cmd, cmdArgs, { stdio: ["pipe", "pipe", "inherit"] });
  const out = (msg) => process.stdout.write(JSON.stringify(msg) + "\n");
  const pending = new Map(); // id → method, for the answers we rewrite
  let refs = new Map();
  const state = {};

  readline.createInterface({ input: process.stdin }).on("line", (line) => {
    let msg;
    try { msg = JSON.parse(line); } catch { return; }
    if (msg.method === "tools/call") {
      const { name, arguments: args = {} } = msg.params ?? {};
      const d = decide(name, args, refs, state);
      log({ tool: name, args, ...(d.allow ? { allowed: true } : { denied: d.deny.slice(0, 60), card: d.card }) });
      if (!d.allow) return out({ jsonrpc: "2.0", id: msg.id, result: { content: [{ type: "text", text: d.deny }], isError: !d.card } });
    }
    if (msg.id !== undefined && msg.method) pending.set(msg.id, msg.method);
    child.stdin.write(JSON.stringify(msg) + "\n");
  });

  readline.createInterface({ input: child.stdout }).on("line", (line) => {
    let msg;
    try { msg = JSON.parse(line); } catch { return; }
    const method = pending.get(msg.id);
    if (method) pending.delete(msg.id);
    if (method === "tools/list" && msg.result?.tools) {
      msg.result.tools = msg.result.tools.filter((t) => ALLOWED.has(t.name));
    }
    if (method === "tools/call" && Array.isArray(msg.result?.content)) {
      const text = msg.result.content.filter((c) => c.type === "text").map((c) => c.text).join("\n");
      const fresh = refsFrom(text);
      if (fresh.size) refs = fresh;
    }
    out(msg);
  });

  child.on("exit", (code) => process.exit(code ?? 0));
  process.stdin.on("end", () => child.kill());
}

if (import.meta.url === `file://${process.argv[1]}`) main();
