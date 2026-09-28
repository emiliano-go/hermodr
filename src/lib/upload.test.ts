import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("large attachments use bounded chunks, clean up failures, and respect account cancellation and send order", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)), cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/upload", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { sendAttachment, UPLOAD_CHUNK_BYTES } = await server.ssrLoadModule(fileURLToPath(new URL("./upload.ts", import.meta.url)));
    const { uploadFixture: fixture } = await server.ssrLoadModule("/ipc.ts");
    class BoundedFile extends File {
      override async arrayBuffer(): Promise<ArrayBuffer> { throw new Error("must not read the whole large file"); }
      override slice(start = 0, end = this.size, type?: string) {
        assert.ok(end - start <= UPLOAD_CHUNK_BYTES);
        return super.slice(start, end, type);
      }
    }
    const bytes = Uint8Array.from({ length: 2 * 1024 * 1024 + 17 }, (_, i) => i % 251);
    const file = new BoundedFile([bytes], "synthetic.mp4");
    await sendAttachment(file, { chat: "test@s" });
    assert.equal(fixture.maxChunk, UPLOAD_CHUNK_BYTES);
    assert.equal(fixture.calls[0], "begin");
    assert.deepEqual(fixture.calls.slice(-2), ["send-staged", "cancel"]);
    assert.deepEqual(Buffer.concat(fixture.chunks), Buffer.from(bytes));
    fixture.calls = [];
    await sendAttachment(new File([new Uint8Array([0, 255, 3])], "small.png"), { chat: "test@s" });
    assert.deepEqual(fixture.calls, ["send-inline"]);
    for (const failure of ["failChunk", "failSend"]) {
      fixture.calls = []; fixture[failure] = true;
      await assert.rejects(sendAttachment(file, { chat: "test@s" }), /Synthetic/);
      assert.equal(fixture.calls.at(-1), "cancel");
      if (failure === "failChunk") assert.ok(!fixture.calls.includes("send-staged"));
      fixture[failure] = false;
    }
    fixture.calls = [];
    const abort = new AbortController();
    fixture.afterChunk = () => abort.abort();
    await assert.rejects(sendAttachment(file, { chat: "test@s" }, abort.signal), { name: "AbortError" });
    assert.deepEqual(fixture.calls, ["begin", "append", "cancel"]);
    fixture.afterChunk = null;

    const { ComposerState } = await server.ssrLoadModule(fileURLToPath(new URL("./state/composer.svelte.ts", import.meta.url)));
    const composer = new ComposerState();
    const sent: string[] = [];
    let release!: () => void;
    const first = composer.enqueue(async () => { await new Promise<void>((resolve) => { release = resolve; }); sent.push("first"); });
    const second = composer.enqueue(async () => { sent.push("second"); });
    await Promise.resolve();
    assert.equal(sent.length, 0);
    release(); await Promise.all([first, second]);
    assert.deepEqual(sent, ["first", "second"]);
    const pending = composer.enqueue(async () => { sent.push("stale"); });
    const rejected = assert.rejects(pending, /Account changed/);
    composer.resetAccount();
    await rejected;
    await composer.enqueue(async () => { sent.push("new account"); });
    assert.deepEqual(sent, ["first", "second", "new account"]);
  } finally { await server.close(); }
});
