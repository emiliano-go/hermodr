import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { targetKey } from "../lib/spaces/spaces.ts";
import type { SpacesState } from "../lib/spaces/spaces.svelte.ts";
import type { SpaceAction, SpaceResolution, SpaceSnapshot, SpaceTarget } from "../lib/utils/wire.ts";

const source = readFileSync(new URL("../lib/spaces/spaces.svelte.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const compiled = compileModule(js, { generate: "server", filename: "spaces.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
const snapshot = (id = "root"): SpaceSnapshot => ({ spaces: [{ id, parent_id: null, name: id, icon: null, color: null, order: 0, created_at: 1 }], items: [] });
const resolution = (chat = "real@lid"): SpaceResolution => ({ chats: [chat], items: [] });
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture() {
  const calls: { command: string; args: any }[] = [];
  const session = { activeAccount: "a" as string | null, connected: false }, messages = { accountGeneration: 1 };
  let handle: (command: string, args: any) => unknown = (command) => command === "resolve_spaces" ? resolution()
    : command === "export_space_metadata" ? '{"version":1}' : snapshot();
  const invoke = (command: string, args: any) => { calls.push({ command, args }); return handle(command, args); };
  let uuid = 0;
  const State = new Function("invoke", "session", "messages", "targetKey", "structuredClone", "crypto", `${compiled}\nreturn SpacesState;`)
    (invoke, session, messages, targetKey, structuredClone, { randomUUID: () => `local-uuid-${++uuid}` }) as new () => SpacesState;
  const state = new State();
  return { state, session, messages, calls, handle: (next: typeof handle) => { handle = next; } };
}

test("offline Spaces operations use only typed local commands and retain native resolution sets", async () => {
  const f = fixture();
  const keywordCounts = { "actual@lid": 3 };
  f.state.keywordCounts = () => keywordCounts;
  await f.state.refresh();
  const native = { chats: ["nested@lid", "label@lid", "search@lid", "inbox@lid"], items: [{ item_id: "missing", chats: [], unavailable: "Message removed locally" }] };
  f.handle((command) => command === "resolve_spaces" ? native : snapshot());
  await f.state.select({ kind: "space", space_id: "root" });
  assert.deepEqual(f.state.resolution, native);
  await f.state.select({ kind: "unsorted" });
  assert.deepEqual(f.calls.at(-1)?.args.selection, { kind: "unsorted" });
  await f.state.mutate({ kind: "rename", id: "root", name: "Changed" });
  assert.equal(f.state.busy, false);
  assert.ok(f.calls.every(({ command, args }) => ["spaces_snapshot", "spaces_action", "resolve_spaces"].includes(command) && args.accountId === "a"));
  keywordCounts["actual@lid"] = 99;
  for (const call of f.calls.filter(({ command }) => command === "resolve_spaces")) assert.deepEqual(call.args.keywordCounts, { "actual@lid": 3 });
  assert.equal(f.session.connected, false);
});

test("account, generation and request changes reject stale snapshot and resolver outcomes", async () => {
  for (const change of ["account", "generation", "request"]) for (const fail of [false, true]) {
    const f = fixture(), old = deferred<SpaceSnapshot>();
    f.handle(() => old.promise);
    const pending = f.state.refresh();
    if (change === "account") f.session.activeAccount = "b";
    if (change === "generation") f.messages.accountGeneration++;
    if (change === "request") { f.handle((command) => command === "resolve_spaces" ? resolution("latest@lid") : snapshot("latest")); await f.state.refresh(); }
    if (fail) old.reject(new Error("Obsolete snapshot failure")); else old.resolve(snapshot("obsolete"));
    await pending;
    assert.equal(f.state.error, null);
    assert.equal(f.state.snapshot.spaces[0]?.id, change === "request" ? "latest" : undefined);
    assert.equal(f.calls.filter(({ command }) => command === "resolve_spaces").length, change === "request" ? 1 : 0);
  }
  const f = fixture(); await f.state.refresh();
  const old = deferred<SpaceResolution>(); f.handle(() => old.promise);
  const pending = f.state.select({ kind: "space", space_id: "root" });
  f.handle(() => resolution("current@lid")); await f.state.select({ kind: "unsorted" });
  old.resolve(resolution("obsolete@lid")); await pending;
  assert.deepEqual(f.state.resolution?.chats, ["current@lid"]);
});

test("reset and native mutation completion cannot overwrite another account or generation", async () => {
  for (const change of ["account", "generation", "reset"]) {
    const f = fixture(); await f.state.refresh();
    const old = deferred<SpaceSnapshot>(); f.handle(() => old.promise);
    const pending = f.state.mutate({ kind: "delete", id: "root" });
    const rejected = assert.rejects(pending, /Account changed/);
    if (change === "account") f.session.activeAccount = "b";
    if (change === "generation") f.messages.accountGeneration++;
    f.state.reset();
    f.handle((command) => command === "resolve_spaces" ? resolution("new@lid") : snapshot("new")); await f.state.refresh();
    old.resolve(snapshot("obsolete")); await rejected;
    assert.equal(f.state.snapshot.spaces[0].id, "new");
    assert.deepEqual(f.state.resolution?.chats, ["new@lid"]);
    assert.equal(f.state.busy, false);
    assert.equal(f.state.error, null);
  }
});

test("typed writes freeze payloads, serialize mutations and invalidate older reads", async () => {
  const f = fixture(); await f.state.refresh();
  const read = deferred<SpaceSnapshot>(), write = deferred<SpaceSnapshot>();
  f.handle((command) => command === "spaces_snapshot" ? read.promise : command === "spaces_action" ? write.promise : resolution("written@lid"));
  const reading = f.state.refresh();
  const action: SpaceAction = { kind: "reorder", parent_id: null, ids: ["root"] };
  const writing = f.state.mutate(action);
  action.ids[0] = "mutated-after-submit";
  assert.deepEqual(f.calls.at(-1)?.args.action.ids, ["root"]);
  await assert.rejects(f.state.mutate({ kind: "delete", id: "root" }), /unavailable/);
  write.resolve(snapshot("written")); await writing;
  read.resolve(snapshot("obsolete")); await reading;
  assert.equal(f.state.snapshot.spaces[0].id, "written");
  assert.deepEqual(f.state.resolution?.chats, ["written@lid"]);
  assert.equal(f.state.busy, false);
});

test("metadata export/import stays scoped and failed writes preserve existing collections", async () => {
  const f = fixture(); await f.state.refresh();
  assert.equal(await f.state.exportMetadata(), '{"version":1}');
  const json = '{"version":1,"snapshot":{"spaces":[],"items":[]}}';
  await f.state.importMetadata(json);
  assert.deepEqual(f.calls.find(({ command }) => command === "import_space_metadata")?.args, { json, accountId: "a" });
  const before = f.state.snapshot;
  f.handle(() => { throw new Error("Invalid metadata"); });
  await assert.rejects(f.state.importMetadata("invalid"), /Invalid metadata/);
  assert.equal(f.state.snapshot, before);
  assert.equal(f.state.error, "Error: Invalid metadata");
  assert.equal(f.state.busy, false);
  const exported = deferred<string>(); f.handle(() => exported.promise);
  const pending = f.state.exportMetadata(), rejected = assert.rejects(pending, /Account changed/);
  f.messages.accountGeneration++; exported.resolve("obsolete"); await rejected;
});

test("adding targets uses local UUIDs, allows multiple Spaces and stops on partial failure", async () => {
  const f = fixture(); await f.state.refresh();
  const value = snapshot(); value.spaces.push({ ...value.spaces[0], id: "other", order: 1 });
  const target: SpaceTarget = { kind: "chat", jid: "actual@lid" };
  value.items.push({ id: "existing", space_id: "root", target, order: 0 });
  f.state.snapshot = value;
  f.handle((command, args) => {
    if (command === "resolve_spaces") return resolution();
    assert.equal(command, "spaces_action");
    if (args.action.target.kind === "label") throw new Error("Synthetic second failure");
    value.items.push({ id: args.action.id, space_id: args.action.space_id, target: args.action.target, order: 0 });
    return structuredClone(value);
  });
  await f.state.add("root", [target]);
  assert.equal(f.calls.filter(({ command }) => command === "spaces_action").length, 0);
  await assert.rejects(f.state.add("other", [target, { kind: "label", label_id: "17" }, { kind: "channel", jid: "actual@newsletter" }]), /second failure/);
  const actions = f.calls.filter(({ command }) => command === "spaces_action");
  assert.equal(actions.length, 2);
  assert.equal(actions[0].args.action.id, "local-uuid-1");
  assert.deepEqual(actions[0].args.action.target, target);
  assert.equal(f.state.snapshot.items.filter((item) => targetKey(item.target) === targetKey(target)).length, 2);
});
