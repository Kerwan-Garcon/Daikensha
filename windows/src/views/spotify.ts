// The Spotify pill and card — DOM ports of SpotifyPill / SpotifyCardView
// (SpotifyViews.swift) and the shared now-playing pieces (NowPlayingViews.swift
// and MusicControlButton). Sizes, colours and wording are the Mac's.
//
// What they show comes from src-tauri/src/spotify.rs (MPRIS on Linux, the
// system media controls on Windows) through
// island/spotify.ts. A click changes the page's copy at once and Spotify
// confirms it, as the Mac's controller does.
//
// The YouTube Music pill and card (YouTubeMusicViews.swift) are the same views
// over another source (views/youtube-music.ts).

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { Bridge } from "../core/bridge";
import { State, type AgentTask } from "../core/state";
import {
  SPOTIFY_GREEN, SPOTIFY_ID, Spotify, currentArtwork, formatTime, isAd, spotifyPosition, volumeLevel, withPlaying,
  type NowPlayingStore, type SpotifyTrack,
} from "../core/spotify";
import type { SpotifyAction } from "../core/bridge";
import { createMiniBot } from "../mochi/minibots";
import { pillDefinition } from "../core/pills";
import { N_, t } from "../i18n/i18n";
import { colorHex } from "../design/tokens";
import { itemAt } from "../core/arrays";


function lighter(hex: string, amount: number): string {
  const v = parseInt(hex.replace("#", ""), 16);
  const c = [(v >> 16) & 255, (v >> 8) & 255, v & 255].map((x) => Math.min(255, Math.round(x + amount * 255)));
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// ── Source (Spotify, or YouTube Music) ────────────────────────────────────────

/** What the idle card says, and its button, when the player isn't simply idle. */
export interface NowPlayingStatus {
  ok: boolean;
  text: string;
  action: string | null;
  run: () => void;
}

/** A player the pill and the card show and drive. */
export interface NowPlayingSource {
  id: string;
  /** The idle card's header. */
  name: string;
  accent: string;
  store: NowPlayingStore;
  /** Mochi dances (State.spotifyPlaying). */
  playing(): boolean;
  control(action: SpotifyAction, value?: number): Promise<boolean | null>;
  open(): Promise<boolean | null>;
  /** English keys, translated where shown. */
  text: { open: string; openWith: string; notInstalled: string; get: string };
  /** An idle card of its own (YouTube Music's connection), else null. */
  status?(): NowPlayingStatus | null;
  /** The repeat button's icon (YouTube Music's repeat-one). */
  repeatIcon?(): string;
}

export const SPOTIFY_SOURCE: NowPlayingSource = {
  id: SPOTIFY_ID,
  name: "Spotify",
  accent: SPOTIFY_GREEN,
  store: Spotify,
  playing: () => State.spotifyPlaying,
  control: (action, value) => Bridge.spotifyControl(action, value),
  open: () => Bridge.spotifyOpen(),
  text: {
    open: N_("Open Spotify"),
    openWith: N_("{0} — open Spotify"),
    notInstalled: N_("Spotify not installed"),
    get: N_("Get Spotify"),
  },
};

// ── Controls (shared by the pill and the card) ───────────────────────────────

/** Play/pause, at once on the page, then in Spotify (SpotifyController.playPause). */
export function togglePlay(source: NowPlayingSource = SPOTIFY_SOURCE) {
  const store = source.store;
  const s = store.state;
  if (!s.running) return;
  store.state = withPlaying(s, !s.playing, Date.now());
  State.notify();
  void source.control("playPause");
}

function seek(source: NowPlayingSource, seconds: number) {
  const store = source.store;
  const s = store.state;
  const track = s.track;
  if (!s.running || !track || isAd(track)) return;
  const target = Math.min(Math.max(0, seconds), Math.max(0, track.duration - 1));
  store.state = { ...s, position: target, positionAt: Date.now() };
  State.notify();
  void source.control("seek", target);
}

function setFlag(source: NowPlayingSource, which: "shuffle" | "repeat", on: boolean) {
  const store = source.store;
  if (!store.state.running) return;
  store.state = { ...store.state, [which]: on };
  State.notify();
  void source.control(which, on ? 1 : 0);
}

// A slider drag sends at most one volume every 120 ms (SpotifyController.setVolume).
const volumeSends = new Map<string, { pending: number | null; timer: number | null }>();

function setVolume(source: NowPlayingSource, value: number) {
  const store = source.store;
  if (!store.state.running) return;
  const v = Math.min(100, Math.max(0, Math.round(value)));
  store.state = { ...store.state, volume: v };
  State.notify();
  let q = volumeSends.get(source.id);
  if (!q) volumeSends.set(source.id, (q = { pending: null, timer: null }));
  const queue = q;
  queue.pending = v;
  if (queue.timer != null) return;
  const send = () => {
    const next = queue.pending;
    queue.pending = null;
    if (next == null) {
      queue.timer = null;
      return;
    }
    void source.control("volume", next);
    queue.timer = window.setTimeout(send, 120);
  };
  send();
}

// ── Pill (overview right column) ──────────────────────────────────────────────

export interface SpotifyPillHost {
  el: HTMLElement;
  sync(): void;
}

/** MusicControlButton: a 20 px round button in the pill's colour. */
function pillButton(color: string, icon: string, onClick: () => void): HTMLElement {
  const b = h("button", { class: "np-pill-btn" }, svg(icon, 8));
  b.style.borderColor = `${color}24`;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  b.addEventListener("mouseenter", () => {
    b.style.background = `${color}2e`;
    b.style.borderColor = `${color}8c`;
    b.style.color = lighter(color, 0.3);
    b.style.boxShadow = `0 0 6px ${color}59`;
  });
  b.addEventListener("mouseleave", () => {
    b.style.background = "";
    b.style.borderColor = `${color}24`;
    b.style.color = "";
    b.style.boxShadow = "";
  });
  return b;
}

/**
 * SpotifyPill: the agent pill's look, with a mini Mochi that dances while
 * Spotify plays, and play/pause + next on hover when a track is loaded.
 */
export function buildSpotifyPill(task: AgentTask, onTap: () => void, source: NowPlayingSource = SPOTIFY_SOURCE): SpotifyPillHost {
  const color = task.color;
  const mini = createMiniBot(task, 24, () => source.playing());
  const label = h("span", { class: "lbl", text: task.name });
  const playBtn = pillButton(color, ICONS.play, () => togglePlay(source));
  const nextBtn = pillButton(color, ICONS.forward, () => void source.control("next"));
  const controls = h("div", { class: "np-pill-controls" }, playBtn, nextBtn);
  const pill = h("div", { class: "pill np-pill", onclick: onTap }, mini, label, controls);
  pill.style.borderColor = `${color}24`;
  let hovered = false;
  let playing: boolean | null = null;

  const sync = () => {
    const s = source.store.state;
    const current = State.tasks.find((x) => x.id === task.id);
    const name = current?.name ?? task.name;
    if (label.textContent !== name) label.textContent = name;
    const show = hovered && s.track != null;
    pill.classList.toggle("controls", show);
    if (playing !== s.playing) {
      playing = s.playing;
      clear(playBtn);
      playBtn.append(svg(s.playing ? ICONS.pause : ICONS.play, 8));
    }
    playBtn.title = s.playing ? t("Pause") : t("Play");
    nextBtn.title = t("Next");
  };

  pill.addEventListener("mouseenter", () => {
    hovered = true;
    pill.style.background = `${color}2e`;
    pill.style.borderColor = `${color}8c`;
    pill.style.boxShadow = `0 2px 10px ${color}59`;
    label.style.color = lighter(color, 0.3);
    sync();
  });
  pill.addEventListener("mouseleave", () => {
    hovered = false;
    pill.style.background = "";
    pill.style.borderColor = `${color}24`;
    pill.style.boxShadow = "";
    label.style.color = "";
    sync();
  });
  sync();
  return { el: pill, sync };
}

// ── Card (overview left card) ─────────────────────────────────────────────────

/**
 * NowPlayingBar: a grey track whose filled part turns green and grows a knob
 * under the pointer; drag or click anywhere on it.
 */
class NowPlayingBar {
  readonly el: HTMLElement;
  private fill: HTMLElement;
  private knob: HTMLElement;
  private hovering = false;
  private dragging = false;
  private fraction = 0;
  enabled = true;
  private onChange: (f: number) => void;
  private onCommit: (f: number) => void;
  private accent: string;

  constructor(onChange: (f: number) => void, onCommit: (f: number) => void, accent = SPOTIFY_GREEN) {
    this.accent = accent;
    this.onChange = onChange;
    this.onCommit = onCommit;
    this.fill = h("i", { class: "np-fill" });
    this.knob = h("i", { class: "np-knob" });
    this.fill.style.background = colorHex("--gray-788");
    this.el = h("div", { class: "np-bar" }, h("i", { class: "np-track" }), this.fill, this.knob);
    this.el.addEventListener("mouseenter", () => {
      this.hovering = true;
      this.paint();
    });
    this.el.addEventListener("mouseleave", () => {
      this.hovering = false;
      this.paint();
    });
    this.el.addEventListener("pointerdown", (e) => {
      if (!this.enabled || e.button !== 0) return;
      e.stopPropagation();
      this.dragging = true;
      try {
        this.el.setPointerCapture(e.pointerId);
      } catch {
        /* capture is a nicety */
      }
      this.onChange(this.at(e.clientX));
      this.paint();
    });
    this.el.addEventListener("pointermove", (e) => {
      if (this.dragging) this.onChange(this.at(e.clientX));
    });
    const end = (e: PointerEvent) => {
      if (!this.dragging) return;
      this.dragging = false;
      this.onCommit(this.at(e.clientX));
      this.paint();
    };
    this.el.addEventListener("pointerup", end);
    this.el.addEventListener("pointercancel", end);
  }

  get active(): boolean {
    return this.enabled && (this.hovering || this.dragging);
  }

  private at(clientX: number): number {
    const r = this.el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / Math.max(1, r.width)));
  }

  set(fraction: number) {
    this.fraction = Math.min(1, Math.max(0, fraction));
    this.paint();
  }

  private paint() {
    const active = this.active;
    this.el.classList.toggle("active", active);
    const pct = `${this.fraction * 100}%`;
    this.fill.style.width = pct;
    this.fill.style.background = active ? this.accent : colorHex("--gray-788");
    this.knob.style.left = `clamp(0px, calc(${pct} - 4.5px), calc(100% - 9px))`;
  }
}

/** NowPlayingIconButton: an icon that brightens under the pointer. */
function iconButton(icon: string, size: number, stroke: number, onClick: () => void): HTMLElement {
  const b = h("button", { class: "np-icon" }, svg(icon, size, stroke ? { stroke } : {}));
  b.addEventListener("click", onClick);
  return b;
}

export interface SpotifyCardHost {
  el: HTMLElement;
  sync(): void;
}

/** SpotifyCardView: now playing, or the idle card (not playing / not installed). */
export function buildSpotifyCard(source: NowPlayingSource = SPOTIFY_SOURCE): SpotifyCardHost {
  const green = source.accent;
  const store = source.store;

  // Now playing: artwork row, progress row, controls row.
  const art = h("div", { class: "np-art" });
  const artImg = h("img", { alt: "", draggable: "false" });
  const artNote = svg(ICONS.musicNote, 16);
  artNote.style.color = `${green}b3`;
  art.append(artNote, artImg);
  art.addEventListener("click", () => void source.open());
  const title = h("span", { class: "np-title" });
  const subtitle = h("div", { class: "np-sub" });
  const head = h("div", { class: "np-head" },
    art,
    h("div", { class: "np-text" }, h("div", { class: "np-title-row" }, dot(green, 6), title), subtitle),
  );

  let dragFraction: number | null = null;
  const elapsed = h("span", { class: "np-time elapsed" });
  const remaining = h("span", { class: "np-time remaining" });
  const progress = new NowPlayingBar(
    (f) => {
      dragFraction = f;
      paintProgress();
    },
    (f) => {
      const d = store.state.track?.duration ?? 0;
      dragFraction = null;
      seek(source, f * d);
    },
    green,
  );
  const progressRow = h("div", { class: "np-progress" }, elapsed, progress.el, remaining);

  const shuffle = iconButton(ICONS.shuffle, 10, 2.2, () => setFlag(source, "shuffle", !store.state.shuffle));
  const prev = iconButton(ICONS.backward, 11, 0, () => void source.control("previous"));
  const next = iconButton(ICONS.forward, 11, 0, () => void source.control("next"));
  const repeat = iconButton(ICONS.repeat, 10, 2.2, () => setFlag(source, "repeat", !store.state.repeat));
  prev.style.color = colorHex("--gray-788");
  next.style.color = colorHex("--gray-788");
  const play = h("button", { class: "np-play", onclick: () => togglePlay(source) });
  const volIcon = h("span", { class: "np-vol-icon" });
  const volume = new NowPlayingBar(
    (f) => setVolume(source, f * 100),
    (f) => setVolume(source, f * 100),
    green,
  );
  const volumeBox = h("div", { class: "np-volume" }, volIcon, volume.el);
  const controls = h("div", { class: "np-controls" },
    h("div", { class: "np-buttons" }, shuffle, prev, play, next, repeat),
    h("div", { class: "grow" }),
    volumeBox,
  );
  const playingEl = h("div", { class: "np-card" }, head, progressRow, controls);

  // Idle: the same layout as the other idle cards.
  const idleDot = dot(colorHex("--color-success"), 5);
  const idleText = h("span");
  const idleAction = h("button", { class: "link-btn", style: `color:${green}d9` });
  let idleRun: () => void = () => void source.open();
  idleAction.addEventListener("click", () => idleRun());
  const idleSub = h("span");
  const idleEl = h("div", { class: "int-card" },
    h("div", { class: "int-head" }, dot(green, 7), h("b", { text: source.name }), idleSub),
    h("div", { class: "int-status" }, idleDot, idleText),
    h("div", { class: "int-actions" }, idleAction),
  );

  const el = h("div", { class: "np-root" });
  let showing: "playing" | "idle" | null = null;
  let shownTrack: SpotifyTrack | null = null;
  let playIcon: boolean | null = null;
  let volIconLevel = -1;
  let timer: number | null = null;
  let repeatPath: string | null = null;

  function paintProgress() {
    const s = store.state;
    const d = s.track?.duration ?? 0;
    const pos = dragFraction != null ? dragFraction * d : spotifyPosition(s, Date.now());
    const f = dragFraction ?? (d > 0 ? Math.min(1, Math.max(0, pos / d)) : 0);
    progress.set(f);
    elapsed.textContent = formatTime(f * d);
    remaining.textContent = d > 0 ? `-${formatTime(Math.max(0, d - f * d))}` : "";
  }

  /** The position runs on its own clock, twice a second, only while it plays on screen. */
  function syncTimer() {
    const run = store.state.playing && showing === "playing" && el.isConnected &&
      State.mode === "expanded" && State.view === "overview";
    if (run && timer == null) {
      timer = window.setInterval(() => {
        if (!(store.state.playing && el.isConnected && State.mode === "expanded" && State.view === "overview")) {
          if (timer != null) window.clearInterval(timer);
          timer = null;
          return;
        }
        paintProgress();
      }, 500);
    } else if (!run && timer != null) {
      window.clearInterval(timer);
      timer = null;
    }
  }

  function syncPlaying(track: SpotifyTrack) {
    const s = store.state;
    if (shownTrack !== track) {
      shownTrack = track;
      title.textContent = isAd(track) ? t("Advertisement") : track.title;
      const sub = [track.artist, track.album].filter((x) => x).join(" · ");
      subtitle.textContent = sub;
      subtitle.style.display = sub ? "" : "none";
    } else if (isAd(track)) {
      title.textContent = t("Advertisement");
    }
    art.title = track.album ? t(source.text.openWith, { 0: track.album }) : t(source.text.open);
    const cover = currentArtwork(s, store);
    if (cover) {
      if (artImg.getAttribute("src") !== cover) artImg.setAttribute("src", cover);
      art.classList.add("has-art");
    } else {
      art.classList.remove("has-art");
      artImg.removeAttribute("src");
    }

    progress.enabled = !isAd(track) && track.duration > 0;
    paintProgress();

    shuffle.style.color = s.shuffle ? green : colorHex("--color-text-faint");
    shuffle.title = s.shuffle ? t("Shuffle on") : t("Shuffle off");
    const path = source.repeatIcon?.() ?? ICONS.repeat;
    if (path !== repeatPath) {
      repeatPath = path;
      clear(repeat);
      repeat.append(svg(path, 10, { stroke: 2.2 }));
    }
    repeat.style.color = s.repeat ? green : colorHex("--color-text-faint");
    repeat.title = s.repeat ? t("Repeat on") : t("Repeat off");
    prev.title = t("Previous");
    next.title = t("Next");
    if (playIcon !== s.playing) {
      playIcon = s.playing;
      clear(play);
      const icon = svg(s.playing ? ICONS.pause : ICONS.play, 9);
      if (!s.playing) icon.style.transform = "translateX(1px)";
      play.append(icon);
    }
    play.title = s.playing ? t("Pause") : t("Play");

    const level = volumeLevel(s.volume);
    if (level !== volIconLevel) {
      volIconLevel = level;
      clear(volIcon);
      volIcon.append(svg(itemAt([ICONS.volume0, ICONS.volume1, ICONS.volume2, ICONS.volume3], level), 10, { stroke: 2.2 }));
    }
    volume.set(s.volume / 100);
    volumeBox.title = t("Volume {0}%", { 0: s.volume });
  }

  function syncIdle() {
    idleSub.textContent = t(pillDefinition(source.id)?.subtitle ?? N_("Integration"));
    const status = source.status?.();
    if (status) {
      idleDot.style.background = status.ok ? colorHex("--color-success") : colorHex("--color-danger");
      idleText.textContent = status.text;
      idleAction.textContent = status.action ?? "";
      idleAction.style.display = status.action ? "" : "none";
      idleRun = status.run;
      return;
    }
    const installed = store.state.installed || store.state.running;
    idleDot.style.background = installed ? colorHex("--color-success") : colorHex("--color-danger");
    idleText.textContent = installed ? t("Not playing") : t(source.text.notInstalled);
    idleAction.textContent = installed ? t(source.text.open) : t(source.text.get);
    idleAction.style.display = "";
    idleRun = () => void source.open();
  }

  return {
    el,
    sync() {
      const track = store.state.track;
      const want = track ? "playing" : "idle";
      if (want !== showing) {
        showing = want;
        clear(el);
        el.append(want === "playing" ? playingEl : idleEl);
        shownTrack = null;
      }
      if (track) syncPlaying(track);
      else syncIdle();
      syncTimer();
    },
  };
}
