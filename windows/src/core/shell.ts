// « Coucou Shell »'s panel (views/shell.ts): the pure parts, so they can be
// tested without a webview — which tabs show, how numbers read, how the app
// search ranks, where a gauge's arc ends and how a sparkline is drawn.

import type { ShellApp, ShellCapabilities, ShellWhen } from "./bridge";

export type ShellTab = "controls" | "today" | "stats" | "clipboard" | "apps" | "wallpapers" | "power" | "windows";

/** The tabs, in the panel's order. */
export const SHELL_TABS: readonly ShellTab[] = ["controls", "today", "stats", "clipboard", "apps", "wallpapers", "power", "windows"];

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

/** The ISO 8601 week number of `d` (weeks start on Monday; week 1 holds the first Thursday). */
export function isoWeek(d: Date): number {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const yearStart = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);
}

export type WeatherKind = "clear" | "partly" | "overcast" | "fog" | "drizzle" | "rain" | "snow" | "storm" | "cloudy";

/** The family of a WMO weather code, as Open-Meteo reports it. */
export function weatherKey(code: number): WeatherKind {
  if (code === 0) return "clear";
  if (code <= 2) return "partly";
  if (code === 3) return "overcast";
  if (code === 45 || code === 48) return "fog";
  if (code >= 51 && code <= 57) return "drizzle";
  if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return "rain";
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return "snow";
  if (code >= 95) return "storm";
  return "cloudy";
}

/** "25:00" for a focus timer's remaining milliseconds. */
export function timerText(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** "2 min ago"-style, in `lang`, for something that happened at `at` (ms). */
export function agoText(at: number, now: number, lang: string): string {
  const fmt = new Intl.RelativeTimeFormat(lang, { numeric: "auto", style: "short" });
  const s = Math.round((at - now) / 1000);
  if (Math.abs(s) < 60) return fmt.format(Math.min(0, s), "second");
  const m = Math.round(s / 60);
  if (Math.abs(m) < 60) return fmt.format(m, "minute");
  const h = Math.round(m / 60);
  if (Math.abs(h) < 24) return fmt.format(h, "hour");
  return fmt.format(Math.round(h / 24), "day");
}

/** A copied text as one short line for the list. */
export function clipPreview(text: string, max = 140): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

/** A local date from minutes since 1970-01-01 in local time (shell/routines.rs). */
export function fromLocalMinutes(minutes: number): Date {
  const days = Math.floor(minutes / 1440);
  const rest = minutes - days * 1440;
  return new Date(1970, 0, 1 + days, Math.floor(rest / 60), rest % 60);
}

/**
 * "Mon, Wed · 09:00", "every day · 09:00" or the date of a one-off, with the
 * week's day names from `dayName` (0 Sunday … 6 Saturday) and `everyDay`.
 */
export function whenText(when: ShellWhen, lang: string, dayName: (d: number) => string, everyDay: string): string {
  if (when.kind === "once") {
    const [y, m, d] = when.date.split("-").map(Number);
    const date = new Date(y, (m || 1) - 1, d || 1);
    return `${date.toLocaleDateString(lang, { weekday: "short", day: "numeric", month: "short" })} · ${when.time}`;
  }
  const days = [...when.days].sort((a, b) => a - b);
  const label = days.length === 7 ? everyDay : days.map((d) => dayName(d % 7)).join(", ");
  return `${label} · ${when.time}`;
}
