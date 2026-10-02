import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";

test("notification delivery stays account-bound through permission and sound awaits", async () => {
  const source = readFileSync(new URL("../lib/utils/notifications.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText;
  const compiled = js.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const calls: { command: string; args: unknown }[] = [];
  const shown: { title: string; options: unknown }[] = [];
  let permission: () => Promise<boolean> = async () => true;
  let invokeImpl: (command: string, args: unknown) => unknown = async () => undefined;
  class FakeNotification {
    static permission = "granted";
    onclick: (() => void) | null = null;
    readonly title: string;
    readonly options: unknown;
    constructor(title: string, options: unknown) {
      this.title = title;
      this.options = options;
      shown.push({ title, options });
    }
  }
  const windowStub = { focus() {}, dispatchEvent() {} };
  const exports = new Function(
    "isPermissionGranted", "requestPermission", "sendNotification", "invoke", "plain",
    "MEDIA_LABELS", "captionOf", "Notification", "window", "CustomEvent",
    `${compiled}\nreturn { showChatNotification };`,
  )(
    () => permission(), async () => true, async () => {},
    (command: string, args: unknown) => { calls.push({ command, args }); return invokeImpl(command, args); },
    (text: string) => text, {}, () => "", FakeNotification, windowStub, class {},
  ) as { showChatNotification: (title: string, body: string, chat: string, accountId: string, current: () => boolean) => Promise<void> };

  let current = true;
  let resolvePermission!: (granted: boolean) => void;
  permission = () => new Promise((resolve) => { resolvePermission = resolve; });
  const staleNative = exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  current = false;
  resolvePermission(true);
  await staleNative;
  assert.deepEqual(calls, []);

  current = true;
  permission = async () => true;
  invokeImpl = async () => undefined;
  await exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  assert.deepEqual(calls, [{
    command: "show_chat_notification",
    args: { accountId: "account-a", chat: "chat", title: "Title", body: "Body" },
  }]);

  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return true;
  };
  await exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  assert.deepEqual(shown[0], { title: "Title", options: { body: "Body", tag: "postal-chat", silent: true } });
  assert.deepEqual(calls.slice(-1)[0], { command: "chat_sound_muted", args: { accountId: "account-a", chat: "chat" } });

  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return null;
  };
  await exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  assert.deepEqual(shown[1], { title: "Title", options: { body: "Body", tag: "postal-chat", silent: false } });

  let resolveSound!: (muted: boolean) => void;
  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return new Promise((resolve) => { resolveSound = resolve; });
  };
  const shownBeforeStale = shown.length;
  const staleWeb = exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  await new Promise<void>((resolve) => setImmediate(resolve));
  current = false;
  resolveSound(true);
  await staleWeb;
  assert.equal(shown.length, shownBeforeStale);

  current = true;
  permission = async () => false;
  const callsBeforeDenied = calls.length;
  await exports.showChatNotification("Title", "Body", "chat", "account-a", () => current);
  assert.equal(calls.length, callsBeforeDenied);
});
