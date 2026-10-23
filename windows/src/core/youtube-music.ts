// The YouTube Music pill on the page: what src-tauri/src/youtube_music.rs
// reports from Pear Desktop's API Server (the same on Windows and Linux) — a
// port of YouTubeMusicController.swift. The card is the Spotify card's
// (views/spotify.ts), so the state has its shape plus where Coucou stands with
// Pear.

import { IDLE_SPOTIFY, type NowPlayingStore, type SpotifyState } from "./spotify";

export const YOUTUBE_MUSIC_ID = "integration_youtube_music";
/** YouTubeMusicController.red. */
export const YOUTUBE_MUSIC_RED = "#FF0033";

/** YouTubeMusicController.Link. */
export type YouTubeMusicLink = "idle" | "unreachable" | "needsAuth" | "waiting" | "denied" | "connected";

export interface YouTubeMusicState extends SpotifyState {
  link: YouTubeMusicLink;
  /** NONE, ALL or ONE. */
  repeatMode: string;
}

export const IDLE_YOUTUBE_MUSIC: YouTubeMusicState = { ...IDLE_SPOTIFY, link: "idle", repeatMode: "NONE" };

/** The page's copy of the player, and the cover of the song that has one. */
export const YouTubeMusic: NowPlayingStore<YouTubeMusicState> = {
  state: { ...IDLE_YOUTUBE_MUSIC },
  artwork: null,
};

/** Music is playing on a declared YouTube Music pill. */
export function youTubeMusicPlaying(s: YouTubeMusicState, activeIntegrations: readonly string[]): boolean {
  return s.playing && s.track != null && activeIntegrations.includes(YOUTUBE_MUSIC_ID);
}
