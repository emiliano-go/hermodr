import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { parse } from "svelte/compiler";
import { createServer } from "vite";
import { quizScopeMatches } from "../lib/utils/quiz-poll.ts";
import { broadcastSendReason } from "../lib/utils/broadcast.ts";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";
import { formatDate, formatTime, t } from "../lib/i18n/localizer.ts";
import { untrack } from "svelte";

const event = () => ({ id: "event", name: "Synthetic event", description: "Details", start: 1791153017, end: 1791265529,
  location: "Place", link: null, canceled: false, invitation: false, invitation_id: null as string | null, pinned: true,
  can_respond: true, extra_guests_allowed: true as boolean | null, is_scheduled_call: null as boolean | null,
  has_reminder: true as boolean | null, reminder_offset_sec: null as number | null,
  responses: [{ responder: "@me", response: "going", extra_guest_count: 2, timestamp_ms: 1 },
    { responder: "actual@lid", response: "maybe", extra_guest_count: null, timestamp_ms: null },
    { responder: "12025550101@s.whatsapp.net", response: "not_going", extra_guest_count: 0, timestamp_ms: 2 }],
});
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve!: () => void, reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function methods(path: string, context: Record<string, any>, derived: string[]) {
  Object.assign(context, { t, normalizeError, LocalizedError, localeDate: formatDate, localeTime: formatTime, untrack });
  const source = readFileSync(new URL(path, import.meta.url), "utf8"), script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("component.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declarations = tree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((node) => node.getText(tree));
  const bindings = [...derived];
  for (const name of ["answers", "pad"]) {
    const node = declarations.find((node) => node.name.getText(tree) === name);
    if (node) {
      let initializer = node.initializer!;
      if (ts.isCallExpression(initializer) && initializer.expression.getText(tree) === "$derived") {
        body.push(`var get_${name} = () => (${initializer.arguments[0].getText(tree)});`);
        bindings.push(name);
      } else body.push(`var ${name} = ${initializer.getText(tree)};`);
    }
  }
  for (const name of derived) {
    const node = declarations.find((node) => node.name.getText(tree) === name)?.initializer;
    assert.ok(node && ts.isCallExpression(node), name);
    body.push(`var get_${name} = () => (${node.arguments[0].getText(tree)});`);
  }
  const effects = tree.statements.filter((node) => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
    && node.expression.expression.getText(tree) === "$effect");
  const scope = effects.find((node) => node.getText(tree).includes("generation++") || node.getText(tree).includes("++generation"));
  assert.ok(scope && ts.isExpressionStatement(scope) && ts.isCallExpression(scope.expression));
  body.push(`var resetScope = ${scope.expression.arguments[0].getText(tree)};`);
  for (const name of bindings) Object.defineProperty(context, name, { configurable: true, get: () => context[`get_${name}`]() });
  runInNewContext(ts.transpileModule(body.join("\n"), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}
function card() {
  const calls: { answer: string; guests: number | undefined }[] = [];
  const context: Record<string, any> = { event: event(), chat: "actual@lid", scope: null, liveScope: { account: "a", chat: "actual@lid", generation: 1, requestKey: "event" },
    generation: 0, alive: true, ownerKey: "", guestSource: "", busy: false, details: false, failed: "", extraGuestCount: 2,
    quizScopeMatches, broadcastSendReason, onrespond: async (answer: string, guests?: number) => { calls.push({ answer, guests }); } };
  const c = methods("../lib/messages/cards/EventCard.svelte", context, ["sendReason", "responseReason", "validGuests", "groups"]);
  c.resetScope(); return { c, calls };
}
function form(initial: ReturnType<typeof event> | null = event()) {
  const values: any[] = [];
  let closes = 0;
  const context: Record<string, any> = { kind: "event", initial, liveScope: { account: "a", chat: "actual@lid", generation: 1, requestKey: "event" },
    generation: 0, busy: false, failed: null, name: " Updated ", description: " Details ", location: " Place ", link: "",
    extraGuestsAllowed: initial?.extra_guests_allowed ?? null, question: "", filled: [], quiz: false, quizDraft: { error: null },
    quizScopeMatches, oncreate: async (value: unknown) => { values.push(value); }, onclose: () => { closes++; } };
  const c = methods("../lib/chat/CreateDialog.svelte", context, ["timingError", "valid"]);
  c.start = c.localTimestamp(initial?.start); c.end = c.localTimestamp(initial?.end);
  c.resetScope(); return { c, values, closes: () => closes };
}

test("event answer changes forward validated optional guests and preserve current native response", async () => {
  const f = card(); await f.c.respond("going");
  f.c.extraGuestCount = 3; await f.c.respond("maybe"); await f.c.respond("not_going");
  f.c.extraGuestCount = undefined; await f.c.respond("going");
  assert.deepEqual(f.calls, [{ answer: "going", guests: 2 }, { answer: "maybe", guests: 3 }, { answer: "not_going", guests: undefined }, { answer: "going", guests: undefined }]);
  assert.equal(f.c.event.responses[0].response, "going");
  f.c.event.extra_guests_allowed = false; f.c.extraGuestCount = NaN; await f.c.respond("going");
  assert.equal(f.calls.at(-1)?.guests, undefined);
});

test("clear, invalid guests, keyless, invitation, canceled and broadcast events cannot emit replies", async () => {
  for (const count of [-1, 1.5, NaN, Infinity, 2147483648]) {
    const f = card(); f.c.extraGuestCount = count; await f.c.respond("going"); assert.equal(f.calls.length, 0);
  }
  const max = card(); max.c.extraGuestCount = 2147483647; await max.c.respond("going"); assert.equal(max.calls[0].guests, 2147483647);
  for (const change of ["key", "invitation", "canceled", "broadcast", "clear"]) {
    const f = card();
    if (change === "key") f.c.event.can_respond = false;
    if (change === "invitation") { f.c.event.invitation = true; f.c.event.can_respond = false; }
    if (change === "canceled") f.c.event.canceled = true;
    if (change === "broadcast") f.c.liveScope.chat = "list@broadcast";
    await f.c.respond(change === "clear" ? "clear" : "going"); assert.equal(f.calls.length, 0, change);
  }
});

test("current event errors remain visible and retry clears them", async () => {
  const f = card(); f.c.onrespond = async () => { throw new Error("Synthetic RSVP failure"); };
  await f.c.respond("going"); assert.ok(f.c.failed instanceof LocalizedError); assert.equal(f.c.failed.code, "error.operation_failed"); assert.match(f.c.failed.diagnostic ?? "", /Synthetic RSVP failure/); assert.equal(f.c.busy, false);
  f.c.onrespond = async () => {}; await f.c.respond("maybe"); assert.equal(f.c.failed, "");
});

test("account, chat, generation, event and unmount discard stale event reply completion", async () => {
  for (const change of ["account", "chat", "generation", "event", "unmount"]) for (const fail of [false, true]) {
    const f = card(), reply = deferred(); f.c.onrespond = () => reply.promise;
    const pending = f.c.respond("going");
    if (change === "account") f.c.liveScope.account = "b";
    if (change === "chat") f.c.liveScope.chat = "other@lid";
    if (change === "generation") f.c.liveScope.generation++;
    if (change === "event") f.c.event.id = "other-event";
    if (change === "unmount") { f.c.alive = false; f.c.generation++; } else f.c.resetScope();
    f.c.busy = true; f.c.failed = "Current owner";
    if (fail) reply.reject(new Error("Obsolete RSVP failure")); else reply.resolve();
    await pending; assert.equal(f.c.failed, "Current owner"); assert.equal(f.c.busy, true);
  }
});

test("cancel uses same event error guard and current response groups update from native snapshots", async () => {
  const f = card(); await f.c.run(async () => { throw new Error("Synthetic cancel failure"); });
  assert.ok(f.c.failed instanceof LocalizedError); assert.equal(f.c.failed.code, "error.operation_failed"); assert.match(f.c.failed.diagnostic ?? "", /Synthetic cancel failure/);
  assert.deepEqual(Array.from(f.c.groups, (group: any) => group.rows.length), [1, 1, 1]);
  f.c.event.responses.push({ responder: "new@lid", response: "going", extra_guest_count: 1, timestamp_ms: 3 });
  assert.deepEqual(Array.from(f.c.groups, (group: any) => [group.rows.length, group.guests]), [[2, 3], [1, 0], [1, 0]]);
});

test("event timestamps preserve untouched seconds and epoch zero with independent end dates", () => {
  const f = form();
  assert.equal(f.c.seconds(f.c.start, f.c.initial.start), f.c.initial.start);
  assert.equal(f.c.seconds(f.c.end, f.c.initial.end), f.c.initial.end);
  assert.notEqual(f.c.start.slice(0, 10), f.c.end.slice(0, 10));
  assert.equal(f.c.seconds(f.c.localTimestamp(0), 0), 0);
  assert.equal(f.c.seconds("", 0), null);
  assert.equal(f.c.eventTimeError(f.c.localTimestamp(0), f.c.localTimestamp(0), { start: 0, end: 0 }), null);
});

test("event form preserves supported nullable metadata and excludes read-only invitation identity", async () => {
  const initial = event(); initial.extra_guests_allowed = null; initial.is_scheduled_call = false; initial.reminder_offset_sec = 900; initial.invitation_id = "private-invitation";
  const f = form(initial); await f.c.submit();
  assert.equal(f.closes(), 1); assert.equal(f.values[0].start, initial.start); assert.equal(f.values[0].end, initial.end);
  assert.equal(f.values[0].name, "Updated"); assert.equal(f.values[0].extra_guests_allowed, null);
  assert.equal(f.values[0].is_scheduled_call, false); assert.equal(f.values[0].has_reminder, true); assert.equal(f.values[0].reminder_offset_sec, 900);
  assert.ok(!Object.hasOwn(f.values[0], "invitation_id"));
});

test("event validation blocks malformed, reversed and end-only times without losing draft", async () => {
  for (const [start, end] of [["invalid", ""], ["", "2026-10-04T12:00"], ["2026-10-04T12:00", "2026-10-03T12:00"]]) {
    const f = form(null); f.c.start = start; f.c.end = end; await f.c.submit();
    assert.ok(f.c.timingError); assert.equal(f.values.length, 0); assert.equal(f.c.name, " Updated ");
  }
});

test("event submit failures retain draft and obsolete scope cannot close newer editor", async () => {
  const current = form(); current.c.oncreate = async () => { throw new Error("Synthetic edit failure"); }; await current.c.submit();
  assert.ok(current.c.failed instanceof LocalizedError); assert.equal(current.c.failed.code, "error.operation_failed"); assert.match(current.c.failed.diagnostic ?? "", /Synthetic edit failure/); assert.equal(current.c.name, " Updated "); assert.equal(current.closes(), 0);
  for (const change of ["account", "chat", "generation", "request", "close"]) for (const fail of [false, true]) {
    const f = form(), save = deferred(); f.c.oncreate = () => save.promise;
    const pending = f.c.submit();
    if (change === "account") f.c.liveScope.account = "b";
    if (change === "chat") f.c.liveScope.chat = "other@lid";
    if (change === "generation") f.c.liveScope.generation++;
    if (change === "request") f.c.liveScope.requestKey = "new-editor";
    if (change === "close") f.c.generation++; else f.c.resetScope();
    f.c.failed = "Current editor";
    if (fail) save.reject(new Error("Obsolete edit failure")); else save.resolve();
    await pending; assert.equal(f.c.failed, "Current editor"); assert.equal(f.closes(), 0);
  }
});

test("event card renders full named attendees, all counts, own answer, guests and source pin", async () => {
  const server = await uiServer();
  try {
    const { render } = await server.ssrLoadModule("svelte/server"), { default: Card } = await load(server, "../lib/messages/cards/EventCard.svelte");
    const names: string[] = [], data = event(); data.canceled = true;
    const body = render(Card, { props: { event: data, title: "Event", pinned: data.pinned,
      names: (jid: string) => { names.push(jid); return jid === "actual@lid" ? "Saved <LID>" : "Phone name"; }, picture: () => null,
      onrespond: async () => { throw new Error("SSR must not reply"); }, onopenurl: () => {} } }).body;
    const text = body.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    assert.deepEqual(names, ["actual@lid", "12025550101@s.whatsapp.net"]);
    assert.ok(text.includes("1 Going") && text.includes("1 Maybe") && text.includes("1 Can't go"));
    assert.ok(text.includes("Your response: Going") && text.includes("2 extra guests") && text.includes("Guest count unavailable"));
    assert.ok(/Saved &lt;LID(?:>|&gt;)/.test(body) && body.includes("Phone name") && body.includes("Pinned event"));
    assert.ok(body.includes("This event was canceled") && body.includes("Attendees"));
    assert.ok(/<button\b[^>]*disabled[^>]*>Clear response/.test(body));
  } finally { await server.close(); }
});

test("invitation/reminder/end metadata preserves known false, unknown offset and valid epoch zero", async () => {
  const server = await uiServer();
  try {
    const { render } = await server.ssrLoadModule("svelte/server"), { default: Card } = await load(server, "../lib/messages/cards/EventCard.svelte");
    for (const has_reminder of [false, true]) for (const reminder_offset_sec of [null, 0, 900]) {
      const data = { ...event(), invitation: true, invitation_id: "internal-only-id", can_respond: false, pinned: false,
        start: null, end: 0, has_reminder, reminder_offset_sec };
      const body = render(Card, { props: { event: data, title: "Invite", onrespond: async () => {}, onopenurl: () => {} } }).body;
      assert.ok(body.includes("Event invitation") && body.includes("read-only on this device") && body.includes(String(new Date(0).getFullYear())));
      assert.ok(body.includes(has_reminder ? "Reminder: Enabled" : "Reminder: None"));
      assert.equal(body.includes("Reminder offset unavailable"), has_reminder && reminder_offset_sec === null);
      assert.equal(body.includes("Reminder offset: 0"), reminder_offset_sec === 0);
      assert.ok(!body.includes("Not pinned") && !body.includes("internal-only-id"));
    }
  } finally { await server.close(); }
});

test("event editor renders independent datetime fields and all event draft controls disable while busy", async () => {
  const source = readFileSync(new URL("../lib/chat/CreateDialog.svelte", import.meta.url), "utf8"), markup = parse(source, { modern: true }) as any;
  const controls: any[] = [];
  function visit(node: any) {
    if (!node || typeof node !== "object") return;
    if (node.type === "RegularElement" && ["input", "textarea"].includes(node.name)) {
      const binding = node.attributes.find((attr: any) => attr.type === "BindDirective" && attr.name === "value");
      if (["name", "description", "start", "end", "location", "link"].includes(binding?.expression?.name)) controls.push(node);
    }
    if (Array.isArray(node)) node.forEach(visit); else Object.values(node).forEach(visit);
  }
  visit(markup); assert.equal(controls.length, 6);
  for (const node of controls) {
    const attr = node.attributes.find((attr: any) => attr.name === "disabled"), value = Array.isArray(attr?.value) ? attr.value[0] : attr?.value;
    assert.ok(value?.expression);
    assert.equal(runInNewContext(source.slice(value.expression.start, value.expression.end), { busy: true }), true);
  }
  const server = await uiServer();
  try {
    const { render } = await server.ssrLoadModule("svelte/server"), { default: Editor } = await load(server, "../lib/chat/CreateDialog.svelte");
    const data = event(), f = form(data);
    const body = render(Editor, { props: { kind: "event", initial: data, oncreate: async () => { throw new Error("SSR must not send"); }, onclose: () => {} } }).body;
    assert.equal([...body.matchAll(/type="datetime-local"/g)].length, 2);
    assert.ok(body.includes(f.c.start) && body.includes(f.c.end) && body.includes("Allow extra guests"));
    assert.ok(body.includes("Reminder settings cannot be changed") && !body.includes("Invitation ID"));
  } finally { await server.close(); }
});

async function uiServer() {
  return createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/events-ui", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
}
async function load(server: Awaited<ReturnType<typeof uiServer>>, path: string) {
  return server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
}
