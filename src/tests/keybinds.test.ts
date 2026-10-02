import assert from "node:assert/strict";
import test from "node:test";

const stateGlobal = globalThis as unknown as { $state?: (value: unknown) => unknown };
const originalState = stateGlobal.$state;
stateGlobal.$state = (value) => value;
const { ACTIONS, conflicting, format, keybinds, matches } = await import("../lib/utils/keybinds.svelte.ts");
if (originalState) stateGlobal.$state = originalState; else delete stateGlobal.$state;

const ctrlF = { key: "f", ctrlKey: true, altKey: false, shiftKey: false, metaKey: false } as KeyboardEvent;
const plainF = { ...ctrlF, ctrlKey: false } as KeyboardEvent;

test("search in chat defaults to Ctrl+F without colliding with other actions", () => {
  assert.ok(ACTIONS.some((action) => action.id === "searchChat"));
  assert.equal(format(keybinds.searchChat), "Ctrl+f");
  assert.equal(matches(ctrlF, keybinds.searchChat), true);
  assert.equal(matches(plainF, keybinds.searchChat), false);
  assert.equal(conflicting().has("searchChat"), false);
});
