// The Linear card (src/views/integrations.ts): what linear.rs sends becomes
// my open issues, each with a button named after its next state, and the
// field that creates an issue in the chosen team.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";

installFakeDom();

const { State, DEFAULT_SETTINGS } = await import("../src/core/state.ts");
const { pillDefinition } = await import("../src/core/pills.ts");
const { hasIntegrationData, renderIntegrationCard } = await import("../src/views/integrations.ts");

const ID = "integration_linear";
const hooks = { detailOpen: false, openDetail() {}, closeDetail() {}, openSettings() {} };
const task = { id: ID, name: "Linear", color: "#5E6AD2", state: "idle", steps: [], stepIndex: 0 };

const DATA = {
  issues: [
    {
      id: "i1", identifier: "ENG-12", title: "Fix the login", url: "https://linear.app/x/issue/ENG-12",
      updatedAt: new Date().toISOString(),
      state: { id: "todo", name: "Todo", type: "unstarted", color: "#e2e2e2" },
      next: { id: "doing", name: "In Progress", type: "started", color: "#f2c94c" },
    },
    {
      id: "i2", identifier: "ENG-13", title: "Ship it", url: "https://linear.app/x/issue/ENG-13",
      updatedAt: new Date().toISOString(),
      state: { id: "doing", name: "In Progress", type: "started", color: "#f2c94c" },
      next: { id: "done", name: "Done", type: "completed", color: "#5e6ad2" },
    },
    {
      id: "i3", identifier: "ENG-14", title: "No next state", url: "https://linear.app/x/issue/ENG-14",
      updatedAt: new Date().toISOString(),
      state: { id: "odd", name: "Odd", type: "unknown", color: "not a colour" },
      next: null,
    },
  ],
  teams: [{ id: "t1", key: "ENG", name: "Engineering" }, { id: "t2", key: "OPS", name: "Operations" }],
};

beforeEach(() => {
  State.settings = { ...DEFAULT_SETTINGS };
  State.integrations[ID] = { data: DATA, error: null, loaded: true, configured: true };
});

test("Linear is a service pill read with its own key", () => {
  const def = pillDefinition(ID);
  assert.equal(def.support, "yes");
  assert.deepEqual(def.connect, { kind: "key", key: "linear-api-key" });
});

test("the card lists my issues with a button to each one's next state", () => {
  const card = renderIntegrationCard(task, hooks);
  const text = card.textContent;
  for (const s of ["ENG-12", "Fix the login", "ENG-13", "Ship it", "ENG-14"]) assert.ok(text.includes(s), s);
  const buttons = card.find(".linear-next").map((b) => b.textContent);
  assert.deepEqual(buttons, ["In Progress", "Done"]);
});

test("a state colour that is not #rrggbb never reaches the style", () => {
  const card = renderIntegrationCard(task, hooks);
  const styles = card.find(".linear-state").map((e) => e.getAttribute("style"));
  assert.ok(styles.every((s) => !s.includes("not a colour")));
});

test("new issues go to the chosen team, else the first", () => {
  const placeholder = () => renderIntegrationCard(task, hooks).find(".linear-input")[0].getAttribute("placeholder");
  assert.ok(placeholder().includes("ENG"));
  State.settings.linearTeam = "t2";
  assert.ok(placeholder().includes("OPS"));
  State.settings.linearTeam = "gone";
  assert.ok(placeholder().includes("ENG"));
});

test("no team, no field; no issue, says so", () => {
  State.integrations[ID] = { data: { issues: [], teams: [] }, error: null, loaded: true, configured: true };
  const card = renderIntegrationCard(task, hooks);
  assert.equal(card.find(".linear-input").length, 0);
  assert.ok(card.textContent.includes("No open issue assigned to you"));
});

test("an error or a first read not done yet leaves the idle card", () => {
  State.integrations[ID] = { data: {}, error: "Invalid API key (401)", loaded: false, configured: true };
  assert.equal(hasIntegrationData(ID), false);
  State.integrations[ID] = { data: {}, error: null, loaded: false, configured: true };
  assert.equal(hasIntegrationData(ID), false);
});
