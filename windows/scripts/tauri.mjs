// `npm run tauri <args>`: the Tauri CLI, with one step first for `dev`.
// Tauri checks Vite's port while its beforeDevCommand starts, so an older dev
// server still answering there would win the race: the open Coucou and its
// dev server are stopped here, before Tauri runs (scripts/dev-stop.mjs).
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
if (args[0] === "dev") await import("./dev-stop.mjs");
const result = spawnSync("tauri", args, { stdio: "inherit", shell: true });
process.exit(result.status ?? 1);
