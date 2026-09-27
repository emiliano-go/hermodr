import assert from "node:assert/strict";
import test from "node:test";
import { noticeText } from "./notices.ts";

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
