import { test } from "node:test";
import assert from "node:assert/strict";
import { cyclicAt, itemAt, lastOf } from "../src/core/arrays.ts";

test("itemAt returns the element at a bounded index", () => {
  assert.equal(itemAt(["a", "b"], 1), "b");
});

test("itemAt throws on a broken invariant instead of returning undefined", () => {
  assert.throws(() => itemAt(["a"], 1), RangeError);
  assert.throws(() => itemAt(["a"], -1), RangeError);
  assert.throws(() => itemAt([], 0), RangeError);
});

test("lastOf is the last element, and throws on an empty array", () => {
  assert.equal(lastOf([1, 2, 3]), 3);
  assert.throws(() => lastOf([]), RangeError);
});

test("cyclicAt wraps both ways, like the palettes and the pill carousel need", () => {
  const ids = ["x", "y", "z"];
  assert.equal(cyclicAt(ids, 3), "x");
  assert.equal(cyclicAt(ids, -1), "z");
  assert.equal(cyclicAt(ids, -4), "z");
  assert.equal(cyclicAt(ids, 7), "y");
});
