// Pieces the panel's tabs share (views/shell.ts, views/shell-tabs.ts).

import { gaugeDash } from "../core/shell";
import type { Msg } from "../i18n/i18n";
import { h, svg } from "./dom";

const NS = "http://www.w3.org/2000/svg";

export interface Tab {
  el: HTMLElement;
  show(): void;
  hide(): void;
  /** Files dropped while this tab shows; true when the tab took them. */
  drop?(paths: string[]): boolean;
}

/** A small note under a card when Windows says no. */
export function note(): { el: HTMLElement; say(text: string): void } {
  const el = h("div", { class: "shell-note" });
  let timer: number | null = null;
  return {
    el,
    say(text: string) {
      el.textContent = text;
      if (timer != null) window.clearTimeout(timer);
      timer = window.setTimeout(() => (el.textContent = ""), 5000);
    },
  };
}

export const message = (err: unknown) => String(err).replace(/^Error:\s*/, "");

export function card(cls: string, ...children: (Node | string | Msg | null)[]): HTMLElement {
  return h("div", { class: `shell-card ${cls}` }, ...children);
}

export function cardTitle(text: Msg, aside?: Node): HTMLElement {
  return h("div", { class: "shell-card-head" }, h("span", { class: "shell-card-title", text }), aside ?? null);
}

/** An underlined link inside a card (« Choisir la sortie… »). */
export function linkButton(text: Msg, onclick: () => void): HTMLElement {
  return h("button", { class: "shell-link", onclick }, h("span", { text }), svg("M7 17 17 7M8 7h9v9", 13, { stroke: 2 }));
}

/** A full-width row of a card that opens something (« Écran et couleurs »). */
export function chipRow(text: Msg, onclick: () => void): HTMLElement {
  return h("button", { class: "shell-chip-row", onclick }, h("span", { text }), svg("M7 17 17 7M8 7h9v9", 14, { stroke: 2 }));
}

/** The footer line under a tab: text on each side, the panel's grip between them. */
export function foot(left: Node | Msg, right: Node | Msg): HTMLElement {
  return h("div", { class: "shell-foot" }, h("span", {}, left), h("i", {}), h("span", {}, right));
}

/**
 * A ring of 80 px (the Stats gauges, the focus timer). `set` turns the arc to
 * `pct` of the circle and writes `value` in the middle, when the ring has one.
 */
export function ring(color: string, withValue = false): { el: SVGSVGElement; set(pct: number, value?: string): void } {
  const s = document.createElementNS(NS, "svg");
  s.setAttribute("viewBox", "0 0 88 88");
  s.setAttribute("class", "shell-ring");
  const circle = (stroke: string) => {
    const c = document.createElementNS(NS, "circle");
    c.setAttribute("cx", "44");
    c.setAttribute("cy", "44");
    c.setAttribute("r", "34");
    c.setAttribute("fill", "none");
    c.setAttribute("stroke", stroke);
    c.setAttribute("stroke-width", "8");
    return c;
  };
  const track = circle("rgba(255,255,255,0.12)");
  const arc = circle(color);
  arc.setAttribute("stroke-linecap", "round");
  arc.setAttribute("transform", "rotate(-90 44 44)");
  arc.setAttribute("stroke-dasharray", gaugeDash(0, 34));
  s.append(track, arc);
  let text: SVGTextElement | null = null;
  if (withValue) {
    text = document.createElementNS(NS, "text");
    text.setAttribute("x", "44");
    text.setAttribute("y", "50");
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("class", "shell-ring-value");
    s.append(text);
  }
  return {
    el: s,
    set(pct, value) {
      arc.setAttribute("stroke-dasharray", gaugeDash(pct, 34));
      if (text && value != null) text.textContent = value;
    },
  };
}

export function switchButton(label: string, on: boolean, onclick: () => void): HTMLElement {
  return h("button", { class: on ? "shell-switch on" : "shell-switch", "aria-label": label, "aria-pressed": String(on), onclick }, h("i", {}));
}

