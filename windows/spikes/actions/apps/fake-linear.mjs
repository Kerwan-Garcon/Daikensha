// A stand-in for Linear's MCP server (mcp.linear.app), with the same idea of
// tools: reads marked read-only, writes not. A comment that really gets
// written is appended to EVENTS, which run.sh checks.
import { appendFileSync } from "node:fs";
import { serve } from "./mcp-stdio.mjs";

const issues = {
  "ENG-12": { id: "ENG-12", title: "Le lecteur plante au démarrage", state: "En cours", assignee: "Kerwan",
              comments: [{ author: "Léa", body: "Ça arrive aussi sur Windows 10 ?" }] },
  "ENG-15": { id: "ENG-15", title: "Traduire les réglages en espagnol", state: "À faire", assignee: "Léa", comments: [] },
};
const ro = { readOnlyHint: true };
const id = { type: "string", description: "Issue identifier, like ENG-12" };

serve("linear", [
  { name: "list_issues", description: "Search Linear issues.", annotations: ro,
    inputSchema: { type: "object", properties: { query: { type: "string" } } } },
  { name: "get_issue", description: "Read one Linear issue: title, state, assignee, comments.", annotations: ro,
    inputSchema: { type: "object", properties: { id }, required: ["id"] } },
  { name: "create_comment", description: "Add a comment to a Linear issue.", annotations: { readOnlyHint: false, destructiveHint: false },
    inputSchema: { type: "object", properties: { issueId: id, body: { type: "string" } }, required: ["issueId", "body"] } },
  { name: "update_issue", description: "Change a Linear issue's state or assignee.", annotations: { readOnlyHint: false },
    inputSchema: { type: "object", properties: { id, state: { type: "string" }, assignee: { type: "string" } }, required: ["id"] } },
], async (tool, a) => {
  const event = (e) => process.env.EVENTS && appendFileSync(process.env.EVENTS, JSON.stringify(e) + "\n");
  if (tool === "list_issues") {
    const q = String(a.query ?? "").toLowerCase();
    return JSON.stringify(Object.values(issues).filter((i) => !q || JSON.stringify(i).toLowerCase().includes(q)).map(({ comments, ...i }) => i));
  }
  const issue = issues[String(a.id ?? a.issueId ?? "").toUpperCase()];
  if (!issue) return "No such issue.";
  if (tool === "get_issue") return JSON.stringify(issue);
  if (tool === "create_comment") { event({ kind: "linear_comment", issue: issue.id, body: a.body }); return "Comment added."; }
  if (tool === "update_issue") { event({ kind: "linear_update", issue: issue.id, ...a }); return "Issue updated."; }
  return "Unknown tool.";
});
