import assert from "node:assert/strict";
import test from "node:test";
import { isPollNotice, structuredNoticeText } from "../lib/utils/structured-notices.ts";

const message = { media_kind: null as string | null, system_kind: null as string | null, system_params: [] as string[], text: "", sender: "100@lid", from_me: false };
const name = (jid: string) => jid === "100@lid" ? "Alice" : jid;

test("poll lines preserve raw question and resolve only their author", () => {
  const poll = { ...message, media_kind: "poll", text: "Dinner @ 6?" };
  assert.equal(structuredNoticeText(poll, name), 'Alice created a poll "Dinner @ 6?".');
  assert.equal(structuredNoticeText({ ...poll, from_me: true }, name), 'You created a poll "Dinner @ 6?".');
  assert.equal(structuredNoticeText({ ...poll, sender: "", text: "" }, name), "A poll was created.");
  assert.equal(structuredNoticeText({ ...poll, media_kind: "event" }, name), null);
  for (const flag of ["spoiler", "revoked", "deleted"]) {
    assert.equal(structuredNoticeText({ ...poll, [flag]: true }, name), null);
    assert.equal(isPollNotice({ ...poll, [flag]: true }), false);
  }
  assert.equal(isPollNotice(poll), true);
  assert.equal(isPollNotice({ ...message, system_kind: "CHAT_POLL_CREATION_MESSAGE" }), true);
  assert.equal(structuredNoticeText({ ...message, system_kind: "CHAT_POLL_CREATION_MESSAGE", system_params: ["unknown payload"] }, name), "Alice created a poll.");
});

test("event update and cancellation lines retain author and raw event title", () => {
  for (const [kind, verb] of [["EVENT_UPDATED", "updated"], ["EVENT_CANCELED", "canceled"]]) {
    const event = { ...message, system_kind: kind, text: "Meet @ home", system_params: ["event-id"] };
    assert.equal(structuredNoticeText(event, name), `Alice ${verb} the event "Meet @ home".`);
    assert.equal(structuredNoticeText({ ...event, from_me: true }, name), `You ${verb} the event "Meet @ home".`);
    assert.equal(structuredNoticeText({ ...event, sender: "", text: "", system_params: ["uninterpreted history payload"] }, name), `The event was ${verb}.`);
  }
});

test("scheduled call lines show only valid timestamps and known call type", () => {
  const at = 2_000_000_000;
  const call = { ...message, system_kind: "SCHEDULED_CALL_CREATED", system_params: ["Sync", String(at), "video"] };
  assert.equal(structuredNoticeText(call, name), `Alice scheduled a video call "Sync" for ${new Date(at * 1000).toLocaleString()}.`);
  assert.equal(structuredNoticeText({ ...call, from_me: true, system_params: ["", "", "voice"] }, name), "You scheduled a voice call.");
  for (const timestamp of ["", "bad", "0", "-1", "9999999999999999"]) {
    assert.equal(structuredNoticeText({ ...call, sender: "", system_params: ["", timestamp, "unknown"] }, name), "A call was scheduled.");
  }
  assert.equal(structuredNoticeText({ ...message, system_kind: "SCHEDULED_CALL_CANCEL" }, name), "Alice canceled a scheduled call.");
  assert.equal(structuredNoticeText({ ...message, system_kind: "SCHEDULED_CALL_CANCEL", sender: "" }, name), "A scheduled call was canceled.");
  assert.equal(structuredNoticeText({ ...message, system_kind: "SCHEDULED_CALL_START_MESSAGE", system_params: ["uninterpreted history payload"] }, name), "A scheduled call started.");
});

test("existing notices retain their established rendering and silent stubs", () => {
  assert.equal(structuredNoticeText({ ...message, system_kind: "GROUP_CHANGE_SUBJECT", system_params: ["Friends"] }, name), 'Alice changed the group name to "Friends".');
  assert.equal(structuredNoticeText({ ...message, system_kind: "CALL_MISSED_VIDEO" }, name), "Missed video call");
  assert.equal(structuredNoticeText({ ...message, system_kind: "E2E_ENCRYPTED" }, name), null);
  assert.equal(structuredNoticeText({ ...message, system_kind: "UNKNOWN_FUTURE" }, name), null);
});
