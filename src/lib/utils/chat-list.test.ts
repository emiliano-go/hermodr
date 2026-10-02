import assert from "node:assert/strict";
import test from "node:test";
import { mergeSummaries } from "./chat-list.ts";
import type { ChatSummary } from "./models.ts";

const summary = (chat: string, over: Partial<ChatSummary> = {}): ChatSummary => ({
  chat,
  display_name: null,
  last_message_at: 1,
  last_text: "hi",
  last_from_me: false,
  last_sender_name: null,
  last_sender: "1@s",
  last_media_kind: null,
  message_count: 1,
  unread_count: 0,
  mention_count: 0,
  pinned: false,
  archived: false,
  muted_until: 0,
  mute_at_all: false,
  marked_unread: false,
  ...over,
});

test("unchanged chats keep their object, changed and new ones are replaced", () => {
  const first = summary("a@s");
  const second = summary("b@s");
  const merged = mergeSummaries(
    [first, second],
    [summary("a@s"), summary("b@s", { unread_count: 3 }), summary("c@s")],
  );
  assert.equal(merged[0], first, "an unchanged row keeps its identity");
  assert.notEqual(merged[1], second, "a changed row is a new object");
  assert.equal(merged[1].unread_count, 3);
  assert.equal(merged[2].chat, "c@s");
});
