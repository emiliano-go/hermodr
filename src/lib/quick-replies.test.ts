import assert from "node:assert/strict";
import test from "node:test";
import { filterQuickReplies, quickReplyScopeMatches, type QuickReplyScope } from "./utils/quick-replies.ts";
import type { QuickReply } from "./utils/wire";

const scope: QuickReplyScope = { account: "synthetic-a", chat: "100@s.whatsapp.net", generation: 1, requestKey: "request-1" };
const replies: QuickReply[] = [
  { id: "one", shortcut: "hours", message: "Open Monday to Friday", keywords: ["Schedule", "week"], count: 2, associated_label_ids: [] },
  { id: "two", shortcut: "hello", message: "Hola 👋", keywords: ["saludo"], count: 1, associated_label_ids: ["label-one"] },
];

test("quick reply matching uses shortcuts, text and keywords without changing catalog order or text", () => {
  const before = structuredClone(replies);
  assert.deepEqual(filterQuickReplies(replies, "  /HOURS  "), [replies[0]]);
  assert.deepEqual(filterQuickReplies(replies, "SCHEDULE Friday"), [replies[0]]);
  assert.deepEqual(filterQuickReplies(replies, "hola"), [replies[1]]);
  assert.deepEqual(filterQuickReplies(replies, "👋 saludo"), [replies[1]]);
  assert.deepEqual(filterQuickReplies(replies, " "), replies);
  assert.deepEqual(filterQuickReplies(replies, "/"), replies);
  assert.deepEqual(filterQuickReplies(replies, "unknown"), []);
  assert.deepEqual(replies, before);
});

test("quick reply data and callbacks reject every stale scope axis", () => {
  assert.equal(quickReplyScopeMatches(scope, scope.account, scope.chat, scope.generation, scope.requestKey), true);
  for (const changed of [{ account: "synthetic-b" }, { chat: "200@s.whatsapp.net" }, { generation: 2 }, { requestKey: "request-2" }]) {
    const current = { ...scope, ...changed };
    assert.equal(quickReplyScopeMatches(scope, current.account, current.chat, current.generation, current.requestKey), false);
  }
  assert.equal(quickReplyScopeMatches(null, scope.account, scope.chat, scope.generation, scope.requestKey), false);
  assert.equal(quickReplyScopeMatches(scope, null, scope.chat, scope.generation, scope.requestKey), false);
  assert.equal(quickReplyScopeMatches(scope, "", scope.chat, scope.generation, scope.requestKey), false);
  assert.equal(quickReplyScopeMatches(scope, scope.account, "", scope.generation, scope.requestKey), false);
  assert.equal(quickReplyScopeMatches({ ...scope, requestKey: 1 }, scope.account, scope.chat, scope.generation, "1"), false);
});
