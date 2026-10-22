// Coucou's own tool `find_app_tools`, as the spike's stand-in: when the user
// names an app, it says which way to act —
//   1. the app is connected: use its own tools (its MCP server, behind the gate);
//   2. the app has an official MCP server, not connected yet: a card offers to
//      connect it (OAuth in the real app); nothing is connected without the click;
//   3. neither: the browser.
// "Official" is decided here, never by the model: the server's registry name
// must sit under the app's own domain (app.linear/… for linear.app). The
// registry is open to anyone, so a name under a hosting domain
// (app.vercel.someone/…, io.github.someone/…) does not count for the app.
import { appendFileSync } from "node:fs";
import { serve } from "./mcp-stdio.mjs";

/** Known apps: their domain, and the official server as the MCP registry lists it (checked 22 Oct 2026). */
export const CATALOG = {
  linear: { title: "Linear", domain: "linear.app", server: "app.linear/linear", url: "https://mcp.linear.app/mcp" },
  notion: { title: "Notion", domain: "notion.com", server: "com.notion/mcp", url: "https://mcp.notion.com/mcp" },
  figma: { title: "Figma", domain: "figma.com", server: "com.figma.mcp/mcp", url: "https://mcp.figma.com/mcp" },
  stripe: { title: "Stripe", domain: "stripe.com", server: "com.stripe/mcp", url: "https://mcp.stripe.com" },
  atlassian: { title: "Atlassian", domain: "atlassian.com", server: "com.atlassian/atlassian-mcp-server", url: "https://mcp.atlassian.com/v2/mcp" },
  jira: { title: "Jira", domain: "atlassian.com", server: "com.atlassian/atlassian-mcp-server", url: "https://mcp.atlassian.com/v2/mcp" },
  // No official server in the registry (22 Oct 2026): the browser.
  slack: { title: "Slack", domain: "slack.com" },
  spotify: { title: "Spotify", domain: "spotify.com" },
  youtube: { title: "YouTube", domain: "youtube.com" },
};

/** Is a registry server name (`com.notion/mcp`) under `domain` (notion.com) or one of its subdomains? */
export function isOfficial(serverName, domain) {
  const host = String(serverName).split("/")[0].split(".").reverse().join(".");
  return host === domain || host.endsWith("." + domain);
}

export function findAppTools(app, connected) {
  const key = String(app).toLowerCase().replace(/[^a-z0-9]/g, "");
  const entry = CATALOG[key];
  if (connected.has(key)) return { text: `${entry?.title ?? app} is connected: use its own tools (mcp__${key}__*), not the browser.` };
  if (entry?.server && isOfficial(entry.server, entry.domain)) {
    const host = new URL(entry.url).host;
    return {
      text: `${entry.title} has an official server (${host}, from ${entry.domain}) but it is not connected. A card to connect it is now shown in the island: tell the user to click it, then ask again. Do not use the browser for ${entry.title} meanwhile.`,
      card: `connect ${entry.title} (${host})`,
    };
  }
  return { text: `No tools for ${entry?.title ?? app}: use the browser.` };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const connected = new Set((process.env.CONNECTED ?? "").split(",").filter(Boolean));
  serve("coucou", [{
    name: "find_app_tools",
    description: "Before acting in an app or website you have no tools for, ask which way to do it: its own tools, connecting it, or the browser.",
    annotations: { readOnlyHint: true },
    inputSchema: { type: "object", properties: { app: { type: "string", description: "The app's name, like Linear or YouTube." } }, required: ["app"] },
  }], async (_tool, a) => {
    const r = findAppTools(a.app, connected);
    if (process.env.GATE_LOG)
      appendFileSync(process.env.GATE_LOG, JSON.stringify({ t: Date.now(), server: "coucou", tool: "find_app_tools", args: a, ...(r.card ? { card: r.card } : { allowed: true }) }) + "\n");
    return r.text;
  });
}
