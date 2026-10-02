import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { broadcastSendReason, isBroadcastList } from "../lib/utils/broadcast.ts";
import type { QuickRepliesState } from "../lib/state/quick-replies.svelte";

const source = readFileSync(new URL("../lib/state/quick-replies.svelte.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const compiled = compileModule(js, { generate: "server", filename: "quick-replies.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
const catalog = (message: string) => ({ complete: false, replies: [{ id: "a", shortcut: "hello", message, keywords: [], count: 0, associated_label_ids: [] }] });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => resolve = done); return { promise, resolve }; }
function fixture() {
  const session = { activeAccount: "synthetic-a", connected: true }, messages = { accountGeneration: 1 }, chats = { selectedChat: "room" };
  const calls: { command: string; args: any }[] = [];
  let handler: (command: string) => any = () => catalog("Current template");
  const invoke = (command: string, args: any) => { calls.push({ command, args }); return handler(command); };
  const State = new Function("invoke", "session", "messages", "chats", `${compiled}\nreturn QuickRepliesState;`)(invoke, session, messages, chats) as new () => QuickRepliesState;
  return { state: new State(), session, messages, chats, calls, handle: (next: typeof handler) => handler = next };
}

test("quick reply catalog rejects stale account, generation and replaced request responses", async () => {
  for (const change of ["account", "generation", "request"]) {
    const f = fixture(), pending = deferred<any>(); f.handle(() => pending.promise);
    const reading = f.state.refresh();
    if (change === "account") f.session.activeAccount = "synthetic-b";
    if (change === "generation") ++f.messages.accountGeneration;
    if (change === "request") { f.handle(() => catalog("Latest")); await f.state.refresh(); }
    pending.resolve(catalog("Obsolete private template")); await reading;
    assert.deepEqual(f.state.replies, change === "request" ? catalog("Latest").replies : []);
    if (change !== "request") assert.equal(f.state.scope("room"), null);
    assert.equal(f.calls[0].args.accountId, "synthetic-a"); f.state.reset();
  }
});

test("sync completion after a chat switch clears account-owned busy state without stale errors", async () => {
  const f = fixture(); await f.state.refresh(); const scope = f.state.scope("room")!, pending = deferred<void>();
  f.handle((command) => command === "sync_quick_replies" ? pending.promise : catalog("Updated"));
  const syncing = f.state.sync(scope); assert.equal(f.state.syncing, true); f.chats.selectedChat = "other";
  pending.resolve(); await syncing;
  assert.equal(f.state.syncing, false); assert.equal(f.state.error, "");
  assert.throws(() => f.state.current(scope), /changed/); f.state.reset();
});

test("first-read failures keep current request errors reachable without exposing old templates", async () => {
  const f = fixture(); f.handle(() => Promise.reject(new Error("Synthetic cache failure"))); await f.state.refresh();
  assert.equal(f.state.scope("room")?.account, "synthetic-a"); assert.equal(f.state.error, "Error: Synthetic cache failure");
  assert.deepEqual(f.state.replies, []); f.state.reset();
});

test("Enter inside a native picker cannot submit the enclosing message composer", () => {
  const component = readFileSync(new URL("../lib/composer/ComposerBar.svelte", import.meta.url), "utf8");
  const script = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("composer.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const fn = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "submitComposer")!;
  const body = ts.transpileModule(fn.getText(tree), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  let modal = true, sent = 0, prevented = 0;
  const createSubmit = (disabled: boolean) => new Function("document", "onsend", "disabled", "selectedChat", "broadcastSendReason", "isBroadcastList", `${body}\nreturn submitComposer;`)(
    { querySelector: () => modal ? {} : null }, () => ++sent, disabled, "synthetic@g.us", broadcastSendReason, isBroadcastList);
  const submit = createSubmit(false);
  const event = { preventDefault: () => ++prevented }; submit(event); assert.equal(sent, 0);
  modal = false; submit(event); assert.equal(sent, 1); assert.equal(prevented, 2);
  createSubmit(true)(event); assert.equal(sent, 1); assert.equal(prevented, 3);
});
