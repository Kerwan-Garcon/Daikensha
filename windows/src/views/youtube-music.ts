// The YouTube Music pill and card — DOM ports of YouTubeMusicPill /
// YouTubeMusicCardView (YouTubeMusicViews.swift): the Spotify views
// (views/spotify.ts) over Pear Desktop's player, with an idle card that says
// where Coucou stands with Pear's API Server.

import { Bridge } from "../core/bridge";
import { State, type AgentTask } from "../core/state";
import { YOUTUBE_MUSIC_ID, YOUTUBE_MUSIC_RED, YouTubeMusic, type YouTubeMusicState } from "../core/youtube-music";
import { N_, t } from "../i18n/i18n";
import { ICONS } from "./icons";
import {
  buildSpotifyCard, buildSpotifyPill,
  type NowPlayingSource, type NowPlayingStatus, type SpotifyCardHost, type SpotifyPillHost,
} from "./spotify";

/** "Connect…": Pear shows its dialog; the page waits with it. */
function connect() {
  YouTubeMusic.state = { ...YouTubeMusic.state, link: "waiting" };
  State.notify();
  void Bridge.youTubeMusicConnect();
}

/** YouTubeMusicCardView.status: the idle card's line and button. */
export function youTubeMusicStatus(s: YouTubeMusicState): NowPlayingStatus | null {
  const open = () => void Bridge.youTubeMusicOpen();
  switch (s.link) {
    case "needsAuth":
      return { ok: false, text: t("Allow Coucou to control YouTube Music"), action: t("Connect…"), run: connect };
    case "waiting":
      return { ok: false, text: t("Click Allow in YouTube Music"), action: null, run: () => {} };
    case "denied":
      return { ok: false, text: t("YouTube Music refused the connection"), action: t("Try again"), run: connect };
    case "unreachable":
      // Pear isn't running, or its plugin is off: both are fixed from Pear.
      if (!s.installed) return null;
      return {
        ok: false, text: t("Turn on the API Server plugin in YouTube Music"), action: t("Open YouTube Music"), run: open,
      };
    default:
      return null;
  }
}

export const YOUTUBE_MUSIC_SOURCE: NowPlayingSource = {
  id: YOUTUBE_MUSIC_ID,
  name: "YouTube Music",
  accent: YOUTUBE_MUSIC_RED,
  store: YouTubeMusic,
  playing: () => State.youTubeMusicPlaying,
  control: (action, value) => Bridge.youTubeMusicControl(action, value),
  open: () => Bridge.youTubeMusicOpen(),
  text: {
    open: N_("Open YouTube Music"),
    openWith: N_("{0} — open YouTube Music"),
    notInstalled: N_("YouTube Music not installed"),
    get: N_("Get Pear Desktop"),
  },
  status: () => youTubeMusicStatus(YouTubeMusic.state),
  repeatIcon: () => (YouTubeMusic.state.repeatMode === "ONE" ? ICONS.repeatOne : ICONS.repeat),
};

export function buildYouTubeMusicPill(task: AgentTask, onTap: () => void): SpotifyPillHost {
  return buildSpotifyPill(task, onTap, YOUTUBE_MUSIC_SOURCE);
}

export function buildYouTubeMusicCard(): SpotifyCardHost {
  return buildSpotifyCard(YOUTUBE_MUSIC_SOURCE);
}
