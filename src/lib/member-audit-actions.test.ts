import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { memberActionReason, memberNoteError } from "./utils/member-sheet.ts";
import { mergeAuditEntries } from "./utils/group-audit.ts";
import { changeText } from "./utils/group-actions.ts";

function functions(file: string, context: Record<string, any>) {
  const source = readFileSync(new URL(file, import.meta.url), "utf8").match(/<script[^>]*lang="ts"[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((item) => item.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

test("member action needs confirmation and permissions, captures owner, rejects stale completion", async () => {
  let release!: () => void;
  const calls: unknown[] = [];
  const context = functions("./contacts/MemberSheet.svelte", { account: "alpha", group: "group@g.us", jid: "member@lid", requestKey: 1,
    generation: 1, busy: false, localLoading: false, liveLoading: false, failure: "", saved: "", confirmation: null,
    permissions: { admin: true, connected: true, self: false, member: { admin: false, owner: false }, blocked: false, supported: ["remove"] },
    memberActionReason, changeText, onaction: (...args: unknown[]) => { calls.push(args); return new Promise<void>((yes) => (release = yes)); } });
  await context.act("remove"); assert.equal(calls.length, 0);
  context.confirmation = "remove";
  const pending = context.act("remove");
  assert.equal(calls.length, 1);
  assert.equal((calls[0] as any)[0].jid, "member@lid");
  context.jid = "new@lid"; context.requestKey++; context.generation++; context.busy = false; context.confirmation = null;
  release(); await pending;
  assert.equal(context.saved, ""); assert.equal(context.confirmation, null); assert.equal(context.busy, false);
  context.confirmation = "remove"; context.permissions.admin = false;
  await context.act("remove"); assert.equal(calls.length, 1);
});

test("member refusals stay visible and local notes save offline only to captured member", async () => {
  const calls: unknown[] = [];
  const context = functions("./contacts/MemberSheet.svelte", { account: "alpha", group: "group@g.us", jid: "member@lid", requestKey: 1,
    generation: 1, busy: false, failure: "", saved: "", confirmation: "remove", local: {}, localLoading: false, liveLoading: false, warnings: 2, notes: "Local notes",
    permissions: { admin: true, connected: true, self: false, member: { admin: false, owner: false }, blocked: false, supported: ["remove"] },
    memberActionReason, memberNoteError, changeText, onaction: async () => [{ jid: "member@lid", ok: false, pending: false, code: "403", error: null }],
    onsavelocal: async (...args: unknown[]) => calls.push(args) });
  await context.act("remove"); assert.match(context.failure, /Not allowed/); assert.equal(context.confirmation, "remove");
  context.permissions.connected = false;
  await context.saveLocal(); assert.equal(calls.length, 1); assert.equal((calls[0] as any)[0].group, "group@g.us");
  assert.deepEqual((calls[0] as any).slice(1), ["Local notes", 2]);
  context.warnings = -1; await context.saveLocal(); assert.equal(calls.length, 1);
  context.warnings = 100001; await context.saveLocal(); assert.equal(calls.length, 1);
  context.warnings = 0; context.notes = "not saved\0"; await context.saveLocal(); assert.equal(calls.length, 1); assert.equal(context.notes, "not saved\0");
});

test("audit ignores stale pages and retains loaded rows after older-page failure", async () => {
  let release!: (value: unknown) => void;
  const existing = [{ id: "known" }];
  const context = functions("./chat/GroupAudit.svelte", { account: "alpha", group: "group@g.us", member: "member@lid", requestKey: 1,
    generation: 1, loading: false, error: "", rows: existing, cursor: "next", loaded: true,
    checked: { filters: { kind: null, actor: null, since: null, until: null } }, mergeAuditEntries,
    onload: () => new Promise((yes) => (release = yes)) });
  const pending = context.load(true);
  context.member = "new@lid"; context.requestKey++; context.generation++; context.rows = []; context.cursor = null; context.loading = false;
  release({ entries: [{ id: 99 }], next_cursor: { timestamp: 1, id: 99 }, has_more: true }); await pending;
  assert.deepEqual(context.rows, []); assert.equal(context.cursor, null); assert.equal(context.error, "");
  context.rows = existing; context.cursor = "next"; context.onload = async () => { throw new Error("Synthetic page failure"); };
  await context.load(true);
  assert.equal(context.rows, existing); assert.equal(context.cursor, "next"); assert.match(context.error, /Synthetic page failure/);
});
