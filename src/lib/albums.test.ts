import assert from "node:assert/strict";
import test from "node:test";
import { groupAlbumRows } from "./utils/albums.ts";
import type { StoredMessage } from "./utils/wire";

function row(id: string, patch: Partial<StoredMessage> = {}): StoredMessage {
  return { id, chat: "synthetic-chat", sender: "synthetic-sender", from_me: false, media_kind: "image", timestamp: 100,
    sort_order: 0, spoiler: false, media_once_kind: null, reply_to_view_once: false, revoked: false, deleted: false,
    system_kind: null, ...patch } as StoredMessage;
}
const ids = (groups: ReturnType<typeof groupAlbumRows>) => groups.map((group) => group.messages.map((message) => message.id));

test("albums use explicit parent associations and preserve every raw row's order and identity", () => {
  const rows = [row("z", { sort_order: 4 }), row("a", { media_kind: "video", sort_order: 1 }), row("m", { timestamp: 99 })];
  const groups = groupAlbumRows(rows, () => "synthetic-parent");
  assert.deepEqual(ids(groups), [["z", "a", "m"]]);
  assert.equal(groups[0].parentId, "synthetic-parent");
  assert.equal(groups[0].prev, undefined);
  for (const [index, message] of rows.entries()) assert.equal(groups[0].messages[index], message);
  assert.deepEqual(ids(groupAlbumRows(rows, () => null)), [["z"], ["a"], ["m"]]);
  assert.deepEqual(ids(groupAlbumRows(rows, (message) => message.id === "a" ? "other-parent" : "synthetic-parent")), [["z"], ["a"], ["m"]]);
  assert.deepEqual(ids(groupAlbumRows(rows, () => " ")), [["z"], ["a"], ["m"]]);
  assert.deepEqual(ids(groupAlbumRows(rows, (message) => message.id)), [["z"], ["a"], ["m"]]);
});

test("albums never cross chat, sender, direction or explicit day/unread boundaries", () => {
  for (const patch of [{ chat: "other-chat" }, { sender: "other-sender" }, { from_me: true }]) {
    assert.deepEqual(ids(groupAlbumRows([row("one"), row("two", patch), row("three")], () => "parent")), [["one"], ["two"], ["three"]]);
  }
  const rows = [row("one"), row("two"), row("three"), row("four")];
  const groups = groupAlbumRows(rows, () => "parent", { breakBefore: new Set(["three"]) });
  assert.deepEqual(ids(groups), [["one", "two"], ["three", "four"]]);
  assert.equal(groups[1].prev, rows[1]);
});

test("hidden and privacy-sensitive children split albums without leaking rows or invoking association extraction", () => {
  const privacy: Partial<StoredMessage>[] = [{ spoiler: true }, { media_kind: "view_once" }, { media_once_kind: "image" },
    { reply_to_view_once: true }, { revoked: true }, { deleted: true }, { system_kind: "UNAVAILABLE_MESSAGE" },
    { system_kind: "E2E_IDENTITY_CHANGED" }, { media_kind: "album" }, { media_kind: "document" }, { media_kind: "gif" }];
  for (const patch of privacy) {
    const extracted: string[] = [];
    const groups = groupAlbumRows([row("one"), row("private", patch), row("three")], (message) => { extracted.push(message.id); return "parent"; });
    assert.deepEqual(ids(groups), [["one"], ["private"], ["three"]]);
    assert.deepEqual(extracted, ["one", "three"]);
    assert.equal(groups[1].parentId, null);
  }
  const rows = [row("one"), row("hidden"), row("three")];
  const groups = groupAlbumRows(rows, () => "parent", { visible: (message) => message.id !== "hidden" });
  assert.deepEqual(ids(groups), [["one"], ["three"]]);
  assert.equal(groups[1].prev, rows[0]);
  assert.doesNotMatch(JSON.stringify(groups), /hidden/);
});

test("partial loaded albums remain usable through prepend, eviction and child patches without changing source rows", () => {
  const rows = [row("one"), row("two"), row("three"), row("four")];
  const before = structuredClone(rows);
  assert.deepEqual(ids(groupAlbumRows(rows.slice(1, 3), () => "parent")), [["two", "three"]]);
  assert.deepEqual(ids(groupAlbumRows(rows.slice(0, 3), () => "parent")), [["one", "two", "three"]]);
  assert.deepEqual(ids(groupAlbumRows(rows.slice(2), () => "parent")), [["three", "four"]]);
  const patched = { ...rows[1], media_path: "synthetic-only.png", status: "read" };
  const groups = groupAlbumRows([rows[0], patched, rows[2]], () => "parent");
  assert.equal(groups[0].messages[1], patched);
  assert.deepEqual(rows, before);
  assert.deepEqual(groupAlbumRows([], () => "parent"), []);
});
