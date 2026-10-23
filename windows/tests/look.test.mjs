// Mochi's faces: stored preference → face, the wardrobe switch, mouths and brows
// (src/mochi/look.ts). Mirrors NotchBuddy/Sources/CoucouKit/MochiLook.swift.

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_LOOK, LOOK_LABELS, MOCHI_LOOKS, browPose, easeBrows, isCharacterLook, mouthForEyes,
  nextLook, parseLook,
} from "../src/mochi/look.ts";
import { DEFAULT_SETTINGS } from "../src/core/state.ts";

test("the raw values are the Mac's, in the wardrobe's order", () => {
  assert.deepEqual([...MOCHI_LOOKS], ["signature", "portrait", "slime", "sage", "classic"]);
  for (const look of MOCHI_LOOKS) assert.ok(LOOK_LABELS[look].length > 0, look);
});

test("Mochi wears the portrait unless told otherwise", () => {
  assert.equal(DEFAULT_LOOK, "portrait");
  assert.equal(DEFAULT_SETTINGS.mochiLook, "portrait");
  assert.equal(parseLook(undefined), "portrait");
  assert.equal(parseLook("cyborg"), "portrait"); // a value from another build
  assert.equal(parseLook(42), "portrait");
  for (const look of MOCHI_LOOKS) assert.equal(parseLook(look), look);
});

test("the Face switch goes round every face and comes back", () => {
  let look = "signature";
  const seen = [];
  for (let i = 0; i < MOCHI_LOOKS.length; i++) {
    seen.push(look);
    look = nextLook(look);
  }
  assert.deepEqual(seen, [...MOCHI_LOOKS]);
  assert.equal(look, "signature");
});

test("only the portrait and the slime have a character's brows and teeth", () => {
  assert.deepEqual(MOCHI_LOOKS.filter(isCharacterLook), ["portrait", "slime"]);
});

test("the mouth follows the eyes", () => {
  assert.equal(mouthForEyes("pill", false), "cat");
  assert.equal(mouthForEyes("wide", false), "oh");
  assert.equal(mouthForEyes("dot", false), "gasp");
  assert.equal(mouthForEyes("happy", false), "smile");
  assert.equal(mouthForEyes("wink", false), "smirk");
  assert.equal(mouthForEyes("tired", false), "wavy");
  assert.equal(mouthForEyes("closed", false), "content");
  assert.equal(mouthForEyes("cup", false), "none"); // the mailbox has its own slot
  assert.equal(mouthForEyes("pill", true), "talking");
});

test("cross, a character grits his teeth; the others just press their lips", () => {
  assert.equal(mouthForEyes("line", false), "flat");
  assert.equal(mouthForEyes("line", false, true), "grit");
  assert.equal(mouthForEyes("flat", false, true), "grit");
});

test("the brows: cross on an error, worried when rate limited, one up while thinking", () => {
  assert.ok(browPose("error", "pill", 0).angle > 0.3);
  assert.ok(browPose("ratelimit", "tired", 0).angle < -0.3);
  assert.ok(browPose("thinking", "pill", 0).asym > 0);
  assert.ok(browPose("approval", "wide", 0).lift > browPose("idle", "pill", 0).lift);
  // An emote's eyes win over the state
  assert.deepEqual(browPose("idle", "line", 0), browPose("error", "line", 0));
});

test("the brows ease towards their pose", () => {
  const from = browPose("idle", "pill", 0);
  const to = browPose("error", "pill", 0);
  assert.deepEqual(easeBrows(from, to, 0), from);
  assert.deepEqual(easeBrows(from, to, 1), to);
  const half = easeBrows(from, to, 0.5);
  assert.ok(half.angle > from.angle && half.angle < to.angle);
});
