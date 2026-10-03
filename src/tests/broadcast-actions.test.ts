import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { broadcastSendReason, broadcastSendError, guardBroadcastSend } from "../lib/utils/broadcast.ts";
import { normalizeError } from "../lib/i18n/errors.ts";
import { t } from "../lib/i18n/localizer.ts";

const BROADCAST_SEND_REASON = t("error.state.broadcast_send");

function extract(path: string, names: string[], context: Record<string, any>) {
  const text = readFileSync(new URL(path, import.meta.url), "utf8").match(/<script(?![^>]*\bmodule\b)[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("leaf.ts", text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = names.map((name) => {
    const declaration = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === name);
    assert.ok(declaration, name);
    return declaration.getText(tree);
  }).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

test("broadcast destinations cannot consume composer state or dispatch while ordinary destinations stay writable", async (t) => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] },
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/broadcast-actions", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("../lib/state/composer.svelte.ts");
    const { ScheduledState } = await load("../lib/state/scheduled.svelte.ts");
    const actions = await load("../lib/state/message-actions.ts");
    const { chats } = await load("../lib/state/chats.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const list = "123@broadcast", ordinary = "12025550101@s.whatsapp.net";
    const calls: { command: string; args: any }[] = [];
    setHandler(async (command: string, args: any) => { calls.push({ command, args }); });
    chats.refreshChats = async () => {};
    messages.reloadMessages = async () => {};
    session.activeAccount = "synthetic-account";
    session.connected = true;
    session.settings.send_typing = true;
    const message = { chat: list, id: "message", sender: ordinary, from_me: true, text: "text", revoked: false,
      deleted: false, media_kind: null, media_once_kind: null, spoiler: false, system_kind: null };

    await t.test("all composer send/stage/edit/schedule entry points preserve blocked state", async () => {
      chats.selectedChat = list;
      const c = new ComposerState(), file = new File(["synthetic"], "synthetic.png", { type: "image/png" });
      const pending = { id: 1, file, kind: "image", caption: "caption", once: false, url: "" };
      c.draft = "Preserved draft"; c.pending = [pending]; c.replyingTo = message; c.recording = true;
      await c.send();
      await assert.rejects(c.sendPending(), /not supported/);
      await assert.rejects(c.sendVoice({ blob: new Blob(["synthetic"]), seconds: 1, waveform: [], viewOnce: false }), /not supported/);
      await assert.rejects(c.sendSoundClip(file, { account: "synthetic-account", chat: list, generation: messages.accountGeneration }), /not supported/);
      await c.stageFile(file);
      c.startEditing(message);
      assert.equal(c.pending[0], pending);
      assert.equal(c.replyingTo, message);
      assert.equal(c.recording, true);
      assert.equal(c.outgoing.length, 0);
      c.reportTyping(); c.typingSentAt = 1; c.stopTyping();
      c.pending = []; c.replyingTo = null; c.recording = false;
      assert.equal(await c.schedule(9999999999), false);
      assert.equal(c.draft, "Preserved draft");
      assert.equal(c.editing, null);
      assert.equal(calls.length, 0);
      assert.ok(String(ui.error).includes("not supported"));
    });

    await t.test("forwarding from a broadcast remains valid while all unsupported destinations fail before queue", async () => {
      chats.selectedChat = list; calls.length = 0;
      const selected = { message };
      ui.picking = selected;
      await assert.rejects(actions.forwardMessages([message], [ordinary, list]), /not supported/);
      assert.equal(calls.length, 0);
      assert.equal(ui.picking, selected);
      await actions.forwardMessages([message], [ordinary]);
      assert.equal(calls.length, 1);
      assert.equal(calls[0].command, "forward_message");
      assert.equal(calls[0].args.chat, list);
      assert.equal(calls[0].args.to, ordinary);
    });

    await t.test("reaction/edit/delete-for-everyone blocked; local star/delete still dispatch", async () => {
      calls.length = 0; chats.selectedChat = list;
      await actions.reactMessages([message], "👍");
      await assert.rejects(actions.saveEvent(list, "event", {}), /not supported/);
      assert.equal(actions.canDeleteForEveryone(message), false);
      ui.deleting = message;
      await actions.deleteMessage(true);
      assert.equal(ui.deleting, message);
      ui.bulkDelete = ["message"];
      await actions.deleteSelected(true);
      assert.deepEqual(ui.bulkDelete, ["message"]);
      assert.equal(calls.length, 0);
      await actions.starMessages([message], true);
      await actions.deleteMessage(false);
      assert.deepEqual(calls.map((call) => call.command), ["star", "delete_message"]);
      calls.length = 0;
      await actions.deleteSelected(false);
      assert.deepEqual(calls.map((call) => call.command), ["delete_messages"]);
    });

    await t.test("scheduled broadcast is visibly blocked without preventing ordinary stored destination", async () => {
      calls.length = 0; chats.selectedChat = list;
      const rows = [{ id: "blocked", chat: list, status: "pending", due_at: 1 },
        { id: "ordinary", chat: ordinary, status: "pending", due_at: 1 }];
      setHandler(async (command: string, args: any) => {
        calls.push({ command, args });
        if (command === "scheduled_messages") return rows;
        if (command === "send_scheduled_message") return true;
      });
      const state = new ScheduledState(); state.selectAccount("synthetic-account");
      await state.tick((run: (signal: AbortSignal) => Promise<unknown>) => run(new AbortController().signal), () => 2);
      const sent = calls.filter((call) => call.command === "send_scheduled_message");
      assert.equal(sent.length, 1);
      assert.equal(sent[0].args.id, "ordinary");
      assert.equal(state.error, BROADCAST_SEND_REASON);
    });
  } finally { await server.close(); }
});

test("expression, contact and forward pickers guard before reply/selection consumption", async () => {
  const calls: string[] = [];
  const list = "123@broadcast", ordinary = "12025550101@s.whatsapp.net";
  const context = extract("../lib/composer/ExpressionPicker.svelte", ["canSendTo", "send", "sendFromLibrary", "uploadFile", "sendMade", "clickSticker"], {
    chat: list, disabled: false, tab: "gif", making: "preserved", broadcastSendReason, broadcastSendError, guardBroadcastSend, normalizeError, t,
    pickerScope: () => ({ account: "synthetic" }), pickerCurrent: () => true, packRequest: 1,
    onerror: () => calls.push("error"), onclose: () => calls.push("close"), onsent: () => calls.push("sent"),
    takereply: () => { calls.push("reply"); return {}; }, enqueue: () => { calls.push("queue"); },
    invoke: () => calls.push("invoke"), sendAttachment: () => calls.push("upload"), toBase64: () => calls.push("bytes"),
  });
  context.sendFromLibrary("synthetic", "gif");
  await context.uploadFile(new File(["synthetic"], "synthetic.gif"));
  await context.sendMade(new File(["synthetic"], "synthetic.png"));
  context.clickSticker({ path: "synthetic" });
  assert.ok([...calls].every((call) => call === "error"));
  assert.equal(context.making, "preserved");
  context.chat = ordinary; context.disabled = true;
  context.sendFromLibrary("synthetic", "gif");
  assert.ok([...calls].every((call) => call === "error"));

  const picker = extract("../lib/chat/ChatPicker.svelte", ["toggle", "forward"], {
    busy: false, chosen: {}, picked: [list], failed: null, broadcastSendReason, broadcastSendError, guardBroadcastSend, normalizeError, t,
    onforward: () => calls.push("forward"), onclose: () => calls.push("close"),
  });
  picker.toggle(list); assert.equal(Object.keys(picker.chosen).length, 0);
  await picker.forward(); assert.equal(picker.failed, BROADCAST_SEND_REASON);
  picker.toggle(ordinary); assert.equal(picker.chosen[ordinary], true);

  const sharing = extract("../lib/contacts/ContactSharing.svelte", ["send"], {
    account: "synthetic", chat: list, connected: true, canSend: true, busy: false, revision: 1, generation: 1,
    broadcastSendReason, broadcastSendError, guardBroadcastSend, normalizeError, t, error: null, result: "preserved", selected: ["12025550101"],
    contacts: [{ name: "Synthetic", phone: "12025550101" }], onshare: () => calls.push("share"),
    shareContacts: () => calls.push("share"),
  });
  await sharing.send();
  assert.equal(sharing.error, BROADCAST_SEND_REASON);
  assert.equal(sharing.result, "preserved");
  assert.deepEqual(sharing.selected, ["12025550101"]);
  assert.ok(!calls.includes("share") && !calls.includes("forward"));
});
