import assert from "node:assert/strict";
import test from "node:test";
import { MessageWindow } from "./message-window.ts";
import type { StoredMessage } from "./models.ts";

const row = (id: string, timestamp: number): StoredMessage =>
  ({ chat: "a@s", id, timestamp, sort_order: 0 }) as StoredMessage;

test("insert keeps newest-first order, replaces duplicates and trims the window", () => {
  const window = new MessageWindow(50);
  window.replace([row("b", 2), row("a", 1)]);
  window.insert(row("c", 3));
  assert.deepEqual(
    window.loaded.map((m) => m.id),
    ["c", "b", "a"],
  );
  window.insert(row("b", 2));
  assert.equal(window.loaded.filter((m) => m.id === "b").length, 1, "a duplicate replaces");
  window.replace(Array.from({ length: 50 }, (_, i) => row(`m${i}`, i)));
  window.insert(row("new", 100));
  assert.equal(window.loaded.length, 50, "the window stays at its limit");
  assert.equal(window.loaded[0].id, "new");
  assert.equal(window.loaded.at(-1)!.id, "m1", "the oldest loaded row makes room");
});

test("patch replaces one row and keeps every other object", () => {
  const window = new MessageWindow(50);
  const first = row("a", 1);
  const second = row("b", 2);
  window.replace([second, first]);
  const patched = row("a", 1);
  window.patch(patched);
  assert.equal(window.loaded[0], second, "the untouched row keeps its identity");
  assert.equal(window.loaded[1], patched);
  const before = window.loaded;
  window.patch(row("missing", 9));
  assert.equal(window.loaded, before, "an unknown row changes nothing");
});
