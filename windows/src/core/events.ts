// Every event Rust (or another window) sends to a webview, with its payload.
// `onEvent` (bridge.ts) takes its payload type from here, so a listener can't
// pick its own type, and a misspelt event name doesn't compile. Rust emits
// these with `app.emit(...)` / `emit_to(...)`; keep the two sides in step.

import type { DesktopSnapshot, Point } from "../mochi/desktop-logic";
import type { BotEmoteName } from "./layout";
import type { HookPayload } from "../island/hooks";
import type { Ask } from "../island/voice";
import type { Say } from "../pill/main";
import type { IntegrationUpdate, ShellCard, ShortcutsReport } from "./bridge";
import type { FlowEntry, FlowPrefs, ModelsStatus, PillState } from "./flow";
import type { GitHubEvent } from "./github";
import type { Looked } from "./look";
import type { SpotifyState } from "./spotify";
import type { YouTubeMusicState } from "./youtube-music";
import type { WaIslandView, WaPillEvent, WaSettingsView } from "./whatsapp";
import type { Settings } from "./state";

export interface EventMap {
  // App and windows
  "settings-changed": Settings;
  "screen-changed": null;
  "sounds-changed": null;
  "recap-check": null;
  "cursor": { x: number; y: number };
  "pointer-inside": boolean;
  "tray": string;
  "shortcut": string;
  "shortcuts-status": ShortcutsReport;
  "hub-page": string;
  "open-wardrobe": null;
  "circle-captured": null;
  // Agents and integrations
  "hook": HookPayload;
  /** A held Claude Code turn ended in the terminal, or its relay went away: no reply any more. */
  "reply-closed": { reply_id: string };
  "chat-delta": string;
  "integration": IntegrationUpdate;
  "github-alerts": GitHubEvent[];
  "spotify": SpotifyState;
  "spotify-artwork": { artUrl: string; dataUrl: string };
  "youtube-music": YouTubeMusicState;
  "youtube-music-artwork": { artUrl: string; dataUrl: string };
  "whatsapp": WaIslandView;
  "whatsapp-pill": WaPillEvent;
  "whatsapp-settings": WaSettingsView | null;
  // Coucou Shell
  "shell-confirm": ShellCard;
  "shell-clipboard": null;
  "shell-routine-ran": string;
  // Flow dictation, the Notetaker and the voice pill
  "flow-history": FlowEntry | null;
  "flow-prefs": FlowPrefs;
  "speech-models": ModelsStatus;
  "notes-changed": string;
  "pill-state": PillState;
  "pill-bands": number[];
  "pill-visible": boolean;
  "pill-pointer": boolean;
  "voice-ask": Ask;
  "voice-say": Say;
  // Phone link
  "phone-link-decided": string;
  "phone-link-getdiff": { conn: number; pillId: string; fileId: number };
  // Desktop Mochi (DESKTOP_EVENTS in mochi/desktop-logic.ts)
  "desktop-mochi-state": DesktopSnapshot;
  "desktop-mochi-emote": { emote: BotEmoteName; duration: number };
  "desktop-mochi-ready": null;
  "desktop-mochi-home": null;
  "desktop-mochi-wardrobe": null;
  "desktop-mochi-dizzy": null;
  "desktop-mochi-dropped": { from: "island" | "desktop"; home: boolean; look?: Looked | null };
  "desktop-cursor": Point;
  "desktop-visible": boolean;
  "desktop-flight": string;
}

export type EventName = keyof EventMap;
