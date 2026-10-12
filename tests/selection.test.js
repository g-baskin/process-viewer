// @ts-check
import test from "node:test";
import assert from "node:assert/strict";
import "../src/selection.js";

const S = globalThis.MultiSelect;

const rows = [10, 20, 30, 40, 50];
const keys = (s) => [...s.keys].sort((a, b) => a - b);

test("plain click selects one row", () => {
  const s = S.click(S.click(S.empty(), rows, 20), rows, 40);
  assert.deepEqual(keys(s), [40]);
  assert.equal(s.focus, 40);
});

test("toggle adds and removes rows", () => {
  let s = S.click(S.empty(), rows, 20);
  s = S.click(s, rows, 40, { toggle: true });
  assert.deepEqual(keys(s), [20, 40]);
  s = S.click(s, rows, 20, { toggle: true });
  assert.deepEqual(keys(s), [40]);
  assert.equal(s.focus, 40);
});

test("extend selects a range from the anchor, either direction", () => {
  let s = S.click(S.empty(), rows, 40);
  s = S.click(s, rows, 20, { extend: true });
  assert.deepEqual(keys(s), [20, 30, 40]);
  s = S.click(s, rows, 50, { extend: true });
  assert.deepEqual(keys(s), [40, 50], "a new range replaces the old one");
  assert.equal(s.anchor, 40);
});

test("toggle + extend adds a range to an existing pick", () => {
  let s = S.click(S.empty(), rows, 10);
  s = S.click(s, rows, 40, { toggle: true });
  s = S.click(s, rows, 50, { toggle: true, extend: true });
  assert.deepEqual(keys(s), [10, 40, 50]);
});

test("extend without a visible anchor falls back to a plain click", () => {
  const s = S.click({ keys: new Set([99]), focus: 99, anchor: 99 }, rows, 30, { extend: true });
  assert.deepEqual(keys(s), [30]);
});

test("step moves and extends within bounds", () => {
  let s = S.step(S.empty(), rows, 1);
  assert.deepEqual(keys(s), [10]);
  s = S.step(s, rows, 1, true);
  s = S.step(s, rows, 1, true);
  assert.deepEqual(keys(s), [10, 20, 30]);
  s = S.step(s, rows, -1);
  assert.deepEqual(keys(s), [20]);
  assert.equal(S.step(S.click(S.empty(), rows, 50), rows, 1).focus, 50);
});

test("all and prune", () => {
  let s = S.all(rows);
  assert.equal(s.keys.size, 5);
  s = S.prune(s, (k) => k !== 10 && k !== 30);
  assert.deepEqual(keys(s), [20, 40, 50]);
  assert.equal(s.focus, 50, "focus moves off a removed row");
  const same = S.prune(s, () => true);
  assert.equal(same, s, "unchanged selection is returned as is");
});
