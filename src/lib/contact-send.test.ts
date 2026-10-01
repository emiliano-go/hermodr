import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8").split('<script lang="ts">')[1].split("</script>")[0];
const tree = ts.createSourceFile("page.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const declaration = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "sendContacts");
assert.ok(declaration);
const compiled = ts.transpileModule(declaration.getText(tree), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;

function fixture() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  const warnings: unknown[] = [], failures: unknown[] = [];
  let queued!: (signal: AbortSignal) => Promise<unknown>;
  const context = {
    session: { activeAccount: "alpha", connected: true },
    chats: { selectedChat: "chat", refreshChats: async () => {} },
    messages: { accountGeneration: 1, reloadMessages: async (_chat: string) => {} },
    members: { chatGroup: null as { can_send: boolean } | null },
    composer: { editing: null, recording: false, draft: "Keep draft", replyingTo: { id: "reply" }, pending: [{ id: 1 }],
      enqueue: (run: typeof queued) => { queued = run; return Promise.resolve().then(() => run(new AbortController().signal)); } },
    ui: { notify: (message: unknown) => warnings.push(message), fail: (error: unknown) => failures.push(error) },
    invoke: async (command: string, args: Record<string, unknown>) => { calls.push({ command, args }); return { message_id: "sent-id", warning: null as string | null }; },
  };
  runInNewContext(compiled, context);
  return { context, calls, warnings, failures, send: (context as unknown as { sendContacts: (contacts: [string, string][], scope: { account: string; chat: string; generation: number }) => Promise<string> }).sendContacts };
}
const scope = { account: "alpha", chat: "chat", generation: 1 };
const contacts: [string, string][] = [["Synthetic", "12025550101"]];

test("contact sender rejects stale queued scope and read-only targets before native sends", async () => {
  for (const change of [(f: ReturnType<typeof fixture>) => { f.context.session.activeAccount = "beta"; },
    (f: ReturnType<typeof fixture>) => { f.context.messages.accountGeneration++; },
    (f: ReturnType<typeof fixture>) => { f.context.chats.selectedChat = "other"; },
    (f: ReturnType<typeof fixture>) => { f.context.session.connected = false; },
    (f: ReturnType<typeof fixture>) => { f.context.members.chatGroup = { can_send: false }; }]) {
    const f = fixture();
    const pending = f.send(contacts, scope); change(f);
    await assert.rejects(pending, /target changed/);
    assert.equal(f.calls.length, 0);
  }
});

test("acknowledged contact sends retain ID despite local warning or UI reload failure", async () => {
  const f = fixture();
  f.context.invoke = async (command, args) => { f.calls.push({ command, args }); return { message_id: "sent-id", warning: "Contact sent; local copy unavailable." }; };
  const failure = new Error("Synthetic reload failure");
  f.context.messages.reloadMessages = async () => { throw failure; };
  assert.equal(await f.send(contacts, scope), "sent-id");
  assert.deepEqual(f.warnings, ["Contact sent; local copy unavailable."]);
  assert.deepEqual(f.failures, [failure]);
  assert.equal(f.calls[0].args.account, "alpha");
  assert.equal(f.calls[0].args.chat, "chat");
  assert.equal(f.calls[0].args.contacts, contacts);
  assert.equal(f.context.composer.draft, "Keep draft");
  assert.deepEqual(f.context.composer.replyingTo, { id: "reply" });
  assert.deepEqual(f.context.composer.pending, [{ id: 1 }]);
});

test("native contact failures preserve original error and cannot produce sent success", async () => {
  const f = fixture(), failure = new Error("Synthetic native refusal");
  f.context.invoke = async () => { throw failure; };
  await assert.rejects(f.send(contacts, scope), (error) => error === failure);
  assert.equal(f.warnings.length, 0);
  f.context.invoke = async () => ({ message_id: "", warning: null });
  await assert.rejects(f.send(contacts, scope), /not acknowledged/);
});
