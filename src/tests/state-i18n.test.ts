import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import { createRequire } from "node:module";
import ts from "typescript";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";
import { englishCatalog, installLocaleProvider, loadCatalog, t, type Catalog } from "../lib/i18n/localizer.ts";
import { displayMessage, isUiMessage, localizedFailure, uiError, uiMessage } from "../lib/state/localized.ts";
import { broadcastSendError, isBroadcastList } from "../lib/utils/broadcast.ts";
import type { ContactSendResult } from "../lib/utils/wire.ts";

const arabic = await loadCatalog("ar");
const svelteServer = createRequire(import.meta.url)("svelte/internal/server");
function loadState(file: string, name: string) {
  const source = readFileSync(new URL(`../lib/state/${file}.svelte.ts`, import.meta.url), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const code = compileModule(js, { generate: "server", filename: `${file}.svelte.js` }).js.code
    .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
  const bindings: Record<string, unknown> = { $: svelteServer, normalizeError, displayMessage, uiError, t, invoke: () => {},
    emptyMediaOverrides: () => ({}), session: {}, messages: {}, chats: {}, composer: {}, ui: {} };
  for (const match of source.matchAll(/export const (\w+)/g)) delete bindings[match[1]];
  const State = new Function(...Object.keys(bindings), `${code}\nreturn ${name};`)(...Object.values(bindings));
  return new State(() => {});
}

test("stored errors keep string getters and diagnostics while language changes", () => {
  let snapshot: { locale: string; catalog: Catalog } = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => snapshot);
  try {
    const cases = [["gallery", "GalleryState", null], ["labels", "LabelsState", null],
      ["quick-replies", "QuickRepliesState", ""], ["transcription", "TranscriptionState", ""],
      ["media-policy", "MediaPolicyState", ""], ["keywords", "KeywordsState", null],
      ["scheduled", "ScheduledState", null], ["member-sheet", "MemberSheetState", ""]] as const;
    for (const [file, name, cleared] of cases) {
      const state = loadState(file, name);
      state.error = uiError("error.operation_failed", {}, "synthetic private detail");
      assert.equal(state.error, "Operation failed.", file);
      snapshot = { locale: "ar", catalog: arabic };
      assert.equal(state.error, t("error.operation_failed"), file);
      assert.equal(state.diagnostic, "synthetic private detail", file);
      assert.ok(!state.error.includes("synthetic"), file);
      state.error = cleared; assert.equal(state.error, cleared, file); assert.equal(state.diagnostic, undefined, file);
      snapshot = { locale: "en", catalog: englishCatalog };
    }
  } finally { restore(); }
});

test("notice arrays retain references, numbers and user parameters through locale switches", () => {
  const state = loadState("ui", "UiState");
  let snapshot: { locale: string; catalog: Catalog } = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => snapshot);
  try {
    state.notify([uiMessage("common.items", { count: 3 }), uiMessage("locale.selected", { language: "<user label>" })], "synthetic notice detail");
    assert.equal(state.notice, `${t("common.items", { count: 3 })}\n${t("locale.selected", { language: "<user label>" })}`);
    const english = state.notice;
    snapshot = { locale: "ar", catalog: arabic };
    assert.equal(state.notice, `${t("common.items", { count: 3 })}\n${t("locale.selected", { language: "<user label>" })}`);
    assert.notEqual(state.notice, english); assert.ok(state.notice.includes("<user label>"));
    assert.equal(state.noticeDiagnostic, "synthetic notice detail");
    state.notice = "replacement notice"; assert.equal(state.noticeDiagnostic, undefined);
    state.notify(uiMessage("common.items", { count: 2 }), "second detail");
    state.notice = null; assert.equal(state.notice, null); assert.equal(state.noticeDiagnostic, undefined);
  } finally { restore(); }
});

test("pairing errors retain protocol flags and diagnostics without cached display text", () => {
  let snapshot: { locale: string; catalog: Catalog } = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => snapshot);
  try {
    for (const [file, name] of [["session", "SessionState"], ["once", "OnceInstance"]]) {
      const state = loadState(file, name);
      state.pairCodeError = { message: uiError("error.operation_failed", {}, "synthetic pairing detail"), throttled: true, unavailable: false };
      assert.equal(state.pairCodeError.message, "Operation failed.");
      snapshot = { locale: "ar", catalog: arabic };
      assert.deepEqual(state.pairCodeError, { message: t("error.operation_failed"), throttled: true, unavailable: false, diagnostic: "synthetic pairing detail" });
      state.pairCodeError = null; assert.equal(state.pairCodeError, null);
      snapshot = { locale: "en", catalog: englishCatalog };
    }
  } finally { restore(); }
});

test("album display references translate known codes and preserve malformed reference diagnostics", () => {
  assert.equal(isUiMessage(uiMessage("page.media_removed", { count: 2 })), true);
  const failure = localizedFailure({ ...uiMessage("common.items", { count: 3 }), diagnostic: "typed detail" }, "error.operation_failed");
  assert.equal(failure.message, t("common.items", { count: 3 })); assert.equal(failure.diagnostic, "typed detail");
  for (const value of [null, "raw warning", { code: "notice.key", params: [] }, { code: "bad key", params: {} },
    { code: "notice.key", params: { nested: {} } }]) assert.equal(isUiMessage(value), false);
  for (const value of [{ code: "unknown.album_key", params: {} },
    { code: "error.operation_failed", params: {}, diagnostic: {} }, { code: "common.items", params: [] }]) {
    const fallback = localizedFailure(value, "error.operation_failed", "raw compatibility detail");
    assert.equal(fallback.code, "error.operation_failed"); assert.match(fallback.diagnostic ?? "", /raw compatibility detail/);
  }
  const raw = normalizeError(new Error("protocol text"));
  assert.ok(raw instanceof LocalizedError); assert.equal(raw.message, "Operation failed."); assert.match(raw.diagnostic ?? "", /protocol text/);
});

test("native locale queue skips replaced or unmounted requests and reports only current failures", async () => {
  const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8").match(/<script(?![^>]*\bmodule\b)[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("route.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const queue = tree.statements.find((node) => ts.isVariableStatement(node)
    && node.declarationList.declarations.some((declaration) => declaration.name.getText(tree) === "nativeLocaleQueue"));
  const effect = tree.statements.find((node) => ts.isExpressionStatement(node) && node.getText(tree).includes('"set_native_locale"'));
  assert.ok(queue && effect);
  const body = ts.transpileModule(`${queue.getText(tree)}\n${effect.getText(tree)}`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const locale = { language: "en" }, calls: string[] = [], failures: LocalizedError[] = [];
  const pending: ((error: Error) => void)[] = [];
  let run!: () => () => void;
  new Function("$effect", "locale", "invoke", "ui", body)(
    (callback: typeof run) => { run = callback; }, locale,
    (command: string, args: { language: string }) => {
      assert.equal(command, "set_native_locale"); calls.push(args.language);
      return new Promise<void>((_resolve, reject) => pending.push(reject));
    }, { fail: (error: unknown) => failures.push(normalizeError(error)) },
  );
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  const old = run(); old(); locale.language = "ar";
  const arabic = run(); await flush(); assert.deepEqual(calls, ["ar"]);
  arabic(); locale.language = "en"; const closed = run(); closed();
  pending.shift()!(new Error("obsolete native failure")); await flush();
  assert.deepEqual(calls, ["ar"]); assert.equal(failures.length, 0);
  const current = run(); await flush(); assert.deepEqual(calls, ["ar", "en"]);
  pending.shift()!(new Error("current native detail")); await flush();
  assert.equal(failures.length, 1); assert.equal(failures[0].message, "Operation failed.");
  assert.match(failures[0].diagnostic ?? "", /current native detail/); current();
});

test("acknowledged contacts keep typed warnings and diagnostics without offering a failed-send retry", async () => {
  const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8").match(/<script(?![^>]*\bmodule\b)[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("route.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declaration = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "sendContacts");
  assert.ok(declaration);
  const body = ts.transpileModule(declaration.getText(tree), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const ui = loadState("ui", "UiState"), notices: { code: string }[] = [];
  const notify = ui.notify.bind(ui);
  ui.notify = (message: { code: string }, diagnostic?: string) => { notices.push(message); notify(message, diagnostic); };
  const scope = { account: "synthetic-account", chat: "synthetic-chat", generation: 3 };
  let result: ContactSendResult;
  let refreshFails = false, calls = 0;
  const bindings = { broadcastSendError, isBroadcastList, uiError, uiMessage, ui,
    session: { activeAccount: scope.account, connected: true }, members: { chatGroup: null },
    chats: { selectedChat: scope.chat, refreshChats: async () => {} },
    messages: { accountGeneration: scope.generation, reloadMessages: async () => { if (refreshFails) throw new Error("cache refresh detail"); } },
    composer: { editing: null, recording: false, enqueue: (run: (signal: AbortSignal) => unknown) => run(new AbortController().signal) },
    invoke: async (command: string) => { assert.equal(command, "send_contacts"); ++calls; return result; } };
  const send = new Function(...Object.keys(bindings), `${body}\nreturn sendContacts;`)(...Object.values(bindings));
  result = { message_id: "acknowledged", warning: "raw compatibility warning", warning_ref: uiMessage("common.items", { count: 3 }), diagnostic: "native warning detail" };
  assert.equal(await send([], scope), "acknowledged"); assert.equal(ui.error, null);
  assert.equal(ui.noticeDiagnostic, "native warning detail"); assert.equal(notices[0].code, "common.items");
  result = { message_id: "legacy-acknowledged", warning: "legacy warning detail" };
  refreshFails = true;
  assert.equal(await send([], scope), "legacy-acknowledged"); assert.equal(notices[1].code, "page.contact_send_warning");
  assert.equal(ui.noticeDiagnostic, "legacy warning detail"); assert.equal(ui.error.code, "error.page.contact_refresh");
  assert.match(ui.error.diagnostic, /cache refresh detail/); assert.equal(calls, 2);
});

test("composer typing callbacks keep broadcast guards and ordinary typing updates", () => {
  const source = readFileSync(new URL("../lib/state/composer.svelte.ts", import.meta.url), "utf8");
  const tree = ts.createSourceFile("composer.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declaration = tree.statements.find((node) => ts.isClassDeclaration(node) && node.name?.text === "ComposerState");
  assert.ok(declaration && ts.isClassDeclaration(declaration));
  const methods = ["reportTyping", "stopTyping"].map((name) => {
    const method = declaration.members.find((node) => ts.isMethodDeclaration(node) && node.name.getText(tree) === name);
    assert.ok(method); return method.getText(tree);
  });
  const chats = { selectedChat: "123@broadcast" }, calls: { chat: string; typing: boolean }[] = [];
  const behavior = new Function("chats", "broadcastSendError", "invoke", `return ({${methods.join(",\n")}});`)(chats, broadcastSendError,
    (command: string, args: { chat: string; typing: boolean }) => { assert.equal(command, "send_typing"); calls.push(args); return Promise.resolve(); });
  const state = { ...behavior, chatSendsTyping: true, typingSentAt: Date.now() - 6000, typingIdle: undefined };
  try {
    state.reportTyping(); state.stopTyping(); assert.equal(calls.length, 0);
    chats.selectedChat = "ordinary@s.whatsapp.net"; state.reportTyping(); state.stopTyping();
    assert.deepEqual(calls, [{ chat: chats.selectedChat, typing: true }, { chat: chats.selectedChat, typing: false }]);
  } finally { clearTimeout(state.typingIdle); }
});
