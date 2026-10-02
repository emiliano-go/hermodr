import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { parse } from "svelte/compiler";
import { BROADCAST_SEND_REASON, broadcastSendReason, guardBroadcastSend, isBroadcastList } from "./utils/broadcast.ts";

function methods(path: string, names: string[], context: Record<string, unknown>) {
  const text = readFileSync(new URL(path, import.meta.url), "utf8");
  const source = text.match(/<script(?![^>]*\bmodule\b)[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("component.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = names.map((name) => {
    const node = tree.statements.find((entry) => ts.isFunctionDeclaration(entry) && entry.name?.text === name);
    assert.ok(node, name);
    return node.getText(tree);
  }).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context as Record<string, any>;
}

test("broadcast destination guard excludes status and ordinary destinations", () => {
  for (const chat of ["123@broadcast", "named-list@broadcast", "status-list@broadcast"]) {
    assert.equal(isBroadcastList(chat), true);
    assert.equal(broadcastSendReason(chat), BROADCAST_SEND_REASON);
    assert.throws(() => guardBroadcastSend(chat), { message: BROADCAST_SEND_REASON });
  }
  for (const chat of [null, undefined, "", "status@broadcast", "123@lid", "123@s.whatsapp.net", "123@g.us", "123@newsletter", "123@broadcast.example"]) {
    assert.equal(isBroadcastList(chat), false);
    assert.equal(broadcastSendReason(chat), null);
    assert.doesNotThrow(() => guardBroadcastSend(chat));
  }
});

test("broadcast UI shows cached evidence, contact naming and disabled send controls", async (t) => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/broadcast", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: BroadcastInfo } = await load("./chat/BroadcastInfo.svelte");
    const { default: ComposerBar } = await load("./composer/ComposerBar.svelte");
    const { default: PollCard } = await load("./messages/cards/PollCard.svelte");
    const { default: EventCard } = await load("./messages/cards/EventCard.svelte");
    const { default: MessageCard } = await load("./messages/cards/MessageCard.svelte");
    const { members } = await load("./state/members.svelte.ts");
    const { session } = await load("./state/session.svelte.ts");
    session.started = false;
    const target = "123@broadcast";
    const info = { chat: target, recipients: ["1@s.whatsapp.net", "2@lid", "3@s.whatsapp.net", "4@lid"], source_timestamp: 1700000000 };
    members.identities = {
      "1@s.whatsapp.net": { saved_name: "Saved <friend>", contact_saved: true },
      "2@lid": { saved_name: "Saved LID alias", contact_saved: true, number: "12025550101" },
      "3@s.whatsapp.net": { contact_saved: false, number: "12025550101", push_name: "Unsaved push" },
      "4@lid": { contact_saved: false, username: "synthetic-user" },
    };
    const named: string[] = [];
    const nameOf = (jid: string) => { named.push(jid); return members.displayName(null, jid); };

    await t.test("recipient renderer delegates every exact JID to existing naming rules", () => {
      const body = render(BroadcastInfo, { props: { info, loading: false, error: null, nameOf } }).body;
      assert.deepEqual([...named], info.recipients);
      assert.ok(/Saved &lt;friend(?:>|&gt;)/.test(body) && body.includes("Saved LID alias"));
      assert.ok(body.includes("~Unsaved push") && body.includes("@synthetic-user"));
      assert.ok(body.includes("Cached from a received message") && body.includes('datetime="2023-11-14T22:13:20.000Z"'));
      assert.ok(!/current members|all recipients|<button/i.test(body));
    });

    await t.test("loading, missing and error metadata cannot invent recipients", () => {
      for (const [loading, error, data, expected] of [
        [true, null, info, "Loading recipient details"], [false, null, null, "Recipient details unavailable"],
        [false, "Synthetic failure", info, "Synthetic failure"], [false, null, { ...info, recipients: [] }, "Recipient details unavailable"],
      ]) {
        const body = render(BroadcastInfo, { props: { info: data, loading, error, nameOf } }).body;
        assert.ok(body.includes(expected) && !body.includes("<li") && !body.includes("0 recipients"));
      }
      for (const source_timestamp of [0, -1, NaN, Number.MAX_SAFE_INTEGER]) {
        const body = render(BroadcastInfo, { props: { info: { ...info, source_timestamp }, loading: false, error: null, nameOf } }).body;
        assert.ok(body.includes("Message time unavailable") && !body.includes("<time"));
      }
    });

    await t.test("disabled composer retains draft and local controls while sending controls are disabled", () => {
      const noop = () => {};
      const props = { disabled: true, connected: true, account: "synthetic", generation: 1, selectedChat: target,
        draft: "Preserved draft", composerInput: undefined, replyingTo: null, replyAuthor: "", replySnippet: "", editing: null,
        pending: [], recording: false, mentionMatches: [], mentionIndex: 0, emojiToken: null, emojiMatches: [], emojiIndex: 0,
        pickerTab: null, receiptsHidden: false, typingHidden: false,
        enqueue: async () => { throw new Error("SSR must never enqueue"); }, takereply: () => ({}),
        onselectmention: noop, onselectemoji: noop, onpickeremoji: noop, onpickersent: noop, onpickererror: noop,
        onstage: noop, oncreatekind: noop, oninput: noop, onkey: noop, onsend: noop, oncancelreply: noop, oncanceledit: noop,
        onremove: noop, ontoggleonce: noop, onsendvoice: noop, onvoiceerror: noop, onreceipts: noop, ontyping: noop,
      };
      const body = render(ComposerBar, { props }).body;
      assert.ok(/<textarea\b[^>]*disabled[^>]*>Preserved draft<\/textarea>/.test(body));
      for (const label of ["Attach", "Send", "Stickers", "Emoji"]) {
        const tag = body.match(new RegExp(`<button\\b[^>]*aria-label="${label}"[^>]*>`))?.[0];
        assert.ok(tag?.includes("disabled"), label);
      }
      for (const label of ["Hide read receipts here", "Stop sending typing here"]) {
        const tag = body.match(new RegExp(`<button\\b[^>]*aria-label="${label}"[^>]*>`))?.[0];
        assert.ok(tag && !tag.includes("disabled"), label);
      }
      const recording = render(ComposerBar, { props: { ...props, recording: true } }).body;
      assert.ok(recording.match(/<button\b[^>]*aria-label="Send voice message"[^>]*>/)?.[0].includes("disabled"));
      assert.ok(!recording.match(/<button\b[^>]*aria-label="Discard recording"[^>]*>/)?.[0].includes("disabled"));
    });

    await t.test("broadcast poll/event send controls disable while tallies and call links remain", () => {
      const poll = { id: "p", name: "Question", options: ["Yes", "No"], multi: false,
        votes: [{ voter: "@me", options: ["Yes"] }], quiz: null };
      const body = render(PollCard, { props: { poll, question: "Question", namer: (jid: string) => jid, picture: () => null,
        onvote: async () => {}, scope: { account: "synthetic", chat: target, generation: 1 } } }).body;
      const text = body.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
      assert.ok(text.includes(BROADCAST_SEND_REASON) && text.includes("1 vote") && text.includes("View votes"), text);
      assert.ok([...body.matchAll(/<button\b[^>]*class="option[^>]*>/g)].every((match) => match[0].includes("disabled")));
      const direct = render(MessageCard, { props: { variant: "poll", poll, question: "Question", namer: (jid: string) => jid,
        picture: () => null, onvote: async () => {} } }).body;
      assert.ok(direct.includes("Question") && !direct.includes(BROADCAST_SEND_REASON));
      const event = { id: "e", name: "Event", canceled: false, responses: [{ responder: "x", response: "going" }],
        start: null, end: null, location: null, description: null, link: "https://example.invalid" };
      const eventBody = render(EventCard, { props: { event, title: "Event", chat: target, onrespond: async () => {},
        onedit: () => {}, oncancel: async () => {}, onopenurl: () => {} } }).body;
      const eventText = eventBody.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
      assert.ok(eventText.includes(BROADCAST_SEND_REASON) && eventText.includes("1 going") && eventText.includes("Join call"), eventText);
      assert.ok(eventBody.match(/<button\b[^>]*>\s*Edit<\/button>/)?.[0].includes("disabled"));
      assert.ok(eventBody.match(/<button\b[^>]*>\s*Cancel event<\/button>/)?.[0].includes("disabled"));
      assert.ok(!eventBody.match(/<button\b[^>]*>[^]*?Join call<\/button>/)?.[0].includes("disabled"));
    });
  } finally { await server.close(); }
});

test("actual composer callbacks reject disabled sending before mutation and queued picker dispatch", async () => {
  const calls: string[] = [];
  const bindings: Record<string, any> = { disabled: true, selectedChat: "123@broadcast", broadcastSendReason,
    document: { querySelector: () => null }, onsend: () => calls.push("send"),
    onstage: () => calls.push("stage"), enqueue: (run: (signal: AbortSignal) => Promise<unknown>) => Promise.resolve().then(() => run(new AbortController().signal)) };
  const f = methods("./composer/ComposerBar.svelte", ["submitComposer", "attach", "enqueuePicker"], bindings);
  f.submitComposer({ preventDefault() {} });
  const input = { files: [new File(["synthetic"], "x.png")], value: "picked" };
  f.attach({ currentTarget: input });
  assert.equal(input.value, "");
  await assert.rejects(f.enqueuePicker(async () => calls.push("picker")), /not supported/);
  assert.equal(calls.length, 0);
  f.disabled = false;
  const queued = f.enqueuePicker(async () => calls.push("picker"));
  f.disabled = true;
  await assert.rejects(queued, /not supported/);
  assert.equal(calls.length, 0);

  const text = readFileSync(new URL("./composer/ComposerBar.svelte", import.meta.url), "utf8");
  const ast = parse(text, { modern: true }) as any;
  const callbacks: string[] = [];
  const walk = (node: any) => {
    if (!node || typeof node !== "object") return;
    if (node.type === "Attribute") for (const value of Array.isArray(node.value) ? node.value : node.value && typeof node.value === "object" ? [node.value] : []) {
      if (value.type === "ExpressionTag" && value.expression.type === "ArrowFunctionExpression") {
        const code = text.slice(value.expression.start, value.expression.end);
        if (/oncreatekind\(|filePicker\?\.click|onsendvoice\(|onquickreply\(|onschedule\(|oninput\(|onkey\(|onbeforeinput\(|onsoundclip\(|onstage\(/.test(code)) callbacks.push(code);
      }
    }
    for (const value of Object.values(node)) if (Array.isArray(value)) value.forEach(walk); else if (value && typeof value === "object") walk(value);
  };
  walk(ast.fragment);
  assert.ok(callbacks.length >= 10);
  for (const callback of callbacks) {
    const context: Record<string, any> = { disabled: true, selectedChat: "123@broadcast", broadcastSendReason,
      filePicker: { click: () => calls.push("file") }, attachMenu: true, dismissedSlash: null, updateCaret: () => {},
      ...Object.fromEntries(["oncreatekind", "onsendvoice", "onquickreply", "onschedule", "oninput", "onkey", "onbeforeinput", "onsoundclip", "onstage"].map((key) => [key, () => calls.push(key)])) };
    const body = ts.transpileModule(`var callback = ${callback};`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
    runInNewContext(body, context);
    try { await context.callback({}, { chat: "123@broadcast" }); } catch (error) { assert.ok(/disabled|not supported|target changed/.test(String(error))); }
  }
  assert.equal(calls.length, 0);
});

test("voice finish cannot send while disabled or after permission changes during stop", () => {
  let stopped = 0, sent = 0, released = 0;
  const recorder = { onstop: null as null | (() => void), stop: () => { stopped++; } };
  const f = methods("./composer/VoiceRecorder.svelte", ["finish"], { disabled: true, recorder, elapsed: 2,
    Blob, chunks: ["synthetic"], once: false, waveform: () => [], release: () => { released++; }, onsend: () => { sent++; } });
  f.finish(); assert.equal(stopped, 0);
  f.disabled = false; f.finish(); assert.equal(stopped, 1);
  f.disabled = true; recorder.onstop!();
  assert.equal(sent, 0); assert.equal(released, 1);
});
