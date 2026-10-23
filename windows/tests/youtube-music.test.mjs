// The YouTube Music pill (Windows and Linux): the report handling in
// src/island/youtube-music.ts, the idle card's connection states and the
// controls in src/views/youtube-music.ts (the Spotify views over another
// source), and Mochi's dance. Pear Desktop's messages and routes are tested in
// src-tauri/src/youtube_music.rs.

import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { emit, sent } from "./tauri.mjs";
import { installFakeDom } from "./fakedom.mjs";
import { IDLE_YOUTUBE_MUSIC, YOUTUBE_MUSIC_ID, YouTubeMusic, youTubeMusicPlaying } from "../src/core/youtube-music.ts";
import { IDLE_SPOTIFY, SPOTIFY_ID, Spotify, currentArtwork, islandDances } from "../src/core/spotify.ts";
import { registerYouTubeMusicHandlers } from "../src/island/youtube-music.ts";
import { buildYouTubeMusicCard, buildYouTubeMusicPill } from "../src/views/youtube-music.ts";
import { buildSpotifyCard } from "../src/views/spotify.ts";
import { DEFAULT_SETTINGS, State } from "../src/core/state.ts";
import { lookup, setLanguage } from "../src/i18n/i18n.ts";

installFakeDom();

const song = (over = {}) => ({
  id: "abc", title: "Get Lucky", artist: "Daft Punk", album: "Random Access Memories",
  duration: 248, artUrl: "https://lh3.googleusercontent.com/abc", ...over,
});

const playing = (over = {}) => ({
  ...IDLE_YOUTUBE_MUSIC, running: true, installed: true, link: "connected", track: song(), playing: true,
  position: 10, positionAt: 1_000, volume: 70, ...over,
});

const island = { reveals: 0, revealSilently() { this.reveals += 1; } };
registerYouTubeMusicHandlers(island);

beforeEach(() => {
  setLanguage("en");
  State.settings = { ...DEFAULT_SETTINGS, activeIntegrations: [YOUTUBE_MUSIC_ID] };
  State.os = "windows";
  State.tasks = [];
  State.focusId = null;
  State.mode = "hidden";
  State.view = "overview";
  State.paused = false;
  State.loadIntegrationTasks();
  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC };
  YouTubeMusic.artwork = null;
  Spotify.state = { ...IDLE_SPOTIFY };
  island.reveals = 0;
});

const ytmTask = () => State.tasks.find((t) => t.id === YOUTUBE_MUSIC_ID);

// ── Reports from Rust ─────────────────────────────────────────────────────────

test("music counts only when it plays on a declared YouTube Music pill", () => {
  assert.ok(youTubeMusicPlaying(playing(), [YOUTUBE_MUSIC_ID]));
  assert.ok(!youTubeMusicPlaying(playing(), [SPOTIFY_ID]));
  assert.ok(!youTubeMusicPlaying(playing({ playing: false }), [YOUTUBE_MUSIC_ID]));
  assert.ok(!youTubeMusicPlaying(playing({ track: null }), [YOUTUBE_MUSIC_ID]));
});

test("the pill wears the song's title, and its own name when nothing plays", () => {
  assert.equal(ytmTask().name, "YouTube Music");
  emit("youtube-music", playing());
  assert.equal(ytmTask().name, "Get Lucky");
  emit("youtube-music", { ...IDLE_YOUTUBE_MUSIC, link: "connected", running: true });
  assert.equal(ytmTask().name, "YouTube Music");
});

test("music starting shows the hidden island once, silently, and Mochi dances", () => {
  emit("youtube-music", playing({ playing: false }));
  assert.equal(island.reveals, 0);
  emit("youtube-music", playing());
  assert.equal(island.reveals, 1);
  emit("youtube-music", playing({ position: 30 }));
  assert.equal(island.reveals, 1, "only on not playing → playing");
  assert.ok(State.youTubeMusicPlaying);
  assert.ok(!State.spotifyPlaying, "the other player is untouched");

  State.settings.activeIntegrations = [];
  emit("youtube-music", { ...IDLE_YOUTUBE_MUSIC });
  emit("youtube-music", playing());
  assert.equal(island.reveals, 1);
  assert.ok(!State.youTubeMusicPlaying);
});

test("expanded, Mochi dances on the music pill that is in front, whichever player it is", () => {
  const base = { music: true, state: "idle", mode: "expanded", view: "overview" };
  assert.ok(islandDances({ ...base, focusId: YOUTUBE_MUSIC_ID, musicPills: [YOUTUBE_MUSIC_ID] }));
  assert.ok(!islandDances({ ...base, focusId: SPOTIFY_ID, musicPills: [YOUTUBE_MUSIC_ID] }));
  assert.ok(islandDances({ ...base, focusId: SPOTIFY_ID }), "Spotify's when not told");
  assert.ok(islandDances({ ...base, mode: "compact", focusId: null, musicPills: [YOUTUBE_MUSIC_ID] }));
});

test("the cover arrives on its own event, and never crosses to Spotify", () => {
  emit("youtube-music", playing());
  emit("youtube-music-artwork", { artUrl: "https://lh3.googleusercontent.com/abc", dataUrl: "data:image/png;base64,AA" });
  assert.equal(currentArtwork(YouTubeMusic.state, YouTubeMusic), "data:image/png;base64,AA");
  assert.equal(currentArtwork(), null);
});

test("the card asks Rust to reconnect each time it comes on screen", () => {
  const before = sent("youtube_music_refresh").length;
  State.mode = "expanded";
  State.view = "overview";
  State.setFocus(YOUTUBE_MUSIC_ID);
  assert.equal(sent("youtube_music_refresh").length, before + 1);
  State.notify();
  assert.equal(sent("youtube_music_refresh").length, before + 1, "not again while it stays");
});

// ── Views ─────────────────────────────────────────────────────────────────────

test("the idle card says where Coucou stands with Pear, with the one useful button", () => {
  const card = buildYouTubeMusicCard();
  const button = () => card.el.find("BUTTON")[0];

  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, installed: true, link: "needsAuth" };
  card.sync();
  assert.match(card.el.textContent, /YouTube Music.*Integration.*Allow Coucou to control YouTube Music.*Connect…/);
  button().fire("click");
  assert.equal(sent("youtube_music_connect").length, 1, "only on the user's click");
  assert.equal(YouTubeMusic.state.link, "waiting");

  card.sync();
  assert.ok(card.el.textContent.includes("Click Allow in YouTube Music"));
  assert.equal(button().style.display, "none", "nothing to click while Pear asks");

  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, installed: true, link: "denied" };
  card.sync();
  assert.match(card.el.textContent, /refused the connection.*Try again/);
  assert.equal(button().style.display, "");

  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, installed: true, link: "unreachable" };
  card.sync();
  assert.match(card.el.textContent, /Turn on the API Server plugin in YouTube Music.*Open YouTube Music/);
  button().fire("click");
  assert.ok(sent("youtube_music_open").length > 0);

  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, link: "unreachable" };
  card.sync();
  assert.match(card.el.textContent, /YouTube Music not installed.*Get Pear Desktop/);

  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, installed: true, running: true, link: "connected" };
  card.sync();
  assert.match(card.el.textContent, /Not playing.*Open YouTube Music/);

  setLanguage("fr");
  YouTubeMusic.state = { ...IDLE_YOUTUBE_MUSIC, installed: true, link: "needsAuth" };
  card.sync();
  assert.ok(card.el.textContent.includes(lookup("Allow Coucou to control YouTube Music", "fr")));
});

test("the playing card drives Pear, and leaves Spotify alone", () => {
  const card = buildYouTubeMusicCard();
  YouTubeMusic.state = playing({ playing: false, repeat: true, repeatMode: "ONE" });
  card.sync();
  const text = card.el.textContent;
  assert.ok(text.includes("Get Lucky"));
  assert.ok(text.includes("Daft Punk · Random Access Memories"));
  assert.ok(text.includes("0:10") && text.includes("-3:58"));

  const [shuffle, prev, play, next, repeat] = card.el.querySelector("np-buttons").children;
  assert.equal(repeat.title, "Repeat on");
  const spotifyBefore = sent("spotify_control").length;
  const before = sent("youtube_music_control").length;
  shuffle.fire("click");
  repeat.fire("click");
  prev.fire("click");
  next.fire("click");
  play.fire("click");
  assert.deepEqual(sent("youtube_music_control").slice(before), [
    { action: "shuffle", value: 1 },
    { action: "repeat", value: 0 },
    { action: "previous", value: null },
    { action: "next", value: null },
    { action: "playPause", value: null },
  ]);
  assert.equal(sent("spotify_control").length, spotifyBefore);
  // The page shows the clicks at once, and keeps the link.
  assert.equal(YouTubeMusic.state.shuffle, true);
  assert.equal(YouTubeMusic.state.repeat, false);
  assert.equal(YouTubeMusic.state.playing, true);
  assert.equal(YouTubeMusic.state.link, "connected");
});

test("the Spotify card is the same as before: its own words and its own player", () => {
  const card = buildSpotifyCard();
  Spotify.state = { ...IDLE_SPOTIFY };
  card.sync();
  assert.match(card.el.textContent, /Spotify not installed.*Get Spotify/);
  card.el.find("BUTTON")[0].fire("click");
  assert.ok(sent("spotify_open").length > 0);
});

test("the pill shows play/pause and next on hover, and sends them to Pear", () => {
  const pill = buildYouTubeMusicPill(ytmTask(), () => {});
  pill.el.fire("mouseenter");
  assert.ok(!pill.el.classList.contains("controls"), "nothing loaded: no controls");
  YouTubeMusic.state = playing();
  pill.sync();
  assert.ok(pill.el.classList.contains("controls"));
  const [playBtn, nextBtn] = pill.el.querySelector("np-pill-controls").children;
  const before = sent("youtube_music_control").length;
  playBtn.fire("click");
  nextBtn.fire("click");
  assert.deepEqual(sent("youtube_music_control").slice(before), [
    { action: "playPause", value: null },
    { action: "next", value: null },
  ]);
  assert.equal(YouTubeMusic.state.playing, false);
});
