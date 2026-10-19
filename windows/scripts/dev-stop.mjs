// Run by `npm run tauri dev` (scripts/tauri.mjs) before the Tauri CLI starts.
// Coucou runs as a single instance: while one is open (an earlier dev build
// left in the tray, or the installed app), the new dev build hands over to it
// and quits at once, the dev command stops Vite with it, and the old window
// is left on a "localhost refused to connect" page. So the open Coucou is
// asked to quit first, the gentle way (`coucou --quit`), so it can restore the
// taskbar and its other state.
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { connect } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const windows = process.platform === "win32";
const exe = resolve(dirname(fileURLToPath(import.meta.url)), "..", "target", "debug", windows ? "coucou.exe" : "coucou");

function running() {
  const out = windows
    ? spawnSync("tasklist", ["/FI", "IMAGENAME eq coucou.exe", "/NH"], { encoding: "utf8" }).stdout ?? ""
    : spawnSync("pgrep", ["-x", "coucou"], { encoding: "utf8" }).stdout ?? "";
  return windows ? out.toLowerCase().includes("coucou.exe") : out.trim() !== "";
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// Vite's port (vite.config.ts). Something still answers on it when the Coucou
// that just quit came from another `tauri dev`: that command stops its Vite a
// moment later. Tauri would see the port answer, start the new Coucou on the
// old Vite, and leave it without one once that Vite stops.
const PORT = 1420;
const portTaken = () =>
  new Promise((done) => {
    const socket = connect({ host: "127.0.0.1", port: PORT });
    socket.setTimeout(500);
    socket.once("connect", () => { socket.destroy(); done(true); });
    socket.once("timeout", () => { socket.destroy(); done(false); });
    socket.once("error", () => done(false));
  });

// Only when one is open: started alone, an older build without --quit would
// simply open Coucou.
if (existsSync(exe) && running()) {
  console.log("[dev-stop] Coucou is open: asking it to quit…");
  spawnSync(exe, ["--quit"], { stdio: "ignore", timeout: 10_000 });
  for (let i = 0; i < 40 && running(); i++) await sleep(200);
  if (running()) {
    console.warn("[dev-stop] Coucou is still open: quit it from its tray icon, then run the command again.");
    process.exit(1);
  }
}

// Also when no Coucou is open: a Vite left over from another `tauri dev`.
if (await portTaken()) {
  console.log(`[dev-stop] Port ${PORT} is in use: waiting for the other dev server to stop…`);
  for (let i = 0; i < 75 && (await portTaken()); i++) await sleep(200);
  if (await portTaken()) {
    console.warn(`[dev-stop] Port ${PORT} is still in use: stop the other \`npm run tauri dev\` (Ctrl+C in its terminal), then run the command again.`);
    process.exit(1);
  }
}
