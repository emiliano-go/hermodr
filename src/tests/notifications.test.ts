import assert from "node:assert/strict";
import test from "node:test";
import {
  groupNotificationBody,
  isChatMuted,
  notificationBody,
  notificationTitle,
  shouldNotify,
} from "../lib/utils/notifications.ts";

const NOW = 1_700_000_000;

test("muted chats stay silent: forever, future, and expired mutes", () => {
  assert.equal(isChatMuted(-1, NOW), true);
  assert.equal(isChatMuted(0, NOW), false);
  assert.equal(isChatMuted(NOW + 60, NOW), true);
  assert.equal(isChatMuted(NOW - 1, NOW), false);
});

test("shouldNotify respects the global toggle, mutes, self messages and the open chat", () => {
  const base = {
    fromMe: false,
    systemKind: null,
    revoked: false,
    mutedUntil: 0,
    notificationsEnabled: true,
    fresh: true,
    isOpenChat: false,
    sentAt: NOW,
  };
  assert.equal(shouldNotify(base, NOW), true);
  assert.equal(shouldNotify({ ...base, notificationsEnabled: false }, NOW), false);
  assert.equal(shouldNotify({ ...base, mutedUntil: -1 }, NOW), false);
  assert.equal(shouldNotify({ ...base, mutedUntil: NOW + 3600 }, NOW), false);
  assert.equal(shouldNotify({ ...base, mutedUntil: NOW - 10 }, NOW), true);
  assert.equal(shouldNotify({ ...base, fromMe: true }, NOW), false);
  assert.equal(shouldNotify({ ...base, fresh: false }, NOW), false);
  assert.equal(shouldNotify({ ...base, isOpenChat: true }, NOW), false);
  assert.equal(shouldNotify({ ...base, revoked: true }, NOW), false);
  assert.equal(shouldNotify({ ...base, systemKind: "GROUP_PARTICIPANT_ADD" }, NOW), false);
});

test("catch-up replays stay silent: only recent arrivals ping", () => {
  const base = {
    fromMe: false,
    systemKind: null,
    revoked: false,
    mutedUntil: 0,
    notificationsEnabled: true,
    fresh: true,
    isOpenChat: false,
    sentAt: NOW,
  };
  assert.equal(shouldNotify({ ...base, sentAt: NOW - 301 }, NOW), false);
  assert.equal(shouldNotify({ ...base, sentAt: NOW - 300 }, NOW), true);
  // A sender's clock ahead of ours must not mute a live message.
  assert.equal(shouldNotify({ ...base, sentAt: NOW + 30 }, NOW), true);
});

test("muting @all silences @all-only mentions but keeps direct ones", () => {
  const base = {
    fromMe: false,
    systemKind: null,
    revoked: false,
    mutedUntil: 0,
    notificationsEnabled: true,
    fresh: true,
    isOpenChat: false,
    sentAt: NOW,
  };
  assert.equal(shouldNotify({ ...base, mentionedAllOnly: true, muteAtAll: true }, NOW), false);
  assert.equal(shouldNotify({ ...base, mentionedAllOnly: true, muteAtAll: false }, NOW), true);
  assert.equal(shouldNotify({ ...base, mentionedAllOnly: false, muteAtAll: true }, NOW), true);
  assert.equal(shouldNotify({ ...base, muteAtAll: true }, NOW), true);
});

test("titles name the DM contact and the group", () => {
  assert.equal(notificationTitle({ isGroup: false, chatName: "Ana", senderName: "Ana" }), "Ana");
  assert.equal(
    notificationTitle({ isGroup: true, chatName: "Family", senderName: "Ana" }),
    "Family",
  );
});

test("bodies preview text and label uncaptioned media", () => {
  assert.equal(notificationBody({ text: "hello *there*", media_kind: null }), "hello there");
  assert.equal(notificationBody({ text: "[image]", media_kind: "image" }), "Photo");
  assert.equal(notificationBody({ text: "[audio]", media_kind: "audio" }), "Audio");
  assert.equal(notificationBody({ text: "look", media_kind: "image" }), "look");
  assert.equal(notificationBody({ text: "Missed call", media_kind: "missed_call" }), "Missed call");
  assert.equal(notificationBody({ text: "", media_kind: "sticker" }), "Sticker");
  assert.equal(
    groupNotificationBody("Ana", "hello"),
    "Ana: hello",
  );
});
