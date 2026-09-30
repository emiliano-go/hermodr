import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { ComposerHistory, type DraftSnapshot } from "./utils/composer-history.ts";

test("draft history groups adjacent typing, separates paste and bounds snapshots", () => {
  const history = new ComposerHistory();
  const snap = (text: string, start = text.length, end = start): DraftSnapshot => ({ text, start, end, mentions: [] });
  history.record(snap(""), snap("a"), "insertText", 0);
  history.record(snap("a"), snap("ab"), "insertText", 300);
  history.record(snap("ab"), snap("abc"), "insertFromPaste", 400);
  assert.equal(history.undo(snap("abc"))?.text, "ab");
  assert.equal(history.undo(snap("ab"))?.text, "");
  assert.equal(history.redo(snap(""))?.text, "ab");
  history.record(snap("ab", 0), snap("xab", 1), "insertText", 500);
  assert.equal(history.redo(snap("xab")), undefined);
  assert.equal(history.undo(snap("xab"))?.start, 0);
  history.reset();
  for (let i = 0; i < 105; i++) history.record(snap(String(i)), snap(String(i + 1)));
  let current = snap("105"), count = 0;
  for (let previous; (previous = history.undo(current)); count++) current = previous;
  assert.equal(count, 100);
  assert.equal(current.text, "5");
});

test("composer shortcuts undo typing, paste, mention and emoji edits within current chat", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/composer-undo", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL(".", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("./state/composer.svelte.ts");
    const { chats } = await load("./state/chats.svelte.ts");
    const { messages } = await load("./state/messages.svelte.ts");
    const { session } = await load("./state/session.svelte.ts");
    const { keybinds, setBinding, resetBindings, matchesDraftHistory, ACTIONS } = await load("./utils/keybinds.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const calls: string[] = [];
    setHandler((command: string) => { calls.push(command); });
    chats.selectedChat = "synthetic-a@s";
    session.settings.send_typing = false;
    const composer = new ComposerState();
    const input = { value: "", selectionStart: 0, selectionEnd: 0, focus() {},
      setSelectionRange(start: number, end: number) { this.selectionStart = start; this.selectionEnd = end; } };
    composer.inputEl = input;
    composer.resetUndo();
    const edit = (text: string, kind = "insertText", caret = text.length) => {
      composer.onComposerBeforeInput({ inputType: kind });
      input.value = text;
      input.setSelectionRange(caret, caret);
      composer.onComposerInput({ currentTarget: input });
    };
    const key = (meta = false, shift = false, value = "z", alt = false) => {
      let prevented = false;
      const event = { key: value, ctrlKey: !meta, metaKey: meta, shiftKey: shift, altKey: alt,
        isComposing: false, preventDefault() { prevented = true; } };
      composer.onComposerKey(event);
      return { event, prevented };
    };
    const settle = async () => { await Promise.resolve(); await Promise.resolve(); input.value = composer.draft; };
    edit("h"); edit("hi");
    assert.equal(key().prevented, true); await settle();
    assert.equal(composer.draft, ""); assert.equal(input.selectionStart, 0);
    assert.equal(key(false, true, "Z").prevented, true); await settle();
    assert.equal(composer.draft, "hi");
    edit("hi paste", "insertFromPaste");
    key(true); await settle(); assert.equal(composer.draft, "hi");
    key(true, true); await settle(); assert.equal(composer.draft, "hi paste");

    composer.draft = "Hi @Ad"; input.value = composer.draft; input.setSelectionRange(6, 6); composer.resetUndo();
    await composer.selectMention({ jid: "synthetic@lid", name: "Ada", token: "Ada" });
    assert.equal(composer.draft, "Hi @Ada ");
    assert.deepEqual(composer.mentionPayload().jids, ["synthetic@lid"]);
    key(); await settle(); assert.equal(composer.draft, "Hi @Ad"); assert.equal(input.selectionStart, 6);
    assert.deepEqual(composer.chosenMentions, []);
    key(false, true); await settle();
    assert.equal(composer.draft, "Hi @Ada "); assert.deepEqual(composer.mentionPayload().jids, ["synthetic@lid"]);
    composer.emojiToken = { query: "smile", start: 0 }; composer.draft = ":smile";
    input.value = composer.draft; input.setSelectionRange(6, 6); composer.resetUndo();
    composer.selectEmoji("😀"); await settle(); assert.equal(composer.draft, "😀");
    key(); await settle(); assert.equal(composer.draft, ":smile");
    key(false, true); await settle(); assert.equal(composer.draft, "😀");
    composer.draft = "word"; input.value = "word"; input.setSelectionRange(1, 3); composer.resetUndo();
    await composer.insertAtCaret("😀"); assert.equal(composer.draft, "w😀d");
    key(); await settle(); assert.equal(composer.draft, "word");
    assert.deepEqual([input.selectionStart, input.selectionEnd], [1, 3]);

    composer.draft = ":ok"; input.value = composer.draft; input.setSelectionRange(3, 3); composer.resetUndo();
    composer.emojiTable = [{ emoji: "👌", shortcodes: ["ok"], label: "ok", tags: [], hexcode: "1F44C", group: 1, order: 1 }];
    edit(":ok:"); await settle(); assert.equal(composer.draft, "👌");
    key(); await settle(); assert.equal(composer.draft, ":ok:");

    setBinding("undoDraft", { key: "u", ctrl: true, meta: false, alt: false, shift: false });
    assert.equal(matchesDraftHistory(key(true).event, "undoDraft"), false);
    assert.equal(key(false, false, "u").prevented, true); await settle(); assert.equal(composer.draft, ":ok");
    resetBindings();
    assert.ok(ACTIONS.find((action: { id: string; description: string }) => action.id === "undoDraft").description.includes("Message deletion and chat clearing are excluded"));
    assert.equal(key(false, false, "z", true).prevented, false);

    const saved = composer.draft;
    chats.selectedChat = "synthetic-b@s"; composer.draft = "other draft"; composer.resetUndo();
    key(); await settle(); assert.equal(composer.draft, "other draft");
    edit("other draft!", "insertFromPaste"); key(); await settle(); assert.equal(composer.draft, "other draft");
    chats.selectedChat = "synthetic-a@s"; composer.draft = composer.drafts[chats.selectedChat]; composer.resetUndo();
    key(); await settle(); assert.equal(composer.draft, saved);
    composer.resetAccount(); key(); await settle(); assert.equal(composer.draft, "");

    composer.draft = "synthetic send"; input.value = composer.draft;
    messages.reloadMessages = async () => {}; chats.refreshChats = async () => {};
    await composer.send(); key(); await settle(); assert.equal(composer.draft, "");
    assert.deepEqual(calls, ["send_text"]);
    assert.equal(keybinds.undoDraft.key, "z");
  } finally { await server.close(); }
});
