import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { get } from "svelte/store";
import { groupNotificationBody, isChatMuted, notificationBody, notificationTitle, shouldNotify } from "../lib/utils/notifications.ts";
import { keywordHidden } from "../lib/utils/keywords.ts";
import type { StoredMessage } from "../lib/utils/models.ts";
import { appendHistory, historyKey, loadHistory, saveHistory } from "../lib/notifications/history.ts";
import type { HistoryStorage, NotificationHistoryEntry } from "../lib/notifications/history.ts";
import { createNotificationHistory } from "../lib/notifications/history-store.ts";

class MemoryStorage implements HistoryStorage {
  values = new Map<string, string>();
  writes = 0;
  removals = 0;
  failWrite = false;
  failRemove = false;
  onRead = () => {};
  getItem(key: string) { this.onRead(); return this.values.get(key) ?? null; }
  setItem(key: string, value: string) {
    if (this.failWrite) throw new Error("quota");
    this.writes++; this.values.set(key, value);
  }
  removeItem(key: string) {
    if (this.failRemove) throw new Error("unavailable");
    this.removals++; this.values.delete(key);
  }
}

function entry(id = "id", timestamp = 1_700_000_000_000): NotificationHistoryEntry {
  return { chat: "room@g.us", id, sender: "123@s.whatsapp.net", chat_name: "Family", sender_name: "Ana", title: "Family", body: "Ana: Hello", timestamp };
}

test("history persists only metadata and previews from the existing privacy formatter", () => {
  const storage = new MemoryStorage();
  const history = createNotificationHistory(() => storage);
  for (const [id, message] of [
    ["spoiler", { text: "SPOILER_SECRET", media_kind: null, spoiler: true }],
    ["once", { text: "VIEW_ONCE_SECRET", media_kind: "view_once" }],
  ] as const) {
    const candidate = { ...entry(id), body: notificationBody(message), text: message.text, media_ref: "WIRE_SECRET", reply_to_text: "QUOTE_SECRET" };
    assert.equal(history.record("account", candidate, () => true), true);
  }
  const raw = storage.getItem(historyKey("account"))!;
  for (const secret of ["SPOILER_SECRET", "VIEW_ONCE_SECRET", "WIRE_SECRET", "QUOTE_SECRET", "media_ref", "reply_to_text"]) assert.equal(raw.includes(secret), false);
  assert.deepEqual(loadHistory("account", storage).entries.map((row) => row.body), ["View once message", "Spoiler message"]);
  assert.equal(get(history).error, null);
});

test("history is capped, newest first, and deduplicated by chat and message id", () => {
  let entries: NotificationHistoryEntry[] = [];
  for (let index = 0; index < 120; index++) entries = appendHistory(entries, entry(String(index), 1_700_000_000_000 + index));
  assert.equal(entries.length, 100);
  assert.equal(entries[0].id, "119");
  assert.equal(entries.at(-1)!.id, "20");
  assert.equal(appendHistory(entries, { ...entry("119", 1_800_000_000_000), body: "replay" }), entries);
  entries = appendHistory(entries, { ...entry("119", 1_800_000_000_000), chat: "different@g.us" });
  assert.equal(entries[0].chat, "different@g.us");
  assert.equal(entries.length, 100);
});

test("each account survives a store restart and clearing leaves other accounts intact", () => {
  const storage = new MemoryStorage();
  const history = createNotificationHistory(() => storage);
  history.record("a/b", entry("a"), () => true);
  history.record("a%2Fb", entry("b"), () => true);
  assert.notEqual(historyKey("a/b"), historyKey("a%2Fb"));
  const restarted = createNotificationHistory(() => storage);
  restarted.load("a/b"); assert.equal(get(restarted).entries[0].id, "a");
  restarted.load("a%2Fb"); assert.equal(get(restarted).entries[0].id, "b");
  assert.equal(restarted.clear("a/b", () => true), true);
  assert.equal(storage.getItem(historyKey("a/b")), null);
  assert.equal(loadHistory("a%2Fb", storage).entries[0].id, "b");
  restarted.load(null); assert.deepEqual(get(restarted).entries, []);
});

test("corrupt or wrongly scoped storage stays untouched until explicit clear", () => {
  const storage = new MemoryStorage();
  for (const raw of ["{SECRET broken", JSON.stringify({ version: 1, account: "other", entries: [entry()] }), JSON.stringify({ version: 1, account: "account", entries: [{ ...entry(), timestamp: "bad" }] })]) {
    storage.values.set(historyKey("account"), raw);
    const history = createNotificationHistory(() => storage);
    assert.equal(history.record("account", entry("new"), () => true), false);
    assert.equal(get(history).entries[0].id, "new");
    assert.ok(get(history).error);
    assert.equal(get(history).error!.includes("SECRET"), false);
    assert.equal(storage.getItem(historyKey("account")), raw);
    assert.equal(history.clear("account", () => true), true);
    assert.equal(history.record("account", entry("saved"), () => true), true);
    assert.equal(loadHistory("account", storage).entries[0].id, "saved");
  }
});

test("quota failures retain temporary entries across account switches without claiming persistence", () => {
  const storage = new MemoryStorage();
  const history = createNotificationHistory(() => storage);
  storage.failWrite = true;
  assert.equal(history.record("a", entry("temporary"), () => true), false);
  assert.match(get(history).error!, /could not be saved/);
  history.load("b"); history.load("a");
  assert.equal(get(history).entries[0].id, "temporary");
  assert.deepEqual(loadHistory("a", storage).entries, []);
  storage.failWrite = false;
  assert.equal(history.record("a", entry("later", 1_700_000_000_100), () => true), true);
  assert.deepEqual(loadHistory("a", storage).entries.map((row) => row.id), ["later", "temporary"]);
  assert.equal(get(history).error, null);
});

test("failed clear keeps entries and exposes the error", () => {
  const storage = new MemoryStorage();
  const history = createNotificationHistory(() => storage);
  history.record("account", entry(), () => true);
  storage.failRemove = true;
  assert.equal(history.clear("account", () => true), false);
  assert.equal(get(history).entries.length, 1);
  assert.match(get(history).error!, /could not be cleared/);
  assert.equal(loadHistory("account", storage).entries.length, 1);
});

test("stale account callbacks cannot load, publish, write or clear old history", () => {
  const storage = new MemoryStorage();
  saveHistory("old", [entry("existing")], storage);
  const writes = storage.writes;
  const history = createNotificationHistory(() => storage);
  let current = true;
  storage.onRead = () => { current = false; };
  assert.equal(history.record("old", entry("late"), () => current), false);
  assert.equal(get(history).account, null);
  assert.equal(storage.writes, writes);
  assert.equal(history.clear("old", () => false), false);
  assert.equal(storage.removals, 0);
  assert.equal(history.load("old", () => false), null);
  storage.onRead = () => {};
  let accesses = 0;
  const lateStorage = createNotificationHistory(() => { if (++accesses === 2) current = false; return storage; });
  current = true;
  assert.equal(lateStorage.record("old", entry("late"), () => current), false);
  assert.equal(storage.writes, writes);
  assert.equal(loadHistory("old", storage).entries[0].id, "existing");
});

function producer() {
  const storage = new MemoryStorage();
  const history = createNotificationHistory(() => storage);
  const session = { activeAccount: "account-a", settings: { notifications_enabled: true } };
  const messages = { accountGeneration: 1 };
  const chats = { selectedChat: "", chats: [{ chat: "room@g.us", muted_until: 0 }], chatLabel: () => "Family" };
  const members = { displayName: (push: string | null, address: string) => push || address,
    senderName: () => "Ana", mentionName: (user: string) => user };
  const rules = { highlight: [] as string[], hide: [] as string[] };
  const keywords = { account: "account-a", hidden: (message: StoredMessage) => keywordHidden(message, rules) };
  const attempts: { title: string; body: string; chat: string; account: string; current: () => boolean; historyCount: number }[] = [];
  let invoke: (...args: unknown[]) => Promise<unknown> = async () => { throw new Error("missing row"); };
  const source = readFileSync(new URL("../lib/state/events.ts", import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText
    .replace(/^import[\s\S]*?from\s+["'][^"']+["'];?\s*/gm, "").replace(/^export /gm, "");
  const bindings = { session, messages, chats, members, keywords, notificationHistory: history,
    shouldNotify, isChatMuted, notificationBody, notificationTitle, groupNotificationBody,
    isPlaceholder: (name: string) => /^\+?[\d\s]+$/.test(name),
    invoke: (...args: unknown[]) => invoke(...args),
    showChatNotification: (title: string, body: string, chat: string, account: string, current: () => boolean) => {
      attempts.push({ title, body, chat, account, current, historyCount: get(history).entries.length });
      return Promise.resolve();
    },
  };
  const functions = new Function(...Object.keys(bindings), `${compiled}\nreturn { notifyForMessage, notifyForHint };`)(...Object.values(bindings)) as {
    notifyForMessage: (message: StoredMessage, fresh: boolean) => void;
    notifyForHint: (chat: string, id: string, fresh: boolean) => Promise<void>;
  };
  return { ...functions, storage, history, session, messages, chats, rules, keywords, attempts,
    setInvoke(handler: typeof invoke) { invoke = handler; } };
}

function incoming(overrides: Partial<StoredMessage> = {}): StoredMessage {
  return { chat: "room@g.us", id: "notification", sender: "123@s.whatsapp.net", timestamp: Math.floor(Date.now() / 1000),
    from_me: false, revoked: false, deleted: false, system_kind: null, media_kind: null, media_once_kind: null,
    spoiler: false, text: "Hello", sender_name: "Ana", ...overrides } as StoredMessage;
}

test("verified notifications enter history before the OS attempt with safe previews and event time", () => {
  for (const message of [incoming(), incoming({ spoiler: true, text: "SPOILER_SECRET" }),
    incoming({ media_kind: "image", media_once_kind: "image", text: "KEPT_VIEW_ONCE_SECRET" })]) {
    const fixture = producer(), before = Date.now();
    fixture.notifyForMessage(message, true);
    const row = get(fixture.history).entries[0];
    assert.equal(fixture.attempts[0].historyCount, 1);
    assert.equal("account" in row, false);
    assert.equal(row.chat_name, "Family"); assert.equal(row.sender_name, "Ana");
    assert.equal(row.title, fixture.attempts[0].title); assert.equal(row.body, fixture.attempts[0].body);
    assert.ok(row.timestamp >= before && row.timestamp <= Date.now());
    assert.equal(row.body.includes("SECRET"), false);
    if (message.spoiler) assert.equal(row.body, "Ana: Spoiler message");
    if (message.media_once_kind) assert.equal(row.body, "Ana: View once message");
    assert.equal(loadHistory("account-a", fixture.storage).entries.length, 1);
  }
});

test("notification history producer shares eligibility and keyword/account guards", () => {
  for (const overrides of [
    { deleted: true }, { revoked: true }, { from_me: true }, { timestamp: Math.floor(Date.now() / 1000) - 301 },
    { system_kind: "UNAVAILABLE_MESSAGE" }, { system_kind: "SILENCED_UNKNOWN_CALLER_VOICE" }, { text: "hide this" },
  ] as Partial<StoredMessage>[]) {
    const fixture = producer(); fixture.rules.hide = ["hide"];
    fixture.notifyForMessage(incoming(overrides), true);
    assert.deepEqual(get(fixture.history).entries, []); assert.deepEqual(fixture.attempts, []);
  }
  for (const change of [
    (fixture: ReturnType<typeof producer>) => { fixture.session.settings.notifications_enabled = false; },
    (fixture: ReturnType<typeof producer>) => { fixture.chats.selectedChat = "room@g.us"; },
    (fixture: ReturnType<typeof producer>) => { fixture.chats.chats[0].muted_until = -1; },
    (fixture: ReturnType<typeof producer>) => { fixture.keywords.account = "account-b"; },
  ]) {
    const fixture = producer(); change(fixture); fixture.notifyForMessage(incoming(), true);
    assert.deepEqual(get(fixture.history).entries, []); assert.deepEqual(fixture.attempts, []);
  }
  const fixture = producer(); fixture.notifyForMessage(incoming(), false);
  assert.deepEqual(get(fixture.history).entries, []);
  fixture.notifyForMessage(incoming(), true); fixture.rules.hide = ["hello"];
  assert.equal(fixture.attempts[0].current(), false);
});

test("hint fallback stays generic without history and a stale verified hint cannot cross accounts", async () => {
  const missing = producer(); await missing.notifyForHint("room@g.us", "missing", true);
  assert.deepEqual(get(missing.history).entries, []);
  assert.equal(missing.attempts[0].body, "New message");
  const stale = producer(); let finish!: (page: unknown) => void;
  stale.setInvoke(() => new Promise((resolve) => { finish = resolve; }));
  const waiting = stale.notifyForHint("room@g.us", "notification", true);
  stale.session.activeAccount = "account-b"; stale.keywords.account = "account-b"; stale.messages.accountGeneration++;
  finish({ messages: [incoming()] }); await waiting;
  assert.deepEqual(get(stale.history).entries, []); assert.deepEqual(stale.attempts, []);
  assert.equal(stale.storage.getItem(historyKey("account-a")), null);
});
