import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

function gate() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => { release = resolve; });
  return { promise, release };
}

test("scheduled sends persist draft payload, wait until due, share send order, and stop at account changes", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/scheduled", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL(".", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("./state/composer.svelte.ts");
    const { ScheduledState } = await load("./state/scheduled.svelte.ts");
    const { session } = await load("./state/session.svelte.ts");
    const { chats } = await load("./state/chats.svelte.ts");
    const { messages } = await load("./state/messages.svelte.ts");
    const { ui } = await load("./state/ui.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const rows: { id: string; chat: string; text: string; mentions: string[]; due_at: number; status: string; error: string | null; attempted: boolean }[] = [];
    const sends: string[] = [];
    const calls: { command: string; args?: Record<string, unknown> }[] = [];
    let fail = false;
    let schedulingFails = false;
    setHandler(async (command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      if (command === "send_typing") return;
      assert.equal(args?.account, "account-a");
      if (command === "schedule_message") {
        if (schedulingFails) throw new Error("synthetic persistence failure");
        const id = `stable-${rows.length}`;
        rows.push({ id, chat: String(args.chat), text: String(args.text), mentions: args.mentions as string[],
          due_at: Number(args.dueAt), status: "pending", error: null, attempted: false });
        return id;
      }
      if (command === "scheduled_messages") return rows.map((row) => ({ ...row }));
      const row = rows.find((row) => row.id === args?.id)!;
      if (command === "send_scheduled_message") {
        row.attempted = true;
        if (fail) { row.status = "uncertain"; row.error = "synthetic send failure"; throw new Error(row.error); }
        sends.push(row.id);
        rows.splice(rows.indexOf(row), 1);
        return true;
      }
      if (command === "retry_scheduled_message") { row.status = "pending"; row.error = null; return; }
      if (command === "cancel_scheduled_message") { rows.splice(rows.indexOf(row), 1); return; }
      if (command === "update_scheduled_message") { row.text = String(args?.text); row.due_at = Number(args?.dueAt); return; }
      throw new Error(`Unexpected synthetic command ${command}`);
    });
    session.activeAccount = "account-a"; session.connected = true;
    chats.selectedChat = "1@s.whatsapp.net";
    chats.refreshChats = async () => {}; messages.reloadMessages = async () => {};
    const errors: string[] = []; ui.fail = (error: unknown) => errors.push(String(error));
    const composer = new ComposerState();
    const scheduler = new ScheduledState(); scheduler.selectAccount("account-a");
    const enqueue = <T>(task: (signal: AbortSignal) => Promise<T>) => composer.enqueue(task);
    const due = Math.floor(Date.now() / 1000) + 60;
    composer.draft = "@Ada hello"; composer.chosenMentions = [{ name: "Ada", jid: "2@lid" }];
    assert.equal(await composer.schedule(due), true);
    assert.equal(composer.draft, ""); assert.equal(rows[0].text, "@2 hello"); assert.deepEqual(rows[0].mentions, ["2@lid"]);
    await scheduler.tick(enqueue, () => due - 1); assert.equal(sends.length, 0);

    const blocked = gate(); const started = gate();
    const earlier = composer.enqueue(async () => { sends.push("earlier"); started.release(); await blocked.promise; });
    await started.promise;
    const pump = scheduler.tick(enqueue, () => due);
    await new Promise((resolve) => setTimeout(resolve, 10));
    await scheduler.tick(enqueue, () => due);
    assert.deepEqual(sends, ["earlier"]);
    blocked.release(); await Promise.all([earlier, pump]);
    assert.deepEqual(sends, ["earlier", "stable-0"]); assert.equal(scheduler.items.length, 0);

    composer.draft = "kept on failure"; schedulingFails = true;
    assert.equal(await composer.schedule(due), false); assert.equal(composer.draft, "kept on failure");
    schedulingFails = false; assert.equal(await composer.schedule(due), true);
    fail = true; await scheduler.tick(enqueue, () => due);
    assert.equal(scheduler.items[0].status, "uncertain"); assert.ok(errors.some((error) => error.includes("synthetic send failure")));
    const attempted = calls.filter((call) => call.command === "send_scheduled_message").length;
    await scheduler.tick(enqueue, () => due + 1);
    assert.equal(calls.filter((call) => call.command === "send_scheduled_message").length, attempted);
    fail = false; await scheduler.change("retry_scheduled_message", { id: "stable-0" });
    await scheduler.tick(enqueue, () => due + 1); assert.equal(rows.length, 0);

    composer.draft = "edit me"; await composer.schedule(due);
    await scheduler.change("update_scheduled_message", { id: "stable-0", text: "edited", dueAt: due + 20 });
    await scheduler.tick(enqueue, () => due + 19); assert.equal(rows[0].text, "edited");
    await scheduler.change("cancel_scheduled_message", { id: "stable-0" }); assert.equal(rows.length, 0);

    composer.draft = "old account"; await composer.schedule(due);
    const oldGate = gate(); const oldStarted = gate();
    const older = composer.enqueue(async () => { oldStarted.release(); await oldGate.promise; });
    await oldStarted.promise;
    const queued = scheduler.tick(enqueue, () => due);
    await new Promise((resolve) => setTimeout(resolve, 10));
    const before = sends.length;
    composer.resetAccount(); session.activeAccount = "account-b"; scheduler.selectAccount("account-b");
    oldGate.release(); await Promise.all([older, queued]);
    assert.equal(sends.length, before); assert.equal(rows[0].status, "pending"); assert.equal(scheduler.items.length, 0);

    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: ScheduleDialog } = await load("./composer/ScheduleDialog.svelte");
    const form = render(ScheduleDialog, { props: { text: "Synthetic scheduled text", dueAt: due,
      onsave: async () => true, onclose: () => {} } }).body;
    assert.ok(form.includes('type="datetime-local"')); assert.ok(form.includes("Synthetic scheduled text"));
    assert.ok(form.includes('aria-label="Schedule message"'));
    const { scheduled } = await load("./state/scheduled.svelte.ts");
    scheduled.items = [
      { id: "pending", chat: "1@s.whatsapp.net", text: "Synthetic pending", mentions: [], due_at: due, status: "pending", attempted: false, error: null },
      { id: "uncertain", chat: "1@s.whatsapp.net", text: "Synthetic uncertain", mentions: [], due_at: due, status: "uncertain", attempted: true, error: "Synthetic interrupted send" },
    ];
    const { default: ScheduledOutbox } = await load("./composer/ScheduledOutbox.svelte");
    const list = render(ScheduledOutbox, { props: { enqueue } }).body;
    for (const text of ["Synthetic pending", "Synthetic interrupted send", "Delivery unconfirmed", "Edit", "Cancel", "Retry"]) assert.ok(list.includes(text));
  } finally { await server.close(); }
});
