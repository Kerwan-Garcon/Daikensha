#!/usr/bin/env node
// A stand-in for Pear Desktop's "API Server" plugin, to try the YouTube Music
// pill without YouTube Music: the same port, routes, token dialog and
// WebSocket messages as pear-desktop/src/plugins/api-server, with three songs.
// Node's built-ins only.
//
//   node scripts/fake-pear.mjs            asks "Allow?" in this terminal
//   node scripts/fake-pear.mjs --allow    allows every client
//   node scripts/fake-pear.mjs --deny     denies every client
//   node scripts/fake-pear.mjs --port 26538
//
// While it runs, type a command and Enter to act as if from YouTube Music's own
// window: p (play/pause), n (next), s (seek to 1:00), v (volume 30), r (repeat),
// f (shuffle), x (forget the token: the next call gets 401, the WebSocket 1008),
// q (quit, as Pear quitting).

import { createServer } from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { createInterface } from "node:readline";

const args = process.argv.slice(2);
const PORT = Number(args[args.indexOf("--port") + 1]) || 26538;
const POLICY = args.includes("--allow") ? "allow" : args.includes("--deny") ? "deny" : "ask";

const SONGS = [
  { videoId: "5NV6Rdv1a3I", title: "Get Lucky (Official Audio) ft. Pharrell Williams, Nile Rodgers", artist: "Daft Punk",
    album: "Random Access Memories", songDuration: 369 },
  { videoId: "a5uQMwRMHcs", title: "Instant Crush", artist: "Daft Punk feat. Julian Casablancas",
    album: "Random Access Memories", songDuration: 338 },
  { videoId: "FGBhQbmPwH8", title: "One More Time", artist: "Daft Punk", album: null, songDuration: 320 },
].map((s) => ({ ...s, views: 1, imageSrc: `https://i.ytimg.com/vi/${s.videoId}/hqdefault.jpg`, mediaType: "AUDIO",
  url: `https://music.youtube.com/watch?v=${s.videoId}` }));

const tokens = new Set();
const sockets = new Set();
const player = { index: 0, playing: true, position: 0, volume: 70, muted: false, repeat: "NONE", shuffle: false };

const log = (...m) => console.log(new Date().toISOString().slice(11, 19), ...m);
const song = () => ({ ...SONGS[player.index], isPaused: !player.playing, elapsedSeconds: Math.floor(player.position) });

// ── WebSocket (RFC 6455, server side, text frames only) ───────────────────────

function frame(opcode, payload) {
  const body = Buffer.from(payload);
  const head = body.length < 126 ? Buffer.from([0x80 | opcode, body.length])
    : Buffer.from([0x80 | opcode, 126, body.length >> 8, body.length & 255]);
  return Buffer.concat([head, body]);
}

function send(type, state) {
  const data = frame(0x1, JSON.stringify({ type, ...state }));
  for (const s of sockets) s.write(data);
}

function closeWith(socket, code, reason) {
  const body = Buffer.alloc(2 + Buffer.byteLength(reason));
  body.writeUInt16BE(code, 0);
  body.write(reason, 2);
  socket.write(frame(0x8, body));
  socket.end();
}

function bearer(req) {
  const h = req.headers.authorization ?? "";
  return h.startsWith("Bearer ") ? h.slice(7) : null;
}

// ── Player ────────────────────────────────────────────────────────────────────

function changeSong(delta) {
  player.index = (player.index + delta + SONGS.length) % SONGS.length;
  player.position = 0;
  send("VIDEO_CHANGED", { song: song(), position: 0 });
}

function setPlaying(on) {
  player.playing = on;
  send("PLAYER_STATE_CHANGED", { isPlaying: on, position: Math.floor(player.position) });
}

function seek(seconds) {
  player.position = Math.max(0, Math.min(seconds, SONGS[player.index].songDuration));
  send("POSITION_CHANGED", { position: player.position });
}

function cycleRepeat(times) {
  const order = ["NONE", "ALL", "ONE"];
  player.repeat = order[(order.indexOf(player.repeat) + times) % 3];
  send("REPEAT_CHANGED", { repeat: player.repeat });
}

function toggleShuffle() {
  player.shuffle = !player.shuffle;
  send("SHUFFLE_CHANGED", { shuffle: player.shuffle });
}

function setVolume(v) {
  player.volume = Math.max(0, Math.min(100, Math.round(v)));
  send("VOLUME_CHANGED", { volume: player.volume, muted: player.muted });
}

// Pear says the position every second while it plays.
setInterval(() => {
  if (!player.playing) return;
  player.position += 1;
  if (player.position >= SONGS[player.index].songDuration) {
    if (player.repeat === "ONE") seek(0);
    else changeSong(1);
    return;
  }
  send("POSITION_CHANGED", { position: player.position });
}, 1000);

// ── HTTP ──────────────────────────────────────────────────────────────────────

const rl = createInterface({ input: process.stdin, output: process.stdout });
let asking = null;

function ask(question) {
  return new Promise((resolve) => {
    asking = resolve;
    process.stdout.write(`${question} [y/N] `);
  });
}

async function readJson(req) {
  let body = "";
  for await (const chunk of req) body += chunk;
  try {
    return JSON.parse(body || "{}");
  } catch {
    return {};
  }
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  const reply = (code, json) => {
    res.writeHead(code, json ? { "content-type": "application/json" } : {});
    res.end(json ? JSON.stringify(json) : undefined);
  };

  const auth = url.pathname.match(/^\/auth\/([^/]+)$/);
  if (req.method === "POST" && auth) {
    const id = decodeURIComponent(auth[1]);
    log(`auth request from "${id}"`);
    const ok = POLICY === "allow" || (POLICY === "ask" && (await ask(`Allow "${id}" to control YouTube Music?`)));
    if (!ok) {
      log("→ denied (403)");
      return reply(403);
    }
    const token = randomBytes(16).toString("hex");
    tokens.add(token);
    log("→ allowed, token issued");
    return reply(200, { accessToken: token });
  }

  if (!url.pathname.startsWith("/api/v1/")) return reply(404);
  if (!tokens.has(bearer(req))) {
    log(`${req.method} ${url.pathname} → 401`);
    return reply(401);
  }
  const route = url.pathname.slice("/api/v1/".length);
  const body = req.method === "POST" ? await readJson(req) : {};
  log(`${req.method} ${route}`, Object.keys(body).length ? JSON.stringify(body) : "");
  switch (`${req.method} ${route}`) {
    case "POST toggle-play": setPlaying(!player.playing); break;
    case "POST play": setPlaying(true); break;
    case "POST pause": setPlaying(false); break;
    case "POST next": changeSong(1); break;
    case "POST previous": changeSong(-1); break;
    case "POST seek-to": seek(Number(body.seconds) || 0); break;
    case "POST volume": setVolume(Number(body.volume) || 0); break;
    case "POST shuffle": toggleShuffle(); break;
    case "POST switch-repeat": cycleRepeat(Number(body.iteration) || 1); break;
    case "GET song": case "GET song-info": return reply(200, song());
    case "GET volume": return reply(200, { state: player.volume, isMuted: player.muted });
    default: return reply(404);
  }
  return reply(204);
});

server.on("upgrade", (req, socket) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (url.pathname !== "/api/v1/ws") return socket.destroy();
  const accept = createHash("sha1")
    .update(`${req.headers["sec-websocket-key"]}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
    .digest("base64");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
    `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
  );
  if (!tokens.has(url.searchParams.get("token"))) {
    log("WebSocket with an unknown token → close 1008");
    return closeWith(socket, 1008, "Unauthorized");
  }
  sockets.add(socket);
  log(`WebSocket open (${sockets.size})`);
  socket.on("data", (data) => {
    // A close frame from the client (opcode 8): answer and hang up.
    if ((data[0] & 0x0f) === 0x8) socket.end(frame(0x8, Buffer.alloc(0)));
  });
  socket.on("close", () => {
    sockets.delete(socket);
    log(`WebSocket closed (${sockets.size})`);
  });
  socket.on("error", () => sockets.delete(socket));
  socket.write(frame(0x1, JSON.stringify({
    type: "PLAYER_INFO", song: song(), isPlaying: player.playing, muted: player.muted,
    position: Math.floor(player.position), volume: player.volume, repeat: player.repeat, shuffle: player.shuffle,
  })));
});

rl.on("line", (line) => {
  const cmd = line.trim().toLowerCase();
  if (asking) {
    const resolve = asking;
    asking = null;
    return resolve(cmd === "y" || cmd === "yes");
  }
  switch (cmd) {
    case "p": setPlaying(!player.playing); break;
    case "n": changeSong(1); break;
    case "s": seek(60); break;
    case "v": setVolume(30); break;
    case "r": cycleRepeat(1); break;
    case "f": toggleShuffle(); break;
    case "x": tokens.clear(); for (const s of sockets) closeWith(s, 1008, "Unauthorized"); log("tokens forgotten"); break;
    case "q": process.exit(0);
    default: if (cmd) log("p n s v r f x q");
  }
});

server.listen(PORT, "127.0.0.1", () => {
  log(`fake Pear Desktop API Server on http://127.0.0.1:${PORT} (${POLICY})`);
  log("commands: p play/pause · n next · s seek 1:00 · v volume 30 · r repeat · f shuffle · x forget tokens · q quit");
});
