import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { createServer } from "vite";
import { normalizeError } from "../lib/i18n/errors.ts";
import { t } from "../lib/i18n/localizer.ts";
import { descendants, directItems, emptyInboxFilters, matchingCandidates, movedIds, ordered, SPACE_KINDS,
  spaceChildren, spaceTree, targetKey, targetTitle } from "../lib/spaces/spaces.ts";
import type { Space, SpaceItem, SpaceTarget } from "../lib/utils/wire.ts";

const root: Space = { id: "root", parent_id: null, name: "Root", icon: null, color: null, order: 0, created_at: 1 };
const targets: SpaceTarget[] = [
  { kind: "chat", jid: "actual@lid" }, { kind: "group", jid: "actual@g.us" }, { kind: "community", jid: "community@g.us" },
  { kind: "channel", jid: "actual@newsletter" }, { kind: "contact", jid: "person@lid" }, { kind: "favorite_contact", jid: "favorite@lid" },
  { kind: "label", label_id: "17" }, { kind: "saved_message", chat: "actual@lid", message_id: "real-message" },
  { kind: "saved_search", query: "label:Customers customer", chat: null },
  { kind: "inbox_view", filters: { ...emptyInboxFilters(), unread: true, label: "17", query: "customer" } },
];
const catalog = targets.map((target, index) => ({ target, title: `Item ${index}`, detail: "Cached" }));
const items: SpaceItem[] = targets.map((target, order) => ({ id: `item-${order}`, space_id: "root", target, order }));
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (failure: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function functions(name: string, context: Record<string, any>) {
  const source = readFileSync(new URL(`../lib/spaces/${name}.svelte`, import.meta.url), "utf8");
  const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("component.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((node) => node.getText(tree));
  const current = tree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations)
    .find((node) => node.name.getText(tree) === "current");
  if (current) body.push(`var current = ${current.initializer!.getText(tree)};`);
  const effect = tree.statements.find((node) => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
    && node.expression.expression.getText(tree) === "$effect");
  assert.ok(effect && ts.isExpressionStatement(effect) && ts.isCallExpression(effect.expression));
  body.push(`var scopeEffect = ${effect.expression.arguments[0].getText(tree)};`);
  runInNewContext(ts.transpileModule(body.join("\n"), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

function uiFixture(name: string) {
  const actions: any[] = [], opened: SpaceTarget[] = [], additions: SpaceTarget[][] = [];
  let closes = 0;
  const context: Record<string, any> = {
    account: "a", generation: 1, spaceId: "root", space: root,
    openedAccount: "a", openedGeneration: 1, openedSpace: "root", closed: false, request: 0, revision: 0, alive: true,
    snapshot: { spaces: [root, { ...root, id: "child", parent_id: "root" }], items }, items, existing: [], catalog,
    selected: [], query: "", searchQuery: "", searchChat: "", filters: emptyInboxFilters(),
    draft: null, moving: null, confirmation: null, collapsed: [], navCollapsed: false, iconPopup: null, emojiQuery: "", emojis: [],
    busy: false, loading: false, working: false, failure: "",
    dialog: { close: () => { closes++; } }, onclose: () => { closes++; },
    onaction: async (action: unknown) => { actions.push(action); },
    onopen: async (target: SpaceTarget) => { opened.push(target); },
    onadd: async (value: SpaceTarget[]) => { additions.push(value); },
    structuredClone, targetKey, targetTitle, directItems, movedIds, descendants, emptyInboxFilters, normalizeError, t,
    crypto: { randomUUID: () => "local-uuid" },
  };
  Object.defineProperties(context, {
    disabled: { get: () => !context.account || context.busy || context.working || !context.alive || context.current && !context.current() },
    assigned: { get: () => new Set(context.existing.filter((item: SpaceItem) => item.space_id === context.spaceId).map((item: SpaceItem) => targetKey(item.target))) },
    picked: { get: () => new Set(context.selected.map(targetKey)) },
    children: { get: () => spaceChildren(context.snapshot.spaces) },
    excluded: { get: () => context.moving ? descendants(context.snapshot.spaces, context.moving.space.id) : new Set() },
    rows: { get: () => directItems(context.items, context.space.id) },
    resolved: { get: () => new Map(context.resolution?.items.map((item: any) => [item.item_id, item]) ?? []) },
  });
  return { c: functions(name, context), actions, opened, additions, closes: () => closes };
}

test("all ten target kinds retain exact reference identity and inbox keys ignore property ordering", () => {
  assert.equal(new Set(targets.map(targetKey)).size, 10);
  assert.equal(Object.keys(SPACE_KINDS).length, 10);
  assert.notEqual(targetKey({ kind: "chat", jid: "actual@lid" }), targetKey({ kind: "contact", jid: "actual@lid" }));
  const view = targets[9]; assert.equal(view.kind, "inbox_view");
  if (view.kind !== "inbox_view") return;
  const reordered = Object.fromEntries(Object.entries(view.filters).reverse()) as typeof view.filters;
  assert.equal(targetKey(view), targetKey({ kind: "inbox_view", filters: reordered }));
  for (const target of targets) assert.ok(targetTitle(target));
  for (const target of targets) assert.equal(matchingCandidates([...catalog, ...catalog], target.kind, "").length, 1);
  assert.equal(matchingCandidates([{ target: targets[0], title: "Álvaro" }], "chat", "alvr").length, 1);
});

test("tree and item ordering preserves input, nesting, descendants and full reorder identifiers", () => {
  const spaces = [{ ...root, id: "last", order: 2 }, { ...root, id: "child", parent_id: "root" }, root,
    { ...root, id: "grandchild", parent_id: "child" }];
  assert.deepEqual(spaceTree(spaces).map(({ space, depth }) => [space.id, depth]), [["root", 0], ["child", 1], ["grandchild", 2], ["last", 0]]);
  assert.deepEqual(spaceTree(spaces, ["child"]).map(({ space }) => space.id), ["root", "child", "last"]);
  assert.deepEqual([...descendants(spaces, "root")], ["root", "child", "grandchild"]);
  assert.deepEqual(movedIds(ordered([root, spaces[0]]), "last", -1), ["last", "root"]);
  assert.equal(movedIds([root], "root", -1), null);
  assert.equal(movedIds([root], "missing", 1), null);
  const original = [...items].reverse();
  assert.deepEqual(directItems([...original, { ...items[0], id: "foreign", space_id: "child" }], "root").map((item) => item.id), items.map((item) => item.id));
  assert.equal(original[0].id, "item-9");
});

test("tree callbacks exclude cyclic parents and retain failed drafts", async () => {
  const f = uiFixture("SpacesTree");
  f.c.create("root"); f.c.draft.name = "  Created  "; f.c.draft.icon = "◇"; f.c.draft.useColor = true;
  f.c.save(); await tick();
  assert.deepEqual(structuredClone(f.actions[0]), { kind: "create", id: "local-uuid", parent_id: "root", name: "Created", icon: "◇", color: "#00a884" });
  f.c.draft = { id: "root", name: " Renamed " }; f.c.save(); await tick();
  assert.deepEqual(structuredClone(f.actions[1]), { kind: "rename", id: "root", name: "Renamed" });
  f.c.moving = { space: root, parent_id: "child" }; f.c.reparent(); await tick();
  assert.equal(f.actions.length, 2);
  f.c.moving = { space: { ...root, id: "child", parent_id: "root" }, parent_id: null }; f.c.reparent(); await tick();
  assert.deepEqual(structuredClone(f.actions[2]), { kind: "reparent", id: "child", parent_id: null });
  const peer = { ...root, id: "peer", order: 1 };
  f.c.snapshot.spaces.push(peer); f.c.move(peer, -1); await tick();
  assert.deepEqual(structuredClone(f.actions[3]), { kind: "reorder", parent_id: null, ids: ["peer", "root"] });
  f.c.confirmation = root; f.c.remove(); await tick();
  assert.deepEqual(structuredClone(f.actions[4]), { kind: "delete", id: "root" });
  assert.ok(f.actions.every((action) => ["create", "rename", "reparent", "reorder", "delete"].includes(action.kind)));
  f.c.cancel(); f.c.create(); f.c.draft.name = "Retained draft";
  f.c.onaction = async () => { throw new Error("Local write failed"); }; f.c.save(); await tick();
  assert.equal(f.c.draft.name, "Retained draft");
  assert.equal(f.c.failure.code, "error.operation_failed"); assert.match(f.c.failure.diagnostic, /Local write failed/);
});

test("picker supports every kind, same-space dedupe and explicit saved query/inbox refs", async () => {
  const f = uiFixture("SpacePicker");
  for (const target of targets) { f.c.choose(target); f.c.choose(target); }
  assert.equal(f.c.selected.length, 10);
  await f.c.save();
  assert.deepEqual(f.additions[0], targets);
  assert.equal(f.c.closed, true);
  assert.equal(f.c.selected.length, 0);
  const view = uiFixture("SpacePicker");
  view.c.existing = [{ ...items[0], space_id: "root" }, { ...items[1], space_id: "another" }];
  view.c.choose(targets[0]); view.c.choose(targets[1]);
  assert.equal(view.c.selected.length, 1);
  view.c.searchQuery = "  exact query  "; view.c.searchChat = "actual@lid"; view.c.addSearch();
  view.c.filters = { ...emptyInboxFilters(), unread: true, mentions: true, label: "17", query: "customers" }; view.c.addInbox();
  view.c.filters.query = "edited after choosing";
  await view.c.save();
  assert.deepEqual(view.additions[0], [targets[1], { kind: "saved_search", query: "exact query", chat: "actual@lid" },
    { kind: "inbox_view", filters: { ...emptyInboxFilters(), unread: true, mentions: true, label: "17", query: "customers" } }]);
});

test("native availability alone controls item opening; unavailable refs remain removable and ordered", async () => {
  const f = uiFixture("SpaceItems");
  f.c.catalog = [];
  f.c.resolution = { chats: [], items: [{ item_id: "item-0", chats: ["actual@lid"], unavailable: null },
    { item_id: "item-7", chats: [], unavailable: "Missing local message" }] };
  f.c.open(items[0]); await tick();
  assert.deepEqual(f.opened, [targets[0]]);
  f.c.open(items[7]); f.c.open(items[1]); await tick();
  assert.equal(f.opened.length, 1);
  f.c.remove(items[7]); await tick();
  f.c.move(items[1], -1); await tick();
  assert.deepEqual(structuredClone(f.actions[0]), { kind: "remove_item", id: "item-7" });
  assert.deepEqual(structuredClone(f.actions[1]), { kind: "reorder_items", space_id: "root", ids: ["item-1", "item-0", ...items.slice(2).map((item) => item.id)] });
});

test("picker close/account/generation/Space changes suppress late success and failure without discarding retries", async () => {
  for (const change of ["account", "generation", "space", "close"]) for (const fail of [false, true]) {
    const f = uiFixture("SpacePicker"), task = deferred<void>();
    f.c.choose(targets[0]); f.c.onadd = () => task.promise;
    const pending = f.c.save();
    if (change === "account") f.c.account = "b";
    if (change === "generation") f.c.generation++;
    if (change === "space") f.c.spaceId = "other";
    if (change === "close") f.c.close(); else f.c.scopeEffect();
    const closes = f.closes();
    if (fail) task.reject(new Error("Obsolete add failure")); else task.resolve();
    await pending;
    assert.equal(f.c.failure, ""); assert.equal(f.c.selected.length, 0); assert.equal(f.closes(), closes);
  }
  const f = uiFixture("SpacePicker"); f.c.choose(targets[0]); f.c.onadd = async () => { throw new Error("Retry me"); };
  await f.c.save();
  assert.equal(f.c.failure.code, "error.operation_failed"); assert.match(f.c.failure.diagnostic, /Retry me/);
  assert.equal(f.c.selected.length, 1); assert.equal(f.c.closed, false);
});

test("tree and item request ownership discards errors after scope change or dialog close", async () => {
  for (const name of ["SpacesTree", "SpaceItems"]) for (const change of ["account", "generation", "close"]) {
    const f = uiFixture(name), task = deferred<void>(), pending = f.c.run(() => task.promise);
    if (change === "account") f.c.account = "b";
    if (change === "generation") f.c.generation++;
    if (change === "close") { if (name === "SpacesTree") f.c.cancel(); else { f.c.alive = false; f.c.revision++; } }
    else f.c.scopeEffect();
    f.c.failure = "Current scope";
    task.reject(new Error("Obsolete metadata failure")); await pending;
    assert.equal(f.c.failure, "Current scope");
  }
});

test("real SSR renders nested navigation, ten picker kinds and authoritative unavailable item controls", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/spaces", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const load = async (name: string) => (await server.ssrLoadModule(fileURLToPath(new URL(`../lib/spaces/${name}.svelte`, import.meta.url)))).default;
    const tree = render(await load("SpacesTree"), { props: { account: "a", generation: 1,
      snapshot: { spaces: [root, { ...root, id: "child", parent_id: "root", name: "Child" }], items }, selected: { kind: "unsorted" },
      onselect: () => {}, onaction: async () => { throw new Error("SSR must not mutate"); } } }).body;
    assert.ok(tree.includes('aria-label="Spaces"') && tree.includes('aria-expanded="true"') && tree.includes("Child"));
    assert.ok(/<button[^>]*aria-selected="true"[^>]*>[\s\S]*?Unsorted/.test(tree) && !tree.includes("Export metadata") && !tree.includes("Import metadata"));
    const body = render(await load("SpaceItems"), { props: { account: "a", generation: 1, space: root, items, catalog: [],
      resolution: { chats: ["actual@lid"], items: items.map((item) => ({ item_id: item.id, chats: [], unavailable: item.id === "item-7" ? "Missing local message" : null })) },
      onopen: () => { throw new Error("SSR must not open"); }, onaction: async () => { throw new Error("SSR must not mutate"); }, onadd: () => {} } }).body;
    assert.ok(body.includes("Unavailable: Missing local message") && body.includes("Local references"));
    assert.equal([...body.matchAll(/aria-label="Remove /g)].length, 10);
    const missing = body.match(/<button\b[^>]*class="open[^>]*disabled[^>]*>actual@lid · real-message/);
    assert.ok(missing);
    const picker = render(await load("SpacePicker"), { props: { account: "a", generation: 1, spaceId: "root", existing: [], catalog,
      onadd: async () => { throw new Error("SSR must not add"); }, onclose: () => {} } }).body;
    assert.ok(picker.includes('aria-label="Add items to Space"') && picker.includes('aria-label="Find local items"'));
    for (const code of Object.values(SPACE_KINDS)) assert.ok(picker.includes(t(code)), code);
    assert.ok(/<button\b[^>]*disabled[^>]*>\s*Add 0 items/.test(picker.replace(/<!--[\s\S]*?-->/g, "")));
  } finally { await server.close(); }
});
