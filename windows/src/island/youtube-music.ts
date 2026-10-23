// YouTube Music's reports → the page (src-tauri/src/youtube_music.rs). The
// parts of YouTubeMusicController.swift that touch the app, as
// island/spotify.ts does for Spotify: the pill wears the song's title, music
// starting shows the compact island without a sound, and the card asks Rust to
// reconnect whenever it comes on screen (Pear may have started since).

import { Bridge, onEvent } from "../core/bridge";
import { pillDefinition } from "../core/pills";
import { YOUTUBE_MUSIC_ID, YouTubeMusic, type YouTubeMusicState } from "../core/youtube-music";
import { State } from "../core/state";
import type { SpotifyHost } from "./spotify";

export function registerYouTubeMusicHandlers(island: SpotifyHost) {
  void onEvent("youtube-music", (s) => applyYouTubeMusic(island, s));
  void onEvent("youtube-music-artwork", (art) => {
    YouTubeMusic.artwork = art;
    State.notify();
  });
  State.subscribe(() => {
    syncPillName();
    refreshWhenShown();
  });
}

/** A report from Rust (or the answer to a refresh). */
export function applyYouTubeMusic(island: SpotifyHost, next: YouTubeMusicState) {
  const wasPlaying = State.youTubeMusicPlaying;
  YouTubeMusic.state = next;
  syncPillName();
  // Only on not playing → playing (YouTubeMusicController.setPlaying).
  if (!wasPlaying && State.youTubeMusicPlaying && !State.paused && State.mode === "hidden") {
    island.revealSilently();
  }
  State.notify();
}

/** YouTubeMusicController.syncTaskName: the song's title, else the pill's name. */
export function syncPillName() {
  const task = State.tasks.find((x) => x.id === YOUTUBE_MUSIC_ID);
  if (!task) return;
  const name = YouTubeMusic.state.track?.title || (pillDefinition(YOUTUBE_MUSIC_ID)?.name ?? "YouTube Music");
  if (task.name !== name) task.name = name;
}

let shown = false;

function refreshWhenShown() {
  const now = State.mode === "expanded" && State.view === "overview" && State.focusTask?.id === YOUTUBE_MUSIC_ID;
  if (now && !shown) void Bridge.youTubeMusicRefresh();
  shown = now;
}
