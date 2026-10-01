import test from "node:test";
import assert from "node:assert/strict";
import { fuzzyScore, quickChats, quickSwitcherKey, messageSnippet, type QuickChat } from "./utils/quick-switcher.ts";

const row = (jid: string, name: string, aliases: string[] = [], kind = "contact"): QuickChat => ({
  jid, name, aliases, number: jid.split("@")[0], kind,
});

test("message excerpts include late body hits within a bounded preview", () => {
  const text = `${"before ".repeat(90)}Actual Match${" after".repeat(90)}`;
  const snippet = messageSnippet(text, "actual match");
  assert.ok(snippet.includes("Actual Match"));
  assert.ok(snippet.startsWith("…") && snippet.endsWith("…"));
  assert.ok(snippet.length <= 182);
  assert.equal(messageSnippet("Short body", "body"), "Short body");
});

test("quick switcher ranks fuzzy names, accents, aliases, numbers, groups and channels", () => {
  const recent = [row("100@s.whatsapp.net", "Álvaro"), row("team@g.us", "Development Group", [], "group")];
  const directory = [row("100@s.whatsapp.net", "Álvaro", ["maintainer"]),
    row("200@s.whatsapp.net", "Alice"), row("news@newsletter", "Postal Updates", [], "channel")];
  assert.deepEqual(quickChats(recent, directory, "").map((chat) => chat.jid), recent.map((chat) => chat.jid));
  assert.equal(quickChats(recent, directory, "alvr")[0].name, "Álvaro");
  assert.equal(quickChats(recent, directory, "mntnr")[0].jid, "100@s.whatsapp.net");
  assert.equal(quickChats(recent, directory, "200")[0].name, "Alice");
  assert.equal(quickChats(recent, directory, "dvg")[0].kind, "group");
  assert.equal(quickChats(recent, directory, "pstup")[0].kind, "channel");
  assert.equal(quickChats(recent, directory, "100").length, 1);
  assert.deepEqual(quickChats(recent, directory, "whatsapp"), []);
  assert.equal(quickChats(recent, directory, "200@s.whatsapp.net")[0].name, "Alice");
  assert.deepEqual(quickChats(recent, directory, "zqx"), []);
  assert.ok(fuzzyScore("alice", "Alice")! > fuzzyScore("alice", "Alice Cooper")!);
  assert.ok(fuzzyScore("alice", "Alice Cooper")! > fuzzyScore("alice", "Malice")!);
  assert.ok(fuzzyScore("alice", "Malice")! > fuzzyScore("alice", "A long ice")!);
  assert.equal(fuzzyScore("ab", "ba"), null);
  assert.deepEqual(quickChats([row("one", "Same"), row("two", "Same")], [], "same").map((chat) => chat.jid), ["one", "two"]);
});

test("quick switcher bounds result lists and keyboard navigation has no empty-list choice", () => {
  const catalog = Array.from({ length: 60 }, (_, index) => row(String(index), "match"));
  assert.equal(quickChats(catalog, [], "").length, 20);
  assert.equal(quickChats([], catalog, "match").length, 30);
  assert.equal(quickSwitcherKey("ArrowUp", 0, 3), 2);
  assert.equal(quickSwitcherKey("ArrowDown", 2, 3), 0);
  assert.equal(quickSwitcherKey("Enter", 0, 0), null);
  assert.equal(quickSwitcherKey("ArrowDown", 0, 0), null);
  assert.equal(quickSwitcherKey("Enter", 0, 1), "choose");
  assert.equal(quickSwitcherKey("Escape", 0, 0), "close");
  assert.equal(quickSwitcherKey("a", 0, 3), null);
});
