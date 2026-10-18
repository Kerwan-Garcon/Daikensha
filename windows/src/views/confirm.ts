// The level 2 card (docs/WINDOWS_SHELL.md): « OK Coucou » asked to type a text
// or to shut the computer down, and nothing happens unless this card's button
// is clicked. Same look as the approval card: Mochi on the left, the exact
// action in a code line, two pill buttons. As there, clicks in the first
// moments after it appears are ignored, so a click meant for another window
// never lands on it.

import { State } from "../core/state";
import { washRGBA } from "../core/layout";
import type { ShellCard, ShellPowerAction } from "../core/bridge";
import { N_, t, tl } from "../i18n/i18n";
import { h } from "./dom";
import type { ViewHost } from "./views";

const CLICK_GUARD_MS = 600;

const QUESTION: Record<ShellPowerAction, string> = {
  lock: N_("Lock the PC?"),
  sleep: N_("Put the PC to sleep?"),
  signOut: N_("Sign out of Windows?"),
  restart: N_("Restart the PC?"),
  shutDown: N_("Shut down the PC?"),
};

const DO: Record<ShellPowerAction, string> = {
  lock: N_("Lock"),
  sleep: N_("Sleep"),
  signOut: N_("Sign out"),
  restart: N_("Restart"),
  shutDown: N_("Shut down"),
};

export function buildConfirm(decide: (ok: boolean) => void): ViewHost {
  const who = h("div", { class: "who-row" });
  const title = h("div", { class: "title one-line" });
  const code = h("div", { class: "code" });
  const hint = h("div", { class: "sub confirm-hint" });
  const yes = h("button", { class: "btn primary" }, h("span", {}));
  const no = h("button", { class: "btn secondary" }, h("span", { text: tl("Cancel") }));
  const row = h("div", { class: "actions" }, no, yes);
  const stack = h("div", { class: "stack" }, who, title, code, hint, row);
  stack.style.padding = "4px 16px 4px 116px";
  const body = h("div", { class: "card wash" }, stack);
  body.style.setProperty("--wash", washRGBA("amber"));
  const el = h("div", { class: "view" }, body);

  let shownFor: string | null = null;
  let shownAt = 0;
  const guarded = (ok: boolean) => () => {
    if (performance.now() - shownAt < CLICK_GUARD_MS) return;
    decide(ok);
  };
  yes.addEventListener("click", guarded(true));
  no.addEventListener("click", guarded(false));

  return {
    el,
    sync() {
      const c: ShellCard | null = State.shellCard;
      if (!c) return;
      if (c.token !== shownFor) {
        shownFor = c.token;
        shownAt = performance.now();
      }
      who.replaceChildren(h("span", { class: "n", text: "Coucou" }), h("span", { text: tl("asks before acting") }));
      const label = yes.firstElementChild as HTMLElement;
      if (c.kind === "type") {
        title.textContent = c.app ? t("Type this text in {app}?", { app: c.app }) : t("Type this text?");
        code.textContent = c.text;
        code.title = c.text;
        hint.textContent = t("Typed in the window in front. Enter is never pressed: you send it.");
        label.textContent = t("Type the text");
      } else {
        title.textContent = t(QUESTION[c.action]);
        code.textContent = c.apps.length > 0 ? c.apps.join(", ") : t("No app has a window open.");
        code.title = "";
        hint.textContent =
          c.action === "lock" || c.action === "sleep"
            ? t("Your apps stay open.")
            : c.apps.length > 0
              ? t("These apps may have unsaved work; they can still ask to save it.")
              : "";
        label.textContent = t(DO[c.action]);
      }
    },
  };
}
