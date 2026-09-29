import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import type { StoredMessage } from "../models.ts";

type Item = { label: string; action: () => unknown };

/** Loads the state modules behind just enough of a window for the IPC layer. */
async function withApp(run: (app: {
  menuItems: (m: StoredMessage, openChat: (chat: string) => Promise<void>) => Item[];
  deleteSelected: (everyone: boolean) => Promise<void>;
  canDeletePickedForEveryone: () => boolean;
  messages: {
    messages: StoredMessage[];
    marks: { reactions: { target: string; sender: string; emoji: string }[] };
    reactorsFor: Map<string, { emoji: string; senders: string[] }[]>;
  };
  ui: {
    reactionsFor: StoredMessage | null;
    removeMember: { chat: string; jid: string; name: string } | null;
    picking: Record<string, true> | null;
    bulkDelete: string[] | null;
  };
  members: { participants: { jid: string; name: string; admin: boolean; owner: boolean; number: string | null; username: string | null; label: string | null }[] };
  session: { me: string | null };
  composer: { editing: { chat: string; id: string; original: string } | null; startEditing: (m?: StoredMessage) => void };
  chats: { selectedChat: string | null };
  calls: { command: string; args: unknown }[];
}) => Promise<void>) {
  const calls: { command: string; args: unknown }[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    addEventListener() {},
    __TAURI_INTERNALS__: { invoke: async (command: string, args: unknown) => { calls.push({ command, args }); } },
  } });
  Object.defineProperty(globalThis, "document", { configurable: true, value: { addEventListener() {} } });
  const server = await createServer({ cacheDir: fileURLToPath(new URL("../../../node_modules/.vite-tests/message-actions", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const messageActions = await server.ssrLoadModule("/src/lib/state/message-actions.ts");
    const { menuItems } = messageActions;
    const { messages } = await server.ssrLoadModule("/src/lib/state/messages.svelte.ts");
    const { ui } = await server.ssrLoadModule("/src/lib/state/ui.svelte.ts");
    const { members } = await server.ssrLoadModule("/src/lib/state/members.svelte.ts");
    const { session } = await server.ssrLoadModule("/src/lib/state/session.svelte.ts");
    const { composer } = await server.ssrLoadModule("/src/lib/state/composer.svelte.ts");
    const { chats } = await server.ssrLoadModule("/src/lib/state/chats.svelte.ts");
    await run({
      menuItems,
      deleteSelected: messageActions.deleteSelected,
      canDeletePickedForEveryone: messageActions.canDeletePickedForEveryone,
      messages, ui, members, session, composer, chats, calls,
    });
  } finally {
    await server.close();
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "document");
  }
}

test("image menu exports originals by message ID and protects deleted and view-once media", async () => {
  await withApp(async ({ menuItems, calls }) => {
    const message = { chat: "123@s.whatsapp.net", id: "photo", from_me: false,
      media_kind: "image", media_path: null, text: "Caption", revoked: false } as StoredMessage;
    const items = menuItems(message, async () => {});
    for (const [label, action] of [["Copy Image", "copy_image"], ["Save Image…", "save"], ["Open Image", "open"]]) {
      await items.find((item: { label: string }) => item.label === label)!.action();
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
  });
});

test("a message somebody reacted to offers its reactor list, and one without does not", async () => {
  await withApp(async ({ menuItems, messages, ui }) => {
    const message = { chat: "123@s.whatsapp.net", id: "hello", from_me: false,
      text: "Hi", revoked: false } as StoredMessage;
    assert.ok(!menuItems(message, async () => {}).some((item) => item.label === "Reactions"));
    messages.marks = { ...messages.marks, reactions: [
      { target: "elsewhere", sender: "59897504482@s.whatsapp.net", emoji: "👍" },
      { target: "hello", sender: "59897504482@s.whatsapp.net", emoji: "👍" },
      { target: "hello", sender: "@me", emoji: "❤️" },
    ] };
    const items = menuItems(message, async () => {});
    assert.equal(items[0].label, "Reactions");
    items[0].action();
    assert.equal(ui.reactionsFor, message);
    // Emoji groups keep the order they arrived in, and ours leads its own.
    assert.deepEqual(messages.reactorsFor.get("hello"), [
      { emoji: "👍", senders: ["59897504482@s.whatsapp.net"] },
      { emoji: "❤️", senders: ["@me"] },
    ]);
  });
});

test("a group admin can remove a member from their message, but not the owner", async () => {
  await withApp(async ({ menuItems, members, session, ui }) => {
    session.me = "59897504482@s.whatsapp.net";
    const member = (jid: string, name: string, admin: boolean, owner: boolean) => ({
      jid, name, admin, owner, number: jid.split("@")[0], username: null, label: null,
    });
    members.participants = [
      member(session.me, "Me", true, false),
      member("111@s.whatsapp.net", "Ana", false, false),
      member("222@s.whatsapp.net", "Owner", true, true),
    ];
    const from = (sender: string) => ({ chat: "99@g.us", id: "x", sender, from_me: false,
      text: "hello", revoked: false } as StoredMessage);
    const remove = menuItems(from("111@s.whatsapp.net"), async () => {})
      .find((item) => item.label.startsWith("Remove "));
    assert.equal(remove?.label, "Remove Ana from group");
    remove!.action();
    assert.deepEqual(ui.removeMember, { chat: "99@g.us", jid: "111@s.whatsapp.net", name: "Ana" });
    const onOwner = menuItems(from("222@s.whatsapp.net"), async () => {});
    assert.ok(!onOwner.some((item) => item.label.startsWith("Remove ")));
    session.me = null;
    const notAdmin = menuItems(from("111@s.whatsapp.net"), async () => {});
    assert.ok(!notAdmin.some((item) => item.label.startsWith("Remove ")));
  });
});

test("Edit targets the picked message, and Ctrl+Up the newest own one", async () => {
  await withApp(async ({ menuItems, messages, composer, chats }) => {
    chats.selectedChat = "99@g.us";
    const own = (id: string, text: string, media_kind: string | null = null) => ({
      chat: "99@g.us", id, sender: "111@s.whatsapp.net", from_me: true,
      media_kind, text, revoked: false,
    }) as StoredMessage;
    // Newest first, as the store returns them.
    messages.messages = [own("new", "newest"), own("mid", "older"), own("photo", "caption", "image")];
    composer.startEditing();
    assert.deepEqual(composer.editing, { chat: "99@g.us", id: "new", original: "newest" });
    const edit = menuItems(own("mid", "older"), async () => {}).find((item) => item.label === "Edit");
    assert.ok(edit);
    edit.action();
    assert.deepEqual(composer.editing, { chat: "99@g.us", id: "mid", original: "older" });
    assert.ok(!menuItems(own("photo", "caption", "image"), async () => {})
      .some((item) => item.label === "Edit"));
  });
});

test("Select messages starts a bulk selection, and deleting uses the bulk command", async () => {
  await withApp(async ({ menuItems, deleteSelected, ui, chats, calls }) => {
    chats.selectedChat = "99@g.us";
    const message = { chat: "99@g.us", id: "a", sender: "111@s.whatsapp.net", from_me: false,
      text: "hi", revoked: false } as StoredMessage;
    const select = menuItems(message, async () => {})
      .find((item) => item.label === "Select messages");
    assert.ok(select);
    select.action();
    assert.deepEqual(ui.picking, { a: true });
    assert.ok(!menuItems({ ...message, revoked: true }, async () => {})
      .some((item) => item.label === "Select messages"));
    ui.picking = { a: true, b: true };
    await deleteSelected(false);
    assert.ok(calls.some((call) => call.command === "delete_messages"
      && JSON.stringify(call.args) === JSON.stringify({ chat: "99@g.us", ids: ["a", "b"], everyone: false })));
    assert.equal(ui.picking, null);
  });
});

test("bulk delete is offered for everyone only when every pick qualifies", async () => {
  await withApp(async ({ canDeletePickedForEveryone, messages, members, session, ui }) => {
    const mine = { chat: "99@g.us", id: "m", sender: "111@s.whatsapp.net", from_me: true,
      text: "x", revoked: false } as StoredMessage;
    const theirs = { chat: "99@g.us", id: "t", sender: "222@s.whatsapp.net", from_me: false,
      text: "y", revoked: false } as StoredMessage;
    messages.messages = [mine, theirs];
    ui.picking = { m: true };
    assert.ok(canDeletePickedForEveryone());
    ui.picking = { m: true, t: true };
    assert.ok(!canDeletePickedForEveryone());
    members.participants = [{
      jid: "111@s.whatsapp.net", name: "Me", admin: true, owner: false,
      number: "111", username: null, label: null,
    }];
    session.me = "111@s.whatsapp.net";
    assert.ok(canDeletePickedForEveryone());
  });
});
