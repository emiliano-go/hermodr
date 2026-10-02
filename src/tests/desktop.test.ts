import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { desktopChatTarget, desktopUnreadCount } from "../lib/utils/desktop.ts";

test("desktop unread counts include messages and explicit unread marks, excluding archived chats", () => {
  assert.equal(desktopUnreadCount([
    { unread_count: 4, marked_unread: true, archived: false },
    { unread_count: 0, marked_unread: true, archived: false },
    { unread_count: 99, marked_unread: true, archived: true },
    { unread_count: 0, marked_unread: false, archived: false },
  ]), 5);
  assert.equal(desktopUnreadCount([]), 0);
  assert.equal(desktopUnreadCount([{ unread_count: -1, marked_unread: false, archived: false }, { unread_count: NaN, marked_unread: true, archived: false }]), 1);
  assert.equal(desktopUnreadCount([{ unread_count: 0xffffffff, marked_unread: true, archived: false }, { unread_count: 5, marked_unread: false, archived: false }]), 0xffffffff);
});

test("desktop notification navigation accepts only a scoped, valid native target", () => {
  assert.deepEqual(desktopChatTarget({ account_id: "synthetic-a", chat: "chat", body: "unused" }, "synthetic-a"), { account_id: "synthetic-a", chat: "chat" });
  for (const value of [null, [], "chat", { account_id: "synthetic-b", chat: "chat" }, { account_id: "synthetic-a", chat: 1 }, { account_id: "synthetic-a", chat: "" }, { account_id: "synthetic-a", chat: "chat\n" }]) assert.equal(desktopChatTarget(value, "synthetic-a"), null);
  assert.equal(desktopChatTarget({ account_id: "synthetic-a", chat: "chat" }, null), null);
});

test("actual Web notification click keeps its account scope and rejects stale runtime callbacks", async () => {
  const source = readFileSync(new URL("../lib/utils/notifications.ts", import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const notifications: FakeNotification[] = [], events: unknown[] = [];
  let focus = 0, current = true;
  class FakeNotification {
    static permission = "granted";
    onclick: (() => void) | null = null;
    constructor(_title: string, _options: unknown) { notifications.push(this); }
  }
  class FakeEvent {
    readonly type: string; readonly options: { detail: unknown };
    constructor(type: string, options: { detail: unknown }) { this.type = type; this.options = options; }
  }
  const { showChatNotification } = new Function("isPermissionGranted", "requestPermission", "sendNotification", "invoke", "plain", "MEDIA_LABELS", "captionOf", "Notification", "window", "CustomEvent", `${compiled}\nreturn {showChatNotification};`)(
    async () => true, async () => true, async () => {},
    async (command: string) => { if (command === "show_chat_notification") throw new Error("Synthetic native unavailable"); return false; },
    (text: string) => text, {}, () => "", FakeNotification,
    { focus() { focus++; }, dispatchEvent(event: unknown) { events.push(event); } }, FakeEvent,
  ) as { showChatNotification: (title: string, body: string, chat: string, account: string, current: () => boolean) => Promise<void> };
  await showChatNotification("Synthetic title", "Synthetic body", "synthetic-chat", "synthetic-a", () => current);
  notifications[0].onclick!();
  assert.equal(focus, 1);
  assert.deepEqual(events, [new FakeEvent("postal:open-chat", { detail: { account_id: "synthetic-a", chat: "synthetic-chat" } })]);
  current = false;
  notifications[0].onclick!();
  assert.equal(focus, 1);
  assert.equal(events.length, 1);
});
