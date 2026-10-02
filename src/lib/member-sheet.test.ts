import assert from "node:assert/strict";
import test from "node:test";
import { memberActionReason, memberBusinessHours, memberFieldText, memberFresh, memberLocalMatches, memberNoteError, memberScopeMatches, memberTyping } from "./utils/member-sheet.ts";
import type { MemberPermissions, MemberAction } from "./utils/member-sheet.ts";

const supported: readonly MemberAction[] = ["promote", "demote", "remove", "block", "unblock", "report"];
const state: MemberPermissions = { admin: true, connected: true, self: false, member: { admin: false, owner: false }, blocked: false, supported };

test("member actions require admin, current connection, supported API and eligible target", () => {
  for (const action of supported) {
    assert.ok(memberActionReason(action, { ...state, admin: false }));
    assert.ok(memberActionReason(action, { ...state, connected: false }));
    assert.ok(memberActionReason(action, { ...state, self: true }));
    assert.ok(memberActionReason(action, { ...state, supported: [] }));
    assert.ok(memberActionReason(action, { ...state, ready: false }));
  }
  assert.equal(memberActionReason("promote", state), null);
  assert.ok(memberActionReason("promote", { ...state, member: { admin: true, owner: false } }));
  assert.ok(memberActionReason("demote", state));
  assert.equal(memberActionReason("demote", { ...state, member: { admin: true, owner: false } }), null);
  for (const action of ["promote", "demote", "remove"] as const) {
    assert.ok(memberActionReason(action, { ...state, member: null }));
    assert.ok(memberActionReason(action, { ...state, member: { admin: null, owner: false } }));
    assert.ok(memberActionReason(action, { ...state, member: { admin: false, owner: null } }));
    assert.ok(memberActionReason(action, { ...state, member: { admin: true, owner: true } }));
  }
  assert.equal(memberActionReason("block", state), null);
  assert.ok(memberActionReason("unblock", state));
  assert.equal(memberActionReason("unblock", { ...state, blocked: true }), null);
  assert.ok(memberActionReason("block", { ...state, blocked: null }));
  assert.ok(memberActionReason("unblock", { ...state, blocked: null }));
});

test("live verification expires at thirty seconds and rejects missing or future timestamps", () => {
  assert.equal(memberFresh(100, 100_000), true);
  assert.equal(memberFresh(100, 129_999), true);
  assert.equal(memberFresh(100, 130_000), false);
  assert.equal(memberFresh(100, 99_999), false);
  for (const at of [null, undefined, NaN, Infinity, -Infinity]) assert.equal(memberFresh(at, 100_000), false);
});

test("typing expiry uses observed milliseconds and never keeps expired or unknown state live", () => {
  assert.equal(memberTyping({ state: "composing", expires_at_ms: 10501 }, 10000), "Typing, expires in 1s");
  assert.equal(memberTyping({ state: "recording", expires_at_ms: 20000 }, 10000), "Recording, expires in 10s");
  assert.equal(memberTyping({ state: "composing", expires_at_ms: 10000 }, 10000), null);
  assert.equal(memberTyping({ state: "paused", expires_at_ms: 20000 }, 10000), null);
  assert.equal(memberTyping({ state: "composing", expires_at_ms: NaN }, 10000), null);
  assert.equal(memberTyping(null, 10000), null);
});

test("local snapshot can follow known aliases but cannot seed another member or group notes", () => {
  const local = { jid: "200@lid", addresses: ["200@lid", "59899000000@s.whatsapp.net"], scope_chat: "100@g.us" };
  assert.equal(memberLocalMatches(local, "200@lid", "100@g.us"), true);
  assert.equal(memberLocalMatches(local, "59899000000@s.whatsapp.net", "100@g.us"), true);
  assert.equal(memberLocalMatches(local, "300@lid", "100@g.us"), false);
  assert.equal(memberLocalMatches(local, "200@lid", "200@g.us"), false);
  assert.equal(memberLocalMatches({ ...local, scope_chat: null }, "200@lid", "100@g.us"), false);
  const scope = { account: "alpha", group: "100@g.us", jid: "200@lid", requestKey: 2 };
  assert.equal(memberScopeMatches(scope, "alpha", "100@g.us", "200@lid", 2), true);
  assert.equal(memberScopeMatches(scope, "beta", "100@g.us", "200@lid", 2), false);
  assert.equal(memberScopeMatches(scope, "alpha", "200@g.us", "200@lid", 2), false);
  assert.equal(memberScopeMatches(scope, "alpha", "100@g.us", "300@lid", 2), false);
  assert.equal(memberScopeMatches(scope, "alpha", "100@g.us", "200@lid", 3), false);
});

test("live field state distinguishes denied access, query failure and retained stale values", () => {
  assert.equal(memberFieldText(null), "Unavailable");
  assert.equal(memberFieldText({ state: "available", value: 0, error: null, stale: false }), "0");
  assert.equal(memberFieldText({ state: "unavailable", value: null, error: null, stale: false }), "Not provided");
  const denied = memberFieldText({ state: "restricted", value: "must not show", error: "403", stale: false });
  assert.equal(denied, "Access denied by server."); assert.doesNotMatch(denied, /privacy|hidden|must not show/i);
  assert.equal(memberFieldText({ state: "error", value: "Saved about", error: "Query failed", stale: true }), "Cached value: Saved about. Refresh failed: Query failed");
  assert.equal(memberBusinessHours({ day: "Monday", mode: "open", open_minutes: 540, close_minutes: 1020 }), "Monday: open 09:00 to 17:00");
});

test("local note policy counts Unicode characters and matches native warning/NUL limits", () => {
  assert.equal(memberNoteError("😀".repeat(4096), 100000), "");
  assert.match(memberNoteError("😀".repeat(4097), 0), /4096 characters/);
  assert.match(memberNoteError("note\0text", 0), /NUL/);
  for (const warnings of [-1, 0.1, 100001, NaN, undefined]) assert.match(memberNoteError("note", warnings), /0 to 100000/);
  assert.equal(memberNoteError("", 0), "");
});
