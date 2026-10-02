import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { appliableLabels, inboxCategories, inboxChats } from "../lib/utils/inbox.ts";
import type { InboxFilters } from "../lib/utils/inbox.ts";
import type { ChatSummary } from "../lib/utils/wire";

const filters: InboxFilters = { unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" };
const base: ChatSummary = { chat: "base@s", display_name: "Base", last_message_at: 0, last_text: "", last_from_me: false,
  last_sender_name: null, last_sender: "", last_media_kind: null, message_count: 0, unread_count: 0, mention_count: 0,
  pinned: false, archived: false, muted_until: 0, mute_at_all: false, marked_unread: false };
const rows = [
  { ...base, chat: "unread@s", display_name: "Alice", unread_count: 2, mention_count: 1 },
  { ...base, chat: "muted@s", display_name: "Bob", muted_until: -1, archived: true },
  { ...base, chat: "manual@s", display_name: "Clara", marked_unread: true, archived: true },
  { ...base, chat: "labelled@s", display_name: "Dara" },
  { ...base, chat: "ordinary@s", display_name: "Elena", muted_until: 100 },
];
const labels = { "unread@s": ["work"], "muted@s": ["work"], "labelled@s": ["personal"] };
const name = (row: ChatSummary) => row.display_name ?? row.chat;
const select = (patch: Partial<InboxFilters>) => inboxChats(rows, { ...filters, ...patch }, labels, name, 100).map((row) => row.chat);

test("default inbox combines five categories without mutating order or including expired mutes", () => {
  assert.deepEqual(select({}), ["unread@s", "muted@s", "manual@s", "labelled@s"]);
  assert.deepEqual(inboxCategories(base, [], 100), { unread: false, mentions: false, labelled: false, muted: false, archived: false });
  assert.equal(inboxCategories({ ...base, muted_until: 101 }, [], 100).muted, true);
  assert.equal(rows[0].chat, "unread@s");
});

test("filters compose as intersections and manual unread counts", () => {
  assert.deepEqual(select({ unread: true }), ["unread@s", "manual@s"]);
  assert.deepEqual(select({ unread: true, mentions: true }), ["unread@s"]);
  assert.deepEqual(select({ unread: true, archived: true }), ["manual@s"]);
  assert.deepEqual(select({ muted: true, archived: true, labelled: true }), ["muted@s"]);
  assert.deepEqual(select({ mentions: true, muted: true }), []);
});

test("label and name search combine with categories and missing associations", () => {
  assert.deepEqual(select({ label: "work", query: "  bOB " }), ["muted@s"]);
  assert.deepEqual(select({ label: "personal", labelled: true }), ["labelled@s"]);
  assert.deepEqual(select({ query: "manual@s" }), ["manual@s"]);
  assert.deepEqual(select({ label: "missing" }), []);
  assert.deepEqual(inboxChats(rows, { ...filters, labelled: true }, {}, name, 100), []);
});

function actionFunction(file: string, name: string, context: Record<string, any>) {
  const source = readFileSync(new URL(file, import.meta.url), "utf8").split('<script lang="ts">')[1].split("</script>")[0];
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declaration = tree.statements.find((item) => ts.isFunctionDeclaration(item) && item.name?.text === name);
  assert.ok(declaration, `Missing production ${name}`);
  const dependencies = name === "save" ? tree.statements.filter((item) => ts.isVariableStatement(item)
    && item.declarationList.declarations.some((declaration) => declaration.name.getText(tree) === "validColor")) : [];
  const compiled = ts.transpileModule([...dependencies, declaration].map((item) => item.getText(tree)).join("\n"), { compilerOptions: { target: ts.ScriptTarget.ES2022 } });
  runInNewContext(compiled.outputText, context);
  return context[name];
}

test("inbox actions retain original account and suppress duplicate dispatch and stale failures", async () => {
  let reject!: (reason: Error) => void;
  const calls: unknown[] = [];
  const context = { account: "alpha", generation: 1, connected: true, loading: false, busy: {}, failures: {}, labels: [],
    labelsWritable: true, labelsLoading: false, onaction: (...args: unknown[]) => { calls.push(args); return new Promise<void>((_, no) => (reject = no)); } };
  const act = actionFunction("../lib/chat/UnifiedInbox.svelte", "act", context);
  const pending = act(rows[0], { kind: "read", read: true });
  await act(rows[0], { kind: "archive", archived: true });
  assert.equal(calls.length, 1);
  assert.equal((calls[0] as unknown[])[0], "alpha");
  context.account = "beta"; context.generation++; context.busy = {}; context.failures = {};
  reject(new Error("old account failure"));
  await pending;
  assert.deepEqual(context.busy, {});
  assert.deepEqual(context.failures, {});
});

test("inbox current failures stay visible and request invalidation rejects same-account completions", async () => {
  let reject!: (reason: Error) => void;
  const context: Record<string, any> = { account: "alpha", generation: 1, connected: true, loading: false, busy: {}, failures: {},
    labels: [], labelsWritable: true, labelsLoading: false, onaction: () => new Promise<void>((_, no) => (reject = no)) };
  const act = actionFunction("../lib/chat/UnifiedInbox.svelte", "act", context);
  let pending = act(rows[0], { kind: "mute", seconds: 0 });
  reject(new Error("current refusal")); await pending;
  assert.match(context.failures[rows[0].chat], /current refusal/);
  assert.equal(context.busy[rows[0].chat], false);
  pending = act(rows[0], { kind: "mute", seconds: -1 });
  context.generation++; context.busy = {}; context.failures = {};
  reject(new Error("obsolete request")); await pending;
  assert.deepEqual(context.failures, {});
});

test("label dialog stale success cannot alter new target snapshots", async () => {
  let release!: () => void;
  let completed = 0;
  const context: Record<string, any> = { account: "alpha", generation: 1, disabled: false, working: false, failure: "" };
  const run = actionFunction("../lib/labels/LabelDialog.svelte", "run", context);
  const pending = run(() => new Promise<void>((yes) => (release = yes)), () => completed++);
  context.generation++; context.working = false;
  release(); await pending;
  assert.equal(completed, 0);
  assert.equal(context.working, false);
  await run(async () => {}, () => completed++);
  assert.equal(completed, 1);
});

test("inbox never offers the groups label for applying and skips assigned ones", () => {
  const options = [{ id: "1", name: "work" }, { id: "2", name: "Groups" }, { id: "3", name: "  GROUPS " }];
  assert.deepEqual(appliableLabels(options, ["1"]).map((label) => label.id), []);
  assert.deepEqual(appliableLabels(options, []).map((label) => label.id), ["1"]);
  assert.deepEqual(appliableLabels(options, undefined).map((label) => label.id), ["1"]);
});

test("label save preserves existing color, creates with empty identity and blocks invalid i32 snapshots", () => {
  for (const color of [-2147483648, 0, 2147483647, -2147483649, 2147483648, 0.1, NaN, undefined]) {
    const calls: unknown[][] = [];
    const context: Record<string, any> = { draft: { id: color === 0 ? "" : "existing", name: " Renamed ", color },
      $derived: (value: unknown) => value, run: (task: () => unknown) => task(), onsave: (...args: unknown[]) => calls.push(args) };
    const save = actionFunction("../lib/labels/LabelDialog.svelte", "save", context);
    save();
    const valid = color !== undefined && Number.isInteger(color) && color >= -2147483648 && color <= 2147483647;
    assert.deepEqual(calls, valid ? [[context.draft.id, "Renamed", color]] : []);
  }
});
