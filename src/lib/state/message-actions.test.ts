import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import type { ChatSummary, ServiceEvent, StoredMessage } from "../utils/models.ts";
import type { EventHost } from "./events.ts";

type Item = { label: string; separated?: boolean; action: () => unknown };

/** Loads the state modules behind just enough of a window for the IPC layer. */
async function withApp(run: (app: {
  menuItems: (m: StoredMessage, openChat: (chat: string) => Promise<void>) => Item[];
  deleteSelected: (everyone: boolean) => Promise<void>;
  canDeletePickedForEveryone: () => boolean;
  pickedInOrder: (
    picking: Record<string, StoredMessage> | null,
    ordered: StoredMessage[],
  ) => StoredMessage[];
  forwardMessages: (batch: StoredMessage[], targets: string[]) => Promise<void>;
  viewableMessages: (ordered: StoredMessage[], viewOnce: Set<string>) => StoredMessage[];
  copyMessages: (batch: StoredMessage[]) => Promise<void>;
  starMessages: (batch: StoredMessage[], starred: boolean) => Promise<void>;
  reactMessages: (batch: StoredMessage[], emoji: string) => Promise<void>;
  loadEvents: () => Promise<{ dispatchServiceEvent: (payload: ServiceEvent, host: EventHost) => Promise<void> }>;
  messages: {
    messages: StoredMessage[];
    marks: { reactions: { target: string; sender: string; emoji: string }[] };
    reactorsFor: Map<string, { emoji: string; senders: string[] }[]>;
    acceptMessages: (rows: StoredMessage[]) => void;
    atLatest: boolean;
    setStatus: (chat: string, id: string, status: string) => void;
    append: (row: StoredMessage) => void;
    patch: (row: StoredMessage) => void;
    refreshRow: (chat: string, id: string, mayAppend: boolean) => Promise<void>;
    reloadMessages: (chat: string) => Promise<boolean>;
    prepareChat: (chat: string, limit?: number) => void;
    resetAccount: () => void;
    downloadMedia: (chat: string | null, message: StoredMessage) => Promise<void>;
    recoverQuote: (chat: string | null, message: StoredMessage) => Promise<string | null>;
    markPlayed: (message: StoredMessage) => void;
  };
  ui: {
    reactionsFor: StoredMessage | null;
    removeMember: { chat: string; jid: string; name: string } | null;
    picking: Record<string, StoredMessage> | null;
    bulkDelete: string[] | null;
    forwarding: StoredMessage[] | null;
    labelTargets: { chat: string; id?: string }[] | null;
    error: string | null;
    scrolledUp: boolean;
  };
  members: {
    participants: { jid: string; name: string; admin: boolean; owner: boolean; number: string | null; username: string | null; label: string | null }[];
    chatGroup: { admin: boolean } | null;
  };
  session: { me: string | null; activeAccount: string | null; gateDone: boolean; syncPending: number; settings: { notifications_enabled: boolean } };
  composer: { editing: { chat: string; id: string; original: string } | null; startEditing: (m?: StoredMessage) => void; resetAccount: () => void };
  chats: {
    selectedChat: string | null;
    searchQuery: string;
    searchResults: unknown[];
    runSearch: () => void;
    resetAccount: () => void;
    chats: ChatSummary[];
  };
  calls: { command: string; args: unknown }[];
}) => Promise<void>, beforeInvoke?: (command: string, args: unknown) => unknown) {
  const calls: { command: string; args: unknown }[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    addEventListener() {},
    __TAURI_INTERNALS__: { invoke: async (command: string, args: unknown) => { calls.push({ command, args }); return await beforeInvoke?.(command, args); } },
  } });
  Object.defineProperty(globalThis, "document", { configurable: true, value: { addEventListener() {} } });
  const server = await createServer({
    configFile: false,
    plugins: [svelte({ configFile: false, prebundleSvelteLibraries: false })],
    resolve: { alias: { $lib: fileURLToPath(new URL("..", import.meta.url)) } },
    cacheDir: fileURLToPath(new URL("../../../node_modules/.vite-tests/message-actions", import.meta.url)),
    optimizeDeps: { noDiscovery: true, include: [], exclude: ["svelte"] },
    ssr: { optimizeDeps: { noDiscovery: true, include: [], exclude: ["svelte"] } },
    server: { middlewareMode: true, ws: false, watch: null },
  });
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
      pickedInOrder: messageActions.pickedInOrder,
      forwardMessages: messageActions.forwardMessages,
      viewableMessages: messageActions.viewableMessages,
      copyMessages: messageActions.copyMessages,
      starMessages: messageActions.starMessages,
      reactMessages: messageActions.reactMessages,
      loadEvents: async () => {
        const events = await server.ssrLoadModule("/src/lib/state/events.ts");
        return { dispatchServiceEvent: events.dispatchServiceEvent };
      },
      messages, ui, members, session, composer, chats, calls,
    });
  } finally {
    await server.close();
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "document");
  }
}

/** Menu labels in sorted order, so assertions never pin down the sequence. */
const labels = (items: Item[]) => items.map((item) => item.label).sort();

test("unavailable rows reject content actions and recovered rows regain normal eligibility", async () => {
  await withApp(async ({ menuItems, pickedInOrder, copyMessages, starMessages, reactMessages, forwardMessages, viewableMessages, messages, composer, chats, calls }) => {
    const marker = { chat: "unavailable@s", id: "same-id", sender: "1@s", from_me: true,
      text: "PRIVATE PAYLOAD", system_kind: "UNAVAILABLE_MESSAGE", media_kind: "image", media_path: "PRIVATE FILE" } as StoredMessage;
    chats.selectedChat = marker.chat;
    messages.prepareChat(marker.chat);
    messages.acceptMessages([marker]);
    assert.deepEqual(menuItems(marker, async () => {}), []);
    assert.deepEqual(viewableMessages([marker], new Set()), []);
    assert.deepEqual(pickedInOrder({ [marker.id]: { ...marker, system_kind: null } }, [marker]), []);
    composer.startEditing(marker);
    assert.equal(composer.editing, null);
    const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    const copied: string[] = [];
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { clipboard: { writeText: async (text: string) => { copied.push(text); } } } });
    try {
      await copyMessages([marker]);
      await starMessages([marker], true);
      await reactMessages([marker], "👍");
      await forwardMessages([marker], ["target@s"]);
      await messages.downloadMedia(marker.chat, marker);
      assert.equal(await messages.recoverQuote(marker.chat, marker), null);
      messages.markPlayed(marker);
      assert.deepEqual(copied, []);
      assert.deepEqual(calls, []);
      const recovered = { ...marker, system_kind: null, media_kind: null, media_path: null, text: "Recovered message" };
      messages.acceptMessages([recovered]);
      assert.deepEqual(pickedInOrder({ [marker.id]: marker }, [recovered]), [recovered]);
      assert.ok(menuItems(recovered, async () => {}).some((item) => item.label === "Reply"));
      await copyMessages([recovered]);
      assert.deepEqual(copied, ["Recovered message"]);
    } finally {
      if (previous) Object.defineProperty(globalThis, "navigator", previous); else Reflect.deleteProperty(globalThis, "navigator");
    }
  });
});

test("unavailable hints stay within the local window, read no placeholders, and reconcile healed badges", async () => {
  const chat = "unavailable@s";
  const marker = { chat, id: "marker", sender: "1@s", timestamp: 100, from_me: false, read: false,
    text: "", system_kind: "UNAVAILABLE_MESSAGE", mentioned: false } as StoredMessage;
  let archive = [marker];
  await withApp(async ({ loadEvents, messages, chats, session, ui, calls }) => {
    const { dispatchServiceEvent } = await loadEvents();
    Object.defineProperty(globalThis, "document", { configurable: true, value: { addEventListener() {}, hasFocus: () => true } });
    const host: EventHost = { scrollToBottom() {}, anchor: () => null, reveal() {}, reconnect: async () => {} };
    const hint = (id = marker.id): ServiceEvent => ({ kind: "messageHint", chat, id, sender: "1@s", from_me: false, fresh: false, change: "content", status: null });
    const settle = () => new Promise((resolve) => setTimeout(resolve, 950));
    session.gateDone = true;
    session.activeAccount = "synthetic-unavailable";
    session.settings.notifications_enabled = false;
    chats.selectedChat = chat;
    ui.scrolledUp = false;
    messages.prepareChat(chat, 50);
    await dispatchServiceEvent(hint(), host);
    await settle();
    assert.equal(messages.messages[0].system_kind, "UNAVAILABLE_MESSAGE");
    assert.equal(chats.chats[0].unread_count, 0);
    assert.equal(calls.some((call) => call.command === "mark_read"), false);

    session.syncPending = 1;
    await dispatchServiceEvent(hint(), host);
    await dispatchServiceEvent({ kind: "synced" }, host);
    await settle();
    assert.equal(calls.some((call) => call.command === "mark_read"), false);

    archive = [{ ...marker, system_kind: null, text: "Recovered message", mentioned: true }];
    ui.scrolledUp = true;
    await dispatchServiceEvent(hint(), host);
    await settle();
    assert.equal(messages.messages[0].text, "Recovered message");
    assert.equal(chats.chats[0].unread_count, 1);
    assert.equal(chats.chats[0].mention_count, 1);
    assert.equal(calls.some((call) => call.command === "mark_read"), false);

    ui.scrolledUp = false;
    session.syncPending = 1;
    await dispatchServiceEvent(hint(), host);
    await dispatchServiceEvent({ kind: "synced" }, host);
    await settle();
    assert.equal(calls.filter((call) => call.command === "mark_read").length, 1);

    archive = [{ ...marker, system_kind: null, text: "Unseen recovered message" }];
    messages.acceptMessages([]);
    chats.selectedChat = "other@s";
    await dispatchServiceEvent(hint(), host);
    await settle();
    assert.equal(chats.chats[0].unread_count, 1);
    assert.equal(messages.messages.length, 0);

    const ordinary = Array.from({ length: 50 }, (_, n) => ({ ...marker, id: `ordinary-${n}`, timestamp: 1000 + n, system_kind: null, read: true }));
    chats.selectedChat = chat;
    messages.acceptMessages(ordinary.toReversed());
    archive = [...ordinary, { ...marker, timestamp: 0 }];
    await dispatchServiceEvent(hint(), host);
    await settle();
    assert.equal(messages.messages.some((row) => row.id === marker.id), false);
    assert.equal(messages.messages.length, 50);
    messages.atLatest = false;
    ui.scrolledUp = true;
    archive = [...ordinary, { ...marker, timestamp: 5000 }];
    await dispatchServiceEvent(hint(), host);
    await settle();
    assert.equal(messages.messages.some((row) => row.id === marker.id), false);
    assert.equal(messages.atLatest, false);
    assert.equal(calls.filter((call) => call.command === "mark_read").length, 1);
  }, (command, value) => {
    const args = value as { anchorId?: string; limit?: number; cursor?: { timestamp: number } };
    if (command === "message_page") return { messages: archive.filter((row) => args.anchorId ? row.id === args.anchorId : !args.cursor || row.timestamp <= args.cursor.timestamp).toSorted((a, b) => b.timestamp - a.timestamp).slice(0, args.limit), has_more: false };
    if (command === "chats") return [{ chat, unread_count: archive.filter((row) => row.system_kind !== "UNAVAILABLE_MESSAGE" && !row.read).length,
      mention_count: archive.filter((row) => row.system_kind !== "UNAVAILABLE_MESSAGE" && row.mentioned && !row.read).length }];
    if (command === "mark_read") archive = archive.map((row) => row.system_kind === "UNAVAILABLE_MESSAGE" ? row : { ...row, read: true });
  });
});

test("a successful local query miss retires only an unavailable cached row", async () => {
  let fail = false;
  await withApp(async ({ messages }) => {
    const marker = { chat: "retired@s", id: "marker", timestamp: 1, system_kind: "UNAVAILABLE_MESSAGE" } as StoredMessage;
    const ordinary = { ...marker, id: "ordinary", system_kind: null };
    messages.prepareChat(marker.chat);
    messages.acceptMessages([marker, ordinary]);
    fail = true;
    await messages.refreshRow(marker.chat, marker.id, false);
    assert.equal(messages.messages.some((row) => row.id === marker.id), true);
    fail = false;
    await messages.refreshRow(marker.chat, marker.id, false);
    await messages.refreshRow(ordinary.chat, ordinary.id, false);
    assert.deepEqual(messages.messages.map((row) => row.id), [ordinary.id]);
  }, (command) => { if (command === "message_page") { if (fail) throw new Error("Synthetic local query failure"); return { messages: [], has_more: false }; } });
});

test("status and pending search updates stay inside their account and chat", async () => {
  await withApp(async ({ messages, chats, calls }) => {
    messages.acceptMessages([
      { chat: "open@s", id: "same-id", timestamp: 2, status: "sent" } as StoredMessage,
      { chat: "other@s", id: "same-id", timestamp: 1, status: "pending" } as StoredMessage,
    ]);
    messages.setStatus("other@s", "same-id", "read");
    assert.equal(messages.messages.find((m) => m.chat === "open@s")?.status, "sent");
    assert.equal(messages.messages.find((m) => m.chat === "other@s")?.status, "read");

    chats.searchQuery = "old account query";
    chats.runSearch();
    chats.resetAccount();
    await new Promise((resolve) => setTimeout(resolve, 250));
    assert.equal(calls.some((call) => call.command === "search"), false);
    assert.equal(chats.searchQuery, "");
    assert.deepEqual(chats.searchResults, []);
  });
});

test("message fetches reject stale account and row data without losing independent updates", async () => {
  type Page = { messages: StoredMessage[] };
  const pending: ((page: Page) => void)[] = [];
  const chat = "same@s";
  const id = "same-id";
  const row = (text: string, extra: Partial<StoredMessage> = {}) => ({
    chat, id, timestamp: 1, status: "sent", spoiler: false, text, ...extra,
  }) as StoredMessage;

  await withApp(async ({ messages }) => {
    messages.prepareChat(chat);
    messages.acceptMessages([row("old account")]);
    const oldAccount = messages.refreshRow(chat, id, false);
    messages.resetAccount();
    messages.prepareChat(chat);
    messages.acceptMessages([row("new account", { spoiler: true })]);
    pending.shift()!({ messages: [row("stale account result")] });
    await oldAccount;
    assert.equal(messages.messages[0].text, "new account");
    assert.equal(messages.messages[0].spoiler, true);

    messages.prepareChat(chat);
    messages.acceptMessages([row("base")]);
    const older = messages.refreshRow(chat, id, false);
    const newer = messages.refreshRow(chat, id, false);
    pending.pop()!({ messages: [row("newer result", { spoiler: true })] });
    await newer;
    pending.shift()!({ messages: [row("older result")] });
    await older;
    assert.equal(messages.messages[0].text, "newer result");
    assert.equal(messages.messages[0].spoiler, true);

    const independent = messages.refreshRow(chat, id, false);
    messages.append({ ...row("other"), id: "other-id" });
    pending.shift()!({ messages: [row("patched target")] });
    await independent;
    assert.equal(messages.messages.find((m) => m.id === id)?.text, "patched target");
    assert.equal(messages.messages.some((m) => m.id === "other-id"), true);

    const fullReload = messages.reloadMessages(chat);
    messages.patch({ ...messages.messages.find((m) => m.id === id)!, text: "inline edit", spoiler: true });
    pending.shift()!({ messages: [row("old full snapshot")] });
    assert.equal(await fullReload, false);
    assert.equal(messages.messages.find((m) => m.id === id)?.text, "inline edit");
    assert.equal(messages.messages.find((m) => m.id === id)?.spoiler, true);

    const content = messages.refreshRow(chat, id, false);
    messages.setStatus(chat, id, "read");
    pending.shift()!({ messages: [row("new content", { status: "sent" })] });
    await content;
    assert.equal(messages.messages.find((m) => m.id === id)?.text, "new content");
    assert.equal(messages.messages.find((m) => m.id === id)?.status, "read");
    messages.atLatest = false;
    const ids = messages.messages.map((m) => m.id);
    messages.append(row("outside window", { id: "outside", timestamp: 10 }));
    assert.deepEqual(messages.messages.map((m) => m.id), ids);
    messages.append({ ...messages.messages.find((m) => m.id === id)!, text: "loaded row update" });
    assert.equal(messages.messages.find((m) => m.id === id)?.text, "loaded row update");
  }, (command) => command === "message_page"
    ? new Promise<Page>((resolve) => pending.push(resolve))
    : undefined);
});

test("the group and DM menus offer their entries, dividers never first", async () => {
  await withApp(async ({ menuItems, messages, members, session }) => {
    session.me = "59897504482@s.whatsapp.net";
    members.participants = [
      { jid: session.me, name: "Me", admin: true, owner: false, number: "59897504482", username: null, label: null },
      { jid: "111@s.whatsapp.net", name: "Ana", admin: false, owner: false, number: "111", username: null, label: null },
    ];
    messages.marks = { ...messages.marks, reactions: [
      { target: "hello", sender: "59897504482@s.whatsapp.net", emoji: "👍" },
    ] };
    const message = { chat: "99@g.us", id: "hello", sender: "111@s.whatsapp.net", from_me: false,
      text: "Hi", revoked: false } as StoredMessage;
    const items = menuItems(message, async () => {});
    assert.deepEqual(labels(items), [
      "Copy", "Delete", "Forward", "Labels", "Message Ana", "Pin", "Reactions",
      "Remove Ana from group", "Reply", "Reply privately", "Report to admins",
      "Select messages", "Star",
    ]);
    // In a 1:1 chat the group-only entries drop out and ours appear.
    const own = { chat: "123@s.whatsapp.net", id: "mine", sender: session.me, from_me: true,
      text: "Hello", revoked: false } as StoredMessage;
    const dm = menuItems(own, async () => {});
    assert.deepEqual(labels(dm), [
      "Copy", "Delete", "Edit", "Forward", "Labels", "Message info", "Pin",
      "Reply", "Select messages", "Star",
    ]);
    // A divider draws above an entry, never above the first one, and each
    // shape does carry at least one.
    for (const menu of [items, dm]) {
      assert.ok(!menu[0].separated);
      assert.ok(menu.some((item, i) => i > 0 && item.separated));
    }
  });
});

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
    const reactions = items.find((item) => item.label === "Reactions");
    assert.ok(reactions);
    reactions.action();
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
    assert.deepEqual(ui.picking, { a: message });
    assert.ok(!menuItems({ ...message, revoked: true }, async () => {})
      .some((item) => item.label === "Select messages"));
    ui.picking = { a: message, b: { ...message, id: "b" } };
    await deleteSelected(false);
    assert.ok(calls.some((call) => call.command === "delete_messages"
      && JSON.stringify(call.args) === JSON.stringify({ chat: "99@g.us", ids: ["a", "b"], everyone: false })));
    assert.equal(ui.picking, null);
  });
});

test("a late bulk delete cannot clear a new selection or reload a different chat/account", async () => {
  let move = () => {};
  await withApp(async ({ deleteSelected, ui, chats, session, calls }) => {
    const old = { chat: "99@g.us", id: "old" } as StoredMessage;
    const next = { chat: "next@s", id: "next" } as StoredMessage;
    chats.selectedChat = old.chat;
    ui.picking = { old };
    move = () => { chats.selectedChat = next.chat; ui.picking = { next }; };
    await deleteSelected(false);
    assert.deepEqual(ui.picking, { next });
    assert.ok(!calls.some((call) => call.command === "message_page"));
    const refreshed = calls.filter((call) => call.command === "chats").length;
    chats.selectedChat = old.chat;
    ui.picking = { old };
    move = () => { session.activeAccount = "next-account"; ui.picking = { next }; };
    await deleteSelected(false);
    assert.deepEqual(ui.picking, { next });
    assert.equal(calls.filter((call) => call.command === "chats").length, refreshed);
  }, (command) => { if (command === "delete_messages") move(); });
});

test("bulk delete is offered for everyone only when every pick qualifies", async () => {
  await withApp(async ({ canDeletePickedForEveryone, messages, members, session, ui }) => {
    const mine = { chat: "99@g.us", id: "m", sender: "111@s.whatsapp.net", from_me: true,
      text: "x", revoked: false } as StoredMessage;
    const theirs = { chat: "99@g.us", id: "t", sender: "222@s.whatsapp.net", from_me: false,
      text: "y", revoked: false } as StoredMessage;
    messages.messages = [mine, theirs];
    ui.picking = { m: mine };
    assert.ok(canDeletePickedForEveryone());
    ui.picking = { m: mine, t: theirs };
    assert.ok(!canDeletePickedForEveryone());
    members.participants = [{
      jid: "111@s.whatsapp.net", name: "Me", admin: true, owner: false,
      number: "111", username: null, label: null,
    }];
    session.me = "111@s.whatsapp.net";
    assert.ok(canDeletePickedForEveryone());
  });
});

test("admin powers follow the core's read, and the roster's number form", async () => {
  await withApp(async ({ canDeletePickedForEveryone, messages, members, session, ui }) => {
    const theirs = { chat: "99@g.us", id: "t", sender: "222@s.whatsapp.net", from_me: false,
      text: "y", revoked: false } as StoredMessage;
    messages.messages = [theirs];
    ui.picking = { t: theirs };
    assert.ok(!canDeletePickedForEveryone());
    session.me = "59897504482@s.whatsapp.net";
    // LID-addressed rosters key us by LID and carry our number separately.
    members.participants = [{
      jid: "11111@lid", name: "Me", admin: true, owner: false,
      number: "59897504482", username: null, label: null,
    }];
    assert.ok(canDeletePickedForEveryone());
    // The core's own answer wins over the roster either way.
    members.chatGroup = { admin: false };
    assert.ok(!canDeletePickedForEveryone());
    members.chatGroup = { admin: true };
    assert.ok(canDeletePickedForEveryone());
  });
});

test("picked messages come back in the chat's order, not the pick order", async () => {
  await withApp(async ({ pickedInOrder }) => {
    const message = (id: string) => ({ chat: "99@g.us", id, timestamp: id.charCodeAt(0) }) as StoredMessage;
    const ordered = [message("a"), message("b"), message("c")];
    assert.deepEqual(
      pickedInOrder({ c: ordered[2], a: ordered[0] }, ordered).map((m) => m.id),
      ["a", "c"],
    );
    const old = message("0");
    const edited = { ...ordered[0], text: "edited" };
    assert.deepEqual(pickedInOrder({ a: ordered[0], old }, [edited]), [old, edited]);
    assert.deepEqual(pickedInOrder(null, ordered), []);
  });
});

test("forwarding sends every message to every chosen chat, in order", async () => {
  await withApp(async ({ forwardMessages, ui, calls }) => {
    const message = (id: string) => ({ chat: "99@g.us", id, sender: "1@s", from_me: false }) as StoredMessage;
    const batch = [message("older"), message("newer")];
    ui.picking = { older: batch[0], newer: batch[1] };
    await forwardMessages(batch, ["x@s", "y@s"]);
    const sent = calls.filter((call) => call.command === "forward_message").map((call) => call.args);
    assert.deepEqual(sent, [
      { chat: "99@g.us", id: "older", to: "x@s" },
      { chat: "99@g.us", id: "newer", to: "x@s" },
      { chat: "99@g.us", id: "older", to: "y@s" },
      { chat: "99@g.us", id: "newer", to: "y@s" },
    ]);
    assert.equal(ui.picking, null, "the selection ends once the batch is out");
    assert.ok(calls.some((call) => call.command === "chats"), "the list refreshes");
  });
});

test("the message menu forwards one message, and Select starts picking", async () => {
  await withApp(async ({ menuItems, ui }) => {
    const message = { chat: "99@g.us", id: "a", sender: "1@s", from_me: false,
      text: "hi", revoked: false } as StoredMessage;
    const items = menuItems(message, async () => {});
    items.find((item) => item.label === "Labels")!.action();
    assert.deepEqual(ui.labelTargets, [{ chat: message.chat, id: message.id }]);
    for (const privateMessage of [{ ...message, spoiler: true }, { ...message, revoked: true },
      { ...message, deleted: true }, { ...message, media_once_kind: "image" }, { ...message, system_kind: "UNAVAILABLE_MESSAGE" },
      { ...message, media_kind: "view_once" }, { ...message, media_kind: "unknown" }]) {
      assert.ok(!menuItems(privateMessage, async () => {}).some((item) => item.label === "Labels"));
    }
    items.find((item) => item.label === "Forward")!.action();
    assert.deepEqual(ui.forwarding, [message]);
    items.find((item) => item.label === "Select messages")!.action();
    assert.deepEqual(ui.picking, { a: message });
  });
});

test("bulk copy, star and reactions target every selected message without copying revoked text", async () => {
  await withApp(async ({ copyMessages, starMessages, reactMessages, ui, calls }) => {
    const batch = ["first", "second"].map((text, index) => ({ chat: "99@g.us", id: String(index), sender: "1@s", from_me: false, text }) as StoredMessage);
    ui.picking = Object.fromEntries(batch.map((message) => [message.id, message]));
    const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    const copied: string[] = [];
    let rejectCopy = false;
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { clipboard: {
      writeText: async (text: string) => { if (rejectCopy) throw new Error("Synthetic clipboard failure"); copied.push(text); },
    } } });
    try {
      await copyMessages([batch[0], { ...batch[0], revoked: true, text: "revoked text" }, batch[1]]);
      assert.deepEqual(copied, ["first\nsecond"]);
      await Promise.all([starMessages(batch, true), reactMessages(batch, "👍")]);
      assert.deepEqual(calls.filter((call) => call.command === "star" || call.command === "react").map((call) => [call.command, call.args]), [
        ["star", { target: { chat: "99@g.us", id: "0", sender: "1@s", fromMe: false }, starred: true }],
        ["star", { target: { chat: "99@g.us", id: "1", sender: "1@s", fromMe: false }, starred: true }],
        ["react", { target: { chat: "99@g.us", id: "0", sender: "1@s", fromMe: false }, emoji: "👍" }],
        ["react", { target: { chat: "99@g.us", id: "1", sender: "1@s", fromMe: false }, emoji: "👍" }],
      ]);
      assert.equal(Object.keys(ui.picking ?? {}).length, 2);
      rejectCopy = true;
      await copyMessages(batch);
      assert.match(ui.error ?? "", /Synthetic clipboard failure/);
    } finally {
      if (previous) Object.defineProperty(globalThis, "navigator", previous);
      else Reflect.deleteProperty(globalThis, "navigator");
    }
  });
});

test("bulk sends stop when account changes, including an in-flight forward batch", async () => {
  let switchAccount = () => {};
  await withApp(async ({ starMessages, reactMessages, forwardMessages, composer, ui, calls }) => {
    const batch = ["a", "b"].map((id) => ({ chat: "99@g.us", id, sender: "1@s", from_me: false }) as StoredMessage);
    ui.picking = { a: batch[0], b: batch[1] };
    switchAccount = () => composer.resetAccount();
    await starMessages(batch, true);
    assert.equal(calls.filter((call) => call.command === "star").length, 1);
    assert.match(ui.error ?? "", /abort/i);
    await reactMessages(batch, "👍");
    assert.equal(calls.filter((call) => call.command === "react").length, 1);
    await assert.rejects(forwardMessages(batch, ["x@s", "y@s"]), /abort/i);
    assert.equal(calls.filter((call) => call.command === "forward_message").length, 1);
    assert.equal(Object.keys(ui.picking ?? {}).length, 2);
  }, (command) => { if (command === "star" || command === "react" || command === "forward_message") switchAccount(); });
});

test("a failed bulk command stops the batch, keeps selection and exposes the failure", async () => {
  let sent = 0;
  await withApp(async ({ reactMessages, ui, calls }) => {
    const batch = ["a", "b", "c"].map((id) => ({ chat: "99@g.us", id, sender: "1@s", from_me: false }) as StoredMessage);
    ui.picking = Object.fromEntries(batch.map((message) => [message.id, message]));
    await reactMessages(batch, "❤️");
    assert.match(ui.error ?? "", /Synthetic reaction failure/);
    assert.equal(calls.filter((call) => call.command === "react").length, 2);
    assert.equal(Object.keys(ui.picking ?? {}).length, 3);
  }, (command) => { if (command === "react" && ++sent === 2) throw new Error("Synthetic reaction failure"); });
});

test("greyed-out media stays viewable, one-time copies stay behind their filter", async () => {
  await withApp(async ({ viewableMessages }) => {
    const media = (id: string, over: Partial<StoredMessage> = {}) => ({
      chat: "99@g.us", id, media_kind: "image", media_path: `/media/${id}.jpg`, ...over,
    }) as StoredMessage;
    const ordered = [
      media("revoked", { revoked: true }),
      media("deleted", { deleted: true }),
      media("plain"),
      media("video", { media_kind: "video" }),
      media("unfetched", { media_path: null }),
      media("once"),
      { chat: "99@g.us", id: "text", text: "hi" } as StoredMessage,
    ];
    const shown = viewableMessages(ordered, new Set(["once"]));
    assert.deepEqual(
      shown.map((m) => m.id),
      ["revoked", "deleted", "plain", "video"],
    );
  });
});
