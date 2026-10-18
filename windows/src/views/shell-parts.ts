// Pieces the panel's tabs share (views/shell.ts, views/shell-tabs.ts).

import type { Msg } from "../i18n/i18n";
import { h, svg } from "./dom";

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

export function linkButton(text: Msg, onclick: () => void): HTMLElement {
  return h("button", { class: "shell-link", onclick }, h("span", { text }), svg("M7 17 17 7M8 7h9v9", 13, { stroke: 2 }));
}

export function switchButton(label: string, on: boolean, onclick: () => void): HTMLElement {
  return h("button", { class: on ? "shell-switch on" : "shell-switch", "aria-label": label, "aria-pressed": String(on), onclick }, h("i", {}));
}

