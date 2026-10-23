// « Coucou Shell »'s other tabs (docs/WINDOWS_SHELL.md): Today, Clipboard,
// Wallpapers and Power. Like the first four (views/shell.ts), nothing here
// runs while its tab is not on screen, except a running focus timer's single
// end-of-timer timeout.

import {
  Bridge, onEvent, type ShellClip, type ShellPowerAction, type ShellCard, type ShellRoutine, type ShellTodo, type ShellWallpapers,
} from "../core/bridge";
import { agoText, clipPreview, clockText, fromLocalMinutes, isoWeek, timerText, weatherKey, whenText, type WeatherKind } from "../core/shell";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import { N_, language, t, tl, weekdayShort } from "../i18n/i18n";
import { clear, h, svg } from "./dom";
import { card, cardTitle, foot, linkButton, message, note, ring, switchButton, type Tab } from "./shell-parts";
import { colorHex } from "../design/tokens";

const settingsLink = (text = tl("Open Settings")) => linkButton(text, () => void Bridge.openSettingsWindow());

// ── Today ─────────────────────────────────────────────────────────────────────

/** weatherKey()'s families, named. */
const WEATHER: Record<WeatherKind, string> = {
  clear: N_("Clear"),
  partly: N_("Partly cloudy"),
  overcast: N_("Overcast"),
  fog: N_("Fog"),
  drizzle: N_("Drizzle"),
  rain: N_("Rain"),
  snow: N_("Snow"),
  storm: N_("Thunderstorm"),
  cloudy: N_("Cloudy"),
};

const SUN = "M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4";
const CLOUD = "M17.5 19a4.5 4.5 0 1 0-1.4-8.8A6 6 0 0 0 4.7 12 3.5 3.5 0 0 0 6.5 19z";

const FOCUS_MIN = 25;
const BREAK_MIN = 5;

/** The focus timer outlives the tab: only its end is scheduled while the panel is folded. */
const focus = {
  mode: "focus" as "focus" | "break",
  /** When it ends (ms), while it runs. */
  endAt: 0,
  /** What was left when it was paused (ms). */
  left: FOCUS_MIN * 60_000,
  doneTimer: null as number | null,
};

function buildToday(): Tab {
  const info = note();
  const time = h("span", { class: "shell-big-time shell-mono" });
  const date = h("span", { class: "shell-card-title" });
  const week = h("span", { class: "shell-dim" });
  const clockCard = card("", time, date, week);

  const weatherBody = h("div", { class: "shell-stack" });
  const weatherCard = card("", weatherBody);

  const focusRing = ring(colorHex("--color-accent"), true);
  const focusTitle = h("span", { class: "shell-card-title", text: tl("Focus") });
  const startBtn = h("button", { class: "shell-pill", onclick: () => toggleFocus() });
  const resetBtn = h("button", { class: "shell-text-btn", text: tl("Reset"), onclick: () => resetFocus(focus.mode) });
  const modeBtn = h("button", { class: "shell-text-btn", onclick: () => resetFocus(focus.mode === "focus" ? "break" : "focus") });
  const focusCard = card(
    "",
    h(
      "div",
      { class: "shell-row" },
      focusRing.el,
      h("div", { class: "shell-grow" }, focusTitle, startBtn, h("div", { class: "shell-row shell-wrap" }, resetBtn, modeBtn)),
    ),
  );

  const todoInput = h("input", { type: "text", class: "shell-name-input", placeholder: tl("Add a to-do"), "aria-label": tl("Add a to-do"), maxlength: "200" });
  const todoList = h("div", { class: "shell-todos" });
  const clearDone = h("button", { class: "shell-text-btn", text: tl("Clear done"), onclick: () => void change({ kind: "clearDone" }) });
  const todoCard = card(
    "shell-span2",
    cardTitle(tl("To do"), clearDone),
    todoList,
    h("div", { class: "shell-row" }, todoInput),
    h("span", { class: "shell-dim", text: tl("Kept on this PC only") }),
  );

  const routineList = h("div", { class: "shell-stack" });
  const routineCard = card("", cardTitle(tl("Routines")), routineList);

  const el = h("div", { class: "shell-tab" }, h("div", { class: "shell-grid3" }, clockCard, weatherCard, focusCard, todoCard, routineCard), info.el);
  let clockTimer: number | null = null;
  let focusTimer: number | null = null;

  function drawClock() {
    const now = new Date();
    const c = clockText(now, language());
    time.textContent = c.time;
    date.textContent = c.long;
    week.textContent = t("Week {n}", { n: String(isoWeek(now)) });
  }

  async function drawWeather() {
    clear(weatherBody);
    if (!State.settings.shellWeatherCity.trim()) {
      weatherBody.append(
        h("span", { class: "shell-dim", text: tl("Weather is off. Type a city in Settings to see it here, from Open-Meteo.") }),
        settingsLink(),
      );
      return;
    }
    weatherBody.append(h("span", { class: "shell-dim", text: tl("Loading…") }));
    try {
      const w = await Bridge.shellWeather();
      clear(weatherBody);
      if (!w) return;
      const kind = weatherKey(w.code);
      const icon = svg(kind === "clear" ? SUN : CLOUD, 36, { stroke: 2 });
      icon.style.color = colorHex("--color-warning");
      weatherBody.append(
        h("div", { class: "shell-row" }, icon, h("span", { class: "shell-temp", text: `${Math.round(w.temperature)}°` })),
        h("span", { class: "shell-card-title", text: `${w.place} · ${t(WEATHER[kind])}` }),
        h("span", { class: "shell-dim", text: t("High {max}° · low {min}°", { max: String(Math.round(w.max)), min: String(Math.round(w.min)) }) }),
        h("span", { class: "shell-dim", text: tl("From Open-Meteo, at most every 30 minutes, only while this tab is open.") }),
      );
    } catch (e) {
      clear(weatherBody);
      weatherBody.append(h("span", { class: "shell-dim", text: message(e) }));
    }
  }

  function left(): number {
    return focus.endAt ? focus.endAt - Date.now() : focus.left;
  }

  function drawFocus() {
    const total = (focus.mode === "focus" ? FOCUS_MIN : BREAK_MIN) * 60_000;
    focusRing.set((100 * left()) / total, timerText(left()));
    focusTitle.textContent = focus.mode === "focus" ? t("Focus") : t("Break");
    startBtn.textContent = focus.endAt ? t("Pause") : t("Start");
    modeBtn.textContent = focus.mode === "focus" ? t("Take a break") : t("Back to focus");
  }

  function scheduleEnd() {
    if (focus.doneTimer != null) window.clearTimeout(focus.doneTimer);
    focus.doneTimer = null;
    if (!focus.endAt) return;
    focus.doneTimer = window.setTimeout(() => {
      focus.doneTimer = null;
      Sound.play("finish");
      resetFocus(focus.mode === "focus" ? "break" : "focus");
    }, Math.max(0, focus.endAt - Date.now()));
  }

  function toggleFocus() {
    if (focus.endAt) {
      focus.left = Math.max(0, focus.endAt - Date.now());
      focus.endAt = 0;
    } else {
      focus.endAt = Date.now() + focus.left;
    }
    scheduleEnd();
    drawFocus();
  }

  function resetFocus(mode: "focus" | "break") {
    focus.mode = mode;
    focus.endAt = 0;
    focus.left = (mode === "focus" ? FOCUS_MIN : BREAK_MIN) * 60_000;
    scheduleEnd();
    drawFocus();
  }

  function drawTodos(list: ShellTodo[]) {
    clearDone.style.display = list.some((x) => x.done) ? "" : "none";
    todoList.replaceChildren(
      ...list.map((x) =>
        h(
          "div",
          { class: x.done ? "shell-todo done" : "shell-todo" },
          h(
            "button",
            { class: "shell-check", "aria-label": x.text, "aria-pressed": String(x.done), onclick: () => void change({ kind: "toggle", id: x.id }) },
            x.done ? svg("M5 12l5 5L20 7", 14, { stroke: 2.5 }) : null,
          ),
          h("span", { class: "shell-grow shell-ellipsis", text: x.text }),
          h("button", { class: "shell-icon-btn", "aria-label": t("Delete"), title: t("Delete"), onclick: () => void change({ kind: "remove", id: x.id }) }, svg("M6 6l12 12M18 6 6 18", 14, { stroke: 2 })),
        ),
      ),
    );
  }

  function drawRoutines(list: ShellRoutine[]) {
    clear(routineList);
    if (list.length === 0) {
      routineList.append(h("span", { class: "shell-dim", text: tl("Ask « OK Coucou »: every Monday at 9, open Unity and Spotify.") }));
      return;
    }
    const lang = language();
    for (const r of list.slice(0, 4)) {
      const next = r.next == null ? "" : fromLocalMinutes(r.next).toLocaleString(lang, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
      routineList.append(
        h(
          "div",
          { class: "shell-routine" },
          h("span", { class: "shell-routine-time shell-mono shell-ellipsis", text: next || whenText(r.when, lang, weekdayShort, t("every day")) }),
          h("span", { class: "shell-ellipsis", text: r.name }),
        ),
      );
    }
  }

  async function change(c: Parameters<typeof Bridge.shellTodoChange>[0]) {
    try {
      drawTodos(await Bridge.shellTodoChange(c));
    } catch (e) {
      info.say(message(e));
    }
  }

  todoInput.addEventListener("focus", () => void Bridge.focusWindow(true));
  todoInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && todoInput.value.trim()) {
      const text = todoInput.value;
      todoInput.value = "";
      void change({ kind: "add", text });
    }
  });

  return {
    el,
    show() {
      drawClock();
      drawFocus();
      clockTimer = window.setInterval(drawClock, 15000);
      // The countdown redraws only while it is on screen.
      focusTimer = window.setInterval(() => focus.endAt && drawFocus(), 1000);
      void drawWeather();
      Bridge.shellTodos().then(drawTodos, (e: unknown) => info.say(message(e)));
      Bridge.shellRoutines().then(drawRoutines, () => drawRoutines([]));
    },
    hide() {
      if (clockTimer != null) window.clearInterval(clockTimer);
      if (focusTimer != null) window.clearInterval(focusTimer);
      clockTimer = focusTimer = null;
      if (document.activeElement === todoInput) {
        todoInput.blur();
        void Bridge.focusWindow(false);
      }
    },
  };
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

/** Display names of the apps Rust never keeps copies from (shell/clipboard.rs). */
const NEVER_FROM = ["KeePass", "KeePassXC", "1Password", "Bitwarden", "LastPass", "Dashlane", "Enpass", "NordPass", "Proton Pass", "RoboForm", "Keeper"];

function buildClipboard(): Tab {
  const info = note();
  const count = h("span", { class: "shell-dim shell-mono" });
  const list = h("div", { class: "shell-clips" });
  const recent = card("shell-list", h("div", { class: "shell-card-head shell-pad" }, h("span", { class: "shell-card-title", text: tl("Recent") }), count), list);
  const toggle = switchButton(tl("Clipboard history"), false, () => void setOn(!State.settings.shellClipboard));
  const side = h(
    "div",
    { class: "shell-stack" },
    card(
      "",
      cardTitle(tl("History"), toggle),
      h("span", { class: "shell-dim", text: tl("In memory only, never on disk. Text only. Password managers are always skipped.") }),
      h("button", { class: "shell-chip-row center", text: tl("Clear all"), onclick: () => void clearAll() }),
    ),
    card(
      "",
      cardTitle(tl("Never kept from")),
      h("div", { class: "shell-wrap" }, ...NEVER_FROM.map((x) => h("span", { class: "shell-chip-static", text: x }))),
      h("span", { class: "shell-dim", text: tl("Only the last 5 copies, cut to 500 characters, and only when you ask. They are sent to Claude with your question.") }),
    ),
  );
  const el = h("div", { class: "shell-tab" }, h("div", { class: "shell-grid2 shell-split" }, recent, side), info.el);
  let unlisten: (() => void) | null = null;
  let live = false;

  function draw(entries: ShellClip[]) {
    count.textContent = `${entries.length} / 25`;
    const now = Date.now();
    const lang = language();
    list.replaceChildren(
      ...(entries.length === 0
        ? [h("span", { class: "shell-dim shell-pad", text: tl("Copy some text and it shows up here.") })]
        : entries.map((c) =>
            h(
              "div",
              { class: "shell-clip" },
              h(
                "div",
                { class: "shell-grow" },
                h("span", { class: "shell-ellipsis", text: clipPreview(c.text) }),
                h("span", { class: "shell-dim", text: [c.app, agoText(c.at, now, lang)].filter(Boolean).join(" · ") }),
              ),
              h("button", { class: "shell-outline", text: tl("Copy"), onclick: () => void copy(c.id) }),
              h("button", { class: "shell-icon-btn", "aria-label": t("Delete"), title: t("Delete"), onclick: () => void remove(c.id) }, svg("M6 6l12 12M18 6 6 18", 14, { stroke: 2 })),
            ),
          )),
    );
  }

  async function refresh() {
    const on = State.settings.shellClipboard;
    toggle.classList.toggle("on", on);
    toggle.setAttribute("aria-pressed", String(on));
    count.style.display = on ? "" : "none";
    if (!on) {
      list.replaceChildren(h("span", { class: "shell-dim shell-pad", text: tl("Turn it on to find what you copied here. It is held in memory only, never on disk, and text only.") }));
      return;
    }
    try {
      draw(await Bridge.shellClipboard());
    } catch (e) {
      info.say(message(e));
    }
  }

  /** The switch saves the setting as Settings does: the same key, read again by Rust. */
  async function setOn(on: boolean) {
    State.settings.shellClipboard = on;
    await Bridge.saveSettings(State.settings);
    await refresh();
  }

  async function copy(id: number) {
    try {
      await Bridge.shellClipboardCopy(id);
      Sound.play("pop");
      info.say(t("Copied."));
      await refresh();
    } catch (e) {
      info.say(message(e));
    }
  }

  async function remove(id: number) {
    try {
      draw(await Bridge.shellClipboardRemove(id));
    } catch (e) {
      info.say(message(e));
    }
  }

  async function clearAll() {
    await Bridge.shellClipboardClear();
    await refresh();
  }

  return {
    el,
    show() {
      live = true;
      void refresh();
      void onEvent("shell-clipboard", () => void refresh()).then((u) => {
        if (live) unlisten = u;
        else u();
      });
    },
    hide() {
      live = false;
      unlisten?.();
      unlisten = null;
    },
  };
}

// ── Wallpapers ────────────────────────────────────────────────────────────────

function buildWallpapers(): Tab {
  const info = note();
  const grid = h("div", { class: "shell-walls" });
  const subtitle = h("span", { class: "shell-dim" });
  const dropZone = h(
    "div",
    { class: "shell-drop" },
    svg("M12 16V4M7 9l5-5 5 5M4 20h16", 24, { stroke: 2 }),
    h("span", { class: "shell-card-title", text: tl("Drop your images here") }),
    h("span", { class: "shell-dim", text: tl("JPG, PNG or WebP, one or several at a time · nothing goes to the Internet") }),
  );
  const targets = h("div", { class: "shell-row shell-wrap" });
  const applyBtn = h("button", { class: "shell-pill", text: tl("Apply"), onclick: () => void apply() });
  const el = h(
    "div",
    { class: "shell-tab" },
    h(
      "div",
      { class: "shell-stack" },
      card("", cardTitle(tl("My selection"), subtitle), grid),
      dropZone,
      h("div", { class: "shell-row shell-between" }, h("div", { class: "shell-row shell-wrap" }, h("span", { class: "shell-dim", text: tl("Apply to") }), targets), applyBtn),
    ),
    info.el,
  );
  let data: ShellWallpapers = { images: [], screens: 0, refused: 0 };
  let selected: string | null = null;
  let screen: number | null = null;

  function draw() {
    if (!data.images.some((i) => i.id === selected)) selected = data.images[0]?.id ?? null;
    if (screen != null && screen >= data.screens) screen = null;
    subtitle.textContent = t("{n} images · copied into Coucou's folder", { n: String(data.images.length) });
    clear(grid);
    if (data.images.length === 0) grid.append(h("span", { class: "shell-dim", text: tl("Your images show up here once dropped.") }));
    for (const img of data.images) {
      const on = img.id === selected;
      const thumb = h("button", { class: on ? "shell-wall on" : "shell-wall", "aria-label": t("Pick this image"), "aria-pressed": String(on), onclick: () => { selected = img.id; draw(); } });
      if (img.thumb) thumb.style.backgroundImage = `url("${img.thumb}")`;
      const del = h("button", { class: "shell-wall-del", "aria-label": t("Delete"), title: t("Delete"), onclick: () => Bridge.shellWallpaperDelete(img.id).then((d) => { data = d; draw(); }, (e: unknown) => info.say(message(e))) }, svg("M6 6l12 12M18 6 6 18", 12, { stroke: 2.5 }));
      grid.append(h("div", { class: "shell-wall-cell" }, thumb, del, on ? h("span", { class: "shell-wall-mark" }, svg("M5 12l5 5L20 7", 14, { stroke: 2.5 })) : null));
    }
    const chips: [number | null, string][] = [[null, t("All screens")]];
    for (let i = 0; i < data.screens && data.screens > 1; i++) chips.push([i, t("Screen {n}", { n: String(i + 1) })]);
    targets.replaceChildren(...chips.map(([i, label]) => h("button", { class: i === screen ? "shell-chip on" : "shell-chip", text: label, onclick: () => { screen = i; draw(); } })));
    (applyBtn).disabled = !selected;
  }

  async function apply() {
    if (!selected) return;
    try {
      await Bridge.shellWallpaperApply(selected, screen);
      Sound.play("pop");
    } catch (e) {
      info.say(message(e));
    }
  }

  return {
    el,
    show() {
      Bridge.shellWallpapers().then((d) => { data = d; draw(); }, (e: unknown) => info.say(message(e)));
    },
    hide() {},
    drop(paths: string[]) {
      dropZone.classList.remove("over");
      if (paths.length === 0) return true;
      Bridge.shellWallpaperAdd(paths).then(
        (d) => {
          data = d;
          if (d.images[0]) selected = d.images[0].id;
          draw();
          if (d.refused > 0) info.say(t("{n} files were not taken: not a JPG, PNG or WebP, too large, or the selection is full.", { n: String(d.refused) }));
        },
        (e: unknown) => info.say(message(e)),
      );
      return true;
    },
  };
}

// ── Power ─────────────────────────────────────────────────────────────────────

/** The mockup's four tiles. Sign out is not one of them (see docs/WINDOWS_SHELL.md). */
const POWER: [ShellPowerAction, string, string][] = [
  ["lock", N_("Lock"), "M6 11h12v10H6zM8 11V7a4 4 0 0 1 8 0v4"],
  ["sleep", N_("Sleep"), "M21 13A9 9 0 1 1 11 3a7 7 0 0 0 10 10z"],
  ["restart", N_("Restart"), "M3 12a9 9 0 1 0 3-6.7M3 4v5h5"],
  ["shutDown", N_("Shut down"), "M12 2v10M5.6 6.6a8 8 0 1 0 12.8 0"],
];

const QUESTION: Record<ShellPowerAction, string> = {
  lock: N_("Lock the PC?"),
  sleep: N_("Put the PC to sleep?"),
  signOut: N_("Sign out of Windows?"),
  restart: N_("Restart the PC?"),
  shutDown: N_("Shut down the PC?"),
};

function buildPower(): Tab {
  const info = note();
  const buttons = h(
    "div",
    { class: "shell-power-grid" },
    ...POWER.map(([action, label, icon]) =>
      h("button", { class: action === "shutDown" ? "shell-power danger" : "shell-power", onclick: () => void ask(action) }, svg(icon, 24, { stroke: 2 }), h("span", { text: tl(label) })),
    ),
  );
  const confirmBox = h("div", { class: "shell-confirm", style: "display:none" });
  const battery = h("span", { class: "shell-dim" });
  const el = h(
    "div",
    { class: "shell-tab" },
    buttons,
    confirmBox,
    foot(battery, tl("Always confirmed by a click")),
    info.el,
  );

  function close() {
    confirmBox.style.display = "none";
    buttons.style.display = "";
    clear(confirmBox);
  }

  async function ask(action: ShellPowerAction) {
    let c: ShellCard;
    try {
      c = await Bridge.shellPowerPrepare(action);
    } catch (e) {
      info.say(message(e));
      return;
    }
    const [, label = "", icon = ""] = POWER.find(([a]) => a === action) ?? [];
    const danger = action === "shutDown" || action === "restart";
    const apps = c.kind === "power" ? c.apps : [];
    const detail =
      action === "lock" || action === "sleep"
        ? t("Your apps stay open.")
        : apps.length > 0
          ? t("Open apps may have unsaved work: {apps}.", { apps: apps.join(", ") })
          : t("No app has a window open.");
    confirmBox.replaceChildren(
      card(
        danger ? "shell-confirm-card danger" : "shell-confirm-card",
        h("span", { class: "shell-badge on" }, svg(icon, 22, { stroke: 2 })),
        h("span", { class: "shell-grow" }, h("span", { class: "shell-confirm-title", text: t(QUESTION[action]) }), h("span", { class: "shell-dim", text: detail })),
        h(
          "div",
          { class: "shell-row" },
          h("button", { class: "shell-chip", onclick: () => { void Bridge.shellConfirmCancel(); close(); } }, h("span", { text: tl("Cancel") })),
          h(
            "button",
            {
              class: danger ? "shell-pill danger" : "shell-pill",
              onclick: () => {
                close();
                Bridge.shellConfirm(c.token).catch((e: unknown) => info.say(message(e)));
              },
            },
            h("span", { text: t(label) }),
          ),
        ),
      ),
    );
    confirmBox.style.display = "";
  }

  return {
    el,
    show() {
      Bridge.shellBattery().then(
        (b) => {
          battery.textContent =
            b.percent == null
              ? b.pluggedIn ? t("On mains power") : ""
              : b.pluggedIn
                ? t("Battery {pct} % · plugged in", { pct: String(b.percent) })
                : t("Battery {pct} % · on battery", { pct: String(b.percent) });
        },
        () => (battery.textContent = ""),
      );
    },
    hide() {
      if (confirmBox.style.display !== "none") void Bridge.shellConfirmCancel();
      close();
    },
  };
}

export { buildClipboard, buildPower, buildToday, buildWallpapers };
