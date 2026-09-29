import assert from "node:assert/strict";
import test from "node:test";
import { noticeText } from "./utils/notices.ts";

test("encryption history stubs do not duplicate the chat's fixed notice", () => {
  assert.equal(noticeText("E2E_ENCRYPTED", [], () => "Alice"), null);
  assert.equal(noticeText("FUTURE_UNKNOWN_NOTICE", [], () => "Alice"), null);
});

test("identity and device notices resolve contact names", () => {
  const name = (jid: string) => jid === "123@lid" ? "Alice" : jid;
  assert.equal(noticeText("E2E_IDENTITY_CHANGED", ["123@lid"], name), "Alice's security code changed.");
  assert.equal(noticeText("DEVICE_ADDED", ["123@lid"], name), "Alice linked a new device.");
  assert.equal(noticeText("DEVICE_REMOVED", ["123@lid"], name), "Alice removed a device.");
});

test("membership and metadata notices retain actor, target and saved names", () => {
  const name = (jid: string) => ({ "1@lid": "Alice", "2@s.whatsapp.net": "Bob", "3@lid": "You" })[jid] ?? jid;
  assert.equal(noticeText("GROUP_PARTICIPANT_ADD", ["2@s.whatsapp.net"], name, "1@lid"), "Alice added Bob.");
  assert.equal(noticeText("GROUP_PARTICIPANT_REMOVE", ["2@s.whatsapp.net"], name, "1@lid"), "Alice removed Bob.");
  assert.equal(noticeText("GROUP_PARTICIPANT_LEAVE", ["3@lid"], name, "3@lid"), "You left.");
  assert.equal(noticeText("GROUP_PARTICIPANT_PROMOTE", ["3@lid"], name), "You are now an admin.");
  assert.equal(noticeText("GROUP_PARTICIPANT_DEMOTE", ["1@lid", "2@s.whatsapp.net"], name), "Alice, Bob are no longer admins.");
  assert.equal(noticeText("GROUP_CHANGE_SUBJECT", ["New name"], name, "1@lid"), 'Alice changed the group name to "New name".');
  assert.equal(noticeText("GROUP_CHANGE_ICON", [], name, "1@lid"), "Alice changed the group icon.");
  assert.equal(noticeText("GROUP_CHANGE_DESCRIPTION", [], name, "1@lid"), "Alice changed the group description.");
  assert.equal(noticeText("GROUP_CREATE", [], name, "3@lid"), "You created the group.");
  assert.equal(noticeText("INDIVIDUAL_CHANGE_NUMBER", ["1@lid", "2"], name), "Alice changed their phone number to Bob.");
});

test("join requests and group permission modes render without guessing missing state", () => {
  const name = () => "Alice";
  assert.equal(noticeText("GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST", [], name, "1@lid"), "Alice requested to join the group.");
  assert.equal(noticeText("GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST_NON_ADMIN_ADD", ["1@lid"], name), "Alice requested to join the group.");
  for (const kind of ["GROUP_CHANGE_RESTRICT", "GROUP_CHANGE_ANNOUNCE", "GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE"]) {
    assert.ok(noticeText(kind, ["on"], name));
    assert.notEqual(noticeText(kind, ["on"], name), noticeText(kind, ["off"], name));
    assert.notEqual(noticeText(kind, [], name), noticeText(kind, ["off"], name));
  }
  for (const kind of ["GROUP_MEMBER_ADD_MODE", "GROUP_MEMBER_LINK_MODE", "GROUP_MEMBER_SHARE_GROUP_HISTORY_MODE", "GROUP_CHANGE_RECENT_HISTORY_SHARING"]) {
    assert.ok(noticeText(kind, [], name));
  }
});

test("disappearing notices name duration and retain failure and keep states", () => {
  for (const [seconds, expected] of [[0, "Disappearing messages were turned off."], [3600, "Disappearing messages were set to 1 hour."], [86400, "Disappearing messages were set to 1 day."], [604800, "Disappearing messages were set to 7 days."]] as const) {
    assert.equal(noticeText("CHANGE_EPHEMERAL_SETTING", [String(seconds)], () => "Alice"), expected);
  }
  for (const params of [[], ["unknown"], ["-1"]]) {
    assert.equal(noticeText("CHANGE_EPHEMERAL_SETTING", params, () => "Alice"), "Disappearing message settings changed.");
  }
  assert.match(noticeText("EPHEMERAL_SETTING_NOT_APPLIED", [], () => "Alice")!, /could not be applied/);
  assert.match(noticeText("EPHEMERAL_KEEP_IN_CHAT", [], () => "Alice")!, /kept/);
});

test("community and lifecycle stubs render while unknown stubs stay hidden", () => {
  for (const kind of ["COMMUNITY_CREATE", "COMMUNITY_LINK_PARENT_GROUP", "COMMUNITY_LINK_SIBLING_GROUP", "COMMUNITY_LINK_SUB_GROUP", "COMMUNITY_UNLINK_PARENT_GROUP", "COMMUNITY_UNLINK_SIBLING_GROUP", "COMMUNITY_UNLINK_SUB_GROUP", "COMMUNITY_PARENT_GROUP_DELETED", "COMMUNITY_CHANGE_DESCRIPTION", "GROUP_DELETE", "GROUP_DEACTIVATED"]) {
    assert.ok(noticeText(kind, [], () => "Alice"), kind);
  }
});

test("voice and video missed calls render and silenced calls never resolve caller identity", () => {
  for (const kind of ["CALL_MISSED_VOICE", "CALL_MISSED_GROUP_VOICE"]) assert.equal(noticeText(kind, [], () => "Alice"), "Missed voice call");
  for (const kind of ["CALL_MISSED_VIDEO", "CALL_MISSED_GROUP_VIDEO"]) assert.equal(noticeText(kind, [], () => "Alice"), "Missed video call");
  for (const kind of ["SILENCED_UNKNOWN_CALLER_AUDIO", "SILENCED_UNKNOWN_CALLER_VIDEO"]) {
    let resolutions = 0;
    const text = noticeText(kind, ["private@lid"], () => { resolutions++; return "Private caller"; });
    assert.match(text!, /unknown number/);
    assert.equal(resolutions, 0);
    assert.ok(!text!.includes("private"));
  }
});
