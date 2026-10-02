import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { MEDIA_TYPES, emptyMediaOverrides } from "../lib/utils/auto-download.ts";
import type { MediaPolicyState } from "../lib/state/media-policy.svelte";

test("Media overrides save true/false/null independently and reject old account completions", async () => {
  const pending: { cmd: string; args: Record<string, unknown>; resolve: (v: unknown) => void; reject: (e: Error) => void }[] = [];
  const invoke = (cmd: string, args: Record<string, unknown>) => new Promise((resolve, reject) => pending.push({ cmd, args, resolve, reject }));
  const source = readFileSync(new URL("../lib/state/media-policy.svelte.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const compiled = compileModule(js, { generate: "server", filename: "media-policy.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const State = new Function("invoke", "emptyMediaOverrides", `${compiled}\nreturn MediaPolicyState;`)(invoke, emptyMediaOverrides) as new () => MediaPolicyState;
  const policy = new State();
  const old = policy.load("old", "first");
  const current = policy.load("new", "second");
  pending[1].resolve({ ...emptyMediaOverrides(), image: false }); await current;
  pending[0].resolve({ ...emptyMediaOverrides(), audio: true }); await old;
  assert.equal(policy.value.audio, null); assert.equal(policy.value.image, false);
  for (const enabled of [true, false, null]) {
    const save = policy.change("audio", enabled);
    const request = pending.at(-1)!;
    assert.equal(request.cmd, "set_chat_media_auto_download");
    assert.deepEqual(request.args, { accountId: "new", chat: "second", overrides: { ...emptyMediaOverrides(), image: false, audio: enabled } });
    request.resolve(undefined); await save;
    assert.equal(policy.value.audio, enabled); assert.equal(policy.value.image, false);
  }
  const lateSave = policy.change("sticker", true);
  const saveRequest = pending.at(-1)!;
  const switched = policy.load("third", "other");
  pending.at(-1)!.resolve(emptyMediaOverrides()); await switched;
  saveRequest.reject(new Error("old account gone")); await lateSave;
  assert.equal(policy.error, ""); assert.equal(policy.value.sticker, null); assert.equal(policy.busy, false);
  const failed = policy.change("gif", true); pending.at(-1)!.reject(new Error("cannot save")); await failed;
  assert.match(policy.error, /cannot save/); assert.equal(policy.value.gif, null);
  assert.deepEqual(MEDIA_TYPES.map(([kind]) => kind), ["image", "video", "audio", "document", "sticker", "gif"]);
});
