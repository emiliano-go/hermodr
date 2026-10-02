import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("attachment staging owns its scope, selection order and preview lifetime", async (t) => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false }), {
    name: "staging-files",
    resolveId(id) { if (id === "virtual:staging-files") return "\0staging-files"; },
    load(id) {
      if (id !== "\0staging-files") return;
      return `export const conversions = new Map(), previews = new Map();
        const defer = (map, file) => new Promise((resolve, reject) => map.set(file.name, { resolve, reject }));
        export const rasterizeSvg = file => defer(conversions, file);
        export const imagePreview = file => defer(previews, file);
        export const base64Of = async () => "";`;
    },
  }], root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/composer-staging", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/files", replacement: "virtual:staging-files" },
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  const revoke = URL.revokeObjectURL;
  const revoked: string[] = [];
  URL.revokeObjectURL = (url) => { revoked.push(url); revoke(url); };
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("../lib/state/composer.svelte.ts");
    const { chats } = await load("../lib/state/chats.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { conversions, previews } = await server.ssrLoadModule("virtual:staging-files");
    const fresh = () => {
      chats.selectedChat = "staging-a@s";
      session.activeAccount = "synthetic-account";
      ui.error = null;
      conversions.clear(); previews.clear();
      return new ComposerState();
    };
    const file = (name: string, type = "text/plain") => new File(["synthetic"], name, { type });
    const settle = async () => { await Promise.resolve(); await Promise.resolve(); };

    await t.test("out-of-order SVG conversion preserves selection and existing tray order", async () => {
      const composer = fresh();
      await composer.stageFile(file("old-a.txt"));
      await composer.stageFile(file("old-b.txt"));
      composer.pending = [...composer.pending].reverse();
      const first = composer.stageFile(file("first.svg", "image/svg+xml"));
      const second = composer.stageFile(file("second.svg", "image/svg+xml"));
      await composer.stageFile(file("third.txt"));
      conversions.get("second.svg").resolve(file("second.png", "image/png"));
      await settle();
      previews.get("second.png").resolve("data:second");
      await second;
      conversions.get("first.svg").resolve(file("first.png", "image/png"));
      await settle();
      previews.get("first.png").resolve("data:first");
      await first;
      assert.deepEqual(composer.pending.map((p: { file: File }) => p.file.name),
        ["old-b.txt", "old-a.txt", "first.png", "second.png", "third.txt"]);
    });

    await t.test("late SVG success or failure cannot cross account, chat, generation or reset", async () => {
      for (const change of [
        () => { session.activeAccount = "other-account"; },
        () => { chats.selectedChat = "staging-b@s"; },
        () => messages.resetAccount(),
        (composer: InstanceType<typeof ComposerState>) => composer.resetAccount(),
      ]) {
        for (const failed of [false, true]) {
          const composer = fresh();
          composer.draft = "unrelated draft";
          const work = composer.stageFile(file("stale.svg", "image/svg+xml"));
          change(composer);
          composer.draft = "current draft";
          const current = composer.pending;
          if (failed) conversions.get("stale.svg").reject(new Error("obsolete conversion"));
          else {
            conversions.get("stale.svg").resolve(file("stale.png", "image/png"));
            await settle();
            previews.get("stale.png")?.resolve("data:stale");
          }
          await work;
          assert.equal(composer.pending, current);
          assert.equal(composer.draft, "current draft");
          assert.equal(ui.error, null);
          assert.equal(previews.size, 0);
        }
      }
    });

    await t.test("removed or handed-off previews stay canceled after same-ID restoration", async () => {
      for (const removed of [true, false]) {
        const composer = fresh();
        const work = composer.stageFile(file("late.png", "image/png"));
        const original = composer.pending[0];
        if (removed) composer.removePending(original.id);
        else composer.pending = [];
        composer.pending = [{ ...original, caption: "restored caption" }];
        const url = URL.createObjectURL(file("preview.png", "image/png"));
        previews.get("late.png").resolve(url);
        await work;
        assert.equal(composer.pending[0].url, "");
        assert.equal(composer.pending[0].caption, "restored caption");
        assert.ok(revoked.includes(url));
      }
    });

    await t.test("tray handoff cancels unfinished SVG while individual removal preserves it", async () => {
      for (const removed of [false, true]) {
        const composer = fresh();
        const work = composer.stageFile(file("waiting.svg", "image/svg+xml"));
        await composer.stageFile(file("ready.txt"));
        if (removed) composer.removePending(composer.pending[0].id);
        else composer.pending = [];
        conversions.get("waiting.svg").resolve(file("waiting.png", "image/png"));
        await settle();
        previews.get("waiting.png")?.resolve("data:waiting");
        await work;
        assert.deepEqual(composer.pending.map((p: { file: File }) => p.file.name), removed ? ["waiting.png"] : []);
      }
    });

    await t.test("scope-stale preview is released while current tray and reply stay untouched", async () => {
      const composer = fresh();
      const work = composer.stageFile(file("scope.png", "image/png"));
      chats.selectedChat = "staging-b@s";
      const reply = { id: "synthetic-reply" };
      composer.replyingTo = reply;
      composer.draft = "current draft";
      await composer.stageFile(file("current.txt"));
      const current = composer.pending;
      const url = URL.createObjectURL(file("preview.png", "image/png"));
      previews.get("scope.png").resolve(url);
      await work;
      assert.equal(composer.pending, current);
      assert.equal(composer.replyingTo, reply);
      assert.equal(composer.draft, "current draft");
      assert.ok(revoked.includes(url));
    });

    await t.test("valid preview preserves user edits and reset releases video URL", async () => {
      const composer = fresh();
      const work = composer.stageFile(file("current.png", "image/png"));
      composer.pending[0].caption = "edited caption";
      composer.toggleOnce(composer.pending[0].id);
      previews.get("current.png").resolve("data:current");
      await work;
      assert.equal(composer.pending[0].url, "data:current");
      assert.equal(composer.pending[0].caption, "edited caption");
      assert.equal(composer.pending[0].once, true);
      await composer.stageFile(file("current.mp4", "video/mp4"));
      const video = composer.pending[1].url;
      composer.resetAccount();
      assert.equal(revoked.filter((url) => url === video).length, 1);
    });
  } finally { URL.revokeObjectURL = revoke; await server.close(); }
});
