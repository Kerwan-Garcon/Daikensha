import { test } from "node:test";
import assert from "node:assert/strict";
import { errorText, text } from "../src/core/decode.ts";

test("text keeps strings and spells out numbers and booleans", () => {
  assert.equal(text("Vercel"), "Vercel");
  assert.equal(text(42), "42");
  assert.equal(text(false), "false");
});

test("text never renders an object or an absent value", () => {
  assert.equal(text({ name: "x" }, "Untitled"), "Untitled");
  assert.equal(text(null, "—"), "—");
  assert.equal(text(undefined), "");
});

test("errorText reads Error messages and plain rejections", () => {
  assert.equal(errorText(new Error("boom")), "boom");
  assert.equal(errorText("denied"), "denied");
  assert.equal(errorText({}), "unknown error");
});
