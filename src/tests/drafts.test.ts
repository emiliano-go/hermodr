import { test } from "node:test";
import assert from "node:assert/strict";
import { draftKey, draftPreview, readDrafts, writeDrafts } from "../lib/utils/drafts.ts";

test("drafts restore per account and chat, then clear after sending or deletion", () => {
  const items = new Map<string, string>();
  const storage = {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => { items.set(key, value); },
    removeItem: (key: string) => { items.delete(key); },
  };
  const first = new Map([["chat-1", "Hello"], ["chat-2", "Another draft"]]);
  writeDrafts(storage, "account-1", first);
  writeDrafts(storage, "account-2", new Map([["chat-1", "Different"]]));
  assert.equal(readDrafts(storage, "account-1").get("chat-1"), "Hello");
  assert.equal(readDrafts(storage, "account-2").get("chat-1"), "Different");
  first.delete("chat-1");
  writeDrafts(storage, "account-1", first);
  assert.equal(readDrafts(storage, "account-1").has("chat-1"), false);
  first.clear();
  writeDrafts(storage, "account-1", first);
  assert.equal(items.has(draftKey("account-1")), false);
});

test("draft storage ignores malformed entries and preserves unusual chat IDs", () => {
  const raw = new Map<string, string>([[draftKey("owner"), JSON.stringify({ version: 1, drafts: { "__proto__": "", valid: "yes", empty: "", wrong: 3 } })]]);
  const storage = { getItem: (key: string) => raw.get(key) ?? null, setItem: (key: string, value: string) => { raw.set(key, value); }, removeItem: (key: string) => { raw.delete(key); } };
  assert.deepEqual([...readDrafts(storage, "owner")], [["valid", "yes"]]);
  const drafts = new Map([["__proto__", "safe"]]);
  writeDrafts(storage, "owner", drafts);
  assert.equal(readDrafts(storage, "owner").get("__proto__"), "safe");
  raw.set(draftKey("owner"), "broken");
  assert.equal(readDrafts(storage, "owner").size, 0);
});

test("draft preview is a single line and blank input has no indicator", () => {
  assert.equal(draftPreview("  First\n  second\tpart  "), "First second part");
  assert.equal(draftPreview(" \n\t "), "");
});
