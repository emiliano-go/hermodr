import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import type { TranscriptionState } from "./state/transcription.svelte";
import type { TranscriptionView } from "./utils/wire";

function view(enabled: boolean, provider = "local") {
  return { settings: { plugin_id: "org.postal.test", provider }, plugins: [{ id: "org.postal.test", enabled, contributes: { transcription: { providers: [{ id: "local" }] } } }] } as TranscriptionView;
}

test("Transcription controls reject stale settings, disabled plugins and undeclared providers", async () => {
  const pending: { resolve: (v: TranscriptionView) => void; reject: (e: Error) => void }[] = [];
  const invoke = () => new Promise<TranscriptionView>((resolve, reject) => pending.push({ resolve, reject }));
  const source = readFileSync(new URL("./state/transcription.svelte.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const compiled = compileModule(js, { generate: "server", filename: "transcription.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const State = new Function("invoke", "listen", `${compiled}\nreturn TranscriptionState;`)(invoke, () => Promise.resolve(() => {})) as new () => TranscriptionState;
  const state = new State();
  assert.equal(state.enabled, false);
  const old = state.load();
  const current = state.load();
  pending[1].resolve(view(true)); await current;
  pending[0].resolve(view(false)); await old;
  assert.equal(state.enabled, true);
  const delayed = state.load();
  state.configure(view(false));
  pending[2].reject(new Error("old request failed")); await delayed;
  assert.equal(state.enabled, false); assert.equal(state.error, "");
  state.configure(view(true, "undeclared")); assert.equal(state.enabled, false);
  const failed = state.load(); pending[3].reject(new Error("offline")); await failed;
  assert.equal(state.error, "Error: offline"); assert.equal(state.enabled, false);
});
