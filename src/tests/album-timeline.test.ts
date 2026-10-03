import { test } from "node:test";
import assert from "node:assert/strict";
import { albumTimeline, visibleReadFrontier } from "../lib/utils/album-timeline.ts";
import type { StoredMessage } from "../lib/utils/wire";

const day = (timestamp: number) => String(Math.floor(timestamp / 86400));
const row = (id: string, parent?: string, extra: Partial<StoredMessage> = {}) => ({
  id, chat: "1@s", sender: "1@s", from_me: false, timestamp: 20, text: id,
  media_kind: parent ? "image" : "album", album: { parent_id: parent ?? null }, ...extra,
} as StoredMessage);

test("album envelopes share time while each child retains identity, captions, unread and parent jump", () => {
  const parent = row("parent", undefined, { timestamp: 19 });
  const children = [row("a", "parent"), row("b", "parent", { text: "second caption" })];
  const groups = albumTimeline([parent, ...children], "parent", () => false, day);
  assert.equal(groups.length, 1);
  assert.equal(groups[0].timestamp, 19);
  assert.deepEqual(groups[0].beforeIds, ["parent"]);
  assert.deepEqual(groups[0].afterIds, []);
  assert.equal(groups[0].unreadId, "a");
  assert.deepEqual(groups[0].messages, children);
  const split = albumTimeline([parent, ...children], "b", () => false, day);
  assert.deepEqual(split.map((group) => group.messages.map((child) => child.id)), [["a"], ["b"]]);
  assert.equal(split.flatMap((group) => [...group.beforeIds, ...group.afterIds]).filter((id) => id === "parent").length, 1);
  assert.equal(split[1].unreadId, "b");
});

test("hidden, private, foreign and day boundaries preserve raw paging and never invent album contents", () => {
  const parent = row("parent");
  const input = [parent, row("a", "parent"), row("hidden", "parent"), row("b", "parent"), row("c", "parent", { timestamp: 86420 })];
  const snapshot = structuredClone(input);
  const groups = albumTimeline(input, null, (message) => message.id === "hidden", day);
  assert.deepEqual(groups.map((group) => group.messages.map((message) => message.id)), [["a"], ["b"], ["c"]]);
  assert.deepEqual(input, snapshot);
  const privateRows = albumTimeline([row("parent", undefined, { spoiler: true }), row("a", "parent"), row("b", "parent")], null, () => false, day);
  assert.equal(privateRows.length, 3);
  assert.ok(privateRows.every((group) => group.parentId === null));
  const foreign = albumTimeline([row("parent", undefined, { sender: "other@s" }), row("a", "parent"), row("b", "parent")], null, () => false, day);
  assert.equal(foreign.length, 3);
  const wrongParent = albumTimeline([row("parent", undefined, { media_kind: null }), row("a", "parent"), row("b", "parent")], null, () => false, day);
  assert.equal(wrongParent.length, 3);
  const partial = albumTimeline([row("a", "missing"), row("b", "missing")], null, () => false, day);
  assert.equal(partial.length, 1);
  assert.deepEqual(partial[0].beforeIds, []);
  assert.deepEqual(partial[0].afterIds, []);
  assert.equal(partial[0].timestamp, 20);
});

test("an interleaved public envelope does not split consecutive associated children", () => {
  const parent = row("parent", undefined, { sort_order: 2 });
  const children = [row("a", "parent", { sort_order: 1 }), row("b", "parent", { sort_order: 3 })];
  const input = [children[0], parent, children[1]], before = structuredClone(input);
  const groups = albumTimeline(input, null, () => false, day);
  assert.equal(groups.length, 1);
  assert.deepEqual(groups[0].messages, children);
  assert.deepEqual(groups[0].beforeIds, []);
  assert.deepEqual(groups[0].afterIds, ["parent"]);
  assert.deepEqual(input, before);
});

test("late parent markers retain raw chronology through split groups, unrelated rows and paging", () => {
  const parent = row("parent", undefined, { sort_order: 3 });
  const a = row("a", "parent", { sort_order: 1 }), b = row("b", "parent", { sort_order: 2 });
  const late = albumTimeline([a, b, parent], null, () => false, day);
  assert.deepEqual(late[0].beforeIds, []);
  assert.deepEqual(late[0].afterIds, ["parent"]);
  const unread = albumTimeline([a, b, parent], "b", () => false, day);
  assert.deepEqual(unread.map((group) => group.messages.map((message) => message.id)), [["a"], ["b"]]);
  assert.deepEqual(unread[0].afterIds, []);
  assert.deepEqual(unread[1].afterIds, ["parent"]);
  const ordinary = row("ordinary", undefined, { media_kind: null });
  const afterOrdinary = albumTimeline([a, b, ordinary, parent], null, () => false, day);
  assert.deepEqual(afterOrdinary[0].afterIds, []);
  assert.deepEqual(afterOrdinary[1].afterIds, ["parent"]);
  const beforeOrdinary = albumTimeline([a, b, parent, ordinary], null, () => false, day);
  assert.deepEqual(beforeOrdinary[1].beforeIds, ["parent"]);
  const hidden = row("hidden", "parent");
  const split = albumTimeline([a, parent, hidden, b], null, (message) => message.id === "hidden", day);
  assert.equal(split.length, 2);
  assert.deepEqual(split[1].beforeIds, ["parent"]);
  const paged = albumTimeline([b, parent], null, () => false, day);
  assert.deepEqual(paged[0].afterIds, ["parent"]);
  assert.deepEqual(albumTimeline([a, b], null, () => false, day)[0].afterIds, []);
});

test("read frontier uses highest raw visible row independent of DOM column order and envelope suppression", () => {
  const rows = [row("a", "parent", { sort_order: 1 }), row("b", "parent", { sort_order: 2 }), row("parent", undefined, { sort_order: 3 })];
  const before = structuredClone(rows);
  assert.equal(visibleReadFrontier(rows, ["b", "a"]), "b");
  assert.equal(visibleReadFrontier(rows, ["parent", "a", "b"]), "parent");
  assert.equal(visibleReadFrontier(rows, ["unknown"]), null);
  assert.equal(visibleReadFrontier(rows, []), null);
  assert.equal(visibleReadFrontier(rows.slice(1), ["b", "parent"]), "parent");
  assert.deepEqual(rows, before);
});

test("read frontier advances through skipped virtual rows without crossing below the fold", () => {
  const messages = Array.from({ length: 400 }, (_, index) => ({ id: `message-${index}` }));
  assert.equal(visibleReadFrontier(messages, ["message-395", "message-399"]), "message-399");
  assert.equal(visibleReadFrontier(messages, ["message-195", "message-199"]), "message-199");
  assert.equal(visibleReadFrontier(messages, ["message-100"]), "message-100");
});
