// « Coucou Shell »'s panel (docs/WINDOWS_SHELL.md): the island folds down into
// tabs — Controls, Today, Stats, Clipboard, Apps, Wallpapers, Power, Windows
// (the second four in views/shell-tabs.ts). Off unless Settings turns it on.
// Mochi stays the character: the island draws him in the panel's header.
//
// Nothing runs while the panel is not on screen: Stats samples once a second
// and Windows refreshes every two seconds only while their tab shows, and
// both stop the moment the island folds or another tab is picked.

import {
  Bridge, type ShellApp, type ShellCapabilities, type ShellControls, type ShellLayout, type ShellSnap, type ShellStats,
  type ShellWindow,
} from "../core/bridge";
import {
  appColor, availableTabs, clockText, formatBytes, formatPct, initial, pushSample, searchApps, sparkline, type ShellTab,
} from "../core/shell";
import { N_, language, t, tl, type Msg } from "../i18n/i18n";
import { clear, h, svg } from "./dom";
import { card, cardTitle, chipRow, foot, linkButton, message, note, ring, switchButton, type Tab } from "./shell-parts";
import { buildClipboard, buildPower, buildToday, buildWallpapers } from "./shell-tabs";
import type { ViewHost } from "./views";
import { colorHex } from "../design/tokens";
import { itemAt } from "../core/arrays";

const ICON: Record<ShellTab, string> = {
  controls: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
  stats: "M22 12h-4l-3 9L9 3l-3 9H2",
  apps: "M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z",
  windows: "M3 3h18v18H3zM12 3v18M12 12h9",
  today: "M3 5h18v16H3zM3 10h18M8 3v4M16 3v4",
  clipboard: "M9 3h6v4H9zM7 5H5v16h14V5h-2",
  wallpapers: "M3 4h18v16H3zM3 16l5-5 4 4 3-3 6 6M15.5 8.5h.01",
  power: "M12 2v10M5.6 6.6a8 8 0 1 0 12.8 0",
};

const LABEL: Record<ShellTab, () => Msg> = {
  controls: () => tl("Controls"),
  stats: () => tl("Stats"),
  apps: () => tl("Apps"),
  windows: () => tl("Open windows"),
  today: () => tl("Today"),
  clipboard: () => tl("Clipboard"),
  wallpapers: () => tl("Wallpapers"),
  power: () => tl("Power"),
};

const UNITS = (): string[] => [t("B"), t("KB"), t("MB"), t("GB"), t("TB")];

// ── Stats ─────────────────────────────────────────────────────────────────────

function gauge(name: Msg, color: string): { el: HTMLElement; set(pct: number | null, detail: string): void } {
  const r = ring(color);
  const detail = h("span", { class: "shell-dim" });
  const el = card("shell-gauge", r.el, h("div", { class: "shell-gauge-text" }, h("span", { class: "shell-card-title", text: name }), detail));
  return {
    el,
    set(pct, d) {
      r.set(pct ?? 0);
      r.el.setAttribute("aria-label", `${String(name)} ${formatPct(pct)}`);
      detail.textContent = d;
    },
  };
}

function buildStats(): Tab {
  const cpu = gauge(tl("CPU"), "var(--shell-accent)");
  const gpu = gauge(tl("GPU"), colorHex("--violet-76"));
  const ram = gauge(tl("RAM"), colorHex("--color-success-alt"));
  const ns = "http://www.w3.org/2000/svg";
  const chart = document.createElementNS(ns, "svg");
  chart.setAttribute("viewBox", "0 0 240 80");
  chart.setAttribute("preserveAspectRatio", "none");
  chart.setAttribute("class", "shell-spark");
  const line = (cls: string) => {
    const p = document.createElementNS(ns, "polyline");
    p.setAttribute("class", cls);
    p.setAttribute("fill", "none");
    p.setAttribute("vector-effect", "non-scaling-stroke");
    chart.append(p);
    return p;
  };
  const downLine = line("down");
  const upLine = line("up");
  const down = h("span", { class: "shell-mono shell-accent" });
  const up = h("span", { class: "shell-mono shell-dim" });
  const net = card("shell-net", cardTitle(tl("Network"), h("span", { class: "shell-dim", text: tl("60 s") })), chart, h("div", { class: "shell-row shell-row-foot" }, down, up));
  const topList = h("div", { class: "shell-top" });
  const top = card("shell-span2", cardTitle(tl("Using the most"), h("span", { class: "shell-dim", text: tl("CPU · memory") })), topList);
  const own = h("span", { class: "shell-dim" });
  const el = h(
    "div",
    { class: "shell-tab" },
    h("div", { class: "shell-grid3" }, cpu.el, gpu.el, ram.el, net, top),
    foot(tl("Updated every second, only while this panel is open"), own),
  );

  let timer: number | null = null;
  let tick = 0;
  let rx: number[] = [];
  let tx: number[] = [];

  async function sample() {
    let s: ShellStats;
    try {
      s = await Bridge.shellStats(tick++ % 2 === 0);
    } catch {
      return;
    }
    const u = UNITS();
    cpu.set(s.cpu, "");
    gpu.el.style.display = s.gpu == null ? "none" : "";
    gpu.set(s.gpu, "");
    ram.set(s.memTotal > 0 ? (100 * s.memUsed) / s.memTotal : null, `${formatBytes(s.memUsed, u)} / ${formatBytes(s.memTotal, u)}`);
    rx = pushSample(rx, s.netRx);
    tx = pushSample(tx, s.netTx);
    downLine.setAttribute("points", sparkline(rx, 240, 80));
    upLine.setAttribute("points", sparkline(tx, 240, 80));
    down.textContent = `↓ ${formatBytes(s.netRx, u)}/s`;
    up.textContent = `↑ ${formatBytes(s.netTx, u)}/s`;
    if (s.top.length > 0) {
      const max = Math.max(1, ...s.top.map((p) => p.cpu));
      topList.replaceChildren(
        ...s.top.map((p) =>
          h(
            "div",
            { class: "shell-top-row" },
            h("span", { class: "shell-ellipsis", text: p.name }),
            h("span", { class: "shell-bar" }, h("i", { style: `width:${Math.max(2, (100 * p.cpu) / max)}%` })),
            h("span", { class: "shell-mono", text: formatPct(p.cpu) }),
            h("span", { class: "shell-mono shell-dim", text: formatBytes(p.mem, u) }),
          ),
        ),
      );
    }
    if (s.ownCpu != null && s.ownMem != null) {
      own.textContent = t("Coucou: {cpu} CPU · {mem}", { cpu: formatPct(s.ownCpu), mem: formatBytes(s.ownMem, u) });
    }
  }

  return {
    el,
    show() {
      if (timer != null) return;
      tick = 0;
      void sample();
      timer = window.setInterval(() => void sample(), 1000);
    },
    hide() {
      if (timer != null) window.clearInterval(timer);
      timer = null;
      rx = [];
      tx = [];
      void Bridge.shellStatsReset();
    },
  };
}

// ── Controls ──────────────────────────────────────────────────────────────────

function buildControls(): Tab {
  const info = note();
  const body = h("div", { class: "shell-grid3" });
  const el = h("div", { class: "shell-tab" }, body, info.el);
  let volumeTimer: number | null = null;

  async function act(p: Promise<unknown>) {
    try {
      await p;
    } catch (e) {
      info.say(message(e));
    }
    await refresh();
  }

  function draw(c: ShellControls) {
    const level = Math.round((c.volume ?? 0) * 100);
    const value = h("span", { class: "shell-mono shell-dim shell-value", text: `${level} %` });
    const slider = h("input", { type: "range", min: "0", max: "100", value: String(level), "aria-label": t("Volume") }) as HTMLInputElement;
    slider.disabled = c.volume == null;
    slider.addEventListener("input", () => {
      value.textContent = `${slider.value} %`;
      if (volumeTimer != null) window.clearTimeout(volumeTimer);
      volumeTimer = window.setTimeout(() => {
        Bridge.shellSetVolume(Number(slider.value) / 100).catch((e) => info.say(message(e)));
      }, 60);
    });
    const mute = h(
      "button",
      { class: c.muted ? "shell-round on" : "shell-round", "aria-label": t("Mute"), "aria-pressed": String(!!c.muted), onclick: () => void act(Bridge.shellSetMute(!c.muted)) },
      svg(c.muted ? "M11 5 6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6" : "M11 5 6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14", 18, { stroke: 2 }),
    );
    const sound = card(
      "shell-span2",
      cardTitle(tl("Sound"), value),
      h("div", { class: "shell-row" }, mute, slider),
      h(
        "div",
        { class: "shell-row shell-between" },
        h("span", { class: "shell-dim shell-ellipsis", text: c.output ? t("Output: {name}", { name: c.output }) : "" }),
        linkButton(tl("Pick the output in Settings"), () => void act(Bridge.shellOpenSettings("sound"))),
      ),
    );
    const screen = card(
      "",
      cardTitle(tl("Screen and colours")),
      chipRow(tl("Light or dark theme"), () => void act(Bridge.shellOpenSettings("colors"))),
      chipRow(tl("Night light"), () => void act(Bridge.shellOpenSettings("nightLight"))),
    );
    const radios = c.radios
      .filter((r) => r.kind === "wifi" || r.kind === "bluetooth")
      .map((r) => {
        const name = r.kind === "wifi" ? t("Wi-Fi") : t("Bluetooth");
        const icon = r.kind === "wifi" ? "M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M2 9a15 15 0 0 1 20 0M12 20h.01" : "m7 7 10 10-5 5V2l5 5L7 17";
        return card(
          "shell-radio",
          h("span", { class: r.on ? "shell-badge on" : "shell-badge" }, svg(icon, 20, { stroke: 2 })),
          h("div", { class: "shell-grow" }, h("span", { class: "shell-card-title", text: name }), h("span", { class: "shell-dim", text: r.on ? tl("On") : tl("Off") })),
          switchButton(name, r.on, () => void act(Bridge.shellSetRadio(r.kind, !r.on))),
        );
      });
    // Windows has no API to switch airplane mode: the card opens its page in Settings.
    const airplane = h(
      "button",
      { class: "shell-card shell-radio shell-card-link", onclick: () => void act(Bridge.shellOpenSettings("network")) },
      h("span", { class: "shell-badge" }, svg("M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2z", 20, { stroke: 2 })),
      h("span", { class: "shell-grow" }, h("span", { class: "shell-card-title", text: tl("Airplane mode") }), h("span", { class: "shell-dim", text: tl("In Settings") })),
      svg("M7 17 17 7M8 7h9v9", 14, { stroke: 2 }),
    );
    const adjustable = c.displays.filter((d) => d.brightness != null);
    const bright = card(
      "shell-span3",
      cardTitle(tl("Brightness")),
      adjustable.length === 0
        ? h(
            "div",
            { class: "shell-row shell-between" },
            h("span", { class: "shell-dim", text: tl("No screen here lets Coucou change its brightness.") }),
            linkButton(tl("Display settings"), () => void act(Bridge.shellOpenSettings("display"))),
          )
        : h(
            "div",
            { class: "shell-grid2" },
            ...adjustable.map((d) => {
              const v = h("span", { class: "shell-mono", text: `${d.brightness} %` });
              const r = h("input", { type: "range", min: "0", max: "100", value: String(d.brightness), "aria-label": d.name }) as HTMLInputElement;
              let timer: number | null = null;
              r.addEventListener("input", () => {
                v.textContent = `${r.value} %`;
                if (timer != null) window.clearTimeout(timer);
                timer = window.setTimeout(() => {
                  Bridge.shellSetBrightness(d.index, Number(r.value)).catch((e) => info.say(message(e)));
                }, 150);
              });
              return h("label", { class: "shell-slider" }, h("span", { class: "shell-row shell-between shell-dim" }, h("span", { text: d.name }), v), r);
            }),
          ),
    );
    body.replaceChildren(sound, screen, ...radios, airplane, bright);
  }

  async function refresh() {
    try {
      draw(await Bridge.shellControls());
    } catch (e) {
      info.say(message(e));
    }
  }

  return { el, show: () => void refresh(), hide() {} };
}

// ── Apps ──────────────────────────────────────────────────────────────────────

const RECENT_KEY = "coucou.shell.recentApps";

function readRecent(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string").slice(0, 12) : [];
  } catch {
    return [];
  }
}

function writeRecent(ids: string[]) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(ids.slice(0, 12)));
  } catch {
    // Private storage off: the recents just aren't remembered.
  }
}

function tile(app: ShellApp, size: number): HTMLElement {
  return h("span", { class: "shell-tile", style: `width:${size}px;height:${size}px;background:${appColor(app.name)};font-size:${Math.round(size * 0.42)}px`, text: initial(app.name) });
}

function buildApps(): Tab {
  const info = note();
  const input = h("input", { type: "text", class: "shell-search-input", placeholder: tl("Search an app"), "aria-label": tl("Search an app"), spellcheck: "false" }) as HTMLInputElement;
  const search = h("label", { class: "shell-search" }, svg("M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3", 18, { stroke: 2 }), input);
  const results = h("div", { class: "shell-results" });
  const recentGrid = h("div", { class: "shell-tiles" });
  const el = h(
    "div",
    { class: "shell-tab" },
    search,
    h(
      "div",
      { class: "shell-grid5" },
      card("shell-span2 shell-list", h("span", { class: "shell-dim shell-pad", text: tl("Results") }), results),
      card("shell-span3", cardTitle(tl("Recent")), recentGrid),
    ),
    foot(tl("Start menu and Store apps. Never an arbitrary path."), tl("Enter to launch")),
    info.el,
  );
  let apps: ShellApp[] = [];
  let shown: ShellApp[] = [];

  async function launch(app: ShellApp) {
    try {
      await Bridge.shellAppLaunch(app.id);
      writeRecent([app.id, ...readRecent().filter((id) => id !== app.id)]);
      input.value = "";
      draw();
    } catch (e) {
      info.say(message(e));
    }
  }

  function draw() {
    shown = searchApps(apps, input.value, 6);
    results.replaceChildren(
      ...shown.map((a, i) =>
        h("button", { class: i === 0 && input.value ? "shell-result on" : "shell-result", onclick: () => void launch(a) }, tile(a, 36), h("span", { class: "shell-ellipsis", text: a.name })),
      ),
    );
    const byId = new Map(apps.map((a) => [a.id, a]));
    const recent = readRecent().map((id) => byId.get(id)).filter((a): a is ShellApp => !!a);
    clear(recentGrid);
    if (recent.length === 0) recentGrid.append(h("span", { class: "shell-dim shell-empty", text: tl("Apps you launch from here show up here.") }));
    for (const a of recent) {
      recentGrid.append(h("button", { class: "shell-tile-btn", onclick: () => void launch(a) }, tile(a, 56), h("span", { class: "shell-ellipsis", text: a.name })));
    }
  }

  input.addEventListener("input", draw);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && shown[0]) void launch(shown[0]);
    if (e.key === "Escape" && input.value) {
      input.value = "";
      draw();
      e.stopPropagation();
    }
  });

  return {
    el,
    show() {
      // The search field needs the keyboard, which the island only takes on demand.
      void Bridge.focusWindow(true);
      window.setTimeout(() => input.focus(), 120);
      Bridge.shellApps()
        .then((list) => {
          apps = list;
          draw();
        })
        .catch((e) => info.say(message(e)));
    },
    hide() {
      input.blur();
      void Bridge.focusWindow(false);
    },
  };
}

// ── Windows ───────────────────────────────────────────────────────────────────

const SNAPS: [ShellSnap, string, string][] = [
  ["left", N_("Left half"), "left:0;top:0;width:50%;height:100%"],
  ["right", N_("Right half"), "left:50%;top:0;width:50%;height:100%"],
  ["full", N_("Full screen"), "left:0;top:0;width:100%;height:100%"],
  ["center", N_("Centre"), "left:20%;top:15%;width:60%;height:70%"],
  ["topLeft", N_("Top left quarter"), "left:0;top:0;width:50%;height:50%"],
  ["topRight", N_("Top right quarter"), "left:50%;top:0;width:50%;height:50%"],
  ["bottomLeft", N_("Bottom left quarter"), "left:0;top:50%;width:50%;height:50%"],
  ["bottomRight", N_("Bottom right quarter"), "left:50%;top:50%;width:50%;height:50%"],
];

function buildWindows(): Tab {
  const info = note();
  const list = h("div", { class: "shell-wins" });
  const placeTitle = h("span", { class: "shell-card-title shell-ellipsis" });
  const snaps = h("div", { class: "shell-snaps" });
  const screens = h("div", { class: "shell-screens" });
  const layoutList = h("div", { class: "shell-layouts" });
  const nameInput = h("input", { type: "text", class: "shell-name-input", placeholder: t("Layout name"), "aria-label": t("Layout name"), maxlength: "40" }) as HTMLInputElement;
  const saveRow = h("div", { class: "shell-row", style: "display:none" }, nameInput, h("button", { class: "shell-pill", onclick: () => void saveLayout(), text: tl("Save") }));
  const el = h(
    "div",
    { class: "shell-tab" },
    h(
      "div",
      { class: "shell-grid5" },
      card("shell-span3 shell-list", list),
      h(
        "div",
        { class: "shell-span2 shell-stack" },
        card("", placeTitle, snaps, screens),
        card(
          "",
          cardTitle(tl("My layouts"), h("button", { class: "shell-text-btn", text: tl("Save the current one"), onclick: () => { saveRow.style.display = ""; void Bridge.focusWindow(true); nameInput.focus(); } })),
          saveRow,
          layoutList,
        ),
      ),
    ),
    info.el,
  );
  let wins: ShellWindow[] = [];
  let selected: string | null = null;
  let timer: number | null = null;

  async function act(id: string, action: Parameters<typeof Bridge.shellWindowAction>[1]) {
    try {
      await Bridge.shellWindowAction(id, action);
    } catch (e) {
      info.say(message(e));
    }
    await refresh();
  }

  function drawPlace() {
    const target = wins.find((w) => w.id === selected);
    placeTitle.textContent = target ? t("Place “{title}”", { title: target.title }) : t("Pick a window");
    snaps.replaceChildren(
      ...SNAPS.map(([to, label, area]) =>
        h(
          "button",
          { class: "shell-snap", "aria-label": t(label), title: t(label), disabled: !target, onclick: () => target && void act(target.id, { kind: "snap", to }) },
          h("span", { class: "shell-snap-screen" }, h("i", { style: area })),
        ),
      ),
    );
    const count = Math.max(1, ...wins.map((w) => w.screen + 1));
    screens.style.display = count > 1 ? "" : "none";
    screens.replaceChildren(
      ...Array.from({ length: count }, (_, i) =>
        h("button", { class: target?.screen === i ? "shell-chip on" : "shell-chip", disabled: !target, onclick: () => target && void act(target.id, { kind: "screen", index: i }), text: t("Screen {n}", { n: String(i + 1) }) }),
      ),
    );
  }

  function draw() {
    if (!wins.some((w) => w.id === selected)) selected = wins.find((w) => w.focused)?.id ?? wins[0]?.id ?? null;
    list.replaceChildren(
      ...wins.map((w) =>
        h(
          "div",
          { class: w.id === selected ? "shell-win on" : "shell-win" },
          h("i", { class: "shell-win-dot", style: `background:${appColor(w.app)}` }),
          h(
            "button",
            { class: "shell-win-main", onclick: () => { selected = w.id; draw(); void act(w.id, { kind: "focus" }); } },
            h("span", { class: "shell-ellipsis", text: w.title }),
            h("span", { class: "shell-dim", text: `${w.app} · ${t("Screen {n}", { n: String(w.screen + 1) })}` }),
          ),
          h("button", { class: "shell-icon-btn", "aria-label": t("Minimise"), title: t("Minimise"), onclick: () => void act(w.id, { kind: "minimize" }) }, svg("M5 12h14", 16, { stroke: 2 })),
          h("button", { class: "shell-icon-btn", "aria-label": t("Maximise"), title: t("Maximise"), onclick: () => void act(w.id, { kind: "maximize" }) }, svg("M5 5h14v14H5z", 16, { stroke: 2 })),
        ),
      ),
    );
    drawPlace();
  }

  /** A layout's picture: one frame per screen, one bar per window on it. */
  function layoutThumb(l: ShellLayout): HTMLElement {
    const count = Math.max(1, ...l.windows.map((w) => w.screen + 1));
    return h(
      "span",
      { class: "shell-layout-thumb", "aria-hidden": "true" },
      ...Array.from({ length: count }, (_, s) => h("div", {}, ...l.windows.filter((w) => w.screen === s).map(() => h("i", {})))),
    );
  }

  function drawLayouts(layouts: ShellLayout[]) {
    clear(layoutList);
    if (layouts.length === 0) layoutList.append(h("span", { class: "shell-dim", text: tl("Save how your windows sit now, and put them back in one click.") }));
    for (const l of layouts) {
      layoutList.append(
        h(
          "div",
          { class: "shell-layout" },
          h(
            "button",
            { class: "shell-layout-main", onclick: () => Bridge.shellLayoutApply(l.name).then(() => refresh(), (e) => info.say(message(e))) },
            layoutThumb(l),
            h(
              "span",
              { class: "shell-grow" },
              h("span", { class: "shell-card-title shell-ellipsis", text: l.name }),
              h("span", { class: "shell-dim shell-ellipsis", text: l.windows.map((w) => w.app).join(", ") }),
            ),
          ),
          h("button", { class: "shell-icon-btn", "aria-label": t("Delete"), title: t("Delete"), onclick: () => Bridge.shellLayoutDelete(l.name).then(drawLayouts, (e) => info.say(message(e))) }, svg("M6 6l12 12M18 6 6 18", 14, { stroke: 2 })),
        ),
      );
    }
  }

  async function saveLayout() {
    try {
      drawLayouts(await Bridge.shellLayoutSave(nameInput.value));
      nameInput.value = "";
      saveRow.style.display = "none";
      void Bridge.focusWindow(false);
    } catch (e) {
      info.say(message(e));
    }
  }
  nameInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") void saveLayout();
  });

  async function refresh() {
    try {
      wins = await Bridge.shellWindows();
      draw();
    } catch (e) {
      info.say(message(e));
    }
  }

  return {
    el,
    show() {
      void refresh();
      Bridge.shellLayouts().then(drawLayouts, (e) => info.say(message(e)));
      if (timer == null) timer = window.setInterval(() => void refresh(), 2000);
    },
    hide() {
      if (timer != null) window.clearInterval(timer);
      timer = null;
      saveRow.style.display = "none";
    },
  };
}

// ── The panel ─────────────────────────────────────────────────────────────────

export interface ShellHost extends ViewHost {
  /** The panel came on screen (true) or left it (false): tabs start or stop. */
  shown(on: boolean): void;
  /** True when the tab on screen takes dropped files (Wallpapers). */
  takesDrops(): boolean;
  /** Files dropped on the panel. */
  drop(paths: string[]): void;
}

/** `onCircle`: the header's lasso button (« Entoure et demande », Island.startCircle). */
export function buildShell(onCircle: () => void): ShellHost {
  const tabs: Record<ShellTab, Tab> = {
    controls: buildControls(),
    today: buildToday(),
    stats: buildStats(),
    clipboard: buildClipboard(),
    apps: buildApps(),
    wallpapers: buildWallpapers(),
    power: buildPower(),
    windows: buildWindows(),
  };
  const nav = h("nav", { class: "shell-tabs", "aria-label": tl("Tabs") });
  const time = h("span", { class: "shell-mono shell-time" });
  const date = h("span", { class: "shell-dim" });
  const clock = h("span", { class: "shell-clock" }, time, date);
  const body = h("div", { class: "shell-body" });
  for (const tab of Object.values(tabs)) body.append(tab.el);
  const el = h(
    "div",
    { class: "view shell-view" },
    h(
      "div",
      { class: "shell-head" },
      nav,
      h("span", { class: "shell-grow" }),
      h(
        "button",
        { class: "shell-icon-btn", "aria-label": tl("Circle and ask"), title: tl("Circle and ask"), onclick: () => onCircle() },
        svg("M12 4c4.4 0 8 2.7 8 6s-3.6 6-8 6c-1 0-2-.1-2.9-.4L5 19l.9-4.1C4.7 13.7 4 12.4 4 11c0-3.9 3.6-7 8-7z", 16, { stroke: 2 }),
      ),
      clock,
    ),
    body,
    h("div", { class: "shell-grip" }, h("i", {})),
  );

  let caps: ShellCapabilities | null = null;
  let current: ShellTab = "stats";
  let live = false;
  let clockTimer: number | null = null;

  function drawClock() {
    const c = clockText(new Date(), language());
    time.textContent = c.time;
    date.textContent = c.short;
    clock.title = c.long;
  }

  function drawTabs() {
    const list = availableTabs(caps);
    if (!list.includes(current)) current = itemAt(list, 0);
    nav.replaceChildren(
      ...list.map((tab) =>
        h(
          "button",
          { class: tab === current ? "shell-tab-btn on" : "shell-tab-btn", "aria-label": LABEL[tab](), "aria-pressed": String(tab === current), onclick: () => pick(tab) },
          svg(ICON[tab], 16, { stroke: 2 }),
          tab === current ? h("span", { text: LABEL[tab]() }) : null,
        ),
      ),
    );
    for (const [name, tab] of Object.entries(tabs) as [ShellTab, Tab][]) tab.el.classList.toggle("on", name === current);
  }

  function pick(tab: ShellTab) {
    if (tab === current) return;
    if (live) tabs[current].hide();
    current = tab;
    drawTabs();
    if (live) tabs[current].show();
  }

  return {
    el,
    sync() {},
    takesDrops: () => live && !!tabs[current].drop,
    drop(paths: string[]) {
      tabs[current].drop?.(paths);
    },
    shown(on: boolean) {
      if (on === live) return;
      live = on;
      if (on) {
        drawClock();
        clockTimer = window.setInterval(drawClock, 15000);
        const start = () => {
          drawTabs();
          if (live) tabs[current].show();
        };
        if (caps) start();
        else
          void Bridge.shellCapabilities().then((c) => {
            caps = c;
            start();
          });
      } else {
        if (clockTimer != null) window.clearInterval(clockTimer);
        clockTimer = null;
        tabs[current].hide();
      }
    },
  };
}
