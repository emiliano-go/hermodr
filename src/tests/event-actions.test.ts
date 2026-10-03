import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { broadcastSendReason, broadcastSendError, guardBroadcastSend } from "../lib/utils/broadcast.ts";
import { uiError } from "../lib/state/localized.ts";

function fixture() {
  const calls: { command: string; args: any }[] = [], refreshes: string[] = [], failures: unknown[] = [];
  const c: any = { session: { activeAccount: "a" }, messages: { accountGeneration: 1,
    reloadMessages: async (chat: string) => { refreshes.push(`messages:${chat}`); },
    loadMarks: async (chat: string) => { refreshes.push(`marks:${chat}`); } },
    chats: { selectedChat: "actual@g.us" }, ui: { fail: (error: unknown) => failures.push(error) },
    structuredClone, guardBroadcastSend, broadcastSendReason, broadcastSendError, uiError,
    invoke: async (command: string, args: any) => { calls.push({ command, args }); },
    composer: { enqueue: async (task: any) => task(new AbortController().signal) } };
  for (const [path, names] of [["../lib/state/message-actions.ts", ["saveEvent", "eventFields"]],
    ["../routes/+page.svelte", ["respondEvent"]]] as const) {
    const source = readFileSync(new URL(path, import.meta.url), "utf8");
    const script = path.endsWith(".svelte") ? source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1] : source;
    const tree = ts.createSourceFile(path, script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
    const body = names.map((name) => {
      const node = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
      assert.ok(node, name); return node.getText(tree);
    }).join("\n");
    runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext } }).outputText.replace(/^export /gm, ""), c);
  }
  return { c, calls, refreshes, failures };
}

test("event edit captures immutable metadata, account and target before enqueue", async () => {
  const { c, calls, refreshes } = fixture();
  let queued!: (signal: AbortSignal) => Promise<void>, release!: () => void;
  c.composer.enqueue = (task: typeof queued) => { queued = task; return new Promise<void>((yes) => { release = yes; }); };
  const fields = { name: "Original", extra_guests_allowed: true, is_scheduled_call: null, has_reminder: true, reminder_offset_sec: 900 };
  const saving = c.saveEvent("actual@g.us", "event", fields);
  fields.name = "Changed"; fields.has_reminder = false;
  await queued(new AbortController().signal); release(); await saving;
  assert.equal(calls.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(calls[0])), { command: "edit_event", args: { accountId: "a", chat: "actual@g.us", id: "event",
    event: { name: "Original", extra_guests_allowed: true, is_scheduled_call: null, has_reminder: true, reminder_offset_sec: 900 } } });
  assert.deepEqual(refreshes, ["messages:actual@g.us", "marks:actual@g.us"]);
  assert.deepEqual(JSON.parse(JSON.stringify(c.eventFields({ ...calls[0].args.event, description: null, start: 0,
    end: 100, location: null, link: null }))), { ...calls[0].args.event, description: null, start: 0, end: 100, location: null, link: null });
});

test("queued event edits and RSVPs reject changed account, generation, chat and cancellation without IPC", async () => {
  for (const operation of ["edit", "respond"]) for (const change of ["account", "generation", "chat", "abort"]) {
    const { c, calls } = fixture();
    c.composer.enqueue = async (task: (signal: AbortSignal) => Promise<void>) => {
      const controller = new AbortController();
      if (change === "account") c.session.activeAccount = "b";
      if (change === "generation") c.messages.accountGeneration++;
      if (change === "chat") c.chats.selectedChat = "new@g.us";
      if (change === "abort") controller.abort();
      await task(controller.signal);
    };
    await assert.rejects(operation === "edit" ? c.saveEvent("actual@g.us", "event", { name: "Name" })
      : c.respondEvent({ chat: "actual@g.us", id: "event" }, "going", 2));
    assert.equal(calls.length, 0, `${operation}/${change}`);
  }
});

test("event RSVP failures propagate and successful sends never become failed retries after UI reload errors", async () => {
  for (const operation of ["edit", "respond"]) {
    const failed = fixture(); failed.c.invoke = async () => { throw new Error("Native send rejected"); };
    await assert.rejects(operation === "edit" ? failed.c.saveEvent("actual@g.us", "event", { name: "Name" })
      : failed.c.respondEvent({ chat: "actual@g.us", id: "event" }, "maybe", 3), /Native send rejected/);
    assert.deepEqual(failed.refreshes, []);
    const sent = fixture();
    sent.c.messages.reloadMessages = sent.c.messages.loadMarks = async () => { throw new Error("Reload failed after send"); };
    await (operation === "edit" ? sent.c.saveEvent("actual@g.us", "event", { name: "Name" })
      : sent.c.respondEvent({ chat: "actual@g.us", id: "event" }, "maybe", 3));
    assert.equal(sent.calls.length, 1); assert.equal(sent.failures.length, 1);
    if (operation === "respond") assert.deepEqual(JSON.parse(JSON.stringify(sent.calls[0].args)),
      { accountId: "a", chat: "actual@g.us", id: "event", response: "maybe", extraGuestCount: 3 });
  }
});

test("late event send completion cannot refresh another account or conversation", async () => {
  for (const operation of ["edit", "respond"]) for (const change of ["account", "generation", "chat"]) {
    const { c, refreshes } = fixture();
    let release!: () => void;
    c.invoke = () => new Promise<void>((yes) => { release = yes; });
    const sending = operation === "edit" ? c.saveEvent("actual@g.us", "event", { name: "Name" })
      : c.respondEvent({ chat: "actual@g.us", id: "event" }, "not_going");
    if (change === "account") c.session.activeAccount = "b";
    if (change === "generation") c.messages.accountGeneration++;
    if (change === "chat") c.chats.selectedChat = "new@g.us";
    release(); await sending; assert.deepEqual(refreshes, [], `${operation}/${change}`);
  }
});
