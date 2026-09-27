import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import type { StoredMessage } from "../models.ts";

test("image menu exports originals by message ID and protects deleted and view-once media", async () => {
  const calls: { command: string; args: unknown }[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    addEventListener() {},
    __TAURI_INTERNALS__: { invoke: async (command: string, args: unknown) => { calls.push({ command, args }); } },
  } });
  Object.defineProperty(globalThis, "document", { configurable: true, value: { addEventListener() {} } });
  const server = await createServer({ server: { middlewareMode: true } });
  try {
    const { menuItems } = await server.ssrLoadModule("/src/lib/state/message-actions.ts");
    const message = { chat: "123@s.whatsapp.net", id: "photo", from_me: false,
      media_kind: "image", media_path: null, text: "Caption", revoked: false } as StoredMessage;
    const items = menuItems(message, async () => {});
    for (const [label, action] of [["Copy Image", "copy_image"], ["Save Image…", "save"], ["Open Image", "open"]]) {
      await items.find((item: { label: string }) => item.label === label).action();
      assert.deepEqual(calls.pop(), { command: "message_media_action",
        args: { chat: message.chat, id: message.id, action } });
    }
    assert.ok(items.some((item: { label: string }) => item.label === "Copy"));
    for (const changed of [{ revoked: true }, { media_kind: "view_once" }, { media_kind: null }]) {
      const labels = menuItems({ ...message, ...changed }, async () => {}).map((item: { label: string }) => item.label);
      assert.ok(!labels.some((label: string) => /Image|Attachment/.test(label)));
    }
    const labels = menuItems({ ...message, media_kind: "video" }, async () => {}).map((item: { label: string }) => item.label);
    assert.ok(labels.includes("Save Attachment…"));
    assert.ok(labels.includes("Open Attachment"));
    assert.ok(!labels.includes("Copy Image"));
  } finally {
    await server.close();
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "document");
  }
});
