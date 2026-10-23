// « Entoure et demande »'s preview (shell/capture.rs): the part of the screen
// the user just circled, before anything is sent. "Ask" opens the chat with it
// as the question's context (Claude only); "Cancel" forgets it. Same card as
// the other island cards: Mochi on the left, two pill buttons.

import { State } from "../core/state";
import { washRGBA } from "../core/layout";
import { tl } from "../i18n/i18n";
import { h } from "./dom";
import type { ViewHost } from "./views";

export function buildCircle(decide: (ask: boolean) => void): ViewHost {
  const img = h("img", { class: "circle-shot", alt: "" });
  const title = h("div", { class: "title one-line", text: tl("Ask about this?") });
  const hint = h("div", { class: "sub confirm-hint", text: tl("Only Claude sees it, with your question. Nothing is saved.") });
  const ask = h("button", { class: "btn primary", onclick: () => decide(true) }, h("span", { text: tl("Ask") }));
  const cancel = h("button", { class: "btn secondary", onclick: () => decide(false) }, h("span", { text: tl("Cancel") }));
  const text = h("div", { class: "circle-text" }, title, hint, h("div", { class: "actions" }, cancel, ask));
  const stack = h("div", { class: "stack circle-stack" }, img, text);
  stack.style.padding = "4px 16px 4px 116px";
  const body = h("div", { class: "card wash" }, stack);
  body.style.setProperty("--wash", washRGBA("cyan"));
  const el = h("div", { class: "view" }, body);
  return {
    el,
    sync() {
      const src = State.capturePreview ?? "";
      if (img.getAttribute("src") !== src) img.setAttribute("src", src);
    },
  };
}
