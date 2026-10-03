import assert from "node:assert/strict";
import test from "node:test";
import { loadedSelection, pickLoaded, selectionShortcut } from "../lib/utils/message-selection.ts";
import type { StoredMessage } from "../lib/utils/wire.ts";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";

test("select loaded excludes hidden and ineligible rows, retaining offscreen loaded messages", () => {
  const base = { chat: "fixture@s", read: false, from_me: false, deleted: false, revoked: false, system_kind: null };
  const rows = [{ ...base, id: "visible" }, { ...base, id: "offscreen" }, { ...base, id: "hidden" },
    { ...base, id: "deleted", deleted: true }, { ...base, id: "revoked", revoked: true },
    { ...base, id: "unavailable", system_kind: "UNAVAILABLE_MESSAGE" }] as StoredMessage[];
  const selected = loadedSelection(rows, (message) => message.id === "hidden");
  assert.deepEqual(Object.keys(selected), ["visible", "offscreen"]);
  assert.equal(selected.offscreen, rows[1]);
  assert.deepEqual(loadedSelection([], () => false), {});
});

test("selection shortcuts respect input editing, composition and existing handlers", () => {
  const event = { key: "a", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, defaultPrevented: false, isComposing: false };
  assert.equal(selectionShortcut(event), "loaded");
  assert.equal(selectionShortcut({ ...event, key: "A", ctrlKey: false, metaKey: true }), "loaded");
  assert.equal(selectionShortcut({ ...event, key: "d" }), "clear");
  assert.equal(selectionShortcut({ ...event, key: "Escape", ctrlKey: false }), "clear");
  for (const patch of [{ ctrlKey: false }, { altKey: true }, { shiftKey: true }, { isComposing: true }, { defaultPrevented: true }]) {
    assert.equal(selectionShortcut({ ...event, ...patch }), null);
  }
  assert.equal(selectionShortcut(event, true), null);
});

test("shift ranges use the latest explicit anchor, skip hidden rows and retain prior picks", () => {
  const rows = ["a", "b", "hidden", "c", "d"].map((id) => ({ id, chat: "fixture@s", deleted: false, revoked: false, system_kind: null }) as StoredMessage);
  const hidden = (message: StoredMessage) => message.id === "hidden";
  let pick = pickLoaded(rows, null, null, rows[0], false, hidden);
  assert.deepEqual(Object.keys(pick.selected!), ["a"]);
  pick = pickLoaded(rows, pick.selected, pick.anchor, rows[4], true, hidden);
  assert.deepEqual(Object.keys(pick.selected!), ["a", "b", "c", "d"]);
  pick = pickLoaded(rows, pick.selected, pick.anchor, rows[3], false, hidden);
  assert.equal(pick.anchor, "c");
  assert.equal(pick.selected!.c, undefined);
  pick = pickLoaded(rows, pick.selected, pick.anchor, rows[1], true, hidden);
  assert.ok(pick.selected!.b && pick.selected!.c);
  assert.equal(pick.selected!.hidden, undefined);
  const stale = pickLoaded(rows, { evicted: rows[0] }, "evicted", rows[4], true, hidden);
  assert.deepEqual(Object.keys(stale.selected!), ["evicted", "d"]);
  assert.equal(stale.anchor, "d");
});

test("SelectionBar keyboard handler updates selected count and clears without taking text-field shortcuts", () => {
  const source = readFileSync(new URL("../lib/messages/SelectionBar.svelte", import.meta.url), "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("bar.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const handler = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "keydown");
  assert.ok(handler);
  const rows = ["visible", "offscreen", "hidden"].map((id) => ({ id, chat: "fixture@s", deleted: false, revoked: false, system_kind: null }) as StoredMessage);
  let selected: Record<string, StoredMessage> | null = null;
  class Target {
    isContentEditable: boolean;
    constructor(editable = false) { this.isContentEditable = editable; }
    closest() { return null; }
  }
  const context = { HTMLElement: Target, busy: false, selectionShortcut,
    onselectloaded: () => { selected = loadedSelection(rows, (row) => row.id === "hidden"); },
    oncancel: () => { selected = null; } };
  const keydown = runInNewContext(ts.transpileModule(`${handler.getText(tree)}\nkeydown`,
    { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  const event = { key: "a", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, defaultPrevented: false, isComposing: false,
    target: new Target(), preventDefault() { this.defaultPrevented = true; } };
  keydown(event);
  assert.equal(Object.keys(selected!).length, 2);
  assert.equal(event.defaultPrevented, true);
  keydown({ ...event, key: "d", defaultPrevented: false });
  assert.equal(selected, null);
  keydown({ ...event, defaultPrevented: false, target: new Target(true) });
  assert.equal(selected, null);
});
