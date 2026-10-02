import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const page = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8");
const source = page.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
const tree = ts.createSourceFile("page.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);

function fixture(vote = false) {
  const names = vote ? ["votePoll"] : ["create"];
  const body = names.map((name) => {
    const declaration = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
    assert.ok(declaration, `Missing route function ${name}`);
    return declaration.getText(tree);
  }).join("\n");
  const compiled = ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const calls: { command: string; args: any }[] = [], refreshes: string[] = [], errors: unknown[] = [];
  const controller = new AbortController();
  const context = {
    session: { activeAccount: "poll-account" as string | null },
    chats: { selectedChat: "poll-chat" as string | null, refreshChats: async () => { refreshes.push("chats"); } },
    messages: { accountGeneration: 1, reloadMessages: async (chat: string) => { refreshes.push(`messages:${chat}`); },
      loadMarks: async (chat: string) => { refreshes.push(`marks:${chat}`); } },
    ui: { creating: "poll" as "poll" | "event" | null, fail: (error: unknown) => { errors.push(error); } },
    composer: { enqueue: (run: (signal: AbortSignal) => Promise<unknown>) => Promise.resolve().then(() => run(controller.signal)) },
    invoke: async (command: string, args: any) => { calls.push({ command, args }); },
    scrollToBottom: () => { refreshes.push("scroll"); },
  };
  runInNewContext(compiled, context);
  const actions = context as unknown as { create(value: unknown): Promise<void>; votePoll(message: any, options: string[]): Promise<void> };
  return { context, actions, calls, refreshes, errors, controller };
}

const poll = { question: "Synthetic question", options: ["First", "Second"], multi: false };
const changes = [
  (f: ReturnType<typeof fixture>) => { f.context.session.activeAccount = "other-account"; },
  (f: ReturnType<typeof fixture>) => { f.context.chats.selectedChat = "other-chat"; },
  (f: ReturnType<typeof fixture>) => { f.context.messages.accountGeneration++; },
];

test("queued creation retains captured kind and poll/event payload with captured account", async () => {
  const f = fixture();
  const work = f.actions.create({ ...poll, accountId: "foreign", chat: "foreign" });
  f.context.ui.creating = "event";
  await work;
  assert.equal(f.calls[0].command, "create_poll");
  assert.equal(f.calls[0].args.accountId, "poll-account");
  assert.equal(f.calls[0].args.chat, "poll-chat");
  assert.equal(f.calls[0].args.question, poll.question);
  assert.notEqual(f.calls[0].args.options, poll.options);
  assert.deepEqual(Array.from(f.calls[0].args.options), poll.options);
  assert.equal(f.calls[0].args.multi, false);
  assert.deepEqual(f.refreshes, ["messages:poll-chat", "marks:poll-chat", "chats", "scroll"]);
  const event = { name: "Synthetic event", start: 123, location: null };
  const g = fixture(); g.context.ui.creating = "event";
  await g.actions.create(event);
  assert.equal(g.calls[0].command, "create_event");
  assert.notEqual(g.calls[0].args.event, event);
  assert.equal(JSON.stringify(g.calls[0].args.event), JSON.stringify(event));
  assert.equal(g.calls[0].args.accountId, "poll-account");
});

test("creation rejects missing or stale queued scope before native dispatch", async () => {
  for (const change of changes) {
    const f = fixture(), work = f.actions.create(poll);
    change(f);
    await assert.rejects(work, /Conversation changed/);
    assert.equal(f.calls.length, 0);
    assert.equal(f.refreshes.length, 0);
  }
  for (const field of ["account", "chat", "kind"]) {
    const f = fixture();
    if (field === "account") f.context.session.activeAccount = null;
    else if (field === "chat") f.context.chats.selectedChat = null;
    else f.context.ui.creating = null;
    await assert.rejects(f.actions.create(poll), /Conversation changed/);
    assert.equal(f.calls.length, 0);
  }
});

test("native creation error propagates while local refresh error does not reverse successful dispatch", async () => {
  const f = fixture(), failure = new Error("synthetic native rejection");
  f.context.invoke = async () => { throw failure; };
  await assert.rejects(f.actions.create(poll), (error) => error === failure);
  assert.equal(f.refreshes.length, 0);
  const g = fixture(), reload = new Error("synthetic reload failure");
  g.context.messages.reloadMessages = async () => { throw reload; };
  await g.actions.create(poll);
  assert.equal(g.calls.length, 1);
  assert.deepEqual(g.errors, [reload]);
});

test("creation skips stale postflight and stops subsequent refreshes after scope switch", async () => {
  for (const change of changes) {
    const f = fixture();
    f.context.invoke = async (command, args) => { f.calls.push({ command, args }); change(f); };
    await f.actions.create(poll);
    assert.equal(f.refreshes.length, 0);
    assert.equal(f.errors.length, 0);
  }
  const g = fixture();
  g.context.messages.reloadMessages = async () => { g.refreshes.push("messages"); g.context.chats.selectedChat = "other-chat"; };
  await g.actions.create(poll);
  assert.deepEqual(g.refreshes, ["messages"]);
});

test("MessageList returns votePoll promise directly", () => {
  assert.ok(/onvote=\{votePoll\}/.test(page), "MessageList must return votePoll directly");
});

test("quiz dispatch captures own answer index and immutable payload before queue", async () => {
  const f = fixture(), payload = { question: "Original question", options: ["First", "Second"], correctIndex: 0 };
  const work = f.actions.create(payload);
  payload.correctIndex = 1; payload.question = "Changed question"; payload.options[0] = "Changed option";
  f.context.ui.creating = "event";
  await work;
  const call = f.calls[0];
  assert.equal(call.command, "create_quiz");
  assert.equal(call.args.question, "Original question");
  assert.equal(call.args.correctIndex, 0);
  assert.deepEqual(Array.from(call.args.options), ["First", "Second"]);
  assert.ok(Object.isFrozen(call.args.options));
  assert.equal(call.args.accountId, "poll-account");
  assert.equal(call.args.chat, "poll-chat");
  const g = fixture(), ordinary = Object.assign(Object.create({ correctIndex: 0 }), poll);
  await g.actions.create(ordinary);
  assert.equal(g.calls[0].command, "create_poll");
  assert.equal(Object.hasOwn(g.calls[0].args, "correctIndex"), false);
});

test("invalid own quiz answer cannot silently create an ordinary poll", async () => {
  for (const correctIndex of [-1, 2, 0.5, "0", NaN]) {
    const f = fixture();
    await assert.rejects(f.actions.create({ question: "Synthetic", options: ["First", "Second"], correctIndex }), /Invalid quiz correct answer/);
    assert.equal(f.calls.length, 0);
  }
});

test("queued vote captures target/options/account and refreshes marks after success", async () => {
  const f = fixture(true), message = { chat: "poll-chat", id: "poll-id" }, options = ["First"];
  const work = f.actions.votePoll(message, options);
  message.id = "other-id"; message.chat = "other-chat"; options.push("Second");
  await work;
  assert.equal(f.calls[0].command, "vote_poll");
  assert.equal(f.calls[0].args.accountId, "poll-account");
  assert.equal(f.calls[0].args.chat, "poll-chat");
  assert.equal(f.calls[0].args.id, "poll-id");
  assert.deepEqual(Array.from(f.calls[0].args.options), ["First"]);
  assert.deepEqual(f.refreshes, ["marks:poll-chat", "chats"]);
});

test("queued vote rejects stale or aborted scope before dispatch", async () => {
  for (const change of changes) {
    const f = fixture(true), work = f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]);
    change(f);
    await assert.rejects(work, /Conversation changed/);
    assert.equal(f.calls.length, 0);
  }
  const f = fixture(true);
  const work = f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]);
  f.controller.abort();
  await assert.rejects(work, { name: "AbortError" });
  assert.equal(f.calls.length, 0);
});

test("native vote failure reaches caller without swallowed act success", async () => {
  const f = fixture(true), failure = new Error("synthetic vote rejection");
  f.context.invoke = async () => { throw failure; };
  await assert.rejects(f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]), (error) => error === failure);
  assert.equal(f.refreshes.length, 0);
  assert.equal(f.errors.length, 0);
});

test("successful vote stays successful after local mark refresh fails", async () => {
  const f = fixture(true), failure = new Error("synthetic marks failure");
  f.context.messages.loadMarks = async () => { throw failure; };
  await f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]);
  assert.equal(f.calls.length, 1);
  assert.deepEqual(f.errors, [failure]);
});

test("vote suppresses stale postflight work and stale refresh error notifications", async () => {
  for (const change of changes) {
    const f = fixture(true);
    f.context.invoke = async () => { change(f); };
    await f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]);
    assert.equal(f.refreshes.length, 0);
    assert.equal(f.errors.length, 0);
  }
  const f = fixture(true);
  f.context.messages.loadMarks = async () => { f.context.chats.selectedChat = "other-chat"; throw new Error("obsolete reload"); };
  await f.actions.votePoll({ chat: "poll-chat", id: "poll-id" }, ["First"]);
  assert.equal(f.errors.length, 0);
  assert.equal(f.refreshes.length, 0);
});
