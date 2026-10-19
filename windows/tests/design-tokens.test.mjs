// The design system (docs/DESIGN_SYSTEM.md): colours and fonts are decided in
// src/design/tokens.css only, the stylesheets use them, and the code asks for
// them by name. These checks keep it that way.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { colorChannels, tokenNames, tokenValue } from "../src/design/tokens.ts";

// fileURLToPath, not .pathname: on Windows that gives "/D:/…", which no path API reads.
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SRC = join(ROOT, "src");
const TOKENS = join(SRC, "design", "tokens.css");

function files(dir, ext) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path, ext);
    return path.endsWith(ext) ? [path] : [];
  });
}

const read = (path) => readFileSync(path, "utf8");
const withoutComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");
// Line comments too (not the "//" of a URL), for the TypeScript sources.
const codeOnly = (ts) => withoutComments(ts).replace(/(^|[^:"'`])\/\/.*$/gm, "$1");
const stylesheets = files(SRC, ".css");
const sources = files(SRC, ".ts");
const pages = readdirSync(ROOT).filter((name) => name.endsWith(".html")).map((name) => join(ROOT, name));

test("no stylesheet but tokens.css writes a colour itself", () => {
  const raw = /#[0-9a-fA-F]{3,8}\b|\brgba?\(\s*\d|\bhsla?\(\s*\d/g;
  const found = stylesheets
    .filter((path) => path !== TOKENS)
    .flatMap((path) => (withoutComments(read(path)).match(raw) ?? []).map((m) => `${relative(ROOT, path)}: ${m}`));
  assert.deepEqual(found, [], "use a token from src/design/tokens.css (or add one there)");
});

test("every var(--…) the stylesheets and the code use is defined", () => {
  const defined = new Set(tokenNames());
  for (const path of stylesheets) for (const m of withoutComments(read(path)).matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) defined.add(m[1]);
  for (const path of sources) {
    const code = read(path);
    for (const m of code.matchAll(/setProperty\(\s*["'`](--[a-zA-Z0-9-]+)/g)) defined.add(m[1]);
    for (const m of code.matchAll(/["'`;]\s*(--[a-zA-Z0-9-]+)\s*:/g)) defined.add(m[1]);
  }
  const missing = [];
  for (const path of [...stylesheets, ...sources]) {
    const text = path.endsWith(".css") ? withoutComments(read(path)) : codeOnly(read(path));
    for (const m of text.matchAll(/var\((--[a-zA-Z0-9-]+)/g)) {
      if (!defined.has(m[1])) missing.push(`${relative(ROOT, path)}: ${m[1]}`);
    }
  }
  assert.deepEqual(missing, []);
});

test("every token the code asks for exists", () => {
  const names = new Set(tokenNames());
  const unknown = [];
  for (const path of sources) {
    for (const m of read(path).matchAll(/\b(?:cssVar|colorHex|colorAlpha|colorChannels|tokenValue)\(\s*"(--[a-z0-9-]+)"/g)) {
      if (!names.has(m[1])) unknown.push(`${relative(ROOT, path)}: ${m[1]}`);
    }
  }
  assert.deepEqual(unknown, []);
});

test("every token resolves, and every colour token is a colour", () => {
  for (const name of tokenNames()) {
    const value = tokenValue(name);
    assert.ok(value.length > 0, name);
    if (/^--color-|^--(gray|red|orange|amber|yellow|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|pink)-/.test(name)) {
      // A "-rgb" channel set or a solid colour; translucent roles (rgb(… / a)) are not single colours.
      if (/\/\s*[\d.]+\)$/.test(value)) continue;
      const rgb = colorChannels(name);
      assert.ok(rgb.every((n) => Number.isInteger(n) && n >= 0 && n <= 255), `${name}: ${value}`);
    }
  }
});

test("the pages hold no style of their own and link the tokens before their stylesheet", () => {
  for (const path of pages) {
    const html = read(path);
    const name = relative(ROOT, path);
    assert.doesNotMatch(html, /<style[\s>]|\sstyle="/, `${name}: move the style into src/styles/`);
    const links = [...html.matchAll(/<link rel="stylesheet" href="([^"]+)"/g)].map((m) => m[1]);
    assert.ok(links.length > 0, `${name}: links no stylesheet`);
    const usesTokens = links.some((href) => withoutComments(read(join(ROOT, href))).includes("var(--"));
    if (usesTokens) assert.equal(links[0], "/src/design/tokens.css", `${name}: tokens.css comes first`);
  }
});

test("stylesheets are linked by the pages, never imported from the code", () => {
  const imports = sources.filter((path) => /^import\s+["'][^"']+\.css["'];/m.test(read(path))).map((path) => relative(ROOT, path));
  assert.deepEqual(imports, []);
});
