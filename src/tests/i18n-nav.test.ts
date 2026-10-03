import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { parse } from "svelte/compiler";
import ts from "typescript";
import { runInNewContext } from "node:vm";
import { normalizeError } from "../lib/i18n/errors.ts";
import { joinRequestResults } from "../lib/utils/join-requests.ts";

test("join request results retain raw outcomes while preserving completed membership and selection", async () => {
  const source = readFileSync(new URL("../lib/chat/GroupRequests.svelte", import.meta.url), "utf8");
  const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("group-requests.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const declarations = tree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations);
  const derived = declarations.find((node) => node.name.getText(tree) === "outcomes")!.initializer!;
  assert.ok(ts.isCallExpression(derived));
  const changes = [{ jid: "one@lid", ok: true, pending: false, code: null, error: null },
    { jid: "two@lid", ok: false, pending: false, code: "403", error: "Synthetic refusal" }];
  const calls: unknown[] = [];
  const context: Record<string, any> = { chat: "group@g.us", session: { activeAccount: "synthetic" }, generation: 1,
    loading: false, busy: false, error: null, outcomeBatch: null, selected: { "one@lid": true, "two@lid": true },
    requests: [{ jid: "one@lid", name: "One" }, { jid: "two@lid", name: "Two" }],
    namer: (name: string | null, jid: string) => name ?? jid, normalizeError, joinRequestResults,
    onchange: async (jids: string[], approve: boolean) => { calls.push([jids, approve]); return changes; },
    onload: async () => [{ jid: "two@lid", name: "Two" }] };
  const functions = tree.statements.filter(ts.isFunctionDeclaration).map((node) => node.getText(tree)).join("\n");
  const code = ts.transpileModule(`${functions}\nvar readOutcomes = () => (${derived.arguments[0].getText(tree)});`,
    { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  Object.defineProperty(context, "picked", { get: () => Object.keys(context.selected) });
  Object.defineProperty(context, "outcomes", { get: () => context.readOutcomes() });
  runInNewContext(code, context);
  await context.act(["one@lid", "two@lid"], true);
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [[["one@lid", "two@lid"], true]]);
  assert.equal(context.outcomeBatch.changes, changes);
  assert.equal(context.outcomeBatch.approve, true);
  assert.equal(context.outcomes[0].completed, true);
  assert.equal(context.outcomes[1].completed, false);
  assert.deepEqual(Object.keys(context.selected), ["two@lid"]);
  assert.equal(context.requests.length, 1);
  assert.equal(context.busy, false);
});

test("floating private-content decision uses wire flags regardless of translated media label", () => {
  const source = readFileSync(new URL("../lib/chat/FloatChat.svelte", import.meta.url), "utf8");
  let expression = "";
  function walk(node: any) {
    if (!node || typeof node !== "object") return;
    if (node.type === "ConditionalExpression" && source.slice(node.test.start, node.test.end).includes("content.notice"))
      expression = source.slice(node.test.start, node.test.end);
    for (const [key, value] of Object.entries(node)) {
      if (["parent", "metadata", "loc"].includes(key)) continue;
      if (Array.isArray(value)) value.forEach(walk); else if (value && typeof value === "object") walk(value);
    }
  }
  walk(parse(source, { modern: true }).fragment);
  assert.ok(expression);
  assert.doesNotMatch(expression, /One-time media|content\.media/);
  const hidden = new Function("content", "message", `return !!(${expression});`);
  for (const media of ["One-time media", "وسائط لمرة واحدة", "unrelated display label"]) {
    assert.equal(hidden({ notice: false, media }, { media_kind: "view_once", media_once_kind: null }), true);
    assert.equal(hidden({ notice: false, media }, { media_kind: "image", media_once_kind: "image" }), true);
    assert.equal(hidden({ notice: false, media }, { media_kind: "image", media_once_kind: null }), false);
  }
  assert.equal(hidden({ notice: true }, {}), true);
  assert.equal(hidden({ notice: false }, { spoiler: true }), true);
});

test("floating error descriptors render live locale messages with escaped optional diagnostics", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/i18n-nav", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: FloatChat } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/FloatChat.svelte", import.meta.url)));
    const { LocalizedError } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/i18n/errors.ts", import.meta.url)));
    const { installLocaleProvider, loadCatalog, t } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/i18n/localizer.ts", import.meta.url)));
    const error = new LocalizedError({ kind: "postal_error", code: "error.spaces_unavailable", params: {}, diagnostic: "<synthetic diagnostic>" });
    const props = { context: { account_id: "synthetic", chat: "123@lid", title: "Synthetic name", connected: false },
      rows: [], draft: "", error, draftReady: false, ondraft: () => {}, onopacity: () => {},
      onsend: async () => { throw new Error("SSR must not send"); }, onolder: async () => {}, onretry: async () => {},
      onretrydraft: () => {}, onclose: async () => {} };
    const english = render(FloatChat, { props }).body;
    assert.ok(english.includes(error.message));
    assert.match(english, /&lt;synthetic diagnostic(?:&gt;|>)/);
    assert.ok(!english.includes(error.code) && !english.includes("<synthetic diagnostic>"));
    const catalog = await loadCatalog("ar"), restore = installLocaleProvider(() => ({ locale: "ar", catalog }));
    try {
      const arabic = render(FloatChat, { props }).body;
      assert.ok(arabic.includes(error.message) && arabic.includes(t("error.technical_details")) && arabic.includes(t("action.dismiss")));
      assert.notEqual(arabic, english);
      assert.equal(error.descriptor.code, "error.spaces_unavailable");
      assert.equal(error.diagnostic, "<synthetic diagnostic>");
      assert.equal(props.context.chat, "123@lid");
    } finally { restore(); }
  } finally { await server.close(); }
});
