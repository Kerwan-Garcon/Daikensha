// The YouTube Music pill — port of YouTubeMusicController.swift.
//
// YouTube Music has no desktop app of its own. Coucou drives Pear Desktop
// (github.com/pear-devs/pear-desktop, formerly th-ch/youtube-music) through its
// "API Server" plugin, on Windows and Linux alike: REST commands and a
// WebSocket on 127.0.0.1:26538, never anywhere else. The first connection is a
// click in Coucou, then "Allow" in Pear's own dialog; the token Pear returns
// lives in the Credential Manager / Secret Service, out of the webview's reach.
//
// While the pill is declared, one task holds the WebSocket and wakes only for
// Pear's messages (PLAYER_INFO first, then one per change). Pear also says the
// position every second while it plays: that is only re-emitted when it jumps
// (a seek). When Pear isn't there the task retries 2, 4, 8… up to 60 s apart,
// and at once when the card comes on screen or after "Open YouTube Music".
// Turning the pill off ends the task.

use serde::Serialize;
use serde_json::Value as Json;
use tauri::AppHandle;

use crate::spotify::Track;

pub const PILL_ID: &str = "integration_youtube_music";
/// The API Server plugin's default port.
pub const PORT: u16 = 26538;
/// What Pear's "Allow" dialog shows.
const CLIENT_ID: &str = "coucou";
/// Where the token lives (secrets::INTERNAL_KEYS).
pub(crate) const TOKEN_KEY: &str = "youtube-music-token";
pub const DOWNLOAD_URL: &str = "https://github.com/pear-devs/pear-desktop/releases/latest";
/// Island events: the player's state, and a song's cover as a data URL.
const STATE_EVENT: &str = "youtube-music";
const ARTWORK_EVENT: &str = "youtube-music-artwork";
/// The biggest cover we read (Google's are a few dozen KB).
const ARTWORK_LIMIT: usize = 3 * 1024 * 1024;

// ── State ─────────────────────────────────────────────────────────────────────

/// Where Coucou stands with Pear's API Server.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Link {
    /// The pill is off.
    #[default]
    Idle,
    /// Nothing answers on the port: Pear isn't running, or its plugin is off.
    Unreachable,
    /// Pear answers but Coucou has no token: waits for the user's click.
    NeedsAuth,
    /// Pear shows its "Allow / Deny" dialog.
    Waiting,
    /// The user clicked "Deny" in Pear.
    Denied,
    Connected,
}

/// What the island is told, whenever it changes. The Spotify card's shape
/// (core/spotify.ts), plus the link and YouTube Music's three repeat modes.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YtmState {
    /// Connected to Pear: the controls work.
    pub running: bool,
    /// A Pear Desktop is there to launch.
    pub installed: bool,
    pub link: Link,
    pub track: Option<Track>,
    pub playing: bool,
    /// Seconds at `position_at`; while playing, the real position runs on from there.
    pub position: f64,
    /// Unix milliseconds.
    pub position_at: f64,
    pub shuffle: bool,
    /// The repeat mode is not NONE.
    pub repeat: bool,
    /// NONE, ALL or ONE.
    pub repeat_mode: String,
    /// 0…100.
    pub volume: i32,
}

impl Default for YtmState {
    fn default() -> Self {
        YtmState {
            running: false,
            installed: false,
            link: Link::Idle,
            track: None,
            playing: false,
            position: 0.0,
            position_at: 0.0,
            shuffle: false,
            repeat: false,
            repeat_mode: "NONE".into(),
            volume: 50,
        }
    }
}

/// What a message touched, so the caller knows whether to tell the island.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Applied {
    pub changed: bool,
    pub track_changed: bool,
    pub started_playing: bool,
}

impl YtmState {
    /// Where playback is at `now_ms`, extrapolated and capped at the end.
    pub fn position_at_time(&self, now_ms: f64) -> f64 {
        let elapsed = if self.playing { ((now_ms - self.position_at) / 1000.0).max(0.0) } else { 0.0 };
        let p = self.position + elapsed;
        match &self.track {
            Some(t) if t.duration > 0.0 => p.min(t.duration),
            _ => p,
        }
    }

    fn anchor(&mut self, position: f64, now_ms: f64) {
        self.position = position.max(0.0);
        self.position_at = now_ms;
    }

    fn set_playing(&mut self, playing: bool, now_ms: f64, applied: &mut Applied) {
        if playing != self.playing {
            // Freeze or restart the clock where it is now.
            let at = self.position_at_time(now_ms);
            self.anchor(at, now_ms);
            self.playing = playing;
            applied.started_playing = playing;
        }
    }

    fn set_repeat(&mut self, mode: &str) {
        self.repeat_mode = mode.to_string();
        self.repeat = mode != "NONE";
    }

    /// Not connected any more: nothing is playing.
    pub fn disconnect(&mut self, link: Link, now_ms: f64) {
        self.link = link;
        self.running = false;
        self.track = None;
        self.playing = false;
        self.anchor(0.0, now_ms);
    }

    /// One WebSocket message from Pear's API Server.
    pub fn apply(&mut self, msg: &Json, now_ms: f64) -> Applied {
        let before = self.clone();
        let mut applied = Applied::default();
        let position = msg.get("position").and_then(Json::as_f64);
        match msg.get("type").and_then(Json::as_str).unwrap_or_default() {
            "PLAYER_INFO" => {
                self.link = Link::Connected;
                self.running = true;
                let track = msg.get("song").and_then(|s| parse_song(s, self.track.as_ref()));
                applied.track_changed = track.as_ref().map(|t| &t.id) != self.track.as_ref().map(|t| &t.id);
                self.track = track;
                if let Some(p) = position {
                    self.anchor(p, now_ms);
                }
                let playing = msg.get("isPlaying").and_then(Json::as_bool).unwrap_or(false);
                self.set_playing(playing && self.track.is_some(), now_ms, &mut applied);
                if let Some(on) = msg.get("shuffle").and_then(Json::as_bool) {
                    self.shuffle = on;
                }
                if let Some(mode) = msg.get("repeat").and_then(Json::as_str) {
                    self.set_repeat(mode);
                }
                self.apply_volume(msg);
            }
            "VIDEO_CHANGED" => {
                if let Some(track) = msg.get("song").and_then(|s| parse_song(s, self.track.as_ref())) {
                    applied.track_changed = self.track.as_ref().map(|t| &t.id) != Some(&track.id);
                    let paused = msg.pointer("/song/isPaused").and_then(Json::as_bool);
                    self.track = Some(track);
                    self.anchor(position.unwrap_or(0.0), now_ms);
                    if let Some(paused) = paused {
                        self.set_playing(!paused, now_ms, &mut applied);
                    }
                }
            }
            "PLAYER_STATE_CHANGED" => {
                if let Some(p) = position {
                    self.anchor(p, now_ms);
                }
                let playing = msg.get("isPlaying").and_then(Json::as_bool).unwrap_or(false);
                self.set_playing(playing && self.track.is_some(), now_ms, &mut applied);
            }
            "POSITION_CHANGED" => {
                // Every second while playing: only a jump is worth telling.
                if let Some(p) = position {
                    if (p - self.position_at_time(now_ms)).abs() > 1.5 {
                        self.anchor(p, now_ms);
                    }
                }
            }
            "VOLUME_CHANGED" => self.apply_volume(msg),
            "REPEAT_CHANGED" => {
                if let Some(mode) = msg.get("repeat").and_then(Json::as_str) {
                    self.set_repeat(mode);
                }
            }
            "SHUFFLE_CHANGED" => {
                if let Some(on) = msg.get("shuffle").and_then(Json::as_bool) {
                    self.shuffle = on;
                }
            }
            _ => {}
        }
        applied.changed = *self != before;
        applied
    }

    fn apply_volume(&mut self, msg: &Json) {
        let Some(v) = msg.get("volume").and_then(Json::as_f64) else { return };
        let muted = msg.get("muted").and_then(Json::as_bool).unwrap_or(false);
        self.volume = if muted { 0 } else { v.round().clamp(0.0, 100.0) as i32 };
    }
}

/// Pear's SongInfo → the track, or None when nothing is loaded. The cover is
/// kept from the previous report of the same song when this one has none.
pub fn parse_song(song: &Json, previous: Option<&Track>) -> Option<Track> {
    let id = song.get("videoId").and_then(Json::as_str).unwrap_or_default().to_string();
    let title = song.get("title").and_then(Json::as_str).unwrap_or_default();
    if id.is_empty() && title.is_empty() {
        return None;
    }
    let art_url = song
        .get("imageSrc")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| previous.filter(|p| p.id == id).and_then(|p| p.art_url.clone()));
    Some(Track {
        id,
        title: crate::spotify::short_title(title),
        artist: crate::spotify::short_artist(song.get("artist").and_then(Json::as_str).unwrap_or_default()),
        album: song.get("album").and_then(Json::as_str).unwrap_or_default().to_string(),
        duration: song.get("songDuration").and_then(Json::as_f64).unwrap_or(0.0).max(0.0),
        art_url,
        object_path: String::new(),
    })
}

/// YouTube Music cycles NONE → ALL → ONE → NONE, one press per step: how many
/// presses turn repeat on (to ALL) or off from `mode`.
pub fn repeat_presses(mode: &str, on: bool) -> u32 {
    match (mode, on) {
        ("NONE", true) => 1,
        ("ALL", false) => 2,
        ("ONE", false) => 1,
        _ => 0,
    }
}

/// A control from the card or the pill → Pear's route and JSON body.
/// None when there is nothing to send (repeat already where asked).
pub fn command_for(action: &str, value: Option<f64>, state: &YtmState) -> Option<(&'static str, Option<Json>)> {
    use serde_json::json;
    let v = value.unwrap_or(0.0);
    Some(match action {
        "playPause" => ("toggle-play", None),
        "play" => ("play", None),
        "pause" => ("pause", None),
        "next" => ("next", None),
        "previous" => ("previous", None),
        "seek" => ("seek-to", Some(json!({ "seconds": v.max(0.0).round() as i64 }))),
        "volume" => ("volume", Some(json!({ "volume": v.round().clamp(0.0, 100.0) as i64 }))),
        // The shuffle is a button: one press flips it.
        "shuffle" => {
            if (v != 0.0) == state.shuffle {
                return None;
            }
            ("shuffle", None)
        }
        "repeat" => {
            let presses = repeat_presses(&state.repeat_mode, v != 0.0);
            if presses == 0 {
                return None;
            }
            ("switch-repeat", Some(json!({ "iteration": presses })))
        }
        _ => return None,
    })
}

/// Only Google's image CDNs: the URL comes from another app.
pub fn artwork_url_allowed(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else { return false };
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return false };
    url.scheme() == "https"
        && ["googleusercontent.com", "ytimg.com", "ggpht.com"]
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// The retry delay after `failures` connections in a row that found nobody.
pub fn backoff_secs(failures: u32) -> u64 {
    (2u64 << failures.min(5)).min(60)
}

/// The top result of Pear's `/search` (YouTube Music's own answer, passed on
/// as is): the first song or video id in the order YouTube Music lists them.
pub fn first_video_id(found: &Json) -> Option<String> {
    match found {
        Json::Object(map) => {
            if let Some(Json::String(id)) = map.get("videoId") {
                if id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return Some(id.clone());
                }
            }
            map.values().find_map(first_video_id)
        }
        Json::Array(list) => list.iter().find_map(first_video_id),
        _ => None,
    }
}

// ── Runtime ───────────────────────────────────────────────────────────────────

use std::sync::LazyLock;
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;

enum Wake {
    /// Try now (the card came on screen, a token arrived, Pear was launched).
    Retry,
    Stop,
}

#[derive(Default)]
struct Shared {
    active: bool,
    state: YtmState,
    wake: Option<mpsc::UnboundedSender<Wake>>,
    /// The cover last sent, so a song's cover is fetched once.
    artwork_for: Option<String>,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Default::default);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn base() -> String {
    format!("http://127.0.0.1:{PORT}")
}

fn client(timeout: Duration) -> Option<reqwest::Client> {
    // Never through a system proxy: it's this machine.
    reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(3)).timeout(timeout).build().ok()
}

fn token() -> Option<String> {
    crate::secrets::get_internal(TOKEN_KEY)
}

fn emit(app: &AppHandle, state: &YtmState) {
    use tauri::Emitter;
    let _ = app.emit_to(crate::island::WINDOW_LABEL, STATE_EVENT, state);
}

/// Changes the shared state, and tells the island when it changed.
fn update(app: &AppHandle, f: impl FnOnce(&mut YtmState)) {
    let state = {
        let mut s = SHARED.lock();
        if !s.active {
            return;
        }
        let before = s.state.clone();
        f(&mut s.state);
        if s.state == before {
            return;
        }
        s.state.clone()
    };
    emit(app, &state);
}

fn wake(w: Wake) {
    if let Some(tx) = &SHARED.lock().wake {
        let _ = tx.send(w);
    }
}

/// Starts the listener when the pill is declared, stops it when it is not.
/// Called at launch and after every settings save.
pub fn sync(app: &AppHandle, active_integrations: &[String]) {
    let on = active_integrations.iter().any(|id| id == PILL_ID);
    let (old, rx) = {
        let mut s = SHARED.lock();
        if s.active == on {
            return;
        }
        s.active = on;
        s.state = YtmState { installed: installed(), ..YtmState::default() };
        s.artwork_for = None;
        let old = s.wake.take();
        let rx = if on {
            let (tx, rx) = mpsc::unbounded_channel();
            s.wake = Some(tx);
            Some(rx)
        } else {
            None
        };
        (old, rx)
    };
    if let Some(old) = old {
        let _ = old.send(Wake::Stop);
    }
    if let Some(rx) = rx {
        let app = app.clone();
        tauri::async_runtime::spawn(listen(app, rx));
    }
}

/// The listener: connect, read until Pear goes, wait, again — until Stop.
async fn listen(app: AppHandle, mut rx: mpsc::UnboundedReceiver<Wake>) {
    let mut failures = 0u32;
    loop {
        let wait = match token() {
            None => {
                // No token: only the user's click goes further.
                let link = if reachable().await { Link::NeedsAuth } else { Link::Unreachable };
                update(&app, |s| {
                    if s.link != Link::Waiting && s.link != Link::Denied {
                        s.disconnect(link, now_ms());
                    }
                });
                if link == Link::NeedsAuth { None } else { Some(backoff_secs(failures)) }
            }
            Some(token) => match session(PORT, &token, &mut rx, |json| on_message(&app, json)).await {
                Ended::Stop => return,
                Ended::Unauthorized => {
                    // Pear no longer knows this token (its secret changed, or the client was removed).
                    let _ = crate::secrets::set_internal(TOKEN_KEY, "");
                    update(&app, |s| s.disconnect(Link::NeedsAuth, now_ms()));
                    failures = 0;
                    continue;
                }
                Ended::Gone { was_connected } => {
                    failures = if was_connected { 0 } else { failures + 1 };
                    update(&app, |s| s.disconnect(Link::Unreachable, now_ms()));
                    Some(backoff_secs(failures))
                }
            },
        };
        let next = match wait {
            Some(secs) => match tokio::time::timeout(Duration::from_secs(secs), rx.recv()).await {
                Ok(next) => next,
                // Time to try again on our own.
                Err(_) => continue,
            },
            None => rx.recv().await,
        };
        // A Retry from the island resets the wait: Pear may have just started.
        match next {
            Some(Wake::Retry) => failures = 0,
            Some(Wake::Stop) | None => return,
        }
        while let Ok(w) = rx.try_recv() {
            if matches!(w, Wake::Stop) {
                return;
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum Ended {
    Stop,
    Unauthorized,
    Gone { was_connected: bool },
}

async fn reachable() -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(("127.0.0.1", PORT))).await,
        Ok(Ok(_))
    )
}

/// One WebSocket session with Pear: each message to `on_message`, until Pear
/// goes, refuses the token, or the pill is turned off.
async fn session(
    port: u16,
    token: &str,
    rx: &mut mpsc::UnboundedReceiver<Wake>,
    mut on_message: impl FnMut(&Json),
) -> Ended {
    let Ok(Ok(tcp)) =
        tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(("127.0.0.1", port))).await
    else {
        return Ended::Gone { was_connected: false };
    };
    let url = format!("ws://127.0.0.1:{port}/api/v1/ws?token={token}");
    let Ok((mut ws, _)) = tokio_tungstenite::client_async(url, tcp).await else {
        return Ended::Gone { was_connected: false };
    };
    let mut connected = false;
    loop {
        tokio::select! {
            w = rx.recv() => match w {
                Some(Wake::Retry) => {}
                Some(Wake::Stop) | None => {
                    let _ = ws.close(None).await;
                    return Ended::Stop;
                }
            },
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let Ok(json) = serde_json::from_str::<Json>(text.as_str()) else { continue };
                    connected = true;
                    on_message(&json);
                }
                Some(Ok(Message::Close(frame))) => {
                    if frame.is_some_and(|f| f.code == CloseCode::Policy) {
                        return Ended::Unauthorized;
                    }
                    return Ended::Gone { was_connected: connected };
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Ended::Gone { was_connected: connected },
            },
        }
    }
}

fn on_message(app: &AppHandle, json: &Json) {
    let (state, applied, art) = {
        let mut s = SHARED.lock();
        if !s.active {
            return;
        }
        let applied = s.state.apply(json, now_ms());
        let art = s.state.track.as_ref().and_then(|t| t.art_url.clone());
        let fetch = art.filter(|a| s.artwork_for.as_deref() != Some(a.as_str()));
        if fetch.is_some() {
            s.artwork_for.clone_from(&fetch);
        }
        (s.state.clone(), applied, fetch)
    };
    if applied.changed {
        emit(app, &state);
    }
    if let Some(art) = art {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { send_artwork(&app, art).await });
    }
}

async fn send_artwork(app: &AppHandle, art_url: String) {
    use tauri::Emitter;
    if !artwork_url_allowed(&art_url) {
        return;
    }
    let Some(client) = client(Duration::from_secs(10)) else { return };
    let Ok(response) = client.get(&art_url).send().await else { return };
    if !response.status().is_success() {
        return;
    }
    let Ok(bytes) = crate::net::read_capped(response, ARTWORK_LIMIT).await else { return };
    let Some(data_url) = crate::spotify::data_url(&bytes) else { return };
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct Artwork {
        art_url: String,
        data_url: String,
    }
    let _ = app.emit_to(crate::island::WINDOW_LABEL, ARTWORK_EVENT, Artwork { art_url, data_url });
}

// ── « OK Coucou » ─────────────────────────────────────────────────────────────

/// Where the pill stands, for the voice: connected to Pear, and playing.
pub fn voice_status() -> (bool, bool) {
    let s = SHARED.lock().unwrap();
    (s.active && s.state.link == Link::Connected, s.active && s.state.playing)
}

/// « mets du lofi sur YouTube Music »: Pear's search, the top result right
/// after the current song, then on to it. The result's title is not known
/// here; the card shows it a moment later.
pub async fn search_and_play(app: &AppHandle, query: &str) -> Result<(), String> {
    let connected = SHARED.lock().unwrap().state.link == Link::Connected;
    let token = token().filter(|_| connected).ok_or("not connected")?;
    let client = client(Duration::from_secs(10)).ok_or("no client")?;
    let post = |path: &str, body: Option<Json>| {
        let mut request = client.post(format!("{}/api/v1/{path}", base())).bearer_auth(&token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send()
    };
    let response = post("search", Some(serde_json::json!({ "query": query }))).await.map_err(|e| e.to_string())?;
    if response.status().as_u16() == 401 {
        let _ = crate::secrets::set_internal(TOKEN_KEY, "");
        update(app, |s| s.disconnect(Link::NeedsAuth, now_ms()));
        wake(Wake::Retry);
        return Err("not connected".into());
    }
    let body = crate::net::read_capped(response, crate::net::MAX_BODY).await?;
    let found: Json = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let id = first_video_id(&found).ok_or("nothing found")?;
    let queued = serde_json::json!({ "videoId": id, "insertPosition": "INSERT_AFTER_CURRENT_VIDEO" });
    post("queue", Some(queued)).await.map_err(|e| e.to_string())?;
    post("next", None).await.map_err(|e| e.to_string())?;
    post("play", None).await.map_err(|e| e.to_string())?;
    Ok(())
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// The player as the page should show it now; reconnects at once if it can
/// (the card coming on screen).
#[tauri::command]
pub fn youtube_music_refresh() -> Option<YtmState> {
    let state = {
        let s = SHARED.lock();
        if !s.active {
            return None;
        }
        s.state.clone()
    };
    if state.link != Link::Connected {
        wake(Wake::Retry);
    }
    Some(state)
}

/// "Connect": asks Pear for a token. Pear shows "Allow / Deny" to the user;
/// nothing is approved on their behalf. True when a token came back.
#[tauri::command]
pub async fn youtube_music_connect(app: AppHandle) -> bool {
    {
        let s = SHARED.lock();
        if !s.active || s.state.link == Link::Waiting {
            return false;
        }
    }
    update(&app, |s| s.link = Link::Waiting);
    // The dialog waits for the user.
    let Some(client) = client(Duration::from_mins(2)) else { return false };
    let response = client.post(format!("{}/auth/{CLIENT_ID}", base())).send().await;
    let status = response.as_ref().map(|r| r.status().as_u16()).unwrap_or(0);
    let token = match response {
        Ok(r) if status == 200 => r.json::<Json>().await.ok().and_then(|j| {
            j.get("accessToken").and_then(Json::as_str).filter(|t| !t.is_empty()).map(str::to_string)
        }),
        _ => None,
    };
    match token {
        Some(token) if crate::secrets::set_internal(TOKEN_KEY, &token).is_ok() => {
            // Still "waiting" until Pear's first message says connected.
            wake(Wake::Retry);
            true
        }
        _ => {
            let link = if status == 403 { Link::Denied } else { Link::Unreachable };
            update(&app, |s| s.disconnect(link, now_ms()));
            if link == Link::Unreachable {
                wake(Wake::Retry);
            }
            false
        }
    }
}

/// A control from the card or the pill: `playPause`, `next`, `previous`,
/// `seek` (seconds), `shuffle` / `repeat` (1 on, 0 off), `volume` (0…100).
#[tauri::command]
pub async fn youtube_music_control(app: AppHandle, action: String, value: Option<f64>) -> bool {
    let (state, token) = {
        let s = SHARED.lock();
        (s.state.clone(), token())
    };
    if state.link != Link::Connected {
        return false;
    }
    let (Some(token), Some((path, body))) = (token, command_for(&action, value, &state)) else { return false };
    // Shuffle and repeat show at once; Pear confirms them on the WebSocket.
    match action.as_str() {
        "shuffle" => update(&app, |s| s.shuffle = !s.shuffle),
        "repeat" => update(&app, |s| s.set_repeat(if value.unwrap_or(0.0) != 0.0 { "ALL" } else { "NONE" })),
        _ => {}
    }
    let Some(client) = client(Duration::from_secs(5)) else { return false };
    let mut request = client.post(format!("{}/api/v1/{path}", base())).bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    match request.send().await.map(|r| r.status().as_u16()) {
        Ok(401) => {
            let _ = crate::secrets::set_internal(TOKEN_KEY, "");
            update(&app, |s| s.disconnect(Link::NeedsAuth, now_ms()));
            wake(Wake::Retry);
            false
        }
        Ok(code) => (200..300).contains(&code),
        Err(_) => false,
    }
}

/// "Open YouTube Music": starts Pear Desktop (a second launch brings the
/// running one forward). Without it, its download page. True when started.
#[tauri::command]
pub async fn youtube_music_open() -> bool {
    let started = match launcher() {
        Some(exe) => {
            #[cfg(windows)]
            let spawned = std::process::Command::new(&exe).spawn();
            #[cfg(not(windows))]
            let spawned = {
                use std::os::unix::process::CommandExt;
                std::process::Command::new(&exe)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .process_group(0)
                    .spawn()
            };
            match spawned {
                Ok(mut child) => {
                    // Reaped when it exits, so it never lingers as a zombie.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    true
                }
                Err(err) => {
                    crate::log::line(format!("youtube music: could not start Pear Desktop: {err}"));
                    false
                }
            }
        }
        None => {
            crate::platform::open_url(DOWNLOAD_URL);
            false
        }
    };
    if started {
        // Its server starts a moment after the window.
        tauri::async_runtime::spawn(async {
            for secs in [3, 6] {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                wake(Wake::Retry);
            }
        });
    }
    started
}

/// For Settings and the idle card: whether there is a Pear Desktop to launch.
#[tauri::command]
pub fn youtube_music_installed() -> bool {
    installed()
}

fn installed() -> bool {
    launcher().is_some()
}

/// Pear Desktop's executable, where its installers put it.
fn launcher() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    #[cfg(windows)]
    {
        let under = |var: &str, rest: &str| std::env::var_os(var).map(|base| PathBuf::from(base).join(rest));
        [
            // The per-user installer (the default), winget and Scoop.
            under("LOCALAPPDATA", r"Programs\youtube-music\YouTube Music.exe"),
            // "Install for all users".
            under("ProgramFiles", r"YouTube Music\YouTube Music.exe"),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    }
    #[cfg(not(windows))]
    {
        // The .deb / .rpm link it into /usr/bin; the AUR package too.
        crate::platform::find_on_path("youtube-music")
            .or_else(|| Some(PathBuf::from("/opt/YouTube Music/youtube-music")).filter(|p| p.is_file()))
    }
}

// ── Tests (the pure parts) ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(id: &str, title: &str) -> Json {
        json!({
            "title": title, "artist": "Daft Punk feat. Pharrell Williams", "views": 1,
            "imageSrc": format!("https://lh3.googleusercontent.com/{id}=w544-h544"),
            "isPaused": false, "songDuration": 248, "elapsedSeconds": 12,
            "album": "Random Access Memories", "videoId": id, "mediaType": "AUDIO",
        })
    }

    fn connected() -> YtmState {
        let mut s = YtmState::default();
        s.apply(&json!({ "type": "PLAYER_INFO", "song": song("abc", "Get Lucky"), "isPlaying": true,
            "muted": false, "position": 12, "volume": 70, "repeat": "NONE", "shuffle": false }), 1_000.0);
        s
    }

    #[test]
    fn player_info_connects_and_reads_everything() {
        let s = connected();
        assert_eq!(s.link, Link::Connected);
        assert!(s.running && s.playing);
        let t = s.track.as_ref().unwrap();
        assert_eq!((t.id.as_str(), t.title.as_str(), t.artist.as_str()), ("abc", "Get Lucky", "Daft Punk"));
        assert_eq!(t.album, "Random Access Memories");
        assert_eq!(t.duration, 248.0);
        assert_eq!(s.position, 12.0);
        assert_eq!(s.volume, 70);
        assert!(!s.repeat && !s.shuffle);
    }

    #[test]
    fn player_info_without_a_song_is_not_playing() {
        let mut s = YtmState::default();
        s.apply(&json!({ "type": "PLAYER_INFO", "isPlaying": true, "position": 0, "volume": 100,
            "muted": true, "repeat": "ONE", "shuffle": true }), 0.0);
        assert_eq!(s.link, Link::Connected);
        assert!(s.track.is_none() && !s.playing);
        assert_eq!(s.volume, 0, "muted shows as 0");
        assert!(s.repeat && s.shuffle);
        assert_eq!(s.repeat_mode, "ONE");
    }

    #[test]
    fn a_new_song_restarts_the_clock_and_cleans_its_title() {
        let mut s = connected();
        let a = s.apply(&json!({ "type": "VIDEO_CHANGED", "song": song("xyz", "Instant Crush (Official Video)"),
            "position": 0 }), 5_000.0);
        assert!(a.changed && a.track_changed);
        assert_eq!(s.track.as_ref().unwrap().title, "Instant Crush");
        assert_eq!((s.position, s.position_at), (0.0, 5_000.0));
    }

    #[test]
    fn play_and_pause_freeze_the_clock_where_it_is() {
        let mut s = connected();
        let a = s.apply(&json!({ "type": "PLAYER_STATE_CHANGED", "isPlaying": false, "position": 20 }), 9_000.0);
        assert!(a.changed && !s.playing);
        assert_eq!(s.position_at_time(60_000.0), 20.0);
        let a = s.apply(&json!({ "type": "PLAYER_STATE_CHANGED", "isPlaying": true, "position": 20 }), 10_000.0);
        assert!(a.started_playing);
        assert_eq!(s.position_at_time(12_000.0), 22.0);
    }

    #[test]
    fn the_seconds_ticking_by_say_nothing_and_a_seek_does() {
        let mut s = connected(); // 12 s at t = 1 s, playing
        assert!(!s.apply(&json!({ "type": "POSITION_CHANGED", "position": 13 }), 2_000.0).changed);
        assert!(!s.apply(&json!({ "type": "POSITION_CHANGED", "position": 14 }), 3_100.0).changed);
        let a = s.apply(&json!({ "type": "POSITION_CHANGED", "position": 120 }), 4_000.0);
        assert!(a.changed);
        assert_eq!(s.position, 120.0);
    }

    #[test]
    fn volume_repeat_and_shuffle_follow_pear() {
        let mut s = connected();
        s.apply(&json!({ "type": "VOLUME_CHANGED", "volume": 33.4, "muted": false }), 0.0);
        assert_eq!(s.volume, 33);
        s.apply(&json!({ "type": "REPEAT_CHANGED", "repeat": "ALL" }), 0.0);
        assert!(s.repeat);
        s.apply(&json!({ "type": "SHUFFLE_CHANGED", "shuffle": true }), 0.0);
        assert!(s.shuffle);
        assert!(!s.apply(&json!({ "type": "SOMETHING_NEW" }), 0.0).changed);
    }

    #[test]
    fn a_cover_is_kept_for_the_same_song() {
        let prev = parse_song(&song("abc", "Get Lucky"), None).unwrap();
        let mut bare = song("abc", "Get Lucky");
        bare["imageSrc"] = Json::Null;
        assert_eq!(parse_song(&bare, Some(&prev)).unwrap().art_url, prev.art_url);
        let mut other = song("xyz", "Lose Yourself to Dance");
        other["imageSrc"] = Json::Null;
        assert_eq!(parse_song(&other, Some(&prev)).unwrap().art_url, None);
        assert!(parse_song(&json!({ "title": "", "videoId": "" }), None).is_none());
    }

    #[test]
    fn repeat_presses_follow_youtube_musics_cycle() {
        assert_eq!(repeat_presses("NONE", true), 1);
        assert_eq!(repeat_presses("ALL", false), 2);
        assert_eq!(repeat_presses("ONE", false), 1);
        assert_eq!(repeat_presses("ALL", true), 0);
        assert_eq!(repeat_presses("NONE", false), 0);
    }

    #[test]
    fn controls_map_to_pears_routes() {
        let s = connected();
        assert_eq!(command_for("playPause", None, &s), Some(("toggle-play", None)));
        assert_eq!(command_for("next", None, &s), Some(("next", None)));
        assert_eq!(command_for("seek", Some(61.6), &s), Some(("seek-to", Some(json!({ "seconds": 62 })))));
        assert_eq!(command_for("volume", Some(140.0), &s), Some(("volume", Some(json!({ "volume": 100 })))));
        assert_eq!(command_for("shuffle", Some(1.0), &s), Some(("shuffle", None)));
        assert_eq!(command_for("shuffle", Some(0.0), &s), None, "already off");
        assert_eq!(command_for("repeat", Some(1.0), &s), Some(("switch-repeat", Some(json!({ "iteration": 1 })))));
        assert_eq!(command_for("repeat", Some(0.0), &s), None, "already off");
        assert_eq!(command_for("rm -rf", None, &s), None);
    }

    #[test]
    fn covers_only_come_from_googles_cdns() {
        assert!(artwork_url_allowed("https://lh3.googleusercontent.com/abc=w544-h544"));
        assert!(artwork_url_allowed("https://i.ytimg.com/vi/abc/hqdefault.jpg"));
        assert!(artwork_url_allowed("https://yt3.ggpht.com/abc"));
        assert!(!artwork_url_allowed("http://lh3.googleusercontent.com/abc"));
        assert!(!artwork_url_allowed("https://evilgoogleusercontent.com/abc"));
        assert!(!artwork_url_allowed("https://127.0.0.1/abc"));
        assert!(!artwork_url_allowed("file:///etc/passwd"));
    }

    #[test]
    fn a_search_plays_the_first_song_youtube_music_lists() {
        // The shape of YouTube Music's answer, cut down: filter chips first
        // (no song), then the top result card, then the songs shelf.
        let found = json!({ "contents": { "tabbedSearchResultsRenderer": { "tabs": [{ "tabRenderer": { "content": {
            "sectionListRenderer": {
                "header": { "chipCloudRenderer": { "chips": [{ "chipCloudChipRenderer": { "navigationEndpoint": {
                    "searchEndpoint": { "query": "lofi", "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D" } } } }] } },
                "contents": [
                    { "musicCardShelfRenderer": { "title": { "runs": [{ "text": "lofi hip hop radio",
                        "navigationEndpoint": { "watchEndpoint": { "videoId": "jfKfPfyJRdk" } } }] } } },
                    { "musicShelfRenderer": { "contents": [{ "musicResponsiveListItemRenderer": {
                        "playlistItemData": { "videoId": "5qap5aO4i9A" } } }] } }
                ]
            } } } }] } } });
        assert_eq!(first_video_id(&found).as_deref(), Some("jfKfPfyJRdk"));
        assert_eq!(first_video_id(&json!({ "contents": [] })), None);
        // Something that is not a YouTube id is skipped.
        assert_eq!(first_video_id(&json!([{ "videoId": "../../etc" }, { "videoId": "dQw4w9WgXcQ" }])).as_deref(),
            Some("dQw4w9WgXcQ"));
    }

    #[test]
    fn retries_slow_down_to_a_minute() {
        assert_eq!(backoff_secs(0), 2);
        assert_eq!(backoff_secs(1), 4);
        assert_eq!(backoff_secs(3), 16);
        assert_eq!(backoff_secs(5), 60);
        assert_eq!(backoff_secs(40), 60);
    }

    // ── The WebSocket client, against a local stand-in for Pear ──

    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    /// A one-connection server: sends `messages`, then closes with `close`
    /// (None: stays open). Returns its port and the path it was asked for.
    async fn fake_pear(messages: Vec<Json>, close: Option<CloseCode>) -> (u16, tokio::sync::oneshot::Receiver<String>) {
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (path_tx, path_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut path = None;
            // tungstenite fixes this callback's signature, large error type included.
            #[allow(clippy::result_large_err)]
            let callback = |req: &Request, resp: Response| {
                path = Some(req.uri().to_string());
                Ok(resp)
            };
            let mut ws = tokio_tungstenite::accept_hdr_async(tcp, callback).await.unwrap();
            let _ = path_tx.send(path.unwrap_or_default());
            for m in messages {
                ws.send(Message::text(m.to_string())).await.unwrap();
            }
            match close {
                Some(code) => {
                    let _ = ws.close(Some(CloseFrame { code, reason: "Unauthorized".into() })).await;
                    while let Some(Ok(_)) = ws.next().await {}
                }
                None => while let Some(Ok(_)) = ws.next().await {},
            }
        });
        (port, path_rx)
    }

    #[tokio::test]
    async fn a_session_reads_pears_messages_until_it_goes() {
        let info = json!({ "type": "PLAYER_INFO", "song": song("abc", "Get Lucky"), "isPlaying": true });
        let (port, path) = fake_pear(vec![info, json!({ "type": "POSITION_CHANGED", "position": 13 })],
            Some(CloseCode::Away)).await;
        let (_tx, mut rx) = mpsc::unbounded_channel();
        let mut seen = vec![];
        let ended = session(port, "tok", &mut rx, |j| seen.push(j["type"].as_str().unwrap().to_string())).await;
        assert_eq!(ended, Ended::Gone { was_connected: true });
        assert_eq!(seen, ["PLAYER_INFO", "POSITION_CHANGED"]);
        assert_eq!(path.await.unwrap(), "/api/v1/ws?token=tok");
    }

    #[tokio::test]
    async fn a_refused_token_ends_the_session_as_unauthorized() {
        let (port, _) = fake_pear(vec![], Some(CloseCode::Policy)).await;
        let (_tx, mut rx) = mpsc::unbounded_channel();
        assert_eq!(session(port, "old", &mut rx, |_| {}).await, Ended::Unauthorized);
    }

    #[tokio::test]
    async fn turning_the_pill_off_hangs_up() {
        let (port, _) = fake_pear(vec![json!({ "type": "PLAYER_INFO" })], None).await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut got = 0;
        let ended = session(port, "tok", &mut rx, |_| {
            got += 1;
            let _ = tx.send(Wake::Stop);
        })
        .await;
        assert_eq!(ended, Ended::Stop);
        assert_eq!(got, 1);
    }

    #[tokio::test]
    async fn nobody_listening_is_pear_gone() {
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        let (_tx, mut rx) = mpsc::unbounded_channel();
        assert_eq!(session(port, "tok", &mut rx, |_| {}).await, Ended::Gone { was_connected: false });
    }

    #[test]
    fn the_state_reaches_the_page_in_the_spotify_cards_shape() {
        let v = serde_json::to_value(connected()).unwrap();
        for key in ["running", "installed", "link", "track", "playing", "position", "positionAt", "shuffle",
            "repeat", "repeatMode", "volume"] {
            assert!(v.get(key).is_some(), "{key}");
        }
        assert_eq!(v["link"], "connected");
        assert_eq!(v["track"]["artUrl"], "https://lh3.googleusercontent.com/abc=w544-h544");
        assert!(v["track"].get("objectPath").is_none());
    }
}
