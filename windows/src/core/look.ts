// Mochi looks at a window — the island side of src-tauri/src/look.rs and of
// WindowLook.swift on the Mac. Dragged out of the island and dropped on another
// app's window, he takes it, comes back, and the chat opens with what he sees;
// the chips under the chat ask for more in the same conversation.

import { N_ } from "../i18n/i18n";

/** What Rust tells the island once Mochi has looked (look.rs Looked). */
export interface Looked {
  appName: string;
  title: string;
  /** The window could not be taken: nothing is kept, the note says so. */
  error: string | null;
}

/** The window the conversation is about, while its chips show. */
export interface LookWindow {
  appName: string;
  title: string;
}

/** The chips under the chat: the label, and what is sent as if typed. */
export const LOOK_ASKS = [
  { label: N_("Summary"), query: N_("Summarize what's in this window for me.") },
  { label: N_("Analysis"), query: N_("Analyze what's in this window in depth: what's good, what's wrong, what to do.") },
  { label: N_("Key points"), query: N_("Give me the key points as a list.") },
  { label: N_("Translate"), query: N_("Translate the text in this window into English.") },
] as const;

/** The user's bubble for the first question. */
export const LOOK_BUBBLE = N_("Look at this window");

/** English name of a language code ("fr" → "French"), for the prompt. */
export function languageName(code: string): string {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" }).of(code) ?? "English";
  } catch {
    return "English";
  }
}

/**
 * What Mochi is asked to do with the window: the Mac's prompt word for word,
 * with the reply in the app's language since the user has not typed anything yet.
 */
export function lookPrompt(lang: string): string {
  return (
    "The user just dropped you onto this window of their computer: the screenshot attached is what " +
    "you are looking at. Tell them what you see. Start with one sentence saying what it is, " +
    "then a short summary of its content, then your analysis: what stands out, anything wrong " +
    "or worth checking, and what they could do next. Stay short and concrete, quote what you " +
    "read when it helps. Do not describe the window's chrome (toolbars, tabs) unless it matters. " +
    `Reply in ${languageName(lang)}.`
  );
}

/** The chip above the chat: "Edge · Docs", the title shortened. */
export function lookChipLabel(w: LookWindow, max = 40): string {
  const title = w.title.length > max ? `${w.title.slice(0, max - 1).trimEnd()}…` : w.title;
  if (!w.appName) return title;
  return title ? `${w.appName} · ${title}` : w.appName;
}
