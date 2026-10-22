// Mochi's face — the pure logic, port of NotchBuddy/Sources/CoucouKit/MochiLook.swift.
// Picked with the "Face" switch in the wardrobe and stored in the preferences as
// `mochiLook`. The drawing lives in ./engine.ts.
//
// signature  a mouth that follows his mood, glints in his eyes, pink cheeks, idle quirks
// portrait   signature + tan skin, cornrows and braids, sunglasses on his head, a red
//            clip, a light beard, a black tee, and eyebrows for every state
// slime      a hooded spirit: blue hair, red eyes, a white hood and robe, the same brows
// sage       glows gold inside turning rune circles that speed up while he works
// classic    the face that shipped before

import { N_, labels } from "../i18n/i18n";
import type { BotStateName } from "../core/layout";
import type { EyeShape } from "./engine";

/** Order of the wardrobe switch. The raw values are the Mac's, keep them stable. */
export const MOCHI_LOOKS = ["signature", "portrait", "slime", "sage", "classic"] as const;

export type MochiLook = (typeof MOCHI_LOOKS)[number];

export const DEFAULT_LOOK: MochiLook = "portrait";

/** The English names, the keys of their translations (src/i18n). */
export const LOOK_KEYS: Record<MochiLook, string> = {
  signature: N_("Signature"),
  portrait: N_("Portrait"),
  slime: N_("Slime"),
  sage: N_("Sage"),
  classic: N_("Classic"),
};

/** In the current language: every read goes through `t()`. */
export const LOOK_LABELS: Record<MochiLook, string> = labels(LOOK_KEYS);

const LOOK_SET: ReadonlySet<string> = new Set(MOCHI_LOOKS);

/** A stored preference → a look. Anything unknown means the default, like `MochiLook.stored`. */
export function parseLook(raw: unknown): MochiLook {
  return typeof raw === "string" && LOOK_SET.has(raw) ? (raw as MochiLook) : DEFAULT_LOOK;
}

/** The next face in the wardrobe switch. */
export function nextLook(look: MochiLook): MochiLook {
  return MOCHI_LOOKS[(MOCHI_LOOKS.indexOf(look) + 1) % MOCHI_LOOKS.length];
}

/** Portrait and slime: a character's face — eyebrows, and cross with open eyes and teeth. */
export const isCharacterLook = (look: MochiLook) => look === "portrait" || look === "slime";

// ── Mouth ─────────────────────────────────────────────────────────────────────

export type MochiMouth =
  | "none"     // mailbox, cup eyes: the box has its own slot
  | "cat"      // ω — at rest, at work
  | "smile"    // open D with a tongue — happy, proud, in love
  | "oh"       // small o — attentive, asking
  | "gasp"     // big o — surprised
  | "smirk"    // one-sided — winking
  | "flat"     // straight — annoyed, error
  | "wavy"     // wobbly — tired, dizzy
  | "content"  // closed little smile — eyes shut
  | "talking"  // opens and closes — speaking aloud
  | "grit";    // teeth out — cross (portrait and slime)

/** The mouth Mochi makes, read from the eyes he shows so the face always agrees. */
export function mouthForEyes(eye: EyeShape, speaking: boolean, character = false): MochiMouth {
  if (speaking) return "talking";
  if (character && (eye === "line" || eye === "flat")) return "grit";
  switch (eye) {
    case "pill": return "cat";
    case "wide": return "oh";
    case "dot": return "gasp";
    case "happy": case "star": case "heart": return "smile";
    case "wink": return "smirk";
    case "line": case "flat": return "flat";
    case "tired": case "spiral": return "wavy";
    case "closed": return "content";
    case "cup": return "none";
  }
}

// ── Eyebrows ──────────────────────────────────────────────────────────────────

/** Fractions of the body radius R; angles in radians. */
export interface Brows {
  lift: number;   // up from rest
  angle: number;  // + inner ends down (cross), − inner ends up (worried)
  arch: number;   // how much each brow bows upward
  asym: number;   // extra lift of the right brow alone (a raised eyebrow)
}

const brows = (lift: number, angle: number, arch: number, asym = 0): Brows => ({ lift, angle, arch, asym });

/** Where the eyebrows sit, per state and per emote — MochiBrows.pose on macOS. */
export function browPose(state: BotStateName, eye: EyeShape, t: number): Brows {
  // An emote's eyes win: they say what he feels right now.
  switch (eye) {
    case "line": return brows(-0.04, 0.45, 0);          // annoyed
    case "dot": return brows(0.11, -0.1, 0.35);         // surprised
    case "heart": return brows(0.06, -0.2, 0.3);        // in love
    case "star": return brows(0.05, 0.1, 0.25, 0.04);   // proud
    case "wink": return brows(0.02, 0.1, 0.2, 0.07);    // smug
    case "happy": return brows(0.07, -0.12, 0.4);       // delighted
    default: break;
  }
  switch (state) {
    case "idle": return brows(0, 0.05, 0.22);
    case "working": return brows(-0.02, 0.2, 0.12);        // focused
    case "thinking": return brows(0.02, 0.12, 0.2, 0.08);  // hmm
    case "searching": return brows(0.04, -0.04, 0.2);
    case "approval": return brows(0.09, -0.16, 0.3);       // waiting for you
    case "question": return brows(0.05, -0.08, 0.25, 0.07);
    case "error": return brows(-0.04, 0.45, 0);            // cross
    case "finished": return brows(0.07, -0.12, 0.4);
    case "ratelimit": return brows(0.03, -0.4, 0.1);       // worried
    case "sleeping": return brows(-0.03, -0.06, 0.15);
    case "dizzy": return brows(0.02, 0.3 * Math.sin(t * 6), 0.15, 0.06 * Math.sin(t * 4));
  }
}

/** A step of the way from `from` to `to` (k: 0 stays, 1 arrives). */
export function easeBrows(from: Brows, to: Brows, k: number): Brows {
  return {
    lift: from.lift + (to.lift - from.lift) * k,
    angle: from.angle + (to.angle - from.angle) * k,
    arch: from.arch + (to.arch - from.arch) * k,
    asym: from.asym + (to.asym - from.asym) * k,
  };
}
