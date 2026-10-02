import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { parse } from "svelte/compiler";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { targetKey, emptyInboxFilters } from "../lib/spaces/spaces.ts";
import { labelSearch } from "../lib/utils/label-search.ts";

const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8");
const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
const tree = ts.createSourceFile("route.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const declaration = (name: string) => tree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations)
  .find((node) => node.name.getText(tree) === name)?.initializer;
const reset = tree.statements.find((node) => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
  && node.expression.expression.getText(tree) === "$effect" && node.expression.arguments[0].getText(tree).includes("spaceCatalogRequest++"));
assert.ok(reset && ts.isExpressionStatement(reset) && ts.isCallExpression(reset.expression));
const body = ["refreshSpaceCatalog", "openSpaceTarget"].map((name) => {
  const node = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
  assert.ok(node, name); return node.getText(tree);
});
for (const [name, variable, by] of [["visible", "visibleChats", false], ["order", "spaceChatOrder", false], ["candidates", "spaceCandidates", true]] as const) {
  const node = declaration(variable); assert.ok(node && ts.isCallExpression(node), variable);
  body.push(`var ${name} = ${by ? node.arguments[0].getText(tree) : `() => (${node.arguments[0].getText(tree)})`};`);
}
body.push(`var resetScope = ${reset.expression.arguments[0].getText(tree)};`);
const compiled = ts.transpileModule(body.join("\n"), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
const markup = parse(source, { modern: true }) as any;
function component(name: string) {
  let found: any, key: any;
  function visit(node: any, enclosingKey?: any) {
    if (!node || typeof node !== "object" || found) return;
    if (node.type === "Component" && node.name === name) { found = node; key = enclosingKey; return; }
    if (Array.isArray(node)) for (const child of node) visit(child, enclosingKey);
    else for (const child of Object.values(node)) visit(child, node.type === "KeyBlock" ? node.expression : enclosingKey);
  }
  visit(markup); assert.ok(found, name); return { node: found, key };
}
const evaluate = (node: any, context: Record<string, any>) => runInNewContext(ts.transpileModule(`(${source.slice(node.start, node.end)})`,
  { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
function attribute(name: string, prop: string, context: Record<string, any>) {
  const attr = component(name).node.attributes.find((attr: any) => attr.name === prop);
  const value = Array.isArray(attr?.value) ? attr.value[0] : attr?.value;
  assert.ok(value?.expression, `${name}.${prop}`);
  return evaluate(value.expression, context);
}
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (failure: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture() {
  const calls: { command: string; args: any }[] = [], opened: any[] = [], searches: string[] = [], mutations: any[] = [];
  const c: Record<string, any> = {
    session: { activeAccount: "a", started: true }, messages: { accountGeneration: 1 },
    spaces: { account: "a", loaded: true, selected: { kind: "space", space_id: "root" }, snapshot: { spaces: [], items: [] },
      resolution: { chats: ["archived@lid", "child@lid", "label@lid", "search@lid", "inbox@lid"], items: [] },
      reset: () => { c.spaces.account = null; }, refresh: async () => { c.spaces.account = c.session.activeAccount; },
      mutate: async (action: any) => { mutations.push(action); }, add: async (space: string, targets: any[]) => { mutations.push({ space, targets }); } },
    chats: { chats: [], visibleChats: [], searchResults: [], chatFilter: "all", labelFilter: "", chatName: (jid: string) => jid },
    labels: { account: "a", view: { labels: [{ id: "17", name: "Customers" }] }, refresh: async () => {}, chatIds: () => [] },
    favorites: { chats: [] }, keywords: { account: "a", counts: {} }, members: { displayName: (name: string) => name },
    ui: { finder: null, showInbox: false }, spaceCatalog: [], spaceGroups: [], spaceSaved: [], spaceCatalogRequest: 0,
    spaceCatalogLoading: false, spaceCatalogError: null, spacePickerFor: null, spaceCommunityFor: null,
    switcherQuery: "", quickSwitcher: false, spaceFinderKey: 0, spaceOpenSeq: 0, chatOpenSeq: 0,
    inboxSeed: undefined, inboxSeedKey: 0, currentInboxFilters: emptyInboxFilters(), targetKey, labelSearch,
    plain: (text: string) => text, untrack: (run: () => unknown) => run(),
    invoke: async (command: string, args?: any) => { calls.push({ command, args }); return []; },
    openChat: async (jid: string) => { opened.push({ jid }); c.chatOpenSeq++; },
    jumpTo: async (chat: string, id: string) => { opened.push({ chat, id }); },
    searchChat: async (query: string, _more = false, localOnly = false) => { searches.push(query); c.ui.finder = { ...c.ui.finder, more: !localOnly }; },
  };
  runInNewContext(compiled, c);
  Object.defineProperty(c, "spaceChatOrder", { get: () => c.order() });
  return { c, calls, opened, searches, mutations };
}

test("route uses native descendant/dynamic membership, archived refs and order without invented chats", () => {
  const { c } = fixture();
  c.chats.chats = ["outside@lid", "inbox@lid", "search@lid", "label@lid", "child@lid", "archived@lid"].map((chat) => ({ chat, mention_count: 1, archived: chat === "archived@lid" }));
  c.chats.visibleChats = c.chats.chats.filter((chat: any) => !chat.archived);
  c.spaces.resolution.chats.push("contact-without-history@lid");
  c.keywords.counts["child@lid"] = 2;
  assert.deepEqual(Array.from(c.visible(), (chat: any) => chat.chat), ["archived@lid", "child@lid", "label@lid", "search@lid", "inbox@lid"]);
  assert.equal(c.visible().find((chat: any) => chat.chat === "child@lid").mention_count, 3);
  c.chats.searchResults = [{ jid: "outside@lid" }, { jid: "child@lid" }];
  assert.deepEqual(Array.from(attribute("ChatSidebar", "searchResults", c), (row: any) => row.jid), ["child@lid"]);
  c.spaces.selected = { kind: "all" };
  assert.ok(c.visible().some((chat: any) => chat.chat === "outside@lid"));
  assert.ok(!c.visible().some((chat: any) => chat.archived));
});

test("route account reset clears view seeds and favorite candidates retain valid target kinds", () => {
  const { c } = fixture();
  c.inboxSeed = { ...emptyInboxFilters(), label: "old-account-label", query: "old query" };
  c.currentInboxFilters = { ...c.inboxSeed }; c.switcherQuery = "old saved query";
  c.session.activeAccount = "b"; c.messages.accountGeneration++; c.resetScope();
  assert.equal(c.inboxSeed, undefined); assert.equal(c.switcherQuery, "");
  assert.deepEqual({ ...c.currentInboxFilters }, emptyInboxFilters());
  c.favorites.chats = ["person@lid", "phone@s.whatsapp.net", "group@g.us", "community@g.us", "channel@newsletter"];
  c.spaceGroups = [{ jid: "community@g.us", community: true, subject: "Community" }];
  const candidates = Array.from(c.candidates(), (row: any) => row.target);
  assert.deepEqual(candidates.filter((target: any) => target.kind === "favorite_contact").map((target: any) => target.jid), ["person@lid", "phone@s.whatsapp.net"]);
  for (const [jid, kind] of [["group@g.us", "group"], ["community@g.us", "community"], ["channel@newsletter", "channel"]])
    assert.ok(candidates.some((target: any) => target.jid === jid && target.kind === kind));
});

test("catalog reads capture account and ignore late account/generation/request results", async () => {
  for (const change of ["account", "generation", "request"]) for (const fail of [false, true]) {
    const { c, calls } = fixture(), pendingRows = deferred<any[]>();
    c.spaceCatalog = [{ jid: "current@lid" }];
    c.invoke = (command: string, args: any) => { calls.push({ command, args }); return pendingRows.promise; };
    const pending = c.refreshSpaceCatalog();
    assert.ok(calls.every((call) => call.args?.accountId === "a"), "every catalog reader must use captured account");
    if (change === "account") c.session.activeAccount = "b";
    if (change === "generation") c.messages.accountGeneration++;
    if (change === "request") c.spaceCatalogRequest++;
    if (fail) pendingRows.reject(new Error("Obsolete catalog failure")); else pendingRows.resolve([]);
    await pending;
    assert.equal(c.spaceCatalog[0].jid, "current@lid"); assert.equal(c.spaceCatalogError, null);
  }
});

test("Space target opening preserves actual references and membership callbacks only mutate local metadata", async () => {
  const f = fixture(), { c } = f;
  await c.openSpaceTarget({ kind: "chat", jid: "actual@lid" });
  await c.openSpaceTarget({ kind: "saved_message", chat: "actual@lid", message_id: "real-message" });
  await c.openSpaceTarget({ kind: "community", jid: "community@g.us" });
  assert.deepEqual(f.opened, [{ jid: "actual@lid" }, { chat: "actual@lid", id: "real-message" }]);
  assert.deepEqual({ ...c.spaceCommunityFor }, { account: "a", generation: 1, jid: "community@g.us" });
  await c.openSpaceTarget({ kind: "saved_search", chat: null, query: "global query" });
  assert.equal(c.switcherQuery, "global query"); assert.equal(c.quickSwitcher, true);
  await c.openSpaceTarget({ kind: "saved_search", chat: "actual@lid", query: "scoped query" });
  assert.deepEqual(f.searches, ["scoped query"]); assert.equal(c.ui.finder.chat, "actual@lid"); assert.equal(c.ui.finder.more, false);
  const filters = { ...emptyInboxFilters(), unread: true, label: "17" };
  await c.openSpaceTarget({ kind: "inbox_view", filters }); filters.label = "changed";
  assert.equal(c.inboxSeed.label, "17"); assert.equal(c.ui.showInbox, true);
  const action = { kind: "remove_item", id: "only-local-reference" };
  await attribute("SpacesTree", "onaction", c)(action); await attribute("SpaceItems", "onaction", c)(action);
  assert.deepEqual(f.mutations, [action, action]); assert.equal(f.calls.length, 0);
});

test("delayed label-search refresh cannot replace newer account, generation, Space request, finder or navigation", async () => {
  for (const change of ["account", "generation", "request", "finder", "navigation"]) {
    const f = fixture(), ready = deferred<void>();
    f.c.labels.refresh = () => ready.promise;
    const pending = f.c.openSpaceTarget({ kind: "saved_search", chat: "actual@lid", query: "label:Customers customer" });
    assert.equal(f.c.ui.finder, null);
    if (change === "account") f.c.session.activeAccount = "b";
    if (change === "generation") f.c.messages.accountGeneration++;
    if (change === "request") await f.c.openSpaceTarget({ kind: "saved_search", chat: null, query: "new target" });
    if (change === "finder") f.c.ui.finder = { mode: "search", chat: "new@lid", query: "new finder", more: true };
    if (change === "navigation") f.c.chatOpenSeq++;
    const current = f.c.ui.finder;
    ready.resolve(); await pending;
    assert.equal(f.c.ui.finder, current); assert.equal(f.searches.length, 0, change);
  }
});

test("older saved-search completion cannot invalidate newer real finder search", async () => {
  const { c } = fixture();
  const finderSource = readFileSync(new URL("../lib/state/finder.ts", import.meta.url), "utf8");
  const finderTree = ts.createSourceFile("finder.ts", finderSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const functions = ["found", "searchChat"].map((name) => {
    const node = finderTree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
    assert.ok(node, name); return node.getText(finderTree);
  });
  const limit = finderTree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations)
    .find((node) => node.name.getText(finderTree) === "SEARCH_LIMIT");
  assert.ok(limit);
  functions.push(`var SEARCH_LIMIT = ${limit.initializer!.getText(finderTree)};`);
  runInNewContext(ts.transpileModule(functions.join("\n"), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText.replace(/^export /gm, ""), c);
  c.messages.messages = []; c.messages.olderExhausted = false; c.ui.fail = (error: unknown) => { throw error; };
  const old = deferred<any[]>(), latest = deferred<any[]>();
  c.invoke = (_command: string, args: any) => args.query === "old query" ? old.promise : latest.promise;
  const opening = c.openSpaceTarget({ kind: "saved_search", chat: "actual@lid", query: "old query" });
  const searching = c.searchChat("new query");
  old.resolve([]); await opening;
  latest.resolve([]); await searching;
  assert.equal(c.ui.finder.query, "new query");
  assert.deepEqual(Array.from(c.ui.finder.items), []);
});

test("QuickSwitcher resets or remounts across account generations", () => {
  const { c } = fixture(); c.quickSwitcher = true;
  const key = component("QuickSwitcher").key; assert.ok(key);
  c.account = "a"; c.generation = 1; const oldKey = evaluate(key, c);
  c.messages.accountGeneration++; c.generation++; c.resetScope();
  assert.ok(!c.quickSwitcher || evaluate(key, c) !== oldKey, "same-account generation changes must invalidate mounted switcher");
});

test("saved scoped search route seeds actual finder input", async () => {
  const { c } = fixture(); await c.openSpaceTarget({ kind: "saved_search", chat: "actual@lid", query: "customer query" });
  const initialQuery = attribute("MessageFinder", "initialQuery", c);
  assert.equal(initialQuery, "customer query");
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/spaces-route", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: Finder } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/messages/MessageFinder.svelte", import.meta.url)));
    const body = render(Finder, { props: { title: "Search messages", placeholder: "Search this chat", items: [], empty: "No matches",
      initialQuery, onquery: () => {}, onopen: () => {}, onclose: () => {} } }).body;
    assert.ok(/<input\b[^>]*value="customer query"/.test(body));
  } finally { await server.close(); }
});

test("actual finder IPC captures account and discards obsolete account/generation results and errors", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/spaces-route-state", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  const previousStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage"), data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value); },
  } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { searchChat } = await load("../lib/state/finder.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const errors: unknown[] = [], calls: { command: string; args: any }[] = [];
    ui.fail = (error: unknown) => { errors.push(error); };
    session.activeAccount = "account-a"; session.started = false; messages.resetAccount(); messages.messages = [];
    ui.finder = { mode: "search", chat: "scope@g.us", items: [], query: "", more: false };
    setHandler((command: string, args: any) => { calls.push({ command, args }); return []; });
    await searchChat("current query", false, true);
    assert.equal(calls[0].command, "search_messages");
    assert.equal(calls[0].args.accountId, "account-a");
    assert.equal(calls[0].args.chat, "scope@g.us");
    assert.equal(ui.finder.query, "current query"); assert.equal(ui.finder.more, false);
    for (const change of ["account", "generation"]) for (const fail of [false, true]) {
      session.activeAccount = "account-a"; messages.resetAccount(); errors.length = 0;
      ui.finder = { mode: "search", chat: "scope@g.us", items: [], query: "", more: false };
      const result = deferred<unknown[]>();
      setHandler((command: string, args: any) => { calls.push({ command, args }); return result.promise; });
      const pending = searchChat("obsolete query", false, true), owned = ui.finder;
      if (change === "account") session.activeAccount = "account-b"; else messages.resetAccount();
      if (fail) result.reject(new Error("Obsolete native scope failure")); else result.resolve([]);
      await pending;
      assert.equal(calls.at(-1)?.args.accountId, "account-a");
      assert.equal(ui.finder, owned); assert.deepEqual(errors, [], `${change} must suppress stale errors`);
    }
  } finally {
    if (previousStorage) Object.defineProperty(globalThis, "localStorage", previousStorage); else Reflect.deleteProperty(globalThis, "localStorage");
    await server.close();
  }
});
