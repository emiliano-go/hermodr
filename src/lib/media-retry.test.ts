import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("media retries stop at the cap, refresh successful downloads and ignore the previous account", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)), server: { middlewareMode: true, hmr: false } });
  let state: { resetAccount(): void } | undefined;
  try {
    const { MessagesState, MAX_DOWNLOAD_TRIES } = await server.ssrLoadModule(fileURLToPath(new URL("./state/messages.svelte.ts", import.meta.url)));
    const { mediaFixture: fixture, windowFixture } = await server.ssrLoadModule("/ipc.ts");
    const messages = new MessagesState();
    state = messages;
    messages.prepareChat("window@s", 100);
    await messages.reloadMessages("window@s");
    const row = messages.messages[0];
    fixture.deferNext = true;
    const first = messages.downloadMedia(row.chat, row, true);
    await messages.downloadMedia(row.chat, row, true);
    assert.equal(fixture.calls, 1, "duplicate clicks share the in-flight download");
    fixture.pending.shift()();
    await first;
    for (let n = 1; n <= MAX_DOWNLOAD_TRIES; n++) await messages.downloadMedia(row.chat, row, true);
    assert.equal(fixture.calls, MAX_DOWNLOAD_TRIES);
    assert.equal(messages.downloadTries[row.id], MAX_DOWNLOAD_TRIES);
    assert.match(messages.downloadErrors[row.id], /sender unavailable/);
    assert.equal(messages.downloading[row.id], undefined);

    messages.resetAccount();
    messages.prepareChat(row.chat, 100);
    fixture.failure = false;
    await messages.downloadMedia(row.chat, row, true);
    assert.ok(messages.messages.find((m: { id: string }) => m.id === row.id)?.media_path);
    assert.equal(messages.downloadTries[row.id], undefined);
    assert.equal(messages.downloadErrors[row.id], undefined);

    fixture.deferNext = true;
    const stale = messages.downloadMedia(row.chat, row, true);
    messages.resetAccount();
    messages.prepareChat(row.chat, 100);
    fixture.deferNext = true;
    const current = messages.downloadMedia(row.chat, row, true);
    fixture.failure = true;
    fixture.pending.shift()();
    await stale;
    assert.equal(messages.downloadErrors[row.id], undefined);
    assert.equal(messages.downloadTries[row.id], undefined);
    assert.equal(messages.downloading[row.id], true, "old completion must not clear the new account's request");
    fixture.pending.shift()();
    await current;
    assert.equal(messages.downloadTries[row.id], 1);
    assert.match(messages.downloadErrors[row.id], /sender unavailable/);
    assert.equal(windowFixture.archive.length, 350);
  } finally {
    state?.resetAccount();
    await server.close();
  }
});
