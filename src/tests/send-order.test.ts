import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

function gate() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => { release = resolve; });
  return { promise, release };
}

test("composer serializes rapid text, attachment batches, and voice preparation", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/send-order", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("../lib/state/composer.svelte.ts");
    const { chats } = await load("../lib/state/chats.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { sendFixture } = await server.ssrLoadModule("/ipc.ts");
    chats.selectedChat = "test@s";
    chats.refreshChats = async () => {};
    messages.reloadMessages = async () => {};
    const errors: string[] = [];
    ui.fail = (error: unknown) => errors.push(String(error));
    const composer = new ComposerState();
    const sends: string[] = [];
    let blocked = gate();
    let started = gate();
    sendFixture.before = async (command: string, args?: Record<string, unknown>) => {
      if (command === "send_media") {
        sends.push(String(args?.name));
        if (args?.name === "first.png") { started.release(); await blocked.promise; }
      } else if (command === "send_text") {
        sends.push(String(args?.text));
        if (args?.text === "1") { started.release(); await blocked.promise; }
      } else if (command === "send_voice") sends.push("voice");
    };
    const numbers = ["1", "2", "3", "4", "5", "6"];
    const rapid = numbers.map((text) => { composer.draft = text; return composer.send(); });
    await started.promise;
    assert.deepEqual(sends, ["1"]);
    blocked.release();
    await Promise.all(rapid);
    assert.deepEqual(sends, numbers);
    sends.length = 0; blocked = gate(); started = gate();
    const item = (id: number, name: string) => ({ id, file: new File(["synthetic"], name), url: "", kind: "image", caption: "", once: false });
    composer.pending = [item(1, "first.png"), item(2, "second.png")];
    const batch = composer.sendPending();
    await started.promise;
    composer.draft = "later text";
    const text = composer.send();
    blocked.release();
    await Promise.all([batch, text]);
    assert.deepEqual(sends, ["first.png", "second.png", "later text"]);
    assert.equal(composer.outgoing.length, 0);

    for (const reset of [false, true]) {
      sends.length = 0; blocked = gate(); started = gate();
      const delayed = blocked;
      class SlowBlob extends Blob {
        override async arrayBuffer() { started.release(); await delayed.promise; return super.arrayBuffer(); }
      }
      const voice = composer.sendVoice({ blob: new SlowBlob(["audio"]), seconds: 1, waveform: [], viewOnce: false });
      await started.promise;
      if (reset) composer.resetAccount();
      composer.draft = "after voice";
      const after = composer.send();
      blocked.release();
      await Promise.all([voice, after]);
      assert.deepEqual(sends, reset ? ["after voice"] : ["voice", "after voice"]);
    }
    sends.length = 0;
    sendFixture.before = async (command: string, args?: Record<string, unknown>) => {
      if (command === "send_media") throw new Error("synthetic upload failure");
      if (command === "send_text") sends.push(String(args?.text));
    };
    composer.pending = [item(3, "failed.png"), item(4, "unsent.png")];
    const failed = composer.sendPending();
    composer.draft = "after failure";
    const afterFailure = composer.send();
    await Promise.all([failed, afterFailure]);
    assert.deepEqual(sends, ["after failure"]);
    assert.deepEqual(composer.pending.map((entry: { file: File }) => entry.file.name), ["failed.png", "unsent.png"]);
    assert.ok(errors.some((error) => error.includes("synthetic upload failure")));
  } finally { await server.close(); }
});
