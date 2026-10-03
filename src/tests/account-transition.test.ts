import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { normalizeError } from "../lib/i18n/errors.ts";
import type { Account, AccountsView, CommandError, ConnectionState } from "../lib/utils/wire.ts";

const options = { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext };
const source = readFileSync(new URL("../lib/state/accounts.ts", import.meta.url), "utf8");
const frontend = ts.transpileModule(source, { compilerOptions: options }).outputText
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");

function methods(file: string, name: string, selected: string[], bindings: Record<string, unknown>) {
  const text = readFileSync(new URL(`../lib/state/${file}.svelte.ts`, import.meta.url), "utf8");
  const tree = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declaration = tree.statements.find((node) => ts.isClassDeclaration(node) && node.name?.text === name);
  assert.ok(declaration && ts.isClassDeclaration(declaration));
  const body = selected.map((name) => {
    const member = declaration.members.find((node) => ts.isMethodDeclaration(node) && node.name.getText(tree) === name);
    assert.ok(member); return member.getText(tree);
  }).join("\n");
  const compiled = ts.transpileModule(`class Extracted { ${body} }`, { compilerOptions: options }).outputText;
  const Class = new Function(...Object.keys(bindings), `${compiled}\nreturn Extracted;`)(...Object.values(bindings));
  return new Class();
}

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = () => new Promise<void>((resolve) => setImmediate(resolve));

function fixture() {
  type Service = { account: string; connected: boolean; qr: string | null };
  const account = (id: string): Account => ({ id, label: `Synthetic ${id}`, jid: `${id}@s.whatsapp.net`, once_paired: false });
  const accounts = ["initial", "alpha", "beta", "gamma"].map(account);
  const initial: Service = { account: "initial", connected: true, qr: null };
  const backend = { active: "initial", service: initial as Service | null, binding: initial as Service | null };
  const hooks = {
    start: async (_id: string) => {},
    accounts: async (view: AccountsView) => view,
    connection: async (view: ConnectionState) => view,
  };
  const calls: { command: string; args: Record<string, unknown> | undefined; resets: number; busy: boolean }[] = [];
  const resetLog: string[] = [];
  let added = 0;
  const start = async (id: string) => {
    backend.service = null;
    await hooks.start(id);
    const service = { account: id, connected: true, qr: null };
    backend.binding = service;
    backend.service = service;
  };
  const invoke = async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
    calls.push({ command, args, resets: resetLog.length, busy: session.connecting });
    if (command === "switch_account") {
      const id = String(args?.id); assert.ok(accounts.some((account) => account.id === id));
      backend.active = id;
      await start(id);
      return;
    }
    if (command === "connect") {
      if (!backend.service) await start(backend.active);
      return;
    }
    if (command === "add_account") {
      const id = `added-${++added}`; accounts.push(account(id)); backend.active = id;
      await start(id); return;
    }
    if (command === "remove_account") {
      const id = String(args?.id), at = accounts.findIndex((account) => account.id === id);
      assert.notEqual(at, -1); accounts.splice(at, 1);
      if (backend.active === id) { backend.active = accounts[0].id; await start(backend.active); }
      return;
    }
    if (command === "accounts") return hooks.accounts({ accounts, active: backend.active });
    if (command === "connection_state") return hooks.connection({ started: !!backend.service,
      connected: backend.service?.connected ?? false, qr: backend.service?.qr ?? null });
    if (command === "qr_svg") return `<svg>${args?.value}</svg>`;
    assert.fail(`Unexpected synthetic command ${command}`);
  };
  const session = methods("session", "SessionState", ["loadAccounts", "showQr", "resetAccount"], { invoke });
  Object.assign(session, { activeAccount: "initial", accountList: accounts, connected: true, started: true,
    connecting: false, connectRequested: false, gateDone: true, qrSvg: null, qrRequest: 0, accountsRequest: 0 });
  const ui = methods("ui", "UiState", ["fail", "resetAccount"], { normalizeError });
  ui.error = null;
  const reset = (name: string) => () => { resetLog.push(name); };
  const messages = { accountGeneration: 1, resetAccount: () => { resetLog.push("messages"); ++messages.accountGeneration; } };
  const bindings = { invoke, session, ui, messages,
    chats: { resetAccount: reset("chats"), refreshChats: async () => {} },
    composer: { resetAccount: reset("composer") }, favorites: { reset: reset("favorites"), refresh: async () => {} },
    labels: { reset: reset("labels"), refresh: async () => {} }, members: { resetAccount: reset("members"), loadAliases: async () => {} },
    player: { stop: reset("player") }, refreshResolvedNames: async () => {} };
  const actions = new Function(...Object.keys(bindings), `${frontend}\nreturn { switchTo, connect, reconnect, syncState, chooseAccount, addAccount, removeAccount };`)(...Object.values(bindings));
  return { actions, session, ui, backend, hooks, calls, messages, resetLog };
}

test("delayed old switch failures cannot reach the newer account's banner", async () => {
  const f = fixture(), gate = deferred<void>();
  f.hooks.start = (id) => id === "alpha" ? gate.promise : Promise.resolve();
  const old = f.actions.switchTo("alpha"); await flush();
  const current = f.actions.switchTo("beta");
  const failure: CommandError = { kind: "postal_error", code: "error.service_start_failed", params: {}, diagnostic: "synthetic alpha startup failure" };
  gate.reject(failure); await Promise.all([old, current]);
  assert.equal(f.ui.error, null);
  assert.equal(f.session.activeAccount, "beta"); assert.equal(f.backend.active, "beta"); assert.equal(f.backend.service?.account, "beta");
});

for (const action of ["add", "remove"] as const) test(`queued ${action} still executes once before a newer switch without stale UI resets`, async () => {
  const f = fixture(), gate = deferred<void>();
  f.hooks.start = (id) => id === "alpha" ? gate.promise : Promise.resolve();
  const old = f.actions.switchTo("alpha"); await flush(); const resets = f.resetLog.length;
  const mutation = action === "add" ? f.actions.addAccount() : f.actions.removeAccount("beta");
  const current = f.actions.switchTo("gamma"); gate.resolve(); await Promise.all([old, mutation, current]);
  const commands = f.calls.filter((call) => ["switch_account", "add_account", "remove_account"].includes(call.command));
  assert.deepEqual(commands.map((call) => call.command), ["switch_account", action === "add" ? "add_account" : "remove_account", "switch_account"]);
  assert.equal(commands[1].resets, resets);
  if (action === "remove") assert.equal(commands[1].args?.id, "beta");
  assert.equal(f.session.activeAccount, "gamma"); assert.equal(f.backend.active, "gamma"); assert.equal(f.ui.error, null);
});

test("same-account choose/connect preserves loaded account data and reset generation", async () => {
  const f = fixture(); await f.actions.chooseAccount("initial");
  assert.deepEqual(f.resetLog, []); assert.equal(f.messages.accountGeneration, 1);
  assert.equal(f.calls.some((call) => call.command === "switch_account"), false);
  assert.equal(f.session.activeAccount, "initial"); assert.equal(f.ui.error, null);
});

test("latest connect reads authoritative account after an older switch skipped UI commits", async () => {
  const f = fixture(), gate = deferred<void>();
  f.hooks.start = (id) => id === "alpha" ? gate.promise : Promise.resolve();
  const old = f.actions.switchTo("alpha"); await flush();
  const current = f.actions.connect(); gate.resolve(); await Promise.all([old, current]);
  assert.equal(f.backend.active, "alpha"); assert.equal(f.session.activeAccount, "alpha");
  assert.equal(f.session.connected, true); assert.equal(f.ui.error, null);
  assert.ok(f.messages.accountGeneration > 1);
});

test("obsolete queued switch preserves reset generation and latest connect busy state", async () => {
  const f = fixture(), gate = deferred<void>(); f.backend.service = null;
  f.hooks.start = (id) => id === "initial" ? gate.promise : Promise.resolve();
  const old = f.actions.connect(); await flush(); const resets = f.resetLog.length;
  const obsolete = f.actions.switchTo("alpha"), current = f.actions.connect();
  gate.resolve(); await Promise.all([old, obsolete, current]);
  const switched = f.calls.find((call) => call.command === "switch_account")!;
  assert.equal(switched.args?.id, "alpha"); assert.equal(switched.resets, resets); assert.equal(switched.busy, true);
  assert.equal(f.session.activeAccount, "alpha"); assert.equal(f.backend.active, "alpha");
  assert.equal(f.session.connecting, false); assert.equal(f.ui.error, null);
  assert.equal(f.calls.filter((call) => call.command === "switch_account").length, 1);
});

test("reset invalidates old accounts and QR reads before their internal commits", async () => {
  const f = fixture(), accounts = deferred<AccountsView>();
  f.hooks.accounts = () => accounts.promise;
  const pending = f.session.loadAccounts(); f.session.resetAccount();
  accounts.resolve({ accounts: [], active: "alpha" }); await pending;
  assert.equal(f.session.activeAccount, "initial");
  const qr = deferred<string>();
  const session = methods("session", "SessionState", ["showQr", "resetAccount"], { invoke: () => qr.promise });
  Object.assign(session, { qrRequest: 0, accountsRequest: 0, qrSvg: null });
  const old = session.showQr("synthetic-old"); session.resetAccount(); qr.resolve("synthetic SVG"); await old;
  assert.equal(session.qrSvg, null);
});

test("authoritative account changes reset cached data before publishing the new identity", async () => {
  const f = fixture(); f.backend.active = "alpha";
  const observed: string[] = [];
  await f.session.loadAccounts(() => true, () => { observed.push(f.session.activeAccount); f.messages.resetAccount(); });
  assert.deepEqual(observed, ["initial"]); assert.equal(f.session.activeAccount, "alpha");
  assert.equal(f.messages.accountGeneration, 2);
});

test("current transition failures retain their code and diagnostic after reset", async () => {
  const f = fixture();
  const failure: CommandError = { kind: "postal_error", code: "error.service_start_failed", params: {}, diagnostic: "synthetic current startup failure" };
  f.hooks.start = async () => { throw failure; };
  await f.actions.switchTo("alpha");
  assert.equal(f.ui.error.code, failure.code); assert.equal(f.ui.error.diagnostic, failure.diagnostic);
  assert.equal(f.session.connecting, false); assert.ok(f.messages.accountGeneration > 1);
});

test("reconnect uses the shared transition queue without nesting another queued transition", async () => {
  const f = fixture(); await f.actions.reconnect();
  assert.equal(f.calls.filter((call) => call.command === "connect").length, 1);
  assert.equal(f.session.connected, true); assert.equal(f.session.connecting, false);
  assert.equal(f.ui.error, null);
});

test("old accounts snapshots cannot rewind the latest account selection", async () => {
  const f = fixture(), gate = deferred<AccountsView>();
  let captured!: AccountsView;
  f.hooks.accounts = (view) => { if (view.active === "alpha") { captured = view; return gate.promise; } return Promise.resolve(view); };
  const old = f.actions.switchTo("alpha"); await flush();
  assert.equal(captured.active, "alpha");
  const current = f.actions.switchTo("beta");
  gate.resolve(captured); await Promise.all([old, current]);
  assert.equal(f.session.activeAccount, "beta"); assert.equal(f.backend.active, "beta"); assert.equal(f.backend.service?.account, "beta");
  assert.equal(f.ui.error, null);
});

test("old connection snapshots cannot overwrite the new account's state or QR", async () => {
  const f = fixture(), gate = deferred<ConnectionState>();
  f.backend.service!.connected = false; f.backend.service!.qr = "synthetic-old-qr";
  let captured!: ConnectionState;
  f.hooks.connection = (view) => { if (view.qr) { captured = view; return gate.promise; } return Promise.resolve(view); };
  const old = f.actions.syncState(); await flush();
  await f.actions.switchTo("beta"); assert.equal(f.session.connected, true); assert.equal(f.session.qrSvg, null);
  gate.resolve(captured); await old;
  assert.equal(f.session.activeAccount, "beta"); assert.equal(f.session.connected, true);
  assert.equal(f.session.qrSvg, null); assert.equal(f.backend.service?.connected, true);
});

test("old connect completion cannot clear the busy flag of the queued latest connect", async () => {
  const f = fixture(), first = deferred<void>(), second = deferred<void>();
  f.backend.service = null; let starts = 0;
  f.hooks.start = () => ++starts === 1 ? first.promise : second.promise;
  const old = f.actions.connect(); await flush();
  const current = f.actions.connect(); await flush(); assert.equal(f.session.connecting, true); assert.equal(starts, 1);
  first.reject(new Error("synthetic first connect failure")); await old; await flush();
  assert.equal(starts, 2); assert.equal(f.session.connecting, true); assert.equal(f.backend.service, null); assert.equal(f.ui.error, null);
  second.resolve(); await current;
  assert.equal(f.session.connecting, false);
});

test("frontend switch dispatch remains FIFO even with delayed startup", async () => {
  const f = fixture(), gate = deferred<void>();
  f.hooks.start = (id) => id === "alpha" ? gate.promise : Promise.resolve();
  const old = f.actions.switchTo("alpha"); await flush();
  const current = f.actions.switchTo("beta"); await flush();
  assert.deepEqual(f.calls.filter((call) => call.command === "switch_account").map((call) => call.args?.id), ["alpha"]);
  gate.resolve(); await Promise.all([old, current]);
  assert.deepEqual(f.calls.filter((call) => call.command === "switch_account").map((call) => call.args?.id), ["alpha", "beta"]);
  assert.equal(f.backend.active, "beta"); assert.equal(f.backend.service?.account, "beta"); assert.equal(f.backend.binding, f.backend.service);
  assert.equal(f.session.activeAccount, "beta");
});
