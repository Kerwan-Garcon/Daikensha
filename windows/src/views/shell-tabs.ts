// « Coucou Shell »'s other tabs (docs/WINDOWS_SHELL.md): Today, Clipboard,
// Wallpapers and Power. Like the first four (views/shell.ts), nothing here
// runs while its tab is not on screen, except a running focus timer's single
// end-of-timer timeout.

import {
  Bridge, onEvent, type ShellClip, type ShellPowerAction, type ShellPowerCard, type ShellTodo, type ShellWallpapers,
} from "../core/bridge";
import { agoText, clipPreview, clockText, isoWeek, timerText, weatherKey, type WeatherKind } from "../core/shell";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import { N_, language, t, tl } from "../i18n/i18n";
import { clear, h, svg } from "./dom";
import { card, cardTitle, linkButton, message, note, type Tab } from "./shell-parts";

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
  const weatherCard = card("shell-span2", cardTitle(tl("Weather")), weatherBody);

  const focusTime = h("span", { class: "shell-big-time shell-mono" });
  const focusMode = h("span", { class: "shell-dim" });
  const startBtn = h("button", { class: "shell-chip on", onclick: () => toggleFocus() });
  const resetBtn = h("button", { class: "shell-chip", text: tl("Reset"), onclick: () => resetFocus(focus.mode) });
  const modeBtn = h("button", { class: "shell-chip", onclick: () => resetFocus(focus.mode === "focus" ? "break" : "focus") });
  const focusCard = card("", cardTitle(tl("Focus")), focusTime, focusMode, h("div", { class: "shell-row" }, startBtn, resetBtn, modeBtn));

  const todoInput = h("input", { type: "text", class: "shell-name-input", placeholder: t("Add a to-do"), "aria-label": t("Add a to-do"), maxlength: "200" }) as HTMLInputElement;
  const todoList = h("div", { class: "shell-todos" });
  const clearDone = h("button", { class: "shell-text-btn", text: tl("Clear done"), onclick: () => void change({ kind: "clearDone" }) });
  const todoCard = card(
    "shell-span2",
    cardTitle(tl("To do"), clearDone),
    h("div", { class: "shell-row" }, todoInput),
    todoList,
    h("span", { class: "shell-dim", text: tl("Kept on this PC only") }),
  );

  const el = h("div", { class: "shell-tab" }, h("div", { class: "shell-grid3" }, clockCard, weatherCard, focusCard, todoCard), info.el);
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
      weatherBody.append(
        h(
          "div",
          { class: "shell-row" },
          h("span", { class: "shell-big-time shell-mono", text: `${Math.round(w.temperature)}°` }),
          h("div", { class: "shell-grow" }, h("span", { class: "shell-card-title", text: `${w.place} · ${t(WEATHER[weatherKey(w.code)])}` }), h("span", { class: "shell-dim", text: t("High {max}° · low {min}°", { max: String(Math.round(w.max)), min: String(Math.round(w.min)) }) })),
        ),
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
    focusTime.textContent = timerText(left());
    focusMode.textContent = focus.mode === "focus" ? t("Focus time") : t("Break");
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
      Bridge.shellTodos().then(drawTodos, (e) => info.say(message(e)));
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
  const off = card(
    "shell-span3",
    cardTitle(tl("The clipboard history is off")),
    h("span", { class: "shell-dim", text: tl("Turn it on in Settings to find what you copied here. It is held in memory only, never on disk, and text only.") }),
    settingsLink(),
  );
  const recent = card("shell-span3 shell-list", h("div", { class: "shell-card-head shell-pad" }, h("span", { class: "shell-card-title", text: tl("Recent") }), count), list);
  const side = h(
    "div",
    { class: "shell-span2 shell-stack" },
    card(
      "",
      cardTitle(tl("History")),
      h("span", { class: "shell-dim", text: tl("In memory only, never on disk. Text only. Password managers are always skipped.") }),
      h("div", { class: "shell-row" }, h("button", { class: "shell-chip", text: tl("Clear all"), onclick: () => void clearAll() })),
    ),
    card("", cardTitle(tl("Never kept from")), h("div", { class: "shell-wrap" }, ...NEVER_FROM.map((x) => h("span", { class: "shell-chip-static", text: x })))),
  );
  const onGrid = h("div", { class: "shell-grid5" }, recent, side);
  const el = h("div", { class: "shell-tab" }, off, onGrid, info.el);
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
              h("button", { class: "shell-chip", text: tl("Copy"), onclick: () => void copy(c.id) }),
              h("button", { class: "shell-icon-btn", "aria-label": t("Delete"), title: t("Delete"), onclick: () => void remove(c.id) }, svg("M6 6l12 12M18 6 6 18", 14, { stroke: 2 })),
            ),
          )),
    );
  }

  async function refresh() {
    const on = State.settings.shellClipboard;
    off.style.display = on ? "none" : "";
    onGrid.style.display = on ? "" : "none";
    if (!on) return;
    try {
      draw(await Bridge.shellClipboard());
    } catch (e) {
      info.say(message(e));
    }
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
      void onEvent<null>("shell-clipboard", () => void refresh()).then((u) => {
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
    svg("M12 16V4M7 9l5-5 5 5M4 20h16", 22, { stroke: 2 }),
    h("span", { class: "shell-card-title", text: tl("Drop your images here") }),
    h("span", { class: "shell-dim", text: tl("JPG, PNG or WebP, one or several at a time · nothing goes to the Internet") }),
  );
  const targets = h("div", { class: "shell-row shell-wrap" });
  const applyBtn = h("button", { class: "shell-chip on", text: tl("Apply"), onclick: () => void apply() });
  const el = h(
    "div",
    { class: "shell-tab" },
    h(
      "div",
      { class: "shell-grid5" },
      card("shell-span3", cardTitle(tl("My selection"), subtitle), grid),
      h("div", { class: "shell-span2 shell-stack" }, dropZone, card("", cardTitle(tl("Apply to")), targets, h("div", { class: "shell-row" }, applyBtn))),
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
      const thumb = h("button", { class: img.id === selected ? "shell-wall on" : "shell-wall", "aria-label": t("Pick this image"), "aria-pressed": String(img.id === selected), onclick: () => { selected = img.id; draw(); } });
      if (img.thumb) thumb.style.backgroundImage = `url("${img.thumb}")`;
      const del = h("button", { class: "shell-wall-del", "aria-label": t("Delete"), title: t("Delete"), onclick: () => Bridge.shellWallpaperDelete(img.id).then((d) => { data = d; draw(); }, (e) => info.say(message(e))) }, svg("M6 6l12 12M18 6 6 18", 12, { stroke: 2.5 }));
      grid.append(h("div", { class: "shell-wall-cell" }, thumb, del));
    }
    const chips: [number | null, string][] = [[null, t("All screens")]];
    for (let i = 0; i < data.screens && data.screens > 1; i++) chips.push([i, t("Screen {n}", { n: String(i + 1) })]);
    targets.replaceChildren(...chips.map(([i, label]) => h("button", { class: i === screen ? "shell-chip on" : "shell-chip", text: label, onclick: () => { screen = i; draw(); } })));
    (applyBtn as HTMLButtonElement).disabled = !selected;
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
      Bridge.shellWallpapers().then((d) => { data = d; draw(); }, (e) => info.say(message(e)));
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
        (e) => info.say(message(e)),
      );
      return true;
    },
  };
}

// ── Power ─────────────────────────────────────────────────────────────────────

const POWER: [ShellPowerAction, string, string][] = [
  ["lock", N_("Lock"), "M6 11h12v10H6zM8 11V7a4 4 0 0 1 8 0v4"],
  ["sleep", N_("Sleep"), "M21 13A9 9 0 1 1 11 3a7 7 0 0 0 10 10z"],
  ["signOut", N_("Sign out"), "M15 3h4v18h-4M10 17l-5-5 5-5M5 12h11"],
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
      h("button", { class: "shell-power", onclick: () => void ask(action) }, h("span", { class: "shell-badge" }, svg(icon, 22, { stroke: 2 })), h("span", { class: "shell-card-title", text: t(label) })),
    ),
  );
  const confirmBox = h("div", { class: "shell-confirm", style: "display:none" });
  const battery = h("span", { class: "shell-dim" });
  const el = h(
    "div",
    { class: "shell-tab" },
    buttons,
    confirmBox,
    h("div", { class: "shell-foot" }, battery, h("span", { class: "shell-dim", text: tl("Always confirmed by a click") })),
    info.el,
  );

  function close() {
    confirmBox.style.display = "none";
    buttons.style.display = "";
    clear(confirmBox);
  }

  async function ask(action: ShellPowerAction) {
    let c: ShellPowerCard;
    try {
      c = await Bridge.shellPowerPrepare(action);
    } catch (e) {
      info.say(message(e));
      return;
    }
    const label = POWER.find(([a]) => a === action)?.[1] ?? "";
    const detail =
      action === "lock" || action === "sleep"
        ? t("Your apps stay open.")
        : c.apps.length > 0
          ? t("Open apps may have unsaved work: {apps}.", { apps: c.apps.join(", ") })
          : t("No app has a window open.");
    confirmBox.replaceChildren(
      card(
        "shell-confirm-card",
        h("span", { class: "shell-confirm-title", text: t(QUESTION[action]) }),
        h("span", { class: "shell-dim", text: detail }),
        h(
          "div",
          { class: "shell-row shell-end" },
          h("button", { class: "btn secondary", onclick: () => { void Bridge.shellPowerCancel(); close(); } }, h("span", { text: tl("Cancel") })),
          h(
            "button",
            {
              class: "btn primary",
              onclick: () => {
                close();
                Bridge.shellPowerConfirm(c.token).catch((e) => info.say(message(e)));
              },
            },
            h("span", { text: t(label) }),
          ),
        ),
      ),
    );
    buttons.style.display = "none";
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
      if (confirmBox.style.display !== "none") void Bridge.shellPowerCancel();
      close();
    },
  };
}

export { buildClipboard, buildPower, buildToday, buildWallpapers };
