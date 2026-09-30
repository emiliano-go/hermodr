import assert from "node:assert/strict";
import test from "node:test";
import { formatBulkReadFailures } from "./utils/bulk-chats.ts";

test("bulk read failures preserve every failed chat and ignore successful manual unread clears", () => {
  const results = [
    { chat: "archived", changed: 3, error: null },
    { chat: "failed", changed: null, error: "synthetic receipt failure" },
    { chat: "manual", changed: 0, error: null },
    { chat: "second", changed: null, error: "synthetic store failure" },
  ];
  assert.deepEqual(formatBulkReadFailures(results, (chat) => `Name ${chat}`), [
    "Name failed: synthetic receipt failure", "Name second: synthetic store failure",
  ]);
  assert.deepEqual(formatBulkReadFailures([], (chat) => chat), []);
  assert.deepEqual(formatBulkReadFailures(results.filter((result) => result.error === null), (chat) => chat), []);
});
