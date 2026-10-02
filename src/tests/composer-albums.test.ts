import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("composer albums preserve scope, ordered uploads and safe retry boundaries", async (t) => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/composer-albums", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("../lib/state/composer.svelte.ts");
    const { chats } = await load("../lib/state/chats.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { setHandler, invoke } = await load("../../tests/scheduled/ipc.ts");
    chats.refreshChats = async () => {};
    messages.reloadMessages = async () => {};
    const item = (id: number, once = false, type = "image/png") => ({ id,
      file: new File([Uint8Array.of(id)], `item-${id}.${type === "image/png" ? "png" : "txt"}`, { type }),
      url: "", kind: type.startsWith("image/") ? "image" : "other", caption: `caption-${id}`, once, quality: "hd" });
    const gate = () => {
      let release!: () => void;
      const promise = new Promise<void>((resolve) => { release = resolve; });
      return { promise, release };
    };
    const fixture = () => {
      session.activeAccount = "album-account";
      session.settings.send_typing = false;
      chats.selectedChat = "album-a@s";
      ui.error = ui.notice = null;
      const composer = new ComposerState();
      composer.pending = [item(1), item(2), item(3)];
      composer.replyingTo = { id: "original-reply", sender: "original@s", text: "original quote" };
      const calls: { command: string; args: any }[] = [];
      const tokens = new Map<string, { name: string; size: number; written: number }>();
      const control = { failUpload: 0, failAppend: 0, before: null as null | ((command: string, args: any) => Promise<void>),
        outcome: null as null | ((args: any) => unknown), albums: 0, created: 0 };
      const result = (args: any, extra: Record<string, unknown> = {}) => ({
        account_id: args.accountId, chat: args.chat, parent_id: args.parentId ?? "album-parent",
        sent_ids: args.items.map((_: unknown, i: number) => `sent-${i}`), next_index: args.items.length,
        uncertain_index: null, uncertain_id: null, parent_uncertain: false, preflight_failed: false,
        warnings: [], error: null, ...extra,
      });
      setHandler(async (command: string, args: any) => {
        calls.push({ command, args });
        if (control.before) await control.before(command, args);
        if (command === "begin_upload") {
          const token = `upload-${++control.created}`;
          if (control.failUpload === control.created) throw new Error("synthetic staging failure");
          assert.equal(args.accountId, "album-account");
          tokens.set(token, { name: args.name, size: args.size, written: 0 });
          return token;
        }
        if (command === "append_upload") {
          assert.equal(args.accountId, "album-account");
          if (args.token === `upload-${control.failAppend}`) throw new Error("synthetic chunk failure");
          const upload = tokens.get(args.token)!;
          assert.equal(args.offset, upload.written);
          upload.written += Buffer.from(args.data, "base64").length;
          return;
        }
        if (command === "cancel_upload") { tokens.delete(args.token); return; }
        if (command === "send_album") {
          control.albums++;
          for (const entry of args.items) {
            const upload = tokens.get(entry.upload)!;
            assert.equal(upload.written, upload.size);
          }
          return control.outcome ? control.outcome(args) : result(args);
        }
        if (command === "send_media") return null;
        if (command === "send_text") return;
        throw new Error(`Unexpected synthetic command ${command}`);
      });
      return { composer, calls, tokens, control, result };
    };

    await t.test("small album files stage in order, use one IPC and keep FIFO until completion", async () => {
      const f = fixture(), blocked = gate(), started = gate();
      f.composer.pending[0].caption = "";
      f.control.before = async (command) => { if (command === "send_album") { started.release(); await blocked.promise; } };
      const sending = f.composer.sendPending("original caption", ["original@lid"]);
      await Promise.race([started.promise, sending]);
      assert.ok(f.calls.some((call) => call.command === "send_album"), "eligible selection must enter album dispatch");
      const later = f.composer.enqueue(() => invoke("send_text", { text: "later" }));
      assert.equal(f.calls.filter((call) => call.command === "send_text").length, 0);
      blocked.release(); await Promise.all([sending, later]);
      const album = f.calls.find((call) => call.command === "send_album")!.args;
      assert.deepEqual(album.items.map((entry: any) => entry.upload), ["upload-1", "upload-2", "upload-3"]);
      assert.equal(album.items[0].caption, "original caption");
      assert.deepEqual(album.mentions, ["original@lid"]);
      assert.equal(album.replyToId, "original-reply");
      assert.ok(album.items.every((entry: any) => entry.quality === "hd" && entry.progress));
      assert.equal(f.control.albums, 1);
      assert.equal(f.calls.filter((call) => call.command === "send_media").length, 0);
      assert.equal(f.tokens.size, 0);
      assert.equal(f.composer.outgoing.length, 0);
    });

    for (const stopped of [0, 1, 2]) await t.test(`known-unsent tail at index ${stopped} continues without consuming newer composer state`, async () => {
      const f = fixture();
      f.composer.pending[0].caption = "";
      f.control.outcome = (args) => f.result(args, { sent_ids: Array.from({ length: stopped }, (_, i) => `sent-${i}`),
        next_index: stopped, error: "synthetic known-unsent failure" });
      await f.composer.sendPending("original caption", ["original@lid"]);
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3].slice(stopped));
      const newer = item(99), reply = { id: "new-reply", sender: "new@s", text: "new quote" };
      f.composer.pending = [...f.composer.pending, newer];
      f.composer.replyingTo = reply;
      f.composer.draft = "new draft";
      f.control.outcome = null;
      await f.composer.send();
      const retried = f.calls.filter((call) => call.command === "send_album").at(-1)!.args;
      assert.equal(retried.parentId, "album-parent");
      assert.equal(retried.replyToId, "original-reply");
      assert.deepEqual(retried.mentions, stopped === 0 ? ["original@lid"] : []);
      assert.equal(retried.items.length, 3 - stopped);
      assert.equal(f.composer.draft, "new draft");
      assert.equal(f.composer.replyingTo, reply);
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [99]);
      assert.equal(f.tokens.size, 0);
    });

    await t.test("staging failure before album dispatch restores all files and cleans tokens", async () => {
      for (const append of [false, true]) {
        const f = fixture();
        if (append) f.control.failAppend = 2;
        else f.control.failUpload = 2;
        await f.composer.sendPending();
        assert.equal(f.control.albums, 0);
        assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3]);
        assert.equal(f.tokens.size, 0);
      }
    });

    await t.test("partial fanout warning remains visible independently of video-preview preference", async () => {
      const f = fixture();
      session.settings.warn_missing_video_preview = false;
      f.control.outcome = (args) => f.result(args, { warnings: ["Album omitted one recipient device."] });
      await f.composer.sendPending();
      assert.equal(ui.notice, "Album omitted one recipient device.");
      assert.equal(f.composer.pending.length, 0);
      assert.equal(f.tokens.size, 0);
    });

    await t.test("typed preflight failure restores all files without inventing a parent", async () => {
      const f = fixture();
      f.control.outcome = (args) => f.result(args, { preflight_failed: true, parent_id: "", sent_ids: [], next_index: 0, error: "preflight denied" });
      await f.composer.sendPending();
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3]);
      assert.equal(f.composer.pending[0].retry.parentId, null);
      assert.equal(f.tokens.size, 0);
    });

    for (const uncertain of [0, 1, 2]) await t.test(`uncertain child ${uncertain} stays blocked while later known-unsent files remain retryable`, async () => {
      const f = fixture();
      f.control.outcome = (args) => f.result(args, { sent_ids: Array.from({ length: uncertain }, (_, i) => `sent-${i}`),
        next_index: uncertain + 1, uncertain_index: uncertain, uncertain_id: `uncertain-${uncertain}`, error: "outcome unknown" });
      await f.composer.sendPending();
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3].slice(uncertain + 1));
      const recovery = f.composer.attachmentRecoveries[0];
      assert.deepEqual(recovery.uncertain.map((entry: any) => entry.id), [uncertain + 1]);
      assert.equal(recovery.uncertainId, `uncertain-${uncertain}`);
      assert.equal(f.tokens.size, 0);
    });

    for (const mode of ["rejection", "foreign-result"]) await t.test(`${mode} after dispatch preserves whole batch for review without automatic retry`, async () => {
      const f = fixture();
      f.control.outcome = (args) => {
        if (mode === "rejection") throw new Error("lost IPC response");
        return f.result(args, { account_id: "foreign-account" });
      };
      await f.composer.sendPending();
      assert.equal(f.composer.pending.length, 0);
      assert.deepEqual(f.composer.attachmentRecoveries[0].uncertain.map((entry: any) => entry.id), [1, 2, 3]);
      assert.equal(f.composer.restoreKnownUnsent("album-a@s"), false);
      await f.composer.sendPending();
      assert.equal(f.control.albums, 1);
      assert.equal(f.tokens.size, 0);
    });

    await t.test("unknown parent retains review state and never resends parent on explicit child continuation", async () => {
      const f = fixture();
      f.control.outcome = (args) => f.result(args, { parent_uncertain: true, sent_ids: [], next_index: 0, error: "parent unknown" });
      await f.composer.sendPending();
      assert.equal(f.composer.attachmentRecoveries[0].parentUncertain, true);
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3]);
      f.control.outcome = null;
      await f.composer.sendPending();
      assert.equal(f.calls.filter((call) => call.command === "send_album").at(-1)!.args.parentId, "album-parent");
    });

    for (const afterDispatch of [false, true]) await t.test(`chat switch ${afterDispatch ? "after" : "before"} dispatch restores only on return`, async () => {
      const f = fixture(), blocked = gate(), started = gate();
      f.control.before = async (command) => { if (command === (afterDispatch ? "send_album" : "append_upload")) { started.release(); await blocked.promise; } };
      f.control.outcome = (args) => f.result(args, { sent_ids: [], next_index: 0, error: "known unsent" });
      const work = f.composer.sendPending();
      await Promise.race([started.promise, work]);
      chats.selectedChat = "album-b@s";
      f.composer.draft = "other chat draft";
      f.composer.pending = [item(99)];
      blocked.release(); await work;
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [99]);
      assert.equal(f.composer.restoreKnownUnsent("album-a@s"), false);
      assert.equal(f.control.albums, afterDispatch ? 1 : 0);
      f.composer.pending = [];
      chats.selectedChat = "album-a@s";
      assert.equal(f.composer.restoreKnownUnsent("album-a@s"), true);
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [1, 2, 3]);
    });

    await t.test("account reset during staging cancels captured tokens and cannot restore old files", async () => {
      const f = fixture(), blocked = gate(), started = gate();
      f.control.before = async (command) => { if (command === "begin_upload") { started.release(); await blocked.promise; } };
      const work = f.composer.sendPending();
      await Promise.race([started.promise, work]);
      f.composer.resetAccount(); messages.resetAccount(); session.activeAccount = "new-account";
      blocked.release(); await work;
      assert.equal(f.control.albums, 0);
      assert.equal(f.tokens.size, 0);
      assert.equal(f.composer.pending.length, 0);
      assert.equal(f.composer.attachmentRecoveries.length, 0);
      assert.equal(ui.error, null);
    });

    await t.test("generation reset after dispatch discards stale result and releases outgoing previews", async () => {
      const f = fixture(), blocked = gate(), started = gate();
      const original = f.composer.pending[0].file;
      const url = URL.createObjectURL(original), revoked: string[] = [], revoke = URL.revokeObjectURL;
      URL.revokeObjectURL = (value) => { revoked.push(value); revoke(value); };
      try {
        f.composer.pending[0].url = url;
        f.control.before = async (command) => { if (command === "send_album") { started.release(); await blocked.promise; } };
        f.control.outcome = (args) => f.result(args, { sent_ids: [], next_index: 0, error: "stale partial result" });
        const work = f.composer.sendPending();
        await started.promise;
        messages.resetAccount();
        f.composer.pending = [item(99)];
        blocked.release(); await work;
        assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [99]);
        assert.equal(f.composer.attachmentRecoveries.length, 0);
        assert.equal(f.composer.outgoing.length, 0);
        assert.equal(f.tokens.size, 0);
        assert.deepEqual(revoked, [url]);
        assert.equal(ui.error, null);
      } finally { URL.revokeObjectURL = revoke; }
    });

    await t.test("lost-response files remain scoped for review and explicit discard releases previews", async () => {
      const f = fixture(), original = f.composer.pending[0].file;
      const url = URL.createObjectURL(original), revoked: string[] = [], revoke = URL.revokeObjectURL;
      URL.revokeObjectURL = (value) => { revoked.push(value); revoke(value); };
      try {
        f.composer.pending[0].url = url;
        f.control.outcome = () => { throw new Error("lost response"); };
        await f.composer.sendPending();
        const recovery = f.composer.currentAttachmentRecoveries[0];
        assert.equal(recovery.uncertain[0].file, original);
        assert.equal(recovery.uncertain[0].url, url);
        assert.equal(revoked.length, 0);
        chats.selectedChat = "album-b@s";
        assert.equal(f.composer.currentAttachmentRecoveries.length, 0);
        f.composer.discardAttachmentRecovery(recovery);
        assert.equal(f.composer.attachmentRecoveries.length, 1);
        chats.selectedChat = "album-a@s";
        f.composer.discardAttachmentRecovery(recovery);
        assert.equal(f.composer.attachmentRecoveries.length, 0);
        assert.deepEqual(revoked, [url]);
      } finally { URL.revokeObjectURL = revoke; }
    });

    await t.test("album size bounds and GIF preserve individual sending", async () => {
      for (const count of [1, 9, 2]) {
        const f = fixture();
        f.composer.pending = Array.from({ length: count }, (_, i) => item(i + 1));
        if (count === 2) f.composer.pending[0].file = new File(["synthetic"], "animated.gif", { type: "image/gif" });
        await f.composer.sendPending();
        assert.equal(f.control.albums, 0);
        assert.equal(f.calls.filter((call) => call.command === "send_media").length, count);
      }
    });

    await t.test("explicit chat clear releases captured recovery and retries but preserves newer draft, reply and file", async () => {
      const f = fixture(), revoked: string[] = [], revoke = URL.revokeObjectURL;
      URL.revokeObjectURL = (url) => { revoked.push(url); revoke(url); };
      try {
        for (const entry of f.composer.pending) entry.url = URL.createObjectURL(entry.file);
        const oldUrls = f.composer.pending.map((entry: any) => entry.url);
        f.control.outcome = (args) => f.result(args, { sent_ids: [], next_index: 1,
          uncertain_index: 0, uncertain_id: "unknown-child", error: "outcome unknown" });
        await f.composer.sendPending();
        const newer = item(99), reply = { id: "new-reply", sender: "new@s", text: "new quote" };
        f.composer.pending = [...f.composer.pending, newer];
        f.composer.replyingTo = reply;
        f.composer.draft = "new draft";
        f.composer.forgetRecovery("album-a@s");
        assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [99]);
        assert.equal(f.composer.replyingTo, reply);
        assert.equal(f.composer.draft, "new draft");
        assert.equal(f.composer.attachmentRecoveries.length, 0);
        assert.deepEqual(revoked.sort(), oldUrls.sort());
      } finally { URL.revokeObjectURL = revoke; }
    });

    await t.test("chat clear fences late in-flight result from restoring cleared retry context", async () => {
      const f = fixture(), blocked = gate(), started = gate();
      f.control.before = async (command) => { if (command === "send_album") { started.release(); await blocked.promise; } };
      f.control.outcome = (args) => f.result(args, { sent_ids: [], next_index: 0, error: "late failure" });
      const work = f.composer.sendPending();
      await started.promise;
      f.composer.forgetRecovery("album-a@s");
      f.composer.pending = [item(99)];
      blocked.release(); await work;
      assert.deepEqual(f.composer.pending.map((entry: any) => entry.id), [99]);
      assert.equal(f.composer.attachmentRecoveries.length, 0);
      assert.equal(f.composer.outgoing.length, 0);
      assert.equal(f.tokens.size, 0);
    });

    await t.test("mixed or view-once selections preserve individual sending", async () => {
      for (const mixed of [false, true]) {
        const f = fixture();
        f.composer.pending = [item(1, !mixed), item(2, false, mixed ? "text/plain" : "image/png")];
        await f.composer.sendPending();
        assert.equal(f.control.albums, 0);
        assert.equal(f.calls.filter((call) => call.command === "send_media").length, 2);
      }
    });
  } finally { await server.close(); }
});
