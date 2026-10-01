import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";

const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
const parsed = ts.createSourceFile("page.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const declaration = (name: string) => parsed.statements.find((statement) =>
  ts.isFunctionDeclaration(statement) && statement.name?.text === name
  || ts.isVariableStatement(statement) && statement.declarationList.declarations.some((entry) => entry.name.getText(parsed) === name))!.getText(parsed);
const effect = (needle: string) => parsed.statements.find((statement) => ts.isExpressionStatement(statement)
  && ts.isCallExpression(statement.expression) && statement.expression.expression.getText(parsed) === "$effect"
  && statement.getText(parsed).includes(needle))!.getText(parsed);
const compile = (text: string) => ts.transpileModule(text, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
function deferred() {
  let resolve!: (value: boolean) => void;
  const promise = new Promise<boolean>((done) => { resolve = done; });
  return { promise, resolve };
}

function fixture() {
  const session = { activeAccount: "account-a" };
  const chats = { selectedChat: "room@g.us" };
  const calls: string[] = [], visible = new Set<string>(), hidden = new Set<string>();
  const ui = { pendingJump: null as { chat: string; id: string } | null, seeking: false, errors: [] as string[],
    fail(error: unknown) { this.errors.push(String(error)); } };
  const hooks = { open: async () => {}, tick: async () => {}, stored: async () => false, recall: async () => false };
  const messages = { accountGeneration: 1, messages: [{ id: "before" }],
    showStoredMessage: async (_chat: string, id: string) => { calls.push(`stored:${id}`); return hooks.stored(); },
    recallDay: async () => { calls.push("recall"); await hooks.recall(); } };
  const bindings = { session, chats, messages, ui, keywords: { hidden: (row: { id: string }) => hidden.has(row.id) },
    openChat: async (chat: string) => { calls.push(`open:${chat}`); chats.selectedChat = chat; await hooks.open(); },
    tick: async () => { calls.push("tick"); await hooks.tick(); },
    scroller: { querySelector: (selector: string) => visible.has(selector.match(/data-id="([^"]+)"/)![1]) ? {} : null },
    scrollToMessage: (id: string) => { calls.push(`scroll:${id}`); } };
  const controller = new Function(...Object.keys(bindings), compile(["jumpSeq", "jumpTo", "loadAndJump"].map(declaration).join("\n"))
    + "\nreturn {jumpTo, loadAndJump};")(...Object.values(bindings)) as { jumpTo: (chat: string, id: string) => Promise<void>; loadAndJump: () => Promise<void> };
  function switchAccount() { session.activeAccount = "account-b"; messages.accountGeneration++; }
  return { ...bindings, ...controller, calls, visible, hidden, hooks, switchAccount };
}

test("jump opening and tick awaits cannot navigate the same JID after an account switch", async () => {
  for (const phase of ["open", "tick"] as const) {
    const f = fixture(), pause = deferred();
    if (phase === "open") f.chats.selectedChat = "other@g.us";
    f.hooks[phase] = async () => { await pause.promise; };
    const jumping = f.jumpTo("room@g.us", "old");
    f.switchAccount();
    f.chats.selectedChat = "room@g.us";
    f.ui.pendingJump = { chat: "room@g.us", id: "new" }; f.ui.seeking = true;
    pause.resolve(true); await jumping;
    assert.equal(f.ui.pendingJump?.id, "new"); assert.equal(f.ui.seeking, true);
    assert.equal(f.calls.some((call) => call.startsWith("stored:") || call.startsWith("scroll:")), false);
    assert.deepEqual(f.ui.errors, []);
  }
});

test("late stored and recall awaits cannot clear a new account's pending jump", async () => {
  for (const phase of ["stored", "recall"] as const) {
    const f = fixture(), pause = deferred(), entered = deferred();
    f.hooks[phase] = async () => { entered.resolve(true); return await pause.promise; };
    const jumping = f.jumpTo("room@g.us", "old"); await entered.promise;
    f.switchAccount(); f.ui.pendingJump = { chat: "room@g.us", id: "new" }; f.ui.seeking = true;
    pause.resolve(true); await jumping;
    assert.equal(f.ui.pendingJump?.id, "new"); assert.equal(f.ui.seeking, true);
    assert.equal(f.calls.includes("scroll:old"), false);
    assert.equal(f.calls.filter((call) => call.startsWith("stored:")).length, 1);
    assert.deepEqual(f.ui.errors, []);
  }
});

test("a newer jump can finish while an older lookup remains pending", async () => {
  const f = fixture(), first = deferred(), entered = deferred();
  f.hooks.stored = async () => { entered.resolve(true); return await first.promise; };
  const old = f.jumpTo("room@g.us", "old"); await entered.promise;
  f.visible.add("new"); await f.jumpTo("room@g.us", "new");
  first.resolve(true); await old;
  assert.deepEqual(f.calls.filter((call) => call.startsWith("scroll:")), ["scroll:new"]);
  assert.equal(f.ui.pendingJump, null); assert.equal(f.ui.seeking, false);
  assert.deepEqual(f.ui.errors, []);
});

test("a stored keyword-hidden jump target reports why it cannot be shown without phone recall", async () => {
  const f = fixture(); f.hidden.add("hidden"); f.messages.messages = [{ id: "hidden" }]; f.hooks.stored = async () => true;
  await f.jumpTo("room@g.us", "hidden");
  assert.deepEqual(f.ui.errors, ["This message is hidden by your keyword rules."]);
  assert.equal(f.calls.includes("recall"), false); assert.equal(f.calls.some((call) => call.startsWith("scroll:")), false);
  assert.equal(f.ui.pendingJump, null); assert.equal(f.ui.seeking, false);
});

test("viewer binding preserves the selected message through reorder and closes hidden selection", () => {
  const ui = { viewerId: "b" as string | null };
  const viewerItems = [{ id: "a" }, { id: "b" }, { id: "c" }];
  let synchronize!: () => void;
  const bindings = { ui, viewerItems, $effect: (callback: () => void) => { synchronize = callback; } };
  const viewer = new Function(...Object.keys(bindings), compile([declaration("viewerPosition"), declaration("setViewerPosition"), effect("viewerPosition")].join("\n"))
    + "\nreturn {viewerPosition,setViewerPosition};")(...Object.values(bindings));
  viewerItems.splice(0, 1); synchronize();
  assert.equal(ui.viewerId, "b"); assert.equal(viewer.viewerPosition(), 0);
  viewerItems.reverse(); synchronize(); assert.equal(viewer.viewerPosition(), 1);
  viewer.setViewerPosition(0); assert.equal(ui.viewerId, "c");
  viewerItems.splice(0, 1); assert.equal(viewer.viewerPosition(), -1); synchronize(); assert.equal(ui.viewerId, null);
});

test("keyword revision refreshes an open pings finder through an untracked read", () => {
  const keywords = { revision: 2 }, ui = { finder: { mode: "pings", chat: "room@g.us" } };
  const calls: string[] = [];
  const bindings = { keywords, ui, $effect: (callback: () => void) => callback(), untrack: (callback: () => void) => { calls.push("untrack"); callback(); },
    openPings: (chat: string) => { calls.push(chat); } };
  const refresh = new Function(...Object.keys(bindings), compile(effect("ui.finder")));
  refresh(...Object.values(bindings)); assert.deepEqual(calls, ["untrack", "room@g.us"]);
  ui.finder.mode = "search"; calls.length = 0; refresh(...Object.values(bindings)); assert.deepEqual(calls, ["untrack"]);
});

test("account reset clears completed finder snippets, viewer and pending navigation", () => {
  const uiSource = readFileSync(new URL("./state/ui.svelte.ts", import.meta.url), "utf8");
  const tree = ts.createSourceFile("ui.ts", uiSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const state = tree.statements.find(ts.isClassDeclaration)!;
  const method = state.members.find((member) => ts.isMethodDeclaration(member) && member.name.getText(tree) === "resetAccount")!.getText(tree);
  const reset = new Function(compile(`class State { ${method} }`) + "\nreturn State.prototype.resetAccount;")();
  const ui = { accountMenu: true, newGroup: true, finder: { items: ["account-a snippet"] }, viewerId: "a",
    pendingJump: { chat: "room@g.us", id: "a" }, seeking: true, starredItems: ["kept"] };
  reset.call(ui);
  assert.deepEqual(ui, { accountMenu: false, newGroup: false, finder: null, viewerId: null, pendingJump: null, seeking: false, starredItems: ["kept"] });
});
