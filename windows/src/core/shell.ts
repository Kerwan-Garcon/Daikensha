// « Coucou Shell »'s panel (views/shell.ts): the pure parts, so they can be
// tested without a webview — which tabs show, how numbers read, how the app
// search ranks, where a gauge's arc ends and how a sparkline is drawn.

import type { ShellApp, ShellCapabilities } from "./bridge";

export type ShellTab = "controls" | "stats" | "apps" | "windows";

/** The tabs, in the panel's order. Phase 1: the four the backend can fill. */
export const SHELL_TABS: readonly ShellTab[] = ["controls", "stats", "apps", "windows"];

/** The tabs this system can fill (only Stats outside Windows). */
export function availableTabs(caps: ShellCapabilities | null): ShellTab[] {
  if (!caps) return ["stats"];
  return SHELL_TABS.filter((t) => caps[t]);
}

/** 1536 → "1,5 Ko" style, in the interface's own units (Ko/Mo/Go in French come from the caller). */
export function formatBytes(bytes: number, units: readonly string[] = ["B", "KB", "MB", "GB", "TB"]): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return `0 ${units[0]}`;
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v >= 100 || i === 0 ? 0 : 1;
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** A share of 0–100 for display: "34 %". */
export function formatPct(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${Math.round(Math.min(100, Math.max(0, v)))} %`;
}

/** Folds case and accents, so "édge" finds "Edge" and "EXPLO" finds "Explorateur". */
export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/**
 * The apps matching `query`, best first: a name that starts with it, then a
 * word inside the name that starts with it, then a name containing it.
 * An empty query keeps the list as it is.
 */
export function searchApps(apps: readonly ShellApp[], query: string, limit = 8): ShellApp[] {
  const q = fold(query.trim());
  if (!q) return apps.slice(0, limit);
  const scored: { app: ShellApp; score: number }[] = [];
  for (const app of apps) {
    const name = fold(app.name);
    let score = -1;
    if (name.startsWith(q)) score = 0;
    else if (name.split(/[\s\-_.]+/).some((w) => w.startsWith(q))) score = 1;
    else if (name.includes(q)) score = 2;
    if (score >= 0) scored.push({ app, score });
  }
  scored.sort((a, b) => a.score - b.score || a.app.name.length - b.app.name.length || a.app.name.localeCompare(b.app.name));
  return scored.slice(0, limit).map((s) => s.app);
}

/** The tile letter of an app: its first letter or digit. */
export function initial(name: string): string {
  const m = name.match(/[\p{L}\p{N}]/u);
  return m ? m[0].toUpperCase() : "?";
}

/** A stable, readable tile colour from the app's name (no icons are loaded). */
export function tileColor(name: string): string {
  const palette = ["#60a5fa", "#34d399", "#f5a524", "#f472b6", "#a78bfa", "#22d3ee", "#fb923c", "#e5e7eb"];
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return palette[h % palette.length];
}

/** A gauge circle's stroke-dasharray for `pct` on a circle of radius `r`. */
export function gaugeDash(pct: number, r: number): string {
  const c = 2 * Math.PI * r;
  const v = Math.min(100, Math.max(0, Number.isFinite(pct) ? pct : 0));
  return `${((c * v) / 100).toFixed(1)} ${c.toFixed(1)}`;
}

/** Polyline points for `values` in a `w`×`h` box, scaled to their own maximum. */
export function sparkline(values: readonly number[], w: number, h: number): string {
  if (values.length === 0) return "";
  const max = Math.max(1, ...values);
  const step = values.length > 1 ? w / (values.length - 1) : 0;
  return values
    .map((v, i) => `${(i * step).toFixed(1)},${(h - 2 - (Math.max(0, v) / max) * (h - 4)).toFixed(1)}`)
    .join(" ");
}

/** Appends `v` to a rolling series of at most `n` values. */
export function pushSample(series: number[], v: number, n = 60): number[] {
  const next = series.length >= n ? series.slice(series.length - n + 1) : series.slice();
  next.push(v);
  return next;
}

/** "14:32" and the short date "sam. 18" in `lang`, the long date for the tooltip. */
export function clockText(now: Date, lang: string): { time: string; short: string; long: string } {
  const time = now.toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
  const short = now.toLocaleDateString(lang, { weekday: "short", day: "numeric" });
  const long = now.toLocaleDateString(lang, { weekday: "long", day: "numeric", month: "long" });
  return { time, short, long };
}
