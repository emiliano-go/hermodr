import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { guardBroadcastSend } from "../lib/utils/broadcast.ts";

const source = readFileSync(new URL("../lib/state/composer.svelte.ts", import.meta.url), "utf8");
const tree = ts.createSourceFile("composer.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const state = tree.statements.find((statement) => ts.isClassDeclaration(statement) && statement.name?.text === "ComposerState") as ts.ClassDeclaration;
const member = (name: string) => state.members.find((entry) => entry.name?.getText(tree) === name)!.getText(tree);
const code = ts.transpileModule(`class Composer {
  ${["outbox", "accountSeq", "uploadsAbort", "stagingTickets", "attachmentChatGenerations", "enqueue", "sendSoundClip", "resetAccount", "resetUndo", "resetHistory"].map(member).join("\n")}
}`, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;

function gate() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => { release = resolve; });
  return { promise, release };
}

function fixture() {
  const session = { activeAccount: "account-a", connected: true };
  const uploads: { file: File; args: Record<string, unknown>; signal: AbortSignal }[] = [];
  const errors: string[] = [], reloads: string[] = [];
  let sent = 0, refreshes = 0, scrolled = 0;
  const hooks = { upload: async (signal: AbortSignal) => { signal.throwIfAborted(); }, reload: async () => {} };
  const chats = { selectedChat: "room@g.us", refreshChats: async () => { refreshes++; } };
  const messages = { accountGeneration: 1, reloadMessages: async (chat: string) => { reloads.push(chat); await hooks.reload(); } };
  const members = { chatGroup: { can_send: true } };
  const bindings = { session, chats, messages, members, guardBroadcastSend, ui: { fail: (error: unknown) => errors.push(String(error)) },
    sendAttachment: async (file: File, args: Record<string, unknown>, signal: AbortSignal) => {
      uploads.push({ file, args, signal }); await hooks.upload(signal); sent++;
    } };
  const Composer = new Function(...Object.keys(bindings), code + "\nreturn Composer;")(...Object.values(bindings));
  const pending = [{ file: new File(["unrelated"], "unrelated.png"), url: "" }];
  const reply = { chat: chats.selectedChat, id: "reply" };
  const composer = new Composer();
  Object.assign(composer, { draft: "draft stays", drafts: { [chats.selectedChat]: "draft stays" }, pending, replyingTo: reply,
    chosenMentions: [{ name: "Alice", jid: "alice@s.whatsapp.net" }], editing: null, recording: false, outgoing: [], sentHistory: [], attachmentRecoveries: [],
    draftUndo: { reset() {} }, host: { scrollToBottom() { scrolled++; } } });
  const scope = { account: session.activeAccount, chat: chats.selectedChat, generation: messages.accountGeneration };
  const clip = new File(["synthetic audio"], "chosen.wav", { type: "audio/wav" });
  return { composer, session, chats, messages, members, uploads, errors, reloads, hooks, scope, clip, pending, reply,
    sent: () => sent, refreshes: () => refreshes, scrolled: () => scrolled };
}

test("soundboard sends exactly the chosen clip through enqueue without consuming the composer", async () => {
  const f = fixture();
  await f.composer.sendSoundClip(f.clip, f.scope);
  assert.equal(f.sent(), 1); assert.equal(f.uploads.length, 1);
  assert.equal(f.uploads[0].file, f.clip); assert.deepEqual(f.uploads[0].args, { chat: "room@g.us" });
  assert.equal(f.uploads[0].signal.aborted, false);
  assert.equal(f.composer.draft, "draft stays"); assert.equal(f.composer.replyingTo, f.reply); assert.equal(f.composer.pending, f.pending);
  assert.deepEqual(f.composer.chosenMentions, [{ name: "Alice", jid: "alice@s.whatsapp.net" }]);
  assert.deepEqual(f.reloads, ["room@g.us"]); assert.equal(f.refreshes(), 1); assert.equal(f.scrolled(), 1);
});

test("soundboard rejects stale account, same-JID generation and queued conversation or permission changes", async () => {
  for (const change of ["account", "generation", "chat", "disconnected", "editing", "recording", "group"] as const) {
    for (const queued of [false, true]) {
      const f = fixture(), started = gate(), blocked = gate();
      let ahead = Promise.resolve();
      if (queued) { ahead = f.composer.enqueue(async () => { started.release(); await blocked.promise; }); await started.promise; }
      const mutate = () => {
        if (change === "account") f.session.activeAccount = "account-b";
        else if (change === "generation") f.messages.accountGeneration++;
        else if (change === "chat") f.chats.selectedChat = "other@g.us";
        else if (change === "disconnected") f.session.connected = false;
        else if (change === "editing") f.composer.editing = { id: "edit" };
        else if (change === "recording") f.composer.recording = true;
        else f.members.chatGroup.can_send = false;
      };
      if (!queued) mutate();
      const failed = assert.rejects(f.composer.sendSoundClip(f.clip, f.scope), /Conversation changed/);
      if (queued) mutate();
      blocked.release(); await Promise.all([ahead, failed]);
      assert.equal(f.uploads.length, 0, `${change}, queued=${queued}`);
      assert.equal(f.sent(), 0); assert.deepEqual(f.reloads, []); assert.equal(f.refreshes(), 0);
    }
  }
});

test("real account reset aborts the active clip upload and cancels queued old-account clips", async () => {
  const f = fixture(), started = gate(), blocked = gate();
  f.hooks.upload = async (signal) => { started.release(); await blocked.promise; signal.throwIfAborted(); };
  const upload = assert.rejects(f.composer.sendSoundClip(f.clip, f.scope), { name: "AbortError" });
  await started.promise;
  const queued = assert.rejects(f.composer.sendSoundClip(f.clip, f.scope), /Account changed before sending/);
  f.composer.resetAccount(); f.session.activeAccount = "account-b"; f.messages.accountGeneration++;
  blocked.release(); await Promise.all([upload, queued]);
  assert.equal(f.uploads.length, 1); assert.equal(f.uploads[0].signal.aborted, true); assert.equal(f.sent(), 0);
  assert.deepEqual(f.reloads, []); assert.equal(f.refreshes(), 0); assert.equal(f.scrolled(), 0); assert.deepEqual(f.errors, []);
});

test("a completed clip send stays successful when the subsequent UI reload fails", async () => {
  const f = fixture(); f.hooks.reload = async () => { throw new Error("synthetic reload failed"); };
  await assert.doesNotReject(f.composer.sendSoundClip(f.clip, f.scope));
  assert.equal(f.sent(), 1); assert.equal(f.uploads.length, 1);
  assert.deepEqual(f.errors, ["Error: synthetic reload failed"]); assert.equal(f.refreshes(), 1);
  assert.equal(f.composer.draft, "draft stays"); assert.equal(f.composer.pending, f.pending); assert.equal(f.composer.replyingTo, f.reply);
});
