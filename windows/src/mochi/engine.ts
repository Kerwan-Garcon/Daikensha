// Mochi — direct port of NotchBuddy/Sources/App/BotEngine.swift to Canvas 2D.
// Same constants, same tweens, same easings, same particles. The only intentional
// difference is the `happy`/`wink` eye arc, which follows the prototype
// (design/prototype/notch-buddy.html, the visual source of truth) — the Swift
// arc angles produce a different shape.

import { Ease, lerp, type EaseFn } from "../core/anim";
import { Sound } from "../core/sound";
import type { BotEmoteName, BotStateName } from "../core/layout";
import { PUMPKIN_BODY, drawOutfitBehind, drawOutfitFront, makeHead } from "./outfits";
import type { Outfit } from "./wardrobe";
import {
  DEFAULT_LOOK, browPose, easeBrows, isCharacterLook, mouthForEyes,
  type Brows, type MochiLook, type MochiMouth,
} from "./look";

// ── Types ─────────────────────────────────────────────────────────────────────

export type EyeShape =
  | "pill" | "wide" | "dot" | "line" | "flat" | "happy" | "closed"
  | "spiral" | "heart" | "star" | "tired" | "wink" | "cup";

export type BadgeKind = "dots" | "bang" | "question" | "dot";

export interface Badge {
  kind: BadgeKind;
  color: RGB;
}

export type RGB = readonly [number, number, number]; // components 0…1

export type TweenKey = readonly [target: number, durationMs: number, ease: EaseFn];

interface Tween {
  prop: PropKey;
  keys: TweenKey[];
  index: number;
  from: number;
  startMs: number;
  onComplete?: () => void;
}

type PropKey =
  | "yaw" | "pitch" | "roll" | "tilt" | "open" | "sx" | "sy"
  | "oy" | "ox" | "tint" | "morph" | "hands" | "blush" | "es" | "badgeS"
  | "outfitPresence" | "shadesDrop";

interface BotStateCfg {
  color: RGB;
  tint: number;
  eye: EyeShape;
  badge: Badge | null;
  bounces: boolean;
  scans: boolean;
  breathes: boolean;
  zz: boolean;
  sweat: boolean;
  look: readonly [number, number] | null;
  tilt: number;
}

interface Particle {
  type: "heart" | "star" | "spark" | "sweat" | "z";
  x: number; y: number; vx: number; vy: number;
  age: number; life: number; rot: number; size: number;
}

// ── Constants (MochiConst / PISTES.mochi) ─────────────────────────────────────

const EYE_W = 0.25;
const EYE_H = 0.27;
const EYE_SP = 0.37;
const EYE_P = -0.12;
const BASE_TOP: RGB = [0.929, 0.929, 0.937]; // #EDEDEF
const BASE_BOTTOM: RGB = [0.769, 0.773, 0.792]; // #C4C5CA
const INK = "rgb(26,20,18)"; // #1A1412
const MINI_INK = "rgb(16,19,26)"; // #10131A

// The looks' palettes (./look.ts) — drawings keep their own colours (docs/DESIGN_SYSTEM.md).
const PORTRAIT = {
  skinTop: "#EBBB93", skinBottom: "#C98D62",
  hair: "#2A211D", braidLine: "#4A3B33", parting: "#6B4A38",
  clip: "#E2554B", clipDark: "#B83A33",
  frame: "#1B1716", lensTop: "#A3A6AC", lensBottom: "#46484D",
  tee: "#1F1B1A", teeRib: "#3A3433", teeth: "#F6E6D3",
};
const SLIME = {
  skinTop: "#F8EDE6", skinBottom: "#E2CABD", eye: "#A3172F", brow: "#3E86B4",
  hairTop: "#9ADBF2", hairBottom: "#4B9FD0", strand: "rgba(196,238,250,0.8)",
  cloth: "#FBFCFE", clothShade: "#D3DAE6", fold: "#B9C3D3",
};
const SAGE = { bodyTop: "#FFF1BE", bodyBottom: "#E8A93C", eye: "#4A2600", gold: "#FFB52E" };

const C = {
  idle: [0.902, 0.914, 0.933] as RGB,
  working: [0.231, 0.62, 1] as RGB,
  thinking: [0.545, 0.361, 0.965] as RGB,
  searching: [0.388, 0.396, 0.949] as RGB,
  approval: [0.961, 0.647, 0.141] as RGB,
  question: [0.133, 0.827, 0.933] as RGB,
  error: [0.957, 0.314, 0.369] as RGB,
  finished: [0.204, 0.831, 0.6] as RGB,
  ratelimit: [0.984, 0.573, 0.235] as RGB,
  sleeping: [0.58, 0.635, 0.722] as RGB,
  dizzy: [0.957, 0.447, 0.714] as RGB,
};

const base = {
  bounces: false, scans: false, breathes: false, zz: false, sweat: false,
  look: null, tilt: 0,
};

export const BOT_STATES: Record<BotStateName, BotStateCfg> = {
  idle: { ...base, color: C.idle, tint: 0, eye: "pill", badge: null },
  working: { ...base, color: C.working, tint: 0.72, eye: "pill", badge: { kind: "dots", color: C.working } },
  thinking: { ...base, color: C.thinking, tint: 0.72, eye: "pill", badge: { kind: "dots", color: C.thinking }, look: [0.55, 0.55] },
  searching: { ...base, color: C.searching, tint: 0.72, eye: "pill", badge: { kind: "dots", color: C.searching }, scans: true },
  approval: { ...base, color: C.approval, tint: 0.78, eye: "wide", badge: { kind: "bang", color: C.approval }, bounces: true },
  question: { ...base, color: C.question, tint: 0.75, eye: "pill", badge: { kind: "question", color: C.question }, tilt: 0.17 },
  error: { ...base, color: C.error, tint: 0.78, eye: "flat", badge: { kind: "dot", color: C.error } },
  finished: { ...base, color: C.finished, tint: 0.35, eye: "happy", badge: { kind: "dot", color: C.finished } },
  ratelimit: { ...base, color: C.ratelimit, tint: 0.72, eye: "tired", badge: { kind: "dot", color: C.ratelimit }, sweat: true },
  sleeping: { ...base, color: C.sleeping, tint: 0.32, eye: "closed", badge: null, breathes: true, zz: true },
  dizzy: { ...base, color: C.dizzy, tint: 0.7, eye: "spiral", badge: null },
};

/** State → sound, as in BotStateCfg.sound. */
export const STATE_SOUND: Partial<Record<BotStateName, string>> = {
  working: "work", thinking: "think", searching: "search", approval: "approval",
  question: "question", error: "error", finished: "finish", ratelimit: "rate",
  sleeping: "sleep", dizzy: "dizzy",
};

const EMOTE_EYE: Record<BotEmoteName, EyeShape> = {
  love: "heart", surprised: "dot", proud: "star", wink: "wink",
  yawn: "tired", happy: "happy", annoyed: "line",
};

// ── Small helpers ─────────────────────────────────────────────────────────────

const now = () => performance.now() / 1000;

export function hexToRGB(hex: string): RGB {
  const h = hex.replace("#", "");
  const v = parseInt(h, 16);
  return [((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255];
}

const rgba = (c: RGB, a = 1) =>
  `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${a})`;

const mix3 = (a: RGB, b: RGB, t: number): RGB => [
  lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t),
];

function roundRectPath(x: CanvasRenderingContext2D, X: number, Y: number, W: number, H: number, R: number) {
  const r = Math.max(0, Math.min(R, W / 2, H / 2));
  x.beginPath();
  x.moveTo(X + r, Y);
  x.arcTo(X + W, Y, X + W, Y + H, r);
  x.arcTo(X + W, Y + H, X, Y + H, r);
  x.arcTo(X, Y + H, X, Y, r);
  x.arcTo(X, Y, X + W, Y, r);
  x.closePath();
}

function heartPath(x: CanvasRenderingContext2D, s: number) {
  x.beginPath();
  x.moveTo(0, s * 0.38);
  x.bezierCurveTo(-s * 1.05, -s * 0.15, -s * 0.5, -s * 0.95, 0, -s * 0.38);
  x.bezierCurveTo(s * 0.5, -s * 0.95, s * 1.05, -s * 0.15, 0, s * 0.38);
  x.closePath();
}

function starPath(x: CanvasRenderingContext2D, ro: number, ri: number) {
  x.beginPath();
  for (let i = 0; i < 10; i++) {
    const r = i % 2 ? ri : ro;
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    x.lineTo(Math.cos(a) * r, Math.sin(a) * r);
  }
  x.closePath();
}

const FONT = `system-ui, "Segoe UI Variable Text", "Segoe UI", sans-serif`;

// ── Dance (BotEngine.applyDance, macOS) ──────────────────────────────────────

/** The dance fades in over 0.3 s and out over 0.5 s. */
export function stepDanceLevel(level: number, dancing: boolean, dt: number): number {
  const target = dancing ? 1 : 0;
  if (level < target) return Math.min(target, level + dt / 0.3);
  if (level > target) return Math.max(target, level - dt / 0.5);
  return level;
}

/**
 * The 112-BPM bounce at `seconds`, for a body of radius R and a dance `level`
 * (0…1): a sideways sway, a hop, a tilt, and a squash on landing.
 */
export function danceTransform(seconds: number, level: number, R: number) {
  const beat = (seconds * 112) / 60;
  const hop = Math.abs(Math.sin(Math.PI * beat));
  const land = Math.pow(1 - hop, 6);
  return {
    dx: 0.08 * R * Math.sin(Math.PI * beat) * level,
    dy: -0.2 * R * hop * level,
    rotate: 0.1 * Math.sin(Math.PI * beat) * level,
    sx: 1 + 0.045 * land * level,
    sy: 1 - 0.06 * land * level,
  };
}

// ── Engine ────────────────────────────────────────────────────────────────────

export class BotEngine {
  isMini = false;
  /** Solid body colour for mini bots / integration pills (null = Mochi gradient). */
  bodyColor: RGB | null = null;

  // Animated state (BotEngine `s`)
  yaw = 0; pitch = 0; roll = 0; tilt = 0; open = 1;
  sx = 1; sy = 1; oy = 0; ox = 0;
  tint = 0; morph = 0; hands = 0; blush = 0; es = 1; badgeS = 0;

  // Outfit (the main Mochi only — minis never wear one). `outfit` is what is
  // drawn; it changes only once the previous one has left.
  outfit: Outfit = "none";
  /** 0 = gone, 1 = fully on. */
  outfitPresence = 0;
  private outfitTarget: Outfit = "none";

  /**
   * Classic face, signature traits (mouth, eye glints, pink cheeks, quirks), or a
   * character on top of them: portrait, slime, sage (./look.ts). Main Mochi only.
   */
  look: MochiLook = DEFAULT_LOOK;
  private nextQuirk = now() + 20 + Math.random() * 20;
  /** Portrait and slime: the brows ease from pose to pose instead of snapping. */
  private brows: Brows = { lift: 0, angle: 0.05, arch: 0.22, asym: 0 };
  /** Portrait: 0 = sunglasses up on his head, 1 = dropped onto his eyes. */
  shadesDrop = 0;
  /** Sage: how far the rune circles have turned, and how fast they turn now. */
  private sagePhase = 0;
  private sageSpin = 0.35;

  // Spring lag of the soft parts (pompoms, hat tips, scarf end), −1…1.
  physDx = 0;
  physDy = 0;
  private physVx = 0;
  private physVy = 0;
  private prevYaw = 0;
  private prevOy = 0;
  private prevRoll = 0;

  // Targets
  tgYaw = 0; tgPitch = 0; tgTilt = 0; tgSy = 1; tgSx = 1; tgEs = 1;

  /** Extra canvas height above the body so hearts can fly out without clipping. */
  particleOverhang = 0;

  /** Dancing to music; `dancingLevel` follows it, 0→1 in 0.3 s, 1→0 in 0.5 s. */
  isDancing = false;
  dancingLevel = 0;

  // Mouth spring (fraction of R)
  slotH = 0; slotHTarget = 0; slotHVel = 0; isChewing = false;

  col: RGB = C.idle;
  colT: RGB = C.idle;

  state: BotStateName = "idle";
  cfg: BotStateCfg = BOT_STATES.idle;

  eyeOverride: EyeShape | null = null;
  eyeOverrideUntil = 0;
  permanentEye: EyeShape | null = null;
  permanentEmote: BotEmoteName | null = null;
  miniNextBehavior = 0;

  badge: Badge | null = null;
  private badgeKey = "none";
  private badgeToken = 0;

  private tweens = new Map<PropKey, Tween>();
  private locks = new Set<PropKey>();
  private particles: Particle[] = [];

  lookX = 0;
  lookY = 0;

  lastTime = now();
  private t0 = now() - Math.random() * 5;
  private nextBlink = now() + 1.5 + Math.random() * 2;
  waveUntil = 0;
  waveStart = 0;
  private greetToken = 0;
  private lastAmbient = 0;
  private slapTimes: number[] = [];
  private miniLookTarget = { x: 0, y: 0 };
  private miniLookNextTime = 0;

  /** Fired when three slaps land inside 1.7 s (→ dizzy + confused view). */
  onDizzy: (() => void) | null = null;

  // ── Public API ──────────────────────────────────────────────────────────────

  setState(next: BotStateName, force = false) {
    if (this.state === next && !force) return;
    const prev = this.state;
    this.state = next;
    this.cfg = BOT_STATES[next];
    this.colT = this.cfg.color;
    if (!this.locks.has("tint")) this.tint = this.cfg.tint;
    if (!this.locks.has("tilt")) this.tgTilt = this.cfg.tilt;
    this.setBadge(this.cfg.badge);

    switch (next) {
      case "finished":
        this.doRoll(950, 1);
        setTimeout(() => this.emit("spark", 5), 500);
        // Portrait: job done, sunglasses on — after the roll
        setTimeout(() => { if (this.state === "finished") this.dropShades(1.6); }, 1100);
        break;
      case "error":
        this.anim("ox", [
          [0.08, 50, Ease.out], [-0.08, 70, Ease.inOut],
          [0.05, 70, Ease.inOut], [0, 90, Ease.out],
        ]);
        break;
      case "approval":
        this.anim("oy", [[-0.2, 150, Ease.out], [0, 300, Ease.back]]);
        break;
      case "dizzy":
        this.doRoll(1300, 2);
        break;
      case "question":
        this.blink();
        break;
      case "ratelimit":
        this.emit("sweat", 1);
        break;
      default:
        if (prev !== "idle" || next !== "idle") this.blink();
    }
  }

  setBadge(b: Badge | null) {
    const key = b ? `${b.kind}-${b.color.join(",")}` : "none";
    if (key === this.badgeKey) return;
    this.badgeKey = key;
    const tok = ++this.badgeToken;
    this.anim("badgeS", [[0, 90, Ease.inOut]]);
    setTimeout(() => {
      if (tok !== this.badgeToken) return;
      this.badge = b;
      if (b) this.anim("badgeS", [[1, 280, Ease.back]]);
    }, 100);
  }

  blink() {
    if (this.locks.has("open")) return;
    this.anim("open", [[0.06, 70, Ease.inOut], [1, 130, Ease.out]]);
  }

  squash() {
    this.physVy += 0.6;
    this.anim("sy", [[0.78, 70, Ease.out], [1.1, 130, Ease.out], [1, 170, Ease.inOut]]);
    this.anim("sx", [[1.16, 70, Ease.out], [0.95, 130, Ease.out], [1, 170, Ease.inOut]]);
  }

  /** Mailbox swallow — opens the slot, chews, then closes. */
  gulp() {
    this.slotHTarget = 0.42;
    setTimeout(() => {
      this.slotHTarget = 0;
      this.isChewing = true;
      setTimeout(() => { this.isChewing = false; }, 800);
    }, 460);
    this.anim("sy", [[0.78, 80, Ease.out], [1.18, 130, Ease.out], [1, 220, Ease.back]]);
    this.anim("sx", [[1.28, 80, Ease.out], [0.92, 130, Ease.out], [1, 220, Ease.back]]);
    this.blink();
  }

  slap() {
    this.interruptGreet();
    if (this.state === "dizzy") return;
    const t = now();
    this.slapTimes = this.slapTimes.filter((s) => t - s < 1.7);
    this.slapTimes.push(t);
    Sound.play("slap");
    this.squash();
    this.physVy -= 1.2;
    this.physVx += Math.random() < 0.5 ? 0.7 : -0.7;
    if (this.slapTimes.length >= 3) {
      this.slapTimes = [];
      this.onDizzy?.();
    } else {
      this.eyeOverride = "line";
      this.eyeOverrideUntil = t + 0.8;
      setTimeout(() => Sound.play("annoyed"), 60);
    }
  }

  doRoll(durationMs: number, turns: number) {
    this.roll = 0;
    this.anim("roll", [[Math.PI * 2 * turns, durationMs, Ease.inOut]], () => { this.roll = 0; });
  }

  /** Peek wave — the "coucou". Timings from BotEngine.greet(). */
  greet() {
    const t = now();
    const tok = ++this.greetToken;
    this.waveStart = t + 0.45;
    this.waveUntil = t + 1.55;
    this.physVx += 0.2;

    this.eyeOverride = "happy";
    this.eyeOverrideUntil = t + 2.0;
    this.anim("oy", [[-0.06, 220, Ease.out], [0.0, 220, Ease.back]]);

    setTimeout(() => {
      if (this.greetToken !== tok) return;
      this.anim("hands", [[1, 280, Ease.out]]);
      this.anim("sy", [[0.95, 100, Ease.out], [1.0, 260, Ease.back]]);
      this.anim("sx", [[1.04, 100, Ease.out], [1.0, 260, Ease.back]]);
      Sound.play("greet");
    }, 250);

    setTimeout(() => { if (this.greetToken === tok) this.blink(); }, 550);
    setTimeout(() => { if (this.greetToken === tok) this.blink(); }, 1500);
    setTimeout(() => {
      if (this.greetToken !== tok) return;
      this.waveUntil = 0;
      this.anim("hands", [[0, 200, Ease.inOut]]);
    }, 1550);
    setTimeout(() => {
      if (this.greetToken !== tok) return;
      this.eyeOverride = "happy";
      this.eyeOverrideUntil = now() + 0.3;
    }, 1750);
  }

  interruptGreet() {
    if (this.hands <= 0.01 && now() >= this.waveUntil) return;
    this.greetToken++;
    this.waveUntil = 0;
    this.waveStart = 0;
    this.anim("hands", [[0, 150, Ease.inOut]]);
  }

  setPermanentEmote(emote: BotEmoteName | null) {
    this.permanentEmote = emote;
    if (emote === "wink") {
      this.miniNextBehavior = now() + 0.8 + Math.random() * 1.7;
      return;
    }
    this.permanentEye = emote ? EMOTE_EYE[emote] : null;
    if (this.permanentEye) {
      this.eyeOverride = this.permanentEye;
      this.eyeOverrideUntil = Number.POSITIVE_INFINITY;
    } else if (this.eyeOverrideUntil === Number.POSITIVE_INFINITY) {
      this.eyeOverride = null;
      this.eyeOverrideUntil = 0;
    }
    this.miniNextBehavior = now() + 0.8 + Math.random() * 1.7;
  }

  triggerEmote(emote: BotEmoteName, duration = 1.8) {
    const t = now();
    this.eyeOverride = EMOTE_EYE[emote];
    this.eyeOverrideUntil = t + duration;

    switch (emote) {
      case "love":
        this.anim("blush", [
          [1, 300, Ease.out], [1, (duration - 0.6) * 1000, Ease.lin], [0, 300, Ease.inOut],
        ]);
        this.emit("heart", 4);
        this.anim("oy", [[-0.1, 160, Ease.out], [0, 300, Ease.back]]);
        break;
      case "surprised":
        this.anim("oy", [[-0.3, 140, Ease.out], [0, 380, Ease.back]]);
        this.anim("es", [[1.25, 120, Ease.out], [1, 500, Ease.inOut]]);
        break;
      case "proud":
        this.emit("star", 5);
        this.dropShades(Math.max(0.4, duration - 0.7));
        this.anim("tilt", [
          [-0.14, 220, Ease.out], [-0.14, (duration - 0.5) * 1000, Ease.lin], [0, 280, Ease.inOut],
        ]);
        this.anim("blush", [
          [0.7, 250, Ease.out], [0.7, (duration - 0.5) * 1000, Ease.lin], [0, 300, Ease.inOut],
        ]);
        break;
      case "wink":
        this.anim("tilt", [
          [0.12, 160, Ease.out], [0.12, (duration - 0.4) * 1000, Ease.lin], [0, 240, Ease.inOut],
        ]);
        break;
      case "yawn":
        this.anim("sy", [[1.12, 500, Ease.inOut], [1, 500, Ease.inOut]]);
        this.anim("sx", [[0.94, 500, Ease.inOut], [1, 500, Ease.inOut]]);
        setTimeout(() => { this.eyeOverride = "closed"; this.emit("z", 2); }, 700);
        break;
      case "happy":
        this.anim("blush", [[0.6, 200, Ease.out], [0, 600, Ease.inOut]]);
        break;
      case "annoyed":
        this.eyeOverride = "line";
        this.eyeOverrideUntil = t + 0.8;
        setTimeout(() => Sound.play("annoyed"), 60);
        break;
    }
  }

  emit(type: Particle["type"], count: number) {
    for (let i = 0; i < count; i++) {
      const isZ = type === "z";
      this.particles.push({
        type,
        x: (Math.random() - 0.5) * 0.9 + (isZ ? 0.55 : 0),
        y: -0.7 - Math.random() * 0.2,
        vx: (Math.random() - 0.5) * 0.35 + (isZ ? 0.18 : 0),
        vy: -(0.45 + Math.random() * 0.35),
        age: -i * 0.14,
        life: 1.3 + Math.random() * 0.5,
        rot: Math.random() * Math.PI * 2,
        size: 0.15 + Math.random() * 0.08,
      });
    }
  }

  animateMorph(target: number, durationMs?: number) {
    const dur = durationMs ?? (target > 0.5 ? 550 : 650);
    this.anim("morph", [[target, dur, Ease.inOut]]);
  }

  resetMorph() {
    this.tweens.delete("morph");
    this.locks.delete("morph");
    this.morph = 0;
  }

  /**
   * Dresses Mochi. Animated: the old outfit leaves (180 ms), the new one drops
   * in (350 ms) and Mochi does a little squash — BotEngine.setOutfit on macOS.
   */
  setOutfit(next: Outfit, animated = true) {
    if (next === this.outfitTarget) return;
    this.outfitTarget = next;
    this.tweens.delete("outfitPresence");
    this.locks.delete("outfitPresence");
    const enter = () => {
      this.outfit = next;
      this.anim("outfitPresence", [[1, 350, Ease.inOut]], () => this.squash());
    };
    if (!animated) {
      this.outfit = next;
      this.outfitPresence = next !== "none" ? 1 : 0;
    } else if (next === "none") {
      this.anim("outfitPresence", [[0, 180, Ease.inOut]], () => { this.outfit = "none"; });
    } else if (this.outfit === "none") {
      this.outfitPresence = 0;
      enter();
    } else {
      this.anim("outfitPresence", [[0, 180, Ease.inOut]], enter);
    }
  }

  /** Wearing something visible: the body then turns as one piece when it rolls. */
  private get rigidRoll(): boolean {
    return !this.isMini && this.outfit !== "none" && this.outfitPresence > 0.05;
  }

  /** True while anything is still moving — lets the island stop its RAF loop. */
  get busy(): boolean {
    return (
      this.tweens.size > 0 ||
      this.particles.length > 0 ||
      this.cfg.bounces || this.cfg.scans || this.cfg.breathes || this.cfg.zz || this.cfg.sweat ||
      this.isMini ||
      this.isDancing || this.dancingLevel > 0.001 ||
      Math.abs(this.tgYaw - this.yaw) > 0.002 ||
      Math.abs(this.tgPitch - this.pitch) > 0.002 ||
      Math.abs(this.tgTilt - this.tilt) > 0.002 ||
      Math.abs(this.tgSy - this.sy) > 0.002 ||
      Math.abs(this.tgSx - this.sx) > 0.002 ||
      Math.abs(this.tgEs - this.es) > 0.002 ||
      this.slotH > 0.001 || Math.abs(this.slotHVel) > 0.001 ||
      Math.abs(this.col[0] - this.colT[0]) > 0.003 ||
      Math.abs(this.col[1] - this.colT[1]) > 0.003 ||
      Math.abs(this.col[2] - this.colT[2]) > 0.003 ||
      (this.outfit !== "none" && (Math.abs(this.physVx) > 0.01 || Math.abs(this.physVy) > 0.01)) ||
      // The looks: the sage's circles always turn; braids, locks and brows settle.
      this.hasSage ||
      (this.hasCharacter && (Math.abs(this.physVx) > 0.01 || Math.abs(this.physVy) > 0.01 ||
        this.browsMoving))
    );
  }

  // ── Tweens ──────────────────────────────────────────────────────────────────

  anim(prop: PropKey, keys: TweenKey[], onComplete?: () => void) {
    this.tweens.set(prop, {
      prop, keys, index: 0, from: this[prop], startMs: performance.now(), onComplete,
    });
    this.locks.add(prop);
  }

  // ── Update ──────────────────────────────────────────────────────────────────

  update(dt: number) {
    const n = now();
    const nowMs = performance.now();

    for (const tw of [...this.tweens.values()]) {
      const k = tw.keys[tw.index];
      const p = Math.min(1, Math.max(0, (nowMs - tw.startMs) / k[1]));
      this[tw.prop] = tw.from + (k[0] - tw.from) * k[2](p);
      if (p >= 1) {
        tw.from = k[0];
        tw.index += 1;
        tw.startMs = nowMs;
        if (tw.index >= tw.keys.length) {
          this.tweens.delete(tw.prop);
          this.locks.delete(tw.prop);
          tw.onComplete?.();
        }
      }
    }

    const t = n - this.t0;
    let ty = this.lookX * 0.62;
    let tp = this.lookY * 0.5;

    if (this.cfg.look) {
      ty = ty * 0.35 + this.cfg.look[0] * 0.55;
      tp = tp * 0.3 + this.cfg.look[1] * 0.5;
    }
    if (this.cfg.scans) {
      ty = Math.sin(t * 2.6) * 0.6;
      tp = -0.06;
    }
    if (this.state === "sleeping") { ty = 0; tp = -0.14; }
    if (this.state === "dizzy") { ty = Math.sin(t * 9) * 0.25; }

    // Mini bots never follow the mouse — they wander.
    if (this.isMini && !this.cfg.look && !this.cfg.scans && this.state !== "sleeping" && this.state !== "dizzy") {
      if (n > this.miniLookNextTime) {
        this.miniLookTarget = {
          x: -0.88 + Math.random() * 1.76,
          y: -0.55 + Math.random() * 1.0,
        };
        this.miniLookNextTime = n + 0.5 + Math.random() * 1.5;
      }
      ty = this.miniLookTarget.x * 0.62;
      tp = this.miniLookTarget.y * 0.5;
    }

    this.tgYaw = ty;
    this.tgPitch = tp;
    this.tgTilt = this.cfg.tilt;

    if (n > this.waveStart && n < this.waveUntil) {
      const wt = n - this.waveStart;
      this.tgTilt = -0.06 + Math.sin(2 * Math.PI * 1.2 * wt) * 0.07;
    }

    const bounce = this.cfg.bounces ? -Math.abs(Math.sin(t * 5.2)) * 0.07 : 0;
    const kGen = 1 - Math.pow(0.0008, dt);
    if (!this.locks.has("oy")) this.oy += (bounce - this.oy) * kGen;

    if (this.cfg.breathes) {
      const amp = this.isMini ? 0.07 : 0.035;
      this.tgSy = 1 + Math.sin(t * 1.8) * amp;
      this.tgSx = 1 - Math.sin(t * 1.8) * amp * 0.57;
    } else if (this.isMini) {
      this.tgSy = 1 + Math.sin(t * 2.2) * 0.04;
      this.tgSx = 1 - Math.sin(t * 2.2) * 0.02;
    } else {
      this.tgSy = 1;
      this.tgSx = 1;
    }

    if (this.isMini && n > this.miniNextBehavior) this.doMiniBehaviorLoop();

    // Signature looks: now and then, a little quirk of his own while he rests.
    if (this.hasSignature && n > this.nextQuirk) this.doQuirk(n);

    // Portrait and slime: the brows ease towards the pose of the moment.
    if (this.hasCharacter) {
      this.brows = easeBrows(this.brows, browPose(this.state, this.shownEye, t), 1 - Math.pow(0.0004, dt));
    }

    // Sage: the circles turn faster while he works, slow down at rest.
    if (this.hasSage) {
      const busy = this.state === "working" || this.state === "thinking" || this.state === "searching";
      const target = busy ? 1.4 : this.state === "sleeping" ? 0.12 : 0.35;
      this.sageSpin += (target - this.sageSpin) * (1 - Math.pow(0.05, dt));
      this.sagePhase = (this.sagePhase + this.sageSpin * dt) % (Math.PI * 200);
    }

    const kLook = 1 - Math.pow(0.0025, dt);
    if (!this.locks.has("yaw")) this.yaw += (this.tgYaw - this.yaw) * kLook;
    if (!this.locks.has("pitch")) this.pitch += (this.tgPitch - this.pitch) * kLook;
    if (!this.locks.has("tilt")) this.tilt += (this.tgTilt - this.tilt) * kGen;
    if (!this.locks.has("sy")) this.sy += (this.tgSy - this.sy) * kGen;
    if (!this.locks.has("sx")) this.sx += (this.tgSx - this.sx) * kGen;
    if (!this.locks.has("es")) this.es += (this.tgEs - this.es) * kGen;

    this.col = mix3(this.col, this.colT, 1 - Math.pow(0.002, dt));

    if (n > this.nextBlink) {
      if (this.state !== "sleeping" && this.state !== "dizzy") {
        this.blink();
        if (Math.random() < 0.22) setTimeout(() => this.blink(), 230);
      }
      this.nextBlink = n + 2.2 + Math.random() * 3.2;
    }

    if (this.eyeOverride && n > this.eyeOverrideUntil) {
      this.eyeOverride = this.permanentEye;
      if (this.permanentEye) this.eyeOverrideUntil = Number.POSITIVE_INFINITY;
    }

    if (n - this.lastAmbient > 1.3) {
      this.lastAmbient = n;
      if (this.cfg.zz) this.emit("z", 1);
      if (!this.isMini && this.cfg.sweat && Math.random() < 0.5) this.emit("sweat", 1);
      // Sage: a golden spark now and then
      if (this.hasSage && Math.random() < 0.45) this.emit("spark", 1);
    }

    for (const p of this.particles) p.age += dt;
    this.particles = this.particles.filter((p) => p.age < p.life);

    // Mouth slot spring — ω₀ = 2π/0.25, ζ = 0.6
    const omega = (2 * Math.PI) / 0.25;
    const zeta = 0.6;
    const acc = omega * omega * (this.slotHTarget - this.slotH) - 2 * zeta * omega * this.slotHVel;
    this.slotHVel += acc * dt;
    this.slotH = Math.max(0, this.slotH + this.slotHVel * dt);

    this.dancingLevel = stepDanceLevel(this.dancingLevel, this.isDancing, dt);

    // Soft-part spring: lags behind head turns, hops and rolls (stiffness 60, damping 9).
    if (dt > 0) {
      const yawVel = (this.yaw - this.prevYaw) / dt;
      const oyVel = (this.oy - this.prevOy) / dt;
      // A finished roll snaps from 2π·turns back to 0: that jump is not motion.
      const dRoll = this.roll - this.prevRoll;
      const rollVel = Math.abs(dRoll) > Math.PI ? 0 : dRoll / dt;
      const centrifugal = this.rigidRoll ? rollVel * 0.18 : 0;
      const tDx = Math.max(-1, Math.min(1, -yawVel * 0.35 - this.tilt * 2 + centrifugal));
      const tDy = Math.max(-1, Math.min(1, oyVel * 0.5));
      this.physVx += (60 * (tDx - this.physDx) - 9 * this.physVx) * dt;
      this.physVy += (60 * (tDy - this.physDy) - 9 * this.physVy) * dt;
      this.physDx += this.physVx * dt;
      this.physDy += this.physVy * dt;
    }
    this.prevYaw = this.yaw;
    this.prevOy = this.oy;
    this.prevRoll = this.roll;

    this.lastTime = n;
  }

  private doMiniBehaviorLoop() {
    const n = now();
    switch (this.permanentEmote) {
      case "happy":
        if (this.locks.has("oy")) { this.miniNextBehavior = n + 0.4; return; }
        this.anim("oy", [[-0.3, 120, Ease.out], [0.03, 200, Ease.inOut], [0, 160, Ease.back]]);
        this.anim("sy", [[0.82, 80, Ease.out], [1.18, 130, Ease.out], [0.88, 160, Ease.inOut], [1, 200, Ease.back]]);
        this.anim("sx", [[1.15, 80, Ease.out], [0.88, 130, Ease.out], [1.06, 160, Ease.inOut], [1, 200, Ease.back]]);
        this.miniNextBehavior = n + 2.2 + Math.random() * 1.2;
        break;
      case "annoyed":
        if (this.locks.has("yaw")) { this.miniNextBehavior = n + 0.5; return; }
        this.anim("yaw", [
          [-0.65, 50, Ease.out], [0.65, 90, Ease.inOut], [-0.5, 80, Ease.inOut],
          [0.4, 75, Ease.inOut], [-0.2, 70, Ease.inOut], [0, 140, Ease.out],
        ]);
        this.miniNextBehavior = n + 3.0 + Math.random() * 2.5;
        break;
      case "wink":
        this.eyeOverride = "wink";
        this.eyeOverrideUntil = n + 0.55;
        this.anim("tilt", [[0.13, 100, Ease.out], [0.13, 320, Ease.lin], [0, 200, Ease.inOut]]);
        this.miniNextBehavior = n + 2.2 + Math.random() * 2.0;
        break;
      case "love":
        this.emit("heart", 2);
        this.anim("tilt", [[-0.1, 180, Ease.out], [0.1, 340, Ease.inOut], [0, 220, Ease.inOut]]);
        this.miniNextBehavior = n + 2.6 + Math.random() * 1.5;
        break;
      default:
        this.miniNextBehavior = n + 3.0 + Math.random() * 2.0;
    }
  }

  // ── Dancing ─────────────────────────────────────────────────────────────────

  setDancing(dancing: boolean) {
    if (this.isDancing !== dancing) this.isDancing = dancing;
  }

  /**
   * Applies the dance bounce and sway around the bottom of the body. Call it on
   * a saved context, before draw(), with the same size (BotEngine.applyDance).
   */
  applyDance(x: CanvasRenderingContext2D, W: number, H: number) {
    if (this.dancingLevel <= 0.001) return;
    const R = W * 0.3;
    const px = W / 2 + this.ox * R;
    const py = H / 2 + this.particleOverhang / 2 + this.oy * R + R * 0.06 + R * 0.88;
    const d = danceTransform(now(), this.dancingLevel, R);
    x.translate(px + d.dx, py + d.dy);
    x.rotate(d.rotate);
    x.scale(d.sx, d.sy);
    x.translate(-px, -py);
  }

  // ── Draw ────────────────────────────────────────────────────────────────────

  /**
   * Draws hands, body, blush, eyes, mouth, badge and particles into a canvas of
   * `w`×`h` CSS pixels (the caller has already applied the DPR transform).
   */
  draw(x: CanvasRenderingContext2D, W: number, H: number) {
    const R = W * 0.3;
    const rx = R * 1.14;
    const ry = R * 0.88;
    const cx = W / 2 + this.ox * R;
    const cy = H / 2 + this.particleOverhang / 2 + this.oy * R + R * 0.06;

    // With an outfit on, a roll turns the whole character — hat included — as
    // one piece instead of rolling the eyes over the body (BotCanvasView, macOS).
    x.save();
    if (this.rigidRoll && Math.abs(this.roll) > 0.001) {
      x.translate(cx, cy);
      x.rotate(this.roll);
      x.translate(-cx, -cy);
    }

    this.drawHandsBehind(x, R, rx, ry, cx, cy);

    x.save();
    x.translate(cx, cy);
    if (this.tilt !== 0) x.rotate(this.tilt);
    x.scale(this.sx, this.sy);

    const dressed = !this.isMini && this.outfit !== "none";
    const head = dressed ? makeHead(R, this.yaw, this.pitch, this.physDx, this.physDy) : null;
    const outfitState = { presence: this.outfitPresence, morph: this.morph };
    if (head) drawOutfitBehind(x, this.outfit, head, outfitState);

    const body = this.bodyPath(rx, ry, R);
    // Behind the body: the portrait's braids, the slime's hood and locks, the sage's circles.
    this.drawBraids(x, R, rx, ry);
    this.drawHoodBack(x, body, R, rx);
    this.drawSageAura(x, R);
    this.drawBody(x, body, R, rx, ry);
    this.drawSageGlow(x, body, R);
    // On the face: beard and tee, hair and clip; the slime's robe, fringe and hood.
    this.drawBeardAndTee(x, body, R, rx, ry);
    this.drawHair(x, body, R, rx, ry);
    this.drawSlimeFront(x, body, R, rx, ry);

    // Signature looks: his rice-cake cheeks stay a little pink.
    const blushVal = Math.max(this.blush, this.tint * 0.5, this.hasSignature ? 0.38 : 0) * (1 - this.morph);
    if (blushVal > 0.01) {
      x.save();
      x.clip(body);
      const yOffset = Math.sin(this.yaw) * rx * 0.8;
      x.fillStyle = `rgba(255,120,150,${0.5 * blushVal})`;
      for (const sd of [-1, 1]) {
        x.beginPath();
        x.ellipse(sd * rx * 0.55 + yOffset, ry * 0.2, R * 0.17, R * 0.1, 0, 0, Math.PI * 2);
        x.fill();
      }
      x.restore();
    }

    this.drawEyes(x, body, R, rx, ry);
    if (this.morph > 0.05) this.drawMouth(x, body, R);
    else if (this.hasSignature) this.drawFaceMouth(x, body, R, rx, ry);
    this.drawBrows(x, body, R, rx, ry);
    this.drawShades(x, R, rx, ry);

    if (head) drawOutfitFront(x, this.outfit, head, outfitState);

    x.restore();
    x.restore();

    if (this.badge && this.badgeS > 0.01 && this.morph < 0.25) {
      this.drawBadge(x, this.badge, R, cx, cy);
    }
    this.drawParticles(x, R, cx, cy);
  }

  private bodyPath(rx: number, ry: number, R: number): Path2D {
    const n = 72;
    const expN = 2.0 / 2.7;
    const tw = R * 1.0;
    const th = R * 0.94;
    const tr = R * 0.42;
    const p = new Path2D();
    const m = this.morph;
    for (let i = 0; i <= n; i++) {
      const a = (i / n) * Math.PI * 2;
      const ca = Math.cos(a);
      const sa = Math.sin(a);
      const px0 = rx * (ca >= 0 ? Math.pow(ca, expN) : -Math.pow(-ca, expN));
      const py0 = ry * (sa >= 0 ? Math.pow(sa, expN) : -Math.pow(-sa, expN));
      let px = px0;
      let py = py0;
      if (m >= 0.005) {
        const rr = rrPoint(ca, sa, tw, th, tr);
        px = lerp(px0, rr.x, m);
        py = lerp(py0, rr.y, m);
      }
      if (i === 0) p.moveTo(px, py);
      else p.lineTo(px, py);
    }
    p.closePath();
    return p;
  }

  /** How much of the pumpkin's orange shows on the body (it comes and goes with the outfit). */
  private get pumpkinAlpha(): number {
    if (this.isMini || this.outfit !== "pumpkin") return 0;
    return Math.min(1, this.outfitPresence * 2.5) * (1 - this.morph);
  }

  private drawBody(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    const pumpkin = this.pumpkinAlpha;
    if (this.bodyColor) {
      // Mini bots: flat solid fill — no gradient, no reflection, no highlight
      x.fillStyle = rgba(this.bodyColor, 1);
      x.fill(body);
      if (pumpkin <= 0.001) return;
    } else {
      const skin = this.skinColors;
      const g = x.createLinearGradient(rx * 0.7, -ry * 0.85, -rx * 0.8, ry * 0.9);
      g.addColorStop(0, skin ? skin[0] : rgba(BASE_TOP));
      g.addColorStop(1, skin ? skin[1] : rgba(BASE_BOTTOM));
      x.fillStyle = g;
      x.fill(body);
    }
    x.save();
    if (pumpkin > 0.001) {
      const pg = x.createLinearGradient(rx * 0.7, -ry * 0.85, -rx * 0.8, ry * 0.9);
      pg.addColorStop(0, PUMPKIN_BODY[0]);
      pg.addColorStop(1, PUMPKIN_BODY[1]);
      x.globalAlpha = pumpkin;
      x.fillStyle = pg;
      x.fill(body);
      // A flat-coloured body only gets the shading while it is a pumpkin.
      x.globalAlpha = this.bodyColor ? pumpkin : 1;
    }

    // Light on skin: the state's colour must not paint him blue or purple.
    const effectiveTint = this.tint * (1 - this.morph) * (this.skinColors ? 0.4 : 1);
    if (effectiveTint > 0.01) {
      const tg = x.createLinearGradient(0, ry, 0, -ry);
      tg.addColorStop(0, rgba(this.col, 0.72 * effectiveTint));
      tg.addColorStop(1, rgba(this.col, 0));
      x.fillStyle = tg;
      x.fill(body);
    }

    const sh = x.createRadialGradient(0, 0, R * 0.15, 0, 0, R * 1.25);
    sh.addColorStop(0, "rgba(0,0,0,0)");
    sh.addColorStop(0.6, "rgba(0,0,0,0)");
    sh.addColorStop(1, "rgba(0,0,0,0.2)");
    x.fillStyle = sh;
    x.fill(body);

    const hl = x.createRadialGradient(rx * 0.34, -ry * 0.46, 0, rx * 0.34, -ry * 0.46, R * 0.42);
    hl.addColorStop(0, "rgba(255,255,255,0.55)");
    hl.addColorStop(1, "rgba(255,255,255,0)");
    x.fillStyle = hl;
    x.fill(body);
    x.restore();
  }

  /** The eyes he shows this frame (state, emote, dance, mailbox). The mouth and brows read them too. */
  private get shownEye(): EyeShape {
    let shape: EyeShape = this.eyeOverride ?? this.cfg.eye;
    // Dancing: happy eyes in the calm states.
    if (this.isDancing && this.dancingLevel > 0.15 && !this.isMini && (this.state === "idle" || this.state === "finished")) {
      shape = "happy";
    }
    if (this.morph > 0.5) {
      if (this.isChewing) shape = "happy";
      else if (this.slotHTarget > 0.05 || this.slotH > 0.1) shape = "cup";
    }
    // Portrait and slime: cross with his eyes open, the brows and teeth say it.
    if (this.hasCharacter && shape === "flat") shape = "pill";
    return shape;
  }

  private drawEyes(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    const shape = this.shownEye;

    x.save();
    x.clip(body);
    const ink = this.eyeInk;
    x.fillStyle = ink;
    x.strokeStyle = ink;

    for (const sd of [-1, 1]) {
      const eyeYaw = sd * EYE_SP + this.yaw;
      // Rolling with an outfit on, the whole body turns: the eyes must not roll again.
      let eyePitch = EYE_P + this.pitch + (this.rigidRoll ? 0 : this.roll);
      eyePitch = (((eyePitch + Math.PI) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
      const cp = Math.cos(eyePitch);
      if (Math.cos(eyeYaw) * cp <= 0.04) continue;

      const ex = Math.sin(eyeYaw) * cp * rx;
      const ey = -Math.sin(eyePitch) * ry + (this.morph > 0 ? ry * 0.14 * this.morph : 0);
      const fx = lerp(Math.max(0.18, Math.cos(eyeYaw)), 1, this.morph * 0.7);
      const fy = lerp(Math.max(0.18, cp), 1, this.morph * 0.7);
      const eyeMult = this.isMini ? 1.9 : 1.0;
      const ew = R * EYE_W * this.es * eyeMult;
      const eh = R * EYE_H * this.es * eyeMult;

      x.save();
      x.translate(ex, ey);
      x.scale(fx, fy);
      this.drawEyeShape(x, shape, ew, eh, sd, ink);
      x.restore();
    }
    x.restore();
  }

  private drawEyeShape(
    x: CanvasRenderingContext2D, shape: EyeShape,
    w: number, h: number, sd: number, ink: string,
  ) {
    const t = now();
    switch (shape) {
      case "wide":
        this.drawEyeShape(x, "pill", w * 1.16, h * 1.12, sd, ink);
        break;
      case "pill": {
        const hh = Math.max(h * this.open, w * 0.3);
        roundRectPath(x, -w / 2, -hh / 2, w, hh, Math.min(w / 2, hh / 2));
        x.fill();
        this.drawCatchlights(x, w, hh, ink);
        break;
      }
      case "dot":
        x.beginPath();
        x.arc(0, 0, w * 0.45, 0, Math.PI * 2);
        x.fill();
        break;
      case "line":
        x.rotate(-sd * 0.2);
        roundRectPath(x, -w * 0.78, -w * 0.21, w * 1.56, w * 0.42, w * 0.21);
        x.fill();
        break;
      case "flat":
        roundRectPath(x, -w * 0.72, -w * 0.2, w * 1.44, w * 0.4, w * 0.2);
        x.fill();
        break;
      case "happy":
        x.lineWidth = w * 0.5;
        x.lineCap = "round";
        x.beginPath();
        x.arc(0, h * 0.18, w * 0.82, Math.PI * 1.12, Math.PI * 1.88);
        x.stroke();
        break;
      case "closed":
        x.lineWidth = w * 0.36;
        x.lineCap = "round";
        x.beginPath();
        x.arc(0, -h * 0.08, w * 0.78, Math.PI * 0.15, Math.PI * 0.85);
        x.stroke();
        break;
      case "spiral": {
        x.lineWidth = w * 0.22;
        x.lineCap = "round";
        x.beginPath();
        for (let a = 0; a < 4.4 * Math.PI; a += 0.2) {
          const r = w * 0.06 + a * w * 0.058;
          const aa = a + t * 9 * sd;
          const px = Math.cos(aa) * r;
          const py = Math.sin(aa) * r;
          if (a === 0) x.moveTo(px, py);
          else x.lineTo(px, py);
        }
        x.stroke();
        break;
      }
      case "heart":
        x.fillStyle = "#FF4D6D";
        heartPath(x, w * 1.2);
        x.fill();
        x.fillStyle = ink;
        break;
      case "star":
        x.fillStyle = "#F7B32B";
        x.rotate(t * 1.5 * sd);
        starPath(x, w * 1.05, w * 0.46);
        x.fill();
        x.fillStyle = ink;
        break;
      case "tired":
        roundRectPath(x, -w / 2, -h * 0.02, w, h * 0.38, w / 2);
        x.fill();
        roundRectPath(x, -w * 0.62, -h * 0.1, w * 1.24, w * 0.22, w * 0.11);
        x.fill();
        break;
      case "wink":
        if (sd < 0) {
          const hh = Math.max(h * this.open, w * 0.3);
          roundRectPath(x, -w / 2, -hh / 2, w, hh, Math.min(w / 2, hh / 2));
          x.fill();
          this.drawCatchlights(x, w, hh, ink);
        } else {
          x.lineWidth = w * 0.5;
          x.lineCap = "round";
          x.beginPath();
          x.arc(0, h * 0.18, w * 0.82, Math.PI * 1.12, Math.PI * 1.88);
          x.stroke();
        }
        break;
      case "cup": {
        // Flat top, rounded bottom corners (U shape) — used while the box is open
        const hh = Math.max(h * this.open, w * 0.3);
        const cr = Math.min(w / 2, hh / 2);
        x.beginPath();
        x.moveTo(-w / 2, -hh / 2);
        x.lineTo(w / 2, -hh / 2);
        x.lineTo(w / 2, hh / 2 - cr);
        x.quadraticCurveTo(w / 2, hh / 2, w / 2 - cr, hh / 2);
        x.lineTo(-w / 2 + cr, hh / 2);
        x.quadraticCurveTo(-w / 2, hh / 2, -w / 2, hh / 2 - cr);
        x.closePath();
        x.fill();
        break;
      }
    }
  }

  // ── Looks (./look.ts) ───────────────────────────────────────────────────────

  private get hasSignature(): boolean { return this.look !== "classic" && !this.isMini; }
  private get hasPortrait(): boolean { return this.look === "portrait" && !this.isMini; }
  private get hasSlime(): boolean { return this.look === "slime" && !this.isMini; }
  private get hasSage(): boolean { return this.look === "sage" && !this.isMini; }
  private get hasCharacter(): boolean { return isCharacterLook(this.look) && !this.isMini; }

  private get browsMoving(): boolean {
    const to = browPose(this.state, this.shownEye, now() - this.t0);
    const b = this.brows;
    return Math.abs(to.lift - b.lift) + Math.abs(to.angle - b.angle) + Math.abs(to.arch - b.arch) +
      Math.abs(to.asym - b.asym) > 0.004;
  }

  /** The body's gradient for the looks that change it (null: Mochi's own grey). */
  private get skinColors(): readonly [string, string] | null {
    if (this.isMini) return null;
    switch (this.look) {
      case "portrait": return [PORTRAIT.skinTop, PORTRAIT.skinBottom];
      case "slime": return [SLIME.skinTop, SLIME.skinBottom];
      case "sage": return [SAGE.bodyTop, SAGE.bodyBottom];
      default: return null;
    }
  }

  /** The eyes' colour: red for the slime, amber for the sage, ink otherwise. */
  private get eyeInk(): string {
    if (this.isMini) return MINI_INK;
    if (this.look === "slime") return SLIME.eye;
    if (this.look === "sage") return SAGE.eye;
    return INK;
  }

  /** Where the front of his head is, along x: everything on his face turns with it. */
  private faceX(rx: number, k: number): number { return Math.sin(this.yaw) * rx * k; }

  /** Two glints in an open pill eye, lit from the upper right; they fade as it closes. */
  private drawCatchlights(x: CanvasRenderingContext2D, w: number, hh: number, ink: string) {
    if (!this.hasSignature || hh <= w * 0.75) return;
    const a = Math.min(1, (hh / w - 0.75) / 0.25);
    x.fillStyle = `rgba(255,255,255,${0.92 * a})`;
    x.beginPath();
    x.arc(w * 0.14, -hh * 0.2, w * 0.2, 0, Math.PI * 2);
    x.fill();
    x.fillStyle = `rgba(255,255,255,${0.55 * a})`;
    x.beginPath();
    x.arc(-w * 0.14, hh * 0.22, w * 0.08, 0, Math.PI * 2);
    x.fill();
    x.fillStyle = ink;
  }

  /** A small mouth below the eyes, on the same sphere: it turns with his head and hides in a roll. */
  private drawFaceMouth(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    const mouth: MochiMouth = this.hasCharacter && this.state === "error" && !this.eyeOverride
      ? "grit"
      : mouthForEyes(this.shownEye, false, this.hasCharacter);
    if (mouth === "none") return;

    const mouthP = EYE_P - 0.22 + this.pitch;
    let seenP = mouthP + (this.rigidRoll ? 0 : this.roll);
    seenP = (((seenP + Math.PI) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
    if (Math.cos(this.yaw) * Math.cos(seenP) <= 0.04) return;
    const cp = Math.cos(mouthP);

    x.save();
    x.clip(body);
    x.translate(Math.sin(this.yaw) * cp * rx, -Math.sin(mouthP) * ry);
    x.scale(Math.max(0.18, Math.cos(this.yaw)), Math.max(0.18, cp));
    const m = R * 0.11;
    x.fillStyle = INK;
    x.strokeStyle = INK;
    x.lineWidth = m * 0.42;
    x.lineCap = "round";
    x.lineJoin = "round";
    x.beginPath();
    switch (mouth) {
      case "cat":
        x.moveTo(-m * 1.1, -m * 0.1);
        x.quadraticCurveTo(-m * 0.55, m * 0.95, 0, -m * 0.1);
        x.quadraticCurveTo(m * 0.55, m * 0.95, m * 1.1, -m * 0.1);
        x.stroke();
        break;
      case "smile": {
        const d = new Path2D();
        d.moveTo(-m * 1.15, -m * 0.35);
        d.lineTo(m * 1.15, -m * 0.35);
        d.quadraticCurveTo(0, m * 2.0, -m * 1.15, -m * 0.35);
        d.closePath();
        x.fill(d);
        x.clip(d);
        x.fillStyle = "#FF7A93";
        x.beginPath();
        x.ellipse(0, m * 0.7, m * 0.6, m * 0.45, 0, 0, Math.PI * 2);
        x.fill();
        break;
      }
      case "oh":
        x.ellipse(0, 0, m * 0.42, m * 0.5, 0, 0, Math.PI * 2);
        x.fill();
        break;
      case "gasp":
        x.ellipse(0, 0, m * 0.6, m * 0.7, 0, 0, Math.PI * 2);
        x.fill();
        break;
      case "smirk":
        x.moveTo(-m * 0.8, m * 0.1);
        x.quadraticCurveTo(m * 0.2, m * 0.8, m * 1.0, -m * 0.35);
        x.stroke();
        break;
      case "flat":
        x.moveTo(-m * 0.8, 0);
        x.lineTo(m * 0.8, 0);
        x.stroke();
        break;
      case "wavy":
        for (let i = 0; i <= 16; i++) {
          const f = i / 16;
          x.lineTo(-m * 1.1 + m * 2.2 * f, Math.sin(f * Math.PI * 4) * m * 0.28);
        }
        x.lineWidth = m * 0.36;
        x.stroke();
        break;
      case "content":
        x.moveTo(-m * 0.6, -m * 0.1);
        x.quadraticCurveTo(0, m * 0.7, m * 0.6, -m * 0.1);
        x.stroke();
        break;
      case "talking": {
        const h = m * (0.3 + 0.9 * Math.abs(Math.sin((now() - this.t0) * Math.PI * 3.5)));
        x.ellipse(0, 0, m * 0.65, h / 2, 0, 0, Math.PI * 2);
        x.fill();
        break;
      }
      case "grit": {
        // Lopsided open mouth, teeth showing
        const g = m * 1.3;
        const d = new Path2D();
        d.moveTo(-g * 1.15, g * 0.25);
        d.quadraticCurveTo(0, -g * 0.8, g * 1.1, -g * 0.15);
        d.quadraticCurveTo(-g * 0.1, g * 1.0, -g * 1.15, g * 0.25);
        d.closePath();
        x.fill(d);
        x.translate(0, -g * 0.04);
        x.scale(0.8, 0.62);
        x.fillStyle = PORTRAIT.teeth;
        x.fill(d);
        break;
      }
    }
    x.restore();
  }

  /** One of his quirks, at rest only: never over a state's animation, a greeting, a dance or an emote. */
  private doQuirk(n: number) {
    this.nextQuirk = n + 22 + Math.random() * 26;
    if (this.state !== "idle" || this.eyeOverride || this.isDancing || this.hands >= 0.01 ||
        this.morph >= 0.01 || this.locks.has("sy") || this.locks.has("oy") || this.locks.has("yaw")) return;
    const pick = Math.floor(Math.random() * 3);
    if (pick === 0) {
      // Mochi stretch: pulled up like rice dough, then snaps back and wobbles
      this.eyeOverride = "happy";
      this.eyeOverrideUntil = n + 1.0;
      this.anim("sy", [[1.14, 380, Ease.inOut], [1.14, 160, Ease.lin], [0.86, 110, Ease.out], [1.04, 160, Ease.inOut], [1, 200, Ease.back]]);
      this.anim("sx", [[0.9, 380, Ease.inOut], [0.9, 160, Ease.lin], [1.12, 110, Ease.out], [0.98, 160, Ease.inOut], [1, 200, Ease.back]]);
    } else if (pick === 1) {
      // Curious peek: glances aside, eyes a touch wider, then back to you
      const side = Math.random() < 0.5 ? 1 : -1;
      this.anim("yaw", [[0.5 * side, 260, Ease.out], [0.5 * side, 600, Ease.lin], [0, 320, Ease.inOut]]);
      this.anim("es", [[1.1, 240, Ease.out], [1.1, 620, Ease.lin], [1, 320, Ease.inOut]]);
      setTimeout(() => this.blink(), 1200);
    } else {
      this.triggerEmote("wink", 0.9);
    }
  }

  // Portrait ───────────────────────────────────────────────────────────────────

  /** Two braids hanging from the sides; each bead lags the one above, they hang with gravity. */
  private drawBraids(x: CanvasRenderingContext2D, R: number, rx: number, ry: number) {
    if (!this.hasPortrait || this.morph >= 0.05) return;
    const t = now() - this.t0;
    const length = R * 0.72;
    x.save();
    for (const sd of [-1, 1]) {
      const baseX = sd * rx * 0.97 + this.faceX(rx, 0.25);
      for (let i = 0; i < 5; i++) {
        const f = i / 4;
        const drift = Math.sin(t * 1.6 + f * 1.3 + sd) * R * 0.018 * f;
        const bx = baseX + sd * R * 0.02 * f
          + this.physDx * R * 0.16 * f * f   // swings behind his moves, the tip most
          + Math.sin(this.tilt) * length * f   // hangs straight down when he tilts
          + drift;
        const by = ry * 0.2 + f * length * Math.cos(this.tilt) + this.physDy * R * 0.1 * f * f;
        const r = R * (0.11 - 0.015 * f);
        x.fillStyle = PORTRAIT.hair;
        x.beginPath();
        x.ellipse(bx, by, r, r * 1.15, 0, 0, Math.PI * 2);
        x.fill();
        x.strokeStyle = PORTRAIT.braidLine;
        x.lineWidth = Math.max(0.5, r * 0.25);
        x.beginPath();
        x.moveTo(bx - r * 0.6, by - r * 0.5);
        x.quadraticCurveTo(bx, by + r * 0.3, bx + r * 0.6, by - r * 0.2);
        x.stroke();
      }
    }
    x.restore();
  }

  /** Cornrows over the top of his head, their partings meeting at the forehead, and a red clip. */
  private drawHair(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    if (!this.hasPortrait || this.morph >= 0.95) return;
    x.save();
    x.clip(body);
    x.globalAlpha = 1 - this.morph;
    const x0 = this.faceX(rx, 0.85);

    // Everything above the hairline; it comes down at the temples like a fade.
    const cap = new Path2D();
    cap.moveTo(-rx * 1.3, -ry * 1.3);
    cap.lineTo(rx * 1.3, -ry * 1.3);
    cap.lineTo(rx * 1.3, -R * 0.05);
    cap.quadraticCurveTo(x0 + rx * 0.55, -R * 0.56, x0, -R * 0.48);
    cap.quadraticCurveTo(x0 - rx * 0.55, -R * 0.56, -rx * 1.3, -R * 0.05);
    cap.closePath();
    x.fillStyle = PORTRAIT.hair;
    x.fill(cap);

    x.save();
    x.clip(cap);
    x.strokeStyle = PORTRAIT.parting;
    x.lineWidth = Math.max(0.5, R * 0.03);
    x.lineCap = "round";
    for (let i = -2; i <= 2; i++) {
      x.beginPath();
      x.moveTo(x0 + i * R * 0.08, -R * 0.44);
      x.quadraticCurveTo(x0 * 0.7 + i * rx * 0.28, -ry * 0.72, x0 * 0.4 + i * rx * 0.48, -ry * 1.05);
      x.stroke();
    }
    x.restore();

    // The red clip, on his left temple (the viewer's right)
    x.translate(x0 + rx * 0.58, -R * 0.58);
    x.fillStyle = PORTRAIT.clip;
    for (const sd of [-1, 1]) {
      x.save();
      x.rotate(sd * 0.9 + 0.3);
      x.beginPath();
      x.ellipse(0, -R * 0.075, R * 0.045, R * 0.065, 0, 0, Math.PI * 2);
      x.fill();
      x.restore();
    }
    x.fillStyle = PORTRAIT.clipDark;
    x.beginPath();
    x.arc(0, 0, R * 0.03, 0, Math.PI * 2);
    x.fill();
    x.restore();
  }

  /** Portrait: drops the sunglasses onto his eyes, holds, puts them back up. */
  dropShades(holdSeconds: number) {
    if (!this.hasPortrait || this.outfit !== "none") return;
    this.anim("shadesDrop", [[1, 260, Ease.back], [1, holdSeconds * 1000, Ease.lin], [0, 420, Ease.inOut]]);
    setTimeout(() => this.squash(), 260);
  }

  /** A point on his head, `a` round from the front and `p` up from the middle — the eyes' sphere. */
  private headPoint(a: number, p: number, rx: number, ry: number): { x: number; y: number } {
    const ya = a + this.yaw;
    const pa = p + this.pitch;
    return { x: Math.sin(ya) * Math.cos(pa) * rx, y: -Math.sin(pa) * ry };
  }

  /**
   * Sunglasses on the crown of his head, bobbing on the head's spring; now and then
   * (done, proud) they drop onto his eyes. Off when he wears an outfit.
   */
  private drawShades(x: CanvasRenderingContext2D, R: number, rx: number, ry: number) {
    if (!this.hasPortrait || this.morph >= 0.05 || this.outfit !== "none") return;
    const d = this.shadesDrop;
    const onHead = this.headPoint(0, 1.05, rx, ry);
    const onEyes = this.headPoint(0, EYE_P, rx, ry);
    x.save();
    x.translate(lerp(onHead.x, onEyes.x, d), lerp(onHead.y, onEyes.y, d) + this.physDy * R * 0.06 * (1 - d));
    x.rotate((this.physDx * 0.12 - this.tilt * 0.3) * (1 - d));
    x.scale(Math.max(0.5, Math.cos(this.yaw)), lerp(0.9, 1, d));
    const lw = R * lerp(0.4, 0.5, d);
    const lh = R * lerp(0.24, 0.32, d);
    const gap = R * lerp(0.06, 0.1, d);
    const rim = R * 0.035;

    x.strokeStyle = PORTRAIT.frame;
    x.lineWidth = R * 0.05;
    x.lineCap = "round";
    x.beginPath();
    x.moveTo(-gap - rim, -lh * 0.1);
    x.quadraticCurveTo(0, -lh * 0.4, gap + rim, -lh * 0.1);
    x.stroke();

    // The glint slides across the lenses as he turns
    const glintShift = -Math.sin(this.yaw) * lw * 0.25;
    for (const sd of [-1, 1]) {
      const lx = sd < 0 ? -gap - lw : gap;
      roundRectPath(x, lx - rim, -lh / 2 - rim, lw + rim * 2, lh + rim * 2, lh * 0.5 + rim);
      x.fillStyle = PORTRAIT.frame;
      x.fill();
      const glass = x.createLinearGradient(0, -lh / 2, 0, lh / 2);
      glass.addColorStop(0, PORTRAIT.lensTop);
      glass.addColorStop(1, PORTRAIT.lensBottom);
      roundRectPath(x, lx, -lh / 2, lw, lh, lh * 0.5);
      x.fillStyle = glass;
      x.fill();
      x.save();
      x.clip();
      x.strokeStyle = "rgba(255,255,255,0.6)";
      x.lineWidth = Math.max(0.5, lh * 0.14);
      x.beginPath();
      x.moveTo(lx + lw / 2 + lw * 0.02 + glintShift, -lh / 2 + lh * 0.3);
      x.lineTo(lx + lw / 2 + lw * 0.22 + glintShift, -lh / 2 + lh * 0.3);
      x.stroke();
      x.restore();
    }
    x.restore();
  }

  /** A light beard shadow on his chin and the collar of a black tee. */
  private drawBeardAndTee(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    if (!this.hasPortrait || this.morph >= 0.05) return;
    x.save();
    x.clip(body);
    const x0 = this.faceX(rx, 0.8);
    const beard = x.createRadialGradient(x0, R * 0.5, 0, x0, R * 0.5, R * 0.5);
    beard.addColorStop(0, "rgba(42,33,29,0.28)");
    beard.addColorStop(1, "rgba(42,33,29,0)");
    x.fillStyle = beard;
    x.beginPath();
    x.ellipse(x0, R * 0.5, R * 0.5, R * 0.16, 0, 0, Math.PI * 2);
    x.fill();

    const top = ry * 0.68;
    const neckline = new Path2D();
    neckline.moveTo(-rx * 1.3, top - R * 0.04);
    neckline.quadraticCurveTo(this.faceX(rx, 0.5), top + R * 0.12, rx * 1.3, top - R * 0.04);
    const tee = new Path2D(neckline);
    tee.lineTo(rx * 1.3, ry * 1.3);
    tee.lineTo(-rx * 1.3, ry * 1.3);
    tee.closePath();
    x.fillStyle = PORTRAIT.tee;
    x.fill(tee);
    x.strokeStyle = PORTRAIT.teeRib;
    x.lineWidth = Math.max(0.5, R * 0.035);
    x.stroke(neckline);
    x.restore();
  }

  /** Eyebrows above each eye, easing between poses and lagging a hop (portrait and slime). */
  private drawBrows(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    if (!this.hasCharacter || this.morph >= 0.05) return;
    const pose = this.brows;
    x.save();
    x.clip(body);
    // The slime's are finer, the blue of his hair
    x.strokeStyle = this.hasSlime ? SLIME.brow : PORTRAIT.hair;
    x.lineWidth = R * (this.hasSlime ? 0.05 : 0.075);
    x.lineCap = "round";
    for (const sd of [-1, 1]) {
      const eyeYaw = sd * EYE_SP + this.yaw;
      let seenP = EYE_P + this.pitch + (this.rigidRoll ? 0 : this.roll);
      seenP = (((seenP + Math.PI) % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
      if (Math.cos(eyeYaw) * Math.cos(seenP) <= 0.04) continue;
      const cp = Math.cos(EYE_P + this.pitch);
      const ex = Math.sin(eyeYaw) * cp * rx;
      const ey = -Math.sin(EYE_P + this.pitch) * ry;
      const lift = pose.lift + (sd > 0 ? pose.asym : 0) - this.physDy * 0.04;
      x.save();
      x.translate(ex, ey - R * (0.2 + lift));
      x.scale(Math.max(0.18, Math.cos(eyeYaw)), Math.max(0.18, cp));
      // The inner end is the one towards his nose: + angle brings it down.
      x.rotate(-sd * pose.angle);
      const len = R * 0.3;
      x.beginPath();
      x.moveTo(-len / 2, 0);
      x.quadraticCurveTo(0, -len * pose.arch, len / 2, 0);
      x.stroke();
      x.restore();
    }
    x.restore();
  }

  // Slime ──────────────────────────────────────────────────────────────────────

  /** Behind the body: the hood, wider than his head, and the blue locks along his cheeks. */
  private drawHoodBack(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number) {
    if (!this.hasSlime || this.morph >= 0.05) return;
    const ry = R * 0.88;
    x.save();
    x.translate(this.faceX(rx, 0.1) + this.physDx * R * 0.03, -R * 0.1);
    x.scale(1.14, 1.12);
    const cloth = x.createLinearGradient(0, -ry, 0, ry);
    cloth.addColorStop(0, SLIME.cloth);
    cloth.addColorStop(1, SLIME.clothShade);
    x.fillStyle = cloth;
    x.fill(body);
    x.restore();

    const t = now() - this.t0;
    x.save();
    const hair = x.createLinearGradient(0, -R * 0.1, 0, R * 0.72);
    hair.addColorStop(0, SLIME.hairTop);
    hair.addColorStop(1, SLIME.hairBottom);
    x.fillStyle = hair;
    for (const sd of [-1, 1]) {
      const x0 = sd * rx * 0.9 + this.faceX(rx, 0.2);
      const swing = this.physDx * R * 0.12 + Math.sin(t * 1.4 + sd) * R * 0.015;
      x.beginPath();
      x.moveTo(x0 - sd * R * 0.12, -R * 0.1);
      x.quadraticCurveTo(x0 + sd * R * 0.2, R * 0.3, x0 + sd * R * 0.06 + swing, R * 0.72);
      x.quadraticCurveTo(x0 + swing * 0.5, R * 0.45, x0 - sd * R * 0.1, R * 0.2);
      x.closePath();
      x.fill();
    }
    x.restore();
  }

  /** On the face: the white robe, the blue fringe falling between his eyes, the hood's front. */
  private drawSlimeFront(x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number) {
    if (!this.hasSlime || this.morph >= 0.95) return;
    x.save();
    x.clip(body);
    x.globalAlpha = 1 - this.morph;
    const x0 = this.faceX(rx, 0.85);

    // Robe, draped across him
    const top = ry * 0.66;
    x.fillStyle = SLIME.cloth;
    x.beginPath();
    x.moveTo(-rx * 1.3, top - R * 0.06);
    x.quadraticCurveTo(this.faceX(rx, 0.5), top + R * 0.14, rx * 1.3, top - R * 0.02);
    x.lineTo(rx * 1.3, ry * 1.3);
    x.lineTo(-rx * 1.3, ry * 1.3);
    x.closePath();
    x.fill();
    x.strokeStyle = SLIME.fold;
    x.lineWidth = Math.max(0.5, R * 0.03);
    x.lineCap = "round";
    x.beginPath();
    x.moveTo(-rx * 0.7, top);
    x.quadraticCurveTo(-rx * 0.05, top + R * 0.12, rx * 0.5, ry * 1.05);
    x.moveTo(rx * 0.2, top + R * 0.06);
    x.quadraticCurveTo(rx * 0.6, top + R * 0.12, rx * 0.95, ry * 0.95);
    x.stroke();

    // Fringe: strands ending in points between and over the eyes, longer at the sides.
    const tips: Array<[number, number]> = [
      [-1.25, 0.55], [-0.92, 0.18], [-0.6, -0.02], [-0.28, -0.1], [0.02, -0.04],
      [0.32, -0.12], [0.62, 0.0], [0.92, 0.18], [1.25, 0.55],
    ];
    const fringe = new Path2D();
    fringe.moveTo(-rx * 1.3, -ry * 1.3);
    fringe.lineTo(-rx * 1.3, R * 0.55);
    let prev = { x: -rx * 1.3, y: R * 0.55 };
    tips.forEach(([tx, ty], i) => {
      const pt = { x: x0 * (1 - Math.abs(tx) * 0.5) + tx * rx, y: ty * R };
      if (i > 0) {
        // A sharp notch up between two pointed strands
        const notch = { x: (prev.x + pt.x) / 2, y: Math.min(prev.y, pt.y) - R * 0.14 };
        fringe.lineTo(notch.x, notch.y);
        fringe.lineTo(pt.x, pt.y);
      } else {
        fringe.lineTo(pt.x, pt.y);
      }
      prev = pt;
    });
    fringe.lineTo(rx * 1.3, R * 0.55);
    fringe.lineTo(rx * 1.3, -ry * 1.3);
    fringe.closePath();
    const hair = x.createLinearGradient(0, -ry, 0, R * 0.3);
    hair.addColorStop(0, SLIME.hairTop);
    hair.addColorStop(1, SLIME.hairBottom);
    x.fillStyle = hair;
    x.fill(fringe);
    x.save();
    x.clip(fringe);
    x.strokeStyle = SLIME.strand;
    x.lineWidth = Math.max(0.5, R * 0.025);
    for (const k of [-0.5, -0.1, 0.3, 0.65]) {
      x.beginPath();
      x.moveTo(x0 * 0.6 + k * rx, -R * 0.55);
      x.quadraticCurveTo(x0 * 0.7 + (k + 0.05) * rx, -R * 0.32, x0 * 0.8 + (k - 0.04) * rx, -R * 0.12);
      x.stroke();
    }
    x.restore();

    // Hood front: over the crown, down the sides, its rim in shadow.
    const rim = new Path2D();
    rim.moveTo(rx * 1.3, R * 0.35);
    rim.quadraticCurveTo(x0 + rx * 0.9, -R * 0.75, x0, -R * 0.6);
    rim.quadraticCurveTo(x0 - rx * 0.9, -R * 0.75, -rx * 1.3, R * 0.35);
    const hood = new Path2D();
    hood.moveTo(-rx * 1.3, -ry * 1.3);
    hood.lineTo(rx * 1.3, -ry * 1.3);
    hood.lineTo(rx * 1.3, R * 0.35);
    hood.quadraticCurveTo(x0 + rx * 0.9, -R * 0.75, x0, -R * 0.6);
    hood.quadraticCurveTo(x0 - rx * 0.9, -R * 0.75, -rx * 1.3, R * 0.35);
    hood.closePath();
    const cloth = x.createLinearGradient(rx * 0.5, -ry, -rx * 0.6, R * 0.3);
    cloth.addColorStop(0, SLIME.cloth);
    cloth.addColorStop(1, SLIME.clothShade);
    x.fillStyle = cloth;
    x.fill(hood);
    x.strokeStyle = SLIME.fold;
    x.lineWidth = Math.max(0.5, R * 0.035);
    x.stroke(rim);
    x.restore();
  }

  // Sage ───────────────────────────────────────────────────────────────────────

  /** Behind the body: light rays, a glow, and rune circles turning both ways. */
  private drawSageAura(x: CanvasRenderingContext2D, R: number) {
    if (!this.hasSage || this.morph >= 0.05) return;
    const ph = this.sagePhase;
    const pulse = 0.5 + 0.5 * Math.sin(ph * 2.2);
    x.save();
    x.scale(Math.max(0.75, Math.cos(this.yaw * 0.6)), 1);

    const glow = x.createRadialGradient(0, 0, R * 0.6, 0, 0, R * 1.5);
    glow.addColorStop(0, "rgba(255,230,154,0.55)");
    glow.addColorStop(1, "rgba(255,230,154,0)");
    x.fillStyle = glow;
    x.beginPath();
    x.arc(0, 0, R * 1.5, 0, Math.PI * 2);
    x.fill();

    // Rays
    x.strokeStyle = `rgba(255,230,154,${0.25 + 0.25 * pulse})`;
    x.lineWidth = Math.max(0.5, R * 0.022);
    x.lineCap = "round";
    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2 + ph * 0.3;
      const r2 = R * (i % 2 === 0 ? 1.55 : 1.3);
      x.beginPath();
      x.moveTo(Math.cos(a) * R * 0.95, Math.sin(a) * R * 0.95);
      x.lineTo(Math.cos(a) * r2, Math.sin(a) * r2);
      x.stroke();
    }

    // Rings: two gold ones, and an outer one in the state's colour
    x.lineWidth = Math.max(0.5, R * 0.025);
    x.strokeStyle = "rgba(255,181,46,0.85)";
    for (const r of [1.1, 1.32]) {
      x.beginPath();
      x.arc(0, 0, R * r, 0, Math.PI * 2);
      x.stroke();
    }
    x.lineWidth = Math.max(0.5, R * 0.018);
    x.strokeStyle = rgba(this.col, 0.55);
    x.beginPath();
    x.arc(0, 0, R * 1.45, 0, Math.PI * 2);
    x.stroke();

    // Runes between the two gold rings, turning forwards
    x.strokeStyle = SAGE.gold;
    x.fillStyle = SAGE.gold;
    x.lineWidth = Math.max(0.5, R * 0.018);
    for (let k = 0; k < 28; k++) {
      const a = (k / 28) * Math.PI * 2 + ph;
      x.save();
      x.translate(Math.cos(a) * R * 1.21, Math.sin(a) * R * 1.21);
      x.rotate(a + Math.PI / 2);
      x.beginPath();
      x.moveTo(0, -R * 0.06);
      x.lineTo(0, R * 0.06);
      switch (k % 4) {
        case 0: x.moveTo(-R * 0.035, -R * 0.02); x.lineTo(R * 0.035, -R * 0.02); break;
        case 1: x.moveTo(-R * 0.03, R * 0.03); x.lineTo(R * 0.03, -R * 0.04); break;
        case 2: x.moveTo(R * 0.025, -R * 0.06); x.arc(0, -R * 0.06, R * 0.025, 0, Math.PI * 2); break;
        default: x.moveTo(-R * 0.035, R * 0.04); x.lineTo(R * 0.035, R * 0.04);
      }
      x.stroke();
      x.restore();
    }

    // Ticks on the inner ring, turning backwards
    x.strokeStyle = "rgba(63,208,192,0.8)";
    x.lineWidth = Math.max(0.5, R * 0.015);
    for (let k = 0; k < 40; k++) {
      const a = (k / 40) * Math.PI * 2 - ph * 1.6;
      const r2 = R * (k % 5 === 0 ? 1.08 : 1.05);
      x.beginPath();
      x.moveTo(Math.cos(a) * R * 1.02, Math.sin(a) * R * 1.02);
      x.lineTo(Math.cos(a) * r2, Math.sin(a) * r2);
      x.stroke();
    }
    x.restore();
  }

  /** On the body: a light from inside, breathing with the circles. */
  private drawSageGlow(x: CanvasRenderingContext2D, body: Path2D, R: number) {
    if (!this.hasSage || this.morph >= 0.05) return;
    const pulse = 0.5 + 0.5 * Math.sin(this.sagePhase * 2.2);
    x.save();
    x.clip(body);
    const g = x.createRadialGradient(0, R * 0.1, 0, 0, R * 0.1, R * 0.9);
    g.addColorStop(0, `rgba(255,255,255,${0.35 + 0.15 * pulse})`);
    g.addColorStop(1, "rgba(255,255,255,0)");
    x.fillStyle = g;
    x.fill(body);
    x.restore();
  }

  /** Mailbox slot: dark pill cut into the box face, with rim and lip highlights. */
  private drawMouth(x: CanvasRenderingContext2D, body: Path2D, R: number) {
    const m = this.morph;
    const hW = R * 1.8 * m;
    const hH = this.slotH * R * m;
    const hX = -hW / 2;
    const boxTop = -R * (0.88 + 0.06 * m);
    const hY = boxTop + R * 0.08 * m;

    x.save();
    x.clip(body);

    x.strokeStyle = `rgba(255,255,255,${0.55 * m})`;
    x.lineWidth = 1;
    x.lineCap = "round";
    x.beginPath();
    x.moveTo(-R * 0.9 * m, boxTop + 1);
    x.lineTo(R * 0.9 * m, boxTop + 1);
    x.stroke();

    if (hH > 0.8) {
      const hR = Math.min(hW / 2, hH / 2);
      const g = x.createLinearGradient(0, hY, 0, hY + hH);
      g.addColorStop(0, "rgb(7,8,10)");
      g.addColorStop(1, "rgb(16,19,26)");
      roundRectPath(x, hX, hY, hW, hH, hR);
      x.fillStyle = g;
      x.fill();
      if (hH > 4) {
        const lipR = Math.min(hR, (hW - 2) / 2);
        x.strokeStyle = `rgba(255,255,255,${0.28 * m})`;
        x.beginPath();
        x.moveTo(hX + lipR, hY + hH - 0.5);
        x.lineTo(hX + hW - lipR, hY + hH - 0.5);
        x.stroke();
      }
    }
    x.restore();
  }

  /** Hands sit behind the body — drawn before it, in world coordinates. */
  private drawHandsBehind(
    x: CanvasRenderingContext2D,
    R: number, rx: number, ry: number, cx: number, cy: number,
  ) {
    if (this.hands <= 0.01 || this.isMini) return;
    if (R <= 14) return; // meaningless at compact/peek sizes

    const n = now();
    const bodyH = 2 * ry;
    const hew = 0.3 * ry * this.hands;
    const heh = 0.26 * ry * this.hands;
    const hwB = rx * this.sx;
    const hhB = ry * this.sy;
    const isWaving = n >= this.waveStart && this.waveStart > 0 && n < this.waveUntil;

    for (const sd of [-1, 1]) {
      let localX: number;
      let localY: number;
      let handRot = 0;

      if (sd > 0 && isWaving) {
        const wt = n - this.waveStart;
        const rise = Math.min(1, wt / 0.18);
        const riseEased = 1 - Math.pow(1 - rise, 3);
        const restX = hwB * 1.08;
        const restY = hhB * 0.7;
        const oscX = Math.cos(13 * wt) * 0.06 * bodyH;
        const oscY = -Math.sin(13 * wt) * 0.14 * bodyH;
        const waveX = hwB * 1.1 + oscX;
        const waveY = -hhB * 0.15 + oscY;
        localX = restX + (waveX - restX) * riseEased;
        localY = restY + (waveY - restY) * riseEased;
        handRot = (-0.5 + Math.sin(13 * wt) * 0.35) * riseEased;
      } else if (sd < 0 && isWaving) {
        const wt = n - this.waveStart;
        localX = -hwB * 1.08;
        localY = hhB * 0.7 + Math.sin(6 * wt) * 0.04 * bodyH;
      } else {
        localX = sd * hwB * 1.08;
        localY = hhB * 0.7;
      }

      const cosT = Math.cos(this.tilt);
      const sinT = Math.sin(this.tilt);
      const worldX = cx + cosT * localX - sinT * localY;
      const worldY = cy + sinT * localX + cosT * localY;

      x.save();
      x.translate(worldX, worldY);
      if (handRot !== 0) x.rotate(handRot);
      const g = x.createLinearGradient(hew * 0.7, -heh * 0.85, -hew * 0.8, heh * 0.9);
      if (this.bodyColor) {
        g.addColorStop(0, rgba(mix3(this.bodyColor, [1, 1, 1], 0.35)));
        g.addColorStop(1, rgba(this.bodyColor));
      } else {
        g.addColorStop(0, rgba(BASE_TOP));
        g.addColorStop(1, rgba(BASE_BOTTOM));
      }
      x.beginPath();
      x.ellipse(0, 0, hew, heh, 0, 0, Math.PI * 2);
      x.fillStyle = g;
      x.fill();
      x.strokeStyle = "rgba(0,0,0,0.08)";
      x.lineWidth = 1;
      x.stroke();
      x.restore();
    }
  }

  private drawBadge(x: CanvasRenderingContext2D, badge: Badge, R: number, cx: number, cy: number) {
    const bs = this.badgeS * (this.isMini ? 1.25 : 1);
    const bx = cx - R * 0.72 * this.sx;
    const by = cy - R * 0.72 * this.sy;
    const t = now();

    x.save();
    x.translate(bx, by);
    x.scale(bs, bs);
    const col = rgba(badge.color);

    if (badge.kind === "dots") {
      if (this.isMini) {
        const phase = (t * 2.4) % 1;
        const dotR = R * 0.22 * (1 + 0.25 * Math.sin(phase * Math.PI * 2));
        x.fillStyle = "#000";
        x.beginPath();
        x.arc(0, 0, R * 0.2, 0, Math.PI * 2);
        x.fill();
        x.fillStyle = col;
        x.beginPath();
        x.arc(0, 0, dotR, 0, Math.PI * 2);
        x.fill();
      } else {
        const pw = R * 0.72;
        const ph = R * 0.36;
        roundRectPath(x, -pw / 2, -ph / 2, pw, ph, ph / 2);
        x.fillStyle = col;
        x.fill();
        for (let i = 0; i < 3; i++) {
          const phase = (((t * 2.4 - i * 0.22) % 1) + 1) % 1;
          const dotR = R * 0.055 * (1 + 0.4 * Math.max(0, Math.sin(phase * Math.PI * 2)));
          x.fillStyle = "#fff";
          x.beginPath();
          x.arc((i - 1) * R * 0.18, 0, dotR, 0, Math.PI * 2);
          x.fill();
        }
      }
    } else if (badge.kind === "bang" || badge.kind === "question") {
      x.fillStyle = "#000";
      x.beginPath();
      x.arc(0, 0, R * 0.3, 0, Math.PI * 2);
      x.fill();
      x.fillStyle = col;
      x.beginPath();
      x.arc(0, 0, R * 0.23, 0, Math.PI * 2);
      x.fill();
      if (!this.isMini) {
        x.fillStyle = "#fff";
        x.font = `900 ${R * 0.32}px ${FONT}`;
        x.textAlign = "center";
        x.textBaseline = "middle";
        x.fillText(badge.kind === "bang" ? "!" : "?", 0, R * 0.02);
      }
    } else {
      x.fillStyle = "#000";
      x.beginPath();
      x.arc(0, 0, R * 0.2, 0, Math.PI * 2);
      x.fill();
      x.fillStyle = col;
      x.beginPath();
      x.arc(0, 0, R * 0.135, 0, Math.PI * 2);
      x.fill();
    }
    x.restore();
  }

  private drawParticles(x: CanvasRenderingContext2D, R: number, cx: number, cy: number) {
    for (const p of this.particles) {
      if (p.age <= 0) continue;
      const k = p.age / p.life;
      const a = k < 0.2 ? k / 0.2 : 1 - (k - 0.2) / 0.8;
      const px = cx + (p.x + p.vx * p.age) * R * 1.3;
      const py = cy + (p.y + p.vy * p.age) * R * 1.3;
      const sz = R * p.size * (1 + k * 0.4);

      x.save();
      x.translate(px, py);
      x.globalAlpha = Math.min(1, Math.max(0, a));
      switch (p.type) {
        case "heart":
          x.rotate(Math.sin(p.age * 6) * 0.3);
          x.fillStyle = "#FF4D6D";
          heartPath(x, sz);
          x.fill();
          break;
        case "star":
          x.rotate(p.rot + p.age * 2);
          x.fillStyle = "#F7B32B";
          starPath(x, sz, sz * 0.45);
          x.fill();
          break;
        case "spark":
          x.rotate(p.rot);
          x.fillStyle = "#fff";
          starPath(x, sz * 0.8, sz * 0.18);
          x.fill();
          break;
        case "sweat":
          x.fillStyle = "#7CC7FF";
          x.beginPath();
          x.moveTo(0, -sz);
          x.quadraticCurveTo(sz * 0.8, sz * 0.2, 0, sz * 0.6);
          x.quadraticCurveTo(-sz * 0.8, sz * 0.2, 0, -sz);
          x.fill();
          break;
        case "z":
          x.fillStyle = "rgb(209,219,235)";
          x.font = `700 ${sz * 1.9}px ${FONT}`;
          x.textAlign = "center";
          x.textBaseline = "middle";
          x.fillText("z", 0, 0);
          break;
      }
      x.restore();
    }
  }
}

/** Ray → rounded-rect boundary intersection, for the mailbox morph. */
function rrPoint(ca: number, sa: number, W: number, H: number, cr: number): { x: number; y: number } {
  const eps = 1e-6;
  const kx = ca >= 0 ? 1 : -1;
  const ky = sa >= 0 ? 1 : -1;
  const cx = kx * (W - cr);
  const cy = ky * (H - cr);

  const dot = ca * cx + sa * cy;
  const disc = dot * dot - (cx * cx + cy * cy - cr * cr);
  if (disc >= 0) {
    const t = dot + Math.sqrt(disc);
    if (t > eps) {
      const px = ca * t;
      const py = sa * t;
      if (Math.abs(px) >= W - cr - eps && Math.abs(py) >= H - cr - eps) return { x: px, y: py };
    }
  }
  if (Math.abs(sa) > eps) {
    const t = (ky * H) / sa;
    if (t > eps) {
      const px = ca * t;
      if (Math.abs(px) <= W - cr + eps) return { x: px, y: ky * H };
    }
  }
  if (Math.abs(ca) > eps) {
    const t = (kx * W) / ca;
    if (t > eps) {
      const py = sa * t;
      if (Math.abs(py) <= H - cr + eps) return { x: kx * W, y: py };
    }
  }
  return { x: kx * W, y: ky * H };
}
