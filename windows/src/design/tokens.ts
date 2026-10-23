// Design tokens in TypeScript: the values of src/design/tokens.css, read from
// that very file when the app is built, so a colour is decided in one place.
//
// - cssVar("--color-accent")  → "var(--color-accent)": for a style set from
//   code. It follows the stylesheet, a future theme included.
// - colorHex("--color-accent") → "#60a5fa": where a real colour is needed —
//   a canvas, a hex alpha suffix (`${hex}14`), hexToRGB().
// - colorAlpha("--color-accent", 0.1) → "rgba(96, 165, 250, 0.1)": a
//   see-through colour for a canvas.
//
// An unknown name throws as soon as it is used, and tests/design-tokens.test.mjs
// checks every name the code mentions.

import source from "./tokens.css?raw";

export type Token = `--${string}`;

const DECLS = new Map<string, string>();
for (const [, name = "", value = ""] of source.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) {
  DECLS.set(name, value.replace(/\s+/g, " ").trim());
}

/** Every token tokens.css defines. */
export function tokenNames(): string[] {
  return [...DECLS.keys()];
}

/** The token's value as written, with the var() it refers to filled in. */
export function tokenValue(name: Token): string {
  const raw = DECLS.get(name);
  if (raw === undefined) throw new Error(`Unknown design token ${name} (src/design/tokens.css)`);
  return raw.replace(/var\((--[a-z0-9-]+)\)/g, (_, inner: Token) => tokenValue(inner));
}

/** `var(--name)`, for a style written from code. */
export function cssVar(name: Token): string {
  tokenValue(name);
  return `var(${name})`;
}

/** The R, G, B of a colour token (#rgb, #rrggbb, "r g b" or rgb(r g b)). */
export function colorChannels(name: Token): [number, number, number] {
  const v = tokenValue(name);
  const hex = (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v))?.[1];
  if (hex) {
    const full = hex.length === 3 ? Array.from(hex, (c) => c + c).join("") : hex;
    return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as [number, number, number];
  }
  const nums = v.replace(/^rgb\(|\)$/g, "").trim().split(/[\s,]+/).map(Number);
  const [r, g, b] = nums;
  if (r !== undefined && g !== undefined && b !== undefined && [r, g, b].every((n) => Number.isFinite(n))) return [r, g, b];
  throw new Error(`Design token ${name} is not a colour: ${v}`);
}

/** The colour as #rrggbb. */
export function colorHex(name: Token): string {
  return `#${colorChannels(name).map((n) => n.toString(16).padStart(2, "0")).join("")}`;
}

/** The colour at `alpha` opacity, as rgba(), for a canvas. */
export function colorAlpha(name: Token, alpha: number): string {
  const [r, g, b] = colorChannels(name);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
