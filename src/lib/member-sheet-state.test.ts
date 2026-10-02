import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { memberActionReason } from "./utils/member-sheet.ts";
import type { MemberSheetState } from "./state/member-sheet.svelte";

const source = readFileSync(new URL("./state/member-sheet.svelte.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const compiled = compileModule(js, { generate: "server", filename: "member-sheet.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => resolve = done);
  return { promise, resolve };
}
function fixture() {
  const session = { activeAccount: "synthetic-a", connected: true }, messages = { accountGeneration: 1 }, chats = { selectedChat: "room@g.us" };
  const calls: { command: string; args: any }[] = [];
  let handler: (command: string, args: any) => any = () => null;
  type Work = (signal: AbortSignal) => Promise<unknown>;
  let queue = (run: Work) => run(new AbortController().signal);
  const invoke = (command: string, args: any) => { calls.push({ command, args }); return handler(command, args); };
  const State = new Function("invoke", "memberActionReason", "session", "messages", "chats", "composer", `${compiled}\nreturn MemberSheetState;`)
    (invoke, memberActionReason, session, messages, chats, { enqueue: (run: Work) => queue(run) }) as new () => MemberSheetState;
  const state = new State();
  state.scope = { account: session.activeAccount, group: chats.selectedChat, jid: "123@lid", requestKey: 1, generation: 1, title: "Member" };
  return { state, session, messages, chats, calls, handle: (next: typeof handler) => handler = next, queue: (next: typeof queue) => queue = next };
}

test("member loader rejects old account, generation, group and closed-sheet responses", async () => {
  for (const change of ["account", "generation", "group", "close"]) {
    const f = fixture(), pending = deferred<any>();
    f.handle(() => pending.promise);
    const load = f.state.load(false);
    if (change === "account") f.session.activeAccount = "synthetic-b";
    if (change === "generation") ++f.messages.accountGeneration;
    if (change === "group") f.chats.selectedChat = "other@g.us";
    if (change === "close") f.state.close();
    pending.resolve({ name: "private old profile" });
    await load;
    assert.equal(f.state.profile, null, change);
    assert.equal(f.state.error, "", change);
    assert.equal(f.calls[0].args.accountId, "synthetic-a");
    assert.equal(f.calls[0].args.live, false);
  }
});

test("old opening cannot launch a live query for the replacement sheet", async () => {
  const f = fixture(), pending = [deferred<any>(), deferred<any>()];
  f.state.close();
  f.handle(() => pending[f.calls.length - 1].promise);
  f.state.open("room@g.us", "123@lid", "Old member");
  f.state.open("room@g.us", "456@lid", "New member");
  pending[0].resolve({ name: "old" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(f.calls.length, 2);
  assert.equal(f.state.scope?.jid, "456@lid");
  assert.equal(f.state.profile, null);
  assert.equal(f.state.localLoading, true);
  f.state.close(); pending[1].resolve(null);
});

test("queued member actions reject changed scope or aborted work before invoking", async () => {
  for (const change of ["account", "generation", "group", "close", "abort"]) {
    const f = fixture(), controller = new AbortController();
    let run!: () => Promise<unknown>;
    f.queue((work) => new Promise((resolve, reject) => run = () => work(controller.signal).then(resolve, reject)));
    const action = f.state.action(f.state.scope!, "remove");
    if (change === "account") f.session.activeAccount = "synthetic-b";
    if (change === "generation") ++f.messages.accountGeneration;
    if (change === "group") f.chats.selectedChat = "other@g.us";
    if (change === "close") f.state.close();
    if (change === "abort") controller.abort();
    const refused = assert.rejects(action, /changed/);
    await run(); await refused;
    assert.equal(f.calls.length, 0, change);
  }
});

test("local note acknowledgement cannot patch a replacement member profile", async () => {
  const f = fixture(), pending = deferred<any>();
  f.handle(() => pending.promise);
  const save = f.state.save(f.state.scope!, "private notes", 2);
  f.state.close();
  const refused = assert.rejects(save, /changed/);
  pending.resolve({ text: "private notes", warnings: 2, updated_at: 10 });
  await refused;
  assert.equal(f.state.profile, null);
  assert.equal(f.calls[0].args.group, "room@g.us");
});
