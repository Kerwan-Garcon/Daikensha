import test from "node:test";
import assert from "node:assert/strict";
import { decide, refsFrom, refOf, CARD_SHOWN } from "./gate.mjs";

const snapshot = `
- generic [ref=e1]:
  - searchbox "Rechercher" [ref=e5]
  - button "Rechercher" [ref=e6] [cursor=pointer]
  - link "Daft Punk - One More Time (Official Video)" [ref=e9] [cursor=pointer]:
    - /url: /watch?v=daft1
  - button "Lecture" [ref=e12]
  - button "S'abonner" [ref=e13]
  - button "Ajouter au panier" [ref=e14]
  - button "Acheter maintenant" [ref=e15]
  - textbox "Mot de passe" [ref=e16]
  - button "Send" [ref=e17]
  - textbox "Écrire un message" [ref=e18]
  - combobox "Search" [ref=e19]
`;
const refs = refsFrom(snapshot);

test("a snapshot's refs carry their role and name", () => {
  assert.deepEqual(refs.get("e9"), { role: "link", name: "Daft Punk - One More Time (Official Video)" });
  assert.deepEqual(refs.get("e1"), { role: "generic", name: "" });
  assert.equal(refs.size, 12);
});

test("ordinary browsing goes through", () => {
  for (const [tool, args] of [
    ["browser_navigate", { url: "https://www.youtube.com" }],
    ["browser_type", { target: "e5", text: "lofi", submit: true }],
    ["browser_click", { target: 'link "Daft Punk" [ref=e9]' }],
    ["browser_click", { target: "e12" }],
    ["browser_click", { ref: "e14" }],
    ["browser_press_key", { key: "Enter" }],
    ["browser_snapshot", {}],
  ]) assert.deepEqual(decide(tool, args, refs), { allow: true }, tool);
});

test("buying, subscribing or sending only puts a card up", () => {
  for (const ref of ["e13", "e15", "e17"]) {
    const d = decide("browser_click", { target: ref, element: "a harmless button" }, refs);
    assert.equal(d.deny, CARD_SHOWN);
    assert.ok(d.card);
  }
});

test("the element is the page's, not the model's description of it", () => {
  // The model calls the buy button "play": the gate reads the snapshot.
  assert.ok(decide("browser_click", { target: "e15", element: "Play button" }, refs).card);
  // A selector can't be checked against the page: refused.
  for (const target of ['button:has-text("Acheter")', "input", "#buy"])
    assert.equal(refOf({ target }), null, target), assert.ok(decide("browser_click", { target }, refs).deny);
  // A ref that isn't on the page is refused.
  assert.ok(decide("browser_click", { target: "e99" }, refs).deny);
});

test("never a password, never anything but the web", () => {
  assert.ok(decide("browser_type", { target: "e16", text: "hunter2" }, refs).deny);
  for (const url of ["file:///etc/passwd", "javascript:alert(1)", "chrome://settings", "nonsense"])
    assert.ok(decide("browser_navigate", { url }, refs).deny, url);
  for (const tool of ["browser_evaluate", "browser_run_code", "browser_file_upload", "browser_install"])
    assert.ok(decide(tool, {}, refs).deny, tool);
});

test("Enter submits a search, never a message", () => {
  const state = {};
  assert.deepEqual(decide("browser_type", { target: "e19", text: "lofi", submit: true }, refs, state), { allow: true });
  assert.deepEqual(decide("browser_press_key", { key: "Enter" }, refs, state), { allow: true });
  assert.ok(decide("browser_type", { target: "e18", text: "salut", submit: true }, refs, {}).card);
  // Typed without submit, then Enter: the same card.
  assert.deepEqual(decide("browser_type", { target: "e18", text: "salut" }, refs, state), { allow: true });
  assert.ok(decide("browser_press_key", { key: "Enter" }, refs, state).card);
  // Other keys are fine.
  assert.deepEqual(decide("browser_press_key", { key: "ArrowDown" }, refs, state), { allow: true });
});
