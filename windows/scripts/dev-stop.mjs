// Run by `npm run tauri dev` before Vite starts (tauri.conf.json, beforeDevCommand).
// Coucou runs as a single instance: while one is open (an earlier dev build
// left in the tray, or the installed app), the new dev build hands over to it
// and quits at once, the dev command stops Vite with it, and the old window
// is left on a "localhost refused to connect" page. So the open Coucou is
// asked to quit first, the gentle way (`coucou --quit`), so it can restore the
// taskbar and its other state.
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
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
