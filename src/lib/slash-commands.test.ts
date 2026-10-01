import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { replaceSlashToken, slashCommandKey, slashCommands, slashToken } from "./utils/slash-commands.ts";

test("slash completion recognizes the caret token without treating URLs or selections as commands", () => {
  assert.deepEqual(slashToken("/", 1), { start: 0, end: 1, query: "", raw: "/" });
  assert.deepEqual(slashToken("Before /gif after", 11), { start: 7, end: 11, query: "gif", raw: "/gif" });
  assert.equal(slashToken("https://example.test/gif", 24), null);
  assert.equal(slashToken("path/gif", 8), null);
  assert.equal(slashToken("/poll", 3, 5), null);
  assert.equal(slashToken("/poll\n", 6), null);
  assert.equal(slashToken("/poll", 99), null);
});

test("fuzzy commands include all seven supported concepts, stable ordering and aliases", () => {
  assert.deepEqual(slashCommands("").map((command) => command.id), ["poll", "event", "sticker", "gif", "location", "mention-all", "keep-in-chat"]);
  assert.equal(slashCommands("pll")[0].id, "poll");
  assert.equal(slashCommands("stkr")[0].id, "sticker");
  assert.equal(slashCommands("cal")[0].id, "event");
  assert.equal(slashCommands("everyone")[0].id, "mention-all");
  assert.equal(slashCommands("keep")[0].id, "keep-in-chat");
  assert.deepEqual(slashCommands("unknowncommand"), []);
});

test("command replacement removes only the captured slash token and rejects changed drafts", () => {
  const draft = "Before /gif after", token = slashToken(draft, 11)!;
  assert.equal(replaceSlashToken(draft, token), "Before  after");
  assert.equal(replaceSlashToken(draft, token, "@all "), "Before @all  after");
  assert.equal(replaceSlashToken("Before /poll after", token), null);
  assert.equal(replaceSlashToken("Before", token), null);
  assert.equal(draft, "Before /gif after");
});

test("slash keyboard wraps and selects without intercepting IME, modifiers or newline intent", () => {
  const key = (key: string, extra = {}) => ({ key, isComposing: false, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...extra });
  assert.equal(slashCommandKey(key("ArrowUp"), 0, 3), 2);
  assert.equal(slashCommandKey(key("ArrowDown"), 2, 3), 0);
  assert.equal(slashCommandKey(key("Enter"), 0, 3), "choose");
  assert.equal(slashCommandKey(key("Tab"), 0, 3), "choose");
  assert.equal(slashCommandKey(key("Escape"), 0, 0), "close");
  assert.equal(slashCommandKey(key("Enter"), 0, 0), null);
  for (const extra of [{ isComposing: true }, { ctrlKey: true }, { metaKey: true }, { altKey: true }, { shiftKey: true }]) {
    assert.equal(slashCommandKey(key("Enter", extra), 0, 3), null);
  }
});

test("actual menu handlers preserve draft, suppress send on choose and reject stale scopes or carets", () => {
  const source = readFileSync(new URL("./composer/SlashCommandMenu.svelte", import.meta.url), "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("menu.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declaration = (name: string) => tree.statements.find((statement) =>
    ts.isFunctionDeclaration(statement) && statement.name?.text === name
    || ts.isVariableStatement(statement) && statement.declarationList.declarations.some((entry) => entry.name.getText(tree) === name))!.getText(tree);
  const code = ts.transpileModule(["index", "active", "options", "enabled", "selected", "current", "choose", "onKey"].map(declaration).join("\n"),
    { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  function menu(extra: Record<string, unknown> = {}) {
    const input = { value: "/poll", selectionStart: 5, selectionEnd: 5 };
    const chosen: unknown[] = []; let closed = 0;
    const bindings = { input, token: slashToken(input.value, 5)!, owner: { account: "a", chat: "room", generation: 1 },
      account: "a", chat: "room", generation: 1, disabled: { location: "Unavailable" }, slashCommandKey, slashCommands,
      $state: (value: unknown) => value, $derived: (value: unknown) => value, onchoose: (value: unknown) => { chosen.push(value); },
      onclose: () => { closed++; }, ...extra };
    const handlers = new Function(...Object.keys(bindings), code + "\nreturn {choose,onKey};")(...Object.values(bindings));
    return { ...handlers, input, chosen, closed: () => closed };
  }
  const event = (key: string, extra = {}) => {
    let prevented = false, stopped = false;
    return { key, keyCode: 0, isComposing: false, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false,
      preventDefault() { prevented = true; }, stopImmediatePropagation() { stopped = true; },
      prevented: () => prevented, stopped: () => stopped, ...extra };
  };
  const active = menu(), enter = event("Enter"); active.onKey(enter);
  assert.equal(enter.prevented(), true); assert.equal(enter.stopped(), true);
  assert.deepEqual(active.chosen, [{ command: "poll", token: slashToken("/poll", 5), account: "a", chat: "room", generation: 1 }]);
  assert.equal(active.input.value, "/poll");
  const escape = menu(), esc = event("Escape"); escape.onKey(esc);
  assert.equal(escape.closed(), 1); assert.equal(escape.input.value, "/poll"); assert.deepEqual(escape.chosen, []);
  for (const extra of [{ account: "b" }, { chat: "other" }, { generation: 2 }, { owner: null },
    { input: { value: "/poll", selectionStart: 0, selectionEnd: 0 } }, { input: { value: "/gif", selectionStart: 5, selectionEnd: 5 } }]) {
    const stale = menu(extra), key = event("Enter"); stale.onKey(key); stale.choose("poll");
    assert.deepEqual(stale.chosen, []); assert.equal(key.prevented(), false);
  }
  for (const extra of [{ isComposing: true }, { keyCode: 229 }, { shiftKey: true }]) {
    const guarded = menu(), key = event("Enter", extra); guarded.onKey(key);
    assert.deepEqual(guarded.chosen, []); assert.equal(key.prevented(), false); assert.equal(guarded.input.value, "/poll");
  }
  const disabled = menu(); disabled.choose("location"); assert.deepEqual(disabled.chosen, []);
});
