import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { createServer } from "vite";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";
import { englishCatalog, installLocaleProvider, loadCatalog, t } from "../lib/i18n/localizer.ts";

const root = fileURLToPath(new URL("../../", import.meta.url)).replaceAll("\\", "/");
const ref = { code: "error.content.no_account_selected", params: {} };
const plugin = { id: "org.test.plugin", name: "Peer <Plugin>", version: "1.2.3", api_version: 1,
  enabled: true, state: "running", activation: "lazy", idle_timeout_secs: 40, entrypoint: "peer.exe",
  capabilities: ["transcribe"], contributes: { commands: [], transcription: { id: "speech", providers: [] } } };
const raw = "Raw <runtime> failure";
const descriptor = { kind: "postal_error", ...ref, diagnostic: raw };
const decode = (html: string) => html.replace(/<[^>]*>/g, "").replaceAll("&lt;", "<").replaceAll("&gt;", ">")
  .replaceAll("&amp;", "&").replaceAll("&#39;", "'").replaceAll("&quot;", '"');
const alerts = (html: string) => [...html.matchAll(/<p\b(?=[^>]*\brole="alert")[^>]*>([\s\S]*?)<\/p>/g)].map((m) => decode(m[1]));
const details = (html: string) => [...html.matchAll(/<pre\b[^>]*>([\s\S]*?)<\/pre>/g)].map((m) => decode(m[1]));

function seeded(source: string, seeds: Record<string, string>) {
  const start = source.indexOf(">") + 1, end = source.indexOf("</script>", start);
  const tree = ts.createSourceFile("component.ts", source.slice(start, end), ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const edits: { start: number; end: number; value: string }[] = [];
  for (const statement of tree.statements) {
    if (!ts.isVariableStatement(statement)) continue;
    for (const declaration of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declaration.name) || !(declaration.name.text in seeds)) continue;
      assert.ok(declaration.initializer && ts.isCallExpression(declaration.initializer));
      assert.equal(declaration.initializer.expression.getText(tree), "$state");
      const argument = declaration.initializer.arguments[0];
      assert.ok(argument);
      edits.push({ start: start + argument.getStart(tree), end: start + argument.end, value: seeds[declaration.name.text] });
    }
  }
  assert.equal(edits.length, Object.keys(seeds).length);
  for (const edit of edits.sort((a, b) => b.start - a.start)) source = source.slice(0, edit.start) + edit.value + source.slice(edit.end);
  return source;
}

async function renderer() {
  const sources = new Map<string, string>();
  let next = 0;
  const server = await createServer({ configFile: root + "tests/browser/vite.config.ts",
    cacheDir: root + "node_modules/.vite-tests/plugin-transcription-i18n",
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false },
    plugins: [{ name: "private-runtime-fixtures", enforce: "pre",
      resolveId(id) { return sources.has(id) ? id : null; },
      load(id) { return sources.get(id); } }],
  });
  const { render } = await server.ssrLoadModule("svelte/server");
  const localizer = await server.ssrLoadModule(root + "src/lib/i18n/localizer.ts");
  const arabic = await loadCatalog("ar");
  return { close: () => server.close(),
    async body(file: string, seeds: Record<string, string>, locale: string, props = {}) {
      const source = readFileSync(root + file, "utf8"), id = root + file.replace(/\.svelte$/, `.fixture-${next++}.svelte`);
      sources.set(id, seeded(source, seeds));
      const restore = localizer.installLocaleProvider(() => ({ locale, catalog: locale === "en" ? localizer.englishCatalog : arabic }));
      try {
        const { default: Component } = await server.ssrLoadModule(id);
        const html = render(Component, { props }).body as string;
        assert.equal(localizer.t(ref.code), (locale === "en" ? localizer.englishCatalog : arabic)[ref.code]);
        const texts = Object.fromEntries([ref.code, "error.operation_failed"].map((code) => [code, localizer.t(code) as string]));
        return { html, text: (code: string) => texts[code] };
      } finally { restore(); }
    } };
}

test("plugin and transcription discovery prefer typed failures, with compatible legacy diagnostics", async () => {
  const ssr = await renderer();
  try {
    for (const locale of ["en", "ar"]) for (const typed of [false, true]) {
      const plugins = [{ ...plugin, error: "Retained legacy runtime", error_message: ref, diagnostic: raw },
        { ...plugin, id: "org.test.legacy", enabled: false, error: "Legacy <plugin> failure" }];
      const discovery = "Discovery <failure>", failures = typed ? [{ ...ref, diagnostic: discovery }] : [];
      const errors = [discovery];
      const manager = await ssr.body("src/lib/settings/PluginManager.svelte", {
        view: JSON.stringify({ plugins, directory: "C:/Peer <directory>", failures, errors }),
        selected: JSON.stringify(plugins[1]),
      }, locale);
      assert.deepEqual(alerts(manager.html), [manager.text(ref.code), manager.text("error.operation_failed"),
        manager.text(typed ? ref.code : "error.operation_failed")]);
      assert.deepEqual(details(manager.html), [raw, "Legacy <plugin> failure", discovery]);
      assert.match(manager.html, /Peer &lt;Plugin(?:&gt;|>)/);
      assert.match(manager.html, /C:\/Peer &lt;directory(?:&gt;|>)/);
      assert.match(manager.html, /<button\b[^>]*disabled[^>]*>/);
      const settings = { plugin_id: null, provider: "", model: null };
      const transcription = await ssr.body("src/lib/settings/TranscriptionSettings.svelte", {
        view: JSON.stringify({ settings, plugins, failures, errors, cloud_consents: [] }), draft: JSON.stringify(settings),
      }, locale, { autoTranscribe: false, onAutoTranscribe: () => assert.fail("SSR must not change settings") });
      assert.deepEqual(alerts(transcription.html), [transcription.text(typed ? ref.code : "error.operation_failed")]);
      assert.deepEqual(details(transcription.html), [discovery]);
      assert.ok(!alerts(transcription.html).some((text) => text.includes(discovery)));
    }
  } finally { await ssr.close(); }
});

test("runtime leaves render typed command failures and raw details separately", async () => {
  const ssr = await renderer();
  try {
    for (const locale of ["en", "ar"]) {
      const failure = `normalizeError(${JSON.stringify(descriptor)})`;
      const manager = await ssr.body("src/lib/settings/PluginManager.svelte", { error: failure, loadError: failure }, locale);
      assert.deepEqual(alerts(manager.html), [manager.text(ref.code), manager.text(ref.code)]);
      assert.deepEqual(details(manager.html), [raw, raw]);
      const settings = await ssr.body("src/lib/settings/TranscriptionSettings.svelte", { error: failure }, locale,
        { autoTranscribe: false, onAutoTranscribe: () => assert.fail("SSR must not change settings") });
      assert.deepEqual(alerts(settings.html), [settings.text(ref.code)]);
      assert.deepEqual(details(settings.html), [raw]);
    }
  } finally { await ssr.close(); }
});

test("transcript presentation keeps speech and provider data while localizing typed and legacy errors", async () => {
  const ssr = await renderer();
  try {
    const transcript = { chat: "peer@lid", id: "voice", text: "Actual <speech>", language: "custom-language", provider: "Peer provider", created_at: 17 };
    for (const locale of ["en", "ar"]) for (const typed of [false, true]) {
      const failure = typed ? `normalizeError(${JSON.stringify(descriptor)})` : `normalizeError(${JSON.stringify(raw)})`;
      const result = await ssr.body("src/lib/messages/Transcript.svelte", { error: failure, transcript: JSON.stringify(transcript) }, locale,
        { accountId: "a", chat: transcript.chat, id: transcript.id, enabled: true });
      assert.deepEqual(alerts(result.html), [result.text(typed ? ref.code : "error.operation_failed")]);
      assert.deepEqual(details(result.html), [raw]);
      assert.match(result.html, /Actual &lt;speech(?:&gt;|>)/);
      assert.ok(result.html.includes("Peer provider") && result.html.includes("custom-language"));
      assert.match(result.html, /<pre\b[^>]*dir="ltr"[^>]*>/);
    }
  } finally { await ssr.close(); }
});

test("partial sticker resync keeps mirror uncertainty and renders raw errors only in technical details", async () => {
  const ssr = await renderer();
  try {
    const report = { packs: 2, stickers: 5, known_packs: 3, packs_changed: 1, stickers_changed: 2, skipped_stickers: 1,
      app_state_synced: false, app_state_retryable: true, app_state_fatal: false, app_state_error: "App <sync> failure",
      pack_failures: [{ pack_id: "Peer <pack>", error: "Pack <fetch> failure" }], mirror_verified: false, catalog_complete: false };
    for (const locale of ["en", "ar"]) {
      const result = await ssr.body("src/lib/media/StickerSync.svelte", { report: JSON.stringify(report) }, locale,
        { account: "synthetic", chat: "peer@lid", generation: 1, connected: true });
      const catalog = locale === "en" ? englishCatalog : await loadCatalog("ar");
      assert.deepEqual(alerts(result.html), [result.text("error.operation_failed")]);
      assert.deepEqual(details(result.html), [report.app_state_error, report.pack_failures[0].error]);
      const visible = decode(result.html);
      for (const code of ["sticker_sync.partial", "sticker_sync.phase_retry", "content.phone_mirror_unverified",
        "content.known_shared_packs_only_full_installed_catalog_unverified"])
        assert.ok(visible.includes(catalog[code] as string));
      for (const code of ["content.phone_mirror_verified", "content.full_catalog_reported"])
        assert.ok(!visible.includes(catalog[code] as string));
      assert.match(result.html, /Peer &lt;pack(?:&gt;|>)/);
      assert.equal(visible.split(result.text("error.operation_failed")).length - 1, 2);
    }
  } finally { await ssr.close(); }
});

function listener() {
  const source = readFileSync(root + "src/lib/messages/Transcript.svelte", "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("Transcript.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const mount = tree.statements.find((node) => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
    && node.expression.expression.getText(tree) === "onMount");
  assert.ok(mount && ts.isExpressionStatement(mount) && ts.isCallExpression(mount.expression));
  let handler!: (event: { payload: Record<string, unknown> }) => void;
  let stopped = 0;
  const context = { accountId: "a", chat: "peer@lid", id: "voice", hidden: false, busy: false, queued: false,
    transcript: null as unknown, error: null as LocalizedError | null, generation: 1, normalizeError,
    listen(channel: string, callback: typeof handler) {
      assert.equal(channel, "transcription-event"); handler = callback;
      return Promise.resolve(() => { stopped++; });
    } };
  const body = `var dispose = (${mount.expression.arguments[0].getText(tree)})();`;
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return { context, emit: (payload: Record<string, unknown>) => handler({ payload }),
    dispose: () => (context as typeof context & { dispose: () => void }).dispose(), stopped: () => stopped };
}

test("transcription events retain descriptors across locale changes and reject unrelated or hidden events", async () => {
  const event = listener();
  await Promise.resolve();
  const arabic = await loadCatalog("ar");
  let locale = "en";
  const restore = installLocaleProvider(() => ({ locale, catalog: locale === "en" ? englishCatalog : arabic }));
  const payload = { account_id: "a", chat: "peer@lid", id: "voice", status: "failed", transcript: null,
    error: "Retained raw error", error_message: ref, diagnostic: raw };
  try {
    event.emit(payload);
    const failure = event.context.error;
    assert.ok(failure instanceof LocalizedError);
    assert.equal(failure.code, ref.code); assert.deepEqual(failure.params, ref.params);
    assert.equal(failure.message, t(ref.code)); assert.equal(failure.diagnostic, raw);
    locale = "ar";
    assert.equal(failure.message, t(ref.code));
    assert.notEqual(failure.message, englishCatalog[ref.code]);
    for (const patch of [{ account_id: "other" }, { chat: "other@lid" }, { id: "other" }]) {
      event.emit({ ...payload, ...patch, status: "started", error_message: undefined, error: null });
      assert.equal(event.context.error, failure); assert.equal(event.context.busy, false);
    }
    event.context.hidden = true;
    event.emit({ ...payload, status: "started", error_message: undefined, error: null });
    assert.equal(event.context.error, failure); assert.equal(event.context.busy, false);
    event.context.hidden = false;
    event.emit({ ...payload, status: "queued", error_message: undefined, error: null });
    assert.equal(event.context.busy, true); assert.equal(event.context.queued, true);
    event.emit({ ...payload, status: "cancelled", error_message: undefined, error: null });
    assert.equal(event.context.busy, false); assert.equal(event.context.queued, false);
    event.emit({ ...payload, error_message: undefined, diagnostic: undefined, error: raw });
    assert.equal(event.context.error?.code, "error.operation_failed");
    assert.equal(event.context.error?.diagnostic, raw);
    event.emit({ ...payload, status: "started", error_message: undefined, diagnostic: undefined, error: null });
    assert.equal(event.context.busy, true); assert.equal(event.context.queued, false); assert.equal(event.context.error, null);
    const transcript = { text: "Actual <speech>", provider: "Peer provider" };
    event.emit({ ...payload, status: "complete", transcript, error_message: undefined, diagnostic: undefined, error: null });
    assert.equal(event.context.busy, false); assert.equal(event.context.transcript, transcript); assert.equal(event.context.error, null);
    event.dispose();
    event.emit(payload);
    assert.equal(event.context.error, null); assert.equal(event.stopped(), 1); assert.equal(event.context.generation, 2);
  } finally { restore(); }
});
