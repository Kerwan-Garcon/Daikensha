// « Coucou Shell »'s panel (src/core/shell.ts): which tabs show, how numbers
// read, how the app search ranks, gauges and sparklines.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  availableTabs, clockText, fold, formatBytes, formatPct, gaugeDash, initial, pushSample, searchApps, sparkline, tileColor,
} from "../src/core/shell.ts";

test("only the tabs this system can fill show, Stats everywhere", () => {
  assert.deepEqual(availableTabs(null), ["stats"]);
  assert.deepEqual(availableTabs({ stats: true, controls: false, apps: false, windows: false }), ["stats"]);
  assert.deepEqual(availableTabs({ stats: true, controls: true, apps: true, windows: true }), ["controls", "stats", "apps", "windows"]);
});

test("numbers read the way the panel shows them", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(5 * 1024 ** 3, ["o", "Ko", "Mo", "Go"]), "5.0 Go");
  assert.equal(formatBytes(NaN), "0 B");
  assert.equal(formatPct(33.6), "34 %");
  assert.equal(formatPct(140), "100 %");
  assert.equal(formatPct(null), "—");
});

test("the app search folds accents and case and ranks starts first", () => {
  const apps = [
    { id: "1", name: "Unicode Picker" },
    { id: "2", name: "Unity Hub" },
    { id: "3", name: "Visual Studio Code" },
    { id: "4", name: "Explorateur de fichiers" },
    { id: "5", name: "Paint" },
    { id: "6", name: "Community Toolkit" },
  ];
  assert.deepEqual(searchApps(apps, "uni").map((a) => a.id), ["2", "1", "6"]);
  assert.deepEqual(searchApps(apps, "code").map((a) => a.id), ["3", "1"]);
  assert.deepEqual(searchApps(apps, "ÉXPLO").map((a) => a.id), ["4"]);
  assert.equal(searchApps(apps, "").length, 6);
  assert.equal(searchApps(apps, "", 2).length, 2);
  assert.deepEqual(searchApps(apps, "zzz"), []);
  assert.equal(fold("Éclairage"), "eclairage");
});

test("tiles, gauges, sparklines and the rolling series", () => {
  assert.equal(initial("7-Zip"), "7");
  assert.equal(initial("  édge"), "É");
  assert.equal(initial("—"), "?");
  assert.equal(tileColor("Edge"), tileColor("Edge"));
  assert.equal(gaugeDash(50, 10), "31.4 62.8");
  assert.equal(gaugeDash(-5, 10), "0.0 62.8");
  assert.equal(gaugeDash(NaN, 10), "0.0 62.8");
  assert.equal(sparkline([], 100, 20), "");
  assert.equal(sparkline([0, 10], 100, 20), "0.0,18.0 100.0,2.0");
  let s = [];
  for (let i = 0; i < 70; i++) s = pushSample(s, i, 60);
  assert.equal(s.length, 60);
  assert.equal(s[59], 69);
});

test("the clock gives a short date and a long one", () => {
  const c = clockText(new Date(2026, 9, 18, 14, 32), "fr");
  assert.equal(c.time, "14:32");
  assert.match(c.short, /18/);
  assert.match(c.long, /octobre/);
});
