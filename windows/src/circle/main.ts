// « Entoure et demande »'s overlay (circle.html, shell/capture.rs): the user
// circles something with the mouse; its bounding box goes to Rust, which closes
// this window and only then takes that part of the screen. Esc, a right click
// or a tiny scribble cancels. The page never sees the pixels.

import { Bridge } from "../core/bridge";
import { resolveLanguage, setLanguage, systemLanguages, t } from "../i18n/i18n";
import { colorAlpha, colorHex } from "../design/tokens";

const canvas = document.getElementById("lasso") as HTMLCanvasElement;
const hint = document.getElementById("hint")!;
const ctx = canvas.getContext("2d")!;
let points: [number, number][] = [];
let drawing = false;
let done = false;

function fit() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(window.innerWidth * dpr);
  canvas.height = Math.round(window.innerHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  draw();
}

function draw() {
  ctx.clearRect(0, 0, window.innerWidth, window.innerHeight);
  if (points.length < 2) return;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(points[0][0], points[0][1]);
  for (const [x, y] of points.slice(1)) ctx.lineTo(x, y);
  if (!drawing) ctx.closePath();
  ctx.fillStyle = colorAlpha("--color-accent-rgb", 0.10);
  ctx.fill();
  ctx.shadowColor = colorAlpha("--color-accent-rgb", 0.9);
  ctx.shadowBlur = 12;
  ctx.strokeStyle = colorHex("--color-accent");
  ctx.lineWidth = 2.5;
  ctx.setLineDash([10, 7]);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.shadowBlur = 0;
}

/** The lasso's bounding box, a little wider, kept on the screen. */
export function bounds(pts: [number, number][], w: number, h: number, pad = 8) {
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  const x = Math.max(0, Math.min(...xs) - pad);
  const y = Math.max(0, Math.min(...ys) - pad);
  return { x, y, width: Math.min(w, Math.max(...xs) + pad) - x, height: Math.min(h, Math.max(...ys) + pad) - y };
}

function cancel() {
  if (done) return;
  done = true;
  void Bridge.circleCancel();
}

async function finish() {
  if (done) return;
  const box = bounds(points, window.innerWidth, window.innerHeight);
  if (points.length < 3 || box.width < 12 || box.height < 12) {
    cancel();
    return;
  }
  done = true;
  // Hide the lasso before the window goes, so it can never end up in the capture.
  points = [];
  draw();
  document.body.style.background = "transparent";
  await Bridge.circleCapture(box.x, box.y, box.width, box.height).catch(() => Bridge.circleCancel());
}

window.addEventListener("pointerdown", (e) => {
  if (e.button === 2) {
    cancel();
    return;
  }
  if (e.button !== 0) return;
  drawing = true;
  document.body.classList.add("drawing");
  points = [[e.clientX, e.clientY]];
  canvas.setPointerCapture(e.pointerId);
});
window.addEventListener("pointermove", (e) => {
  if (!drawing) return;
  const last = points[points.length - 1];
  if (Math.hypot(e.clientX - last[0], e.clientY - last[1]) < 2) return;
  points.push([e.clientX, e.clientY]);
  draw();
});
window.addEventListener("pointerup", () => {
  if (!drawing) return;
  drawing = false;
  draw();
  void finish();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") cancel();
});
window.addEventListener("contextmenu", (e) => e.preventDefault());
window.addEventListener("blur", () => {
  // Another window took the focus (Alt+Tab): the overlay goes, as with Esc.
  if (!drawing) cancel();
});
window.addEventListener("resize", fit);

async function main() {
  const boot = await Bridge.boot();
  setLanguage(resolveLanguage(boot?.settings.language, systemLanguages()));
  hint.textContent = t("Circle what you want to ask about · Esc to cancel");
  fit();
  window.focus();
}

void main();
