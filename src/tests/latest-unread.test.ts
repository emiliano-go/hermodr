import assert from "node:assert/strict";
import test from "node:test";
import { latestUnreadCount } from "../lib/utils/latest-unread.ts";
import type { StoredMessage } from "../lib/utils/wire.ts";

test("Latest count combines the visible boundary and unread summary without double counting", () => {
  const base = { chat: "fixture@s", read: false, from_me: false, deleted: false, revoked: false, system_kind: null };
  const rows = [{ ...base, id: "above" }, { ...base, id: "boundary" }, { ...base, id: "new" },
    { ...base, id: "own", from_me: true }, { ...base, id: "read", read: true },
    { ...base, id: "unavailable", system_kind: "UNAVAILABLE_MESSAGE" }] as StoredMessage[];
  assert.equal(latestUnreadCount(rows, "boundary", 0), 1);
  assert.equal(latestUnreadCount(rows, "boundary", 1), 1);
  assert.equal(latestUnreadCount(rows, "boundary", 7), 7);
  assert.equal(latestUnreadCount([...rows, { ...rows[2], id: "next" }], "boundary", 0), 2);
  assert.equal(latestUnreadCount(rows, "new", 0), 0);
  assert.equal(latestUnreadCount([], null, 0), 0);
  assert.equal(latestUnreadCount(rows, "boundary", NaN), 1);
});
