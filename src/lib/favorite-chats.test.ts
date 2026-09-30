import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { favoriteRows } from "./utils/favorite-chats.ts";
import type { FavoritesState } from "./state/favorites.svelte";

test("Favorites view follows synced order and retains favorite contacts without message history", () => {
  const unseen = favoriteRows([], ["100@s.whatsapp.net"])[0];
  const archived = { ...unseen, chat: "200@g.us", archived: true, pinned: true, last_message_at: 999 };
  const recent = { ...unseen, chat: "300@s.whatsapp.net", last_message_at: 9999 };
  const ids = ["100@s.whatsapp.net", "200@g.us", "100@s.whatsapp.net"];
  const rows = favoriteRows([recent, archived], ids);
  assert.deepEqual(rows.map((row) => row.chat), ["100@s.whatsapp.net", "200@g.us"]);
  assert.equal(rows[0].message_count, 0);
  assert.equal(rows[0].last_message_at, 0);
  assert.equal(rows[1], archived);
  assert.deepEqual(favoriteRows([recent, archived], []), []);
  assert.deepEqual(ids, ["100@s.whatsapp.net", "200@g.us", "100@s.whatsapp.net"]);
});

test("Favorite state rejects old account responses and exposes current command failures", async () => {
  const requests: { cmd: string; args: unknown; resolve: (value: unknown) => void; reject: (reason: unknown) => void }[] = [];
  const errors: unknown[] = [];
  const session = { activeAccount: "A" as string | null };
  const invoke = (cmd: string, args?: unknown) => new Promise((resolve, reject) => requests.push({ cmd, args, resolve, reject }));
  const source = readFileSync(new URL("./state/favorites.svelte.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const compiled = compileModule(js, { generate: "server", filename: "favorites.svelte.js" }).js.code
    .replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const State = new Function("invoke", "session", "ui", "favoriteRows", `${compiled}\nreturn FavoritesState;`)
    (invoke, session, { fail: (error: unknown) => errors.push(error) }, favoriteRows) as new () => FavoritesState;
  const favorites = new State();
  const old = favorites.refresh();
  session.activeAccount = "B";
  favorites.reset();
  const current = favorites.refresh();
  assert.deepEqual(requests[0].args, { account: "A" });
  assert.deepEqual(requests[1].args, { account: "B" });
  requests[1].resolve(["200@g.us"]);
  await current;
  requests[0].resolve(["100@s.whatsapp.net"]);
  await old;
  assert.deepEqual(favorites.chats, ["200@g.us"]);

  const pending = favorites.toggle("300@g.us");
  assert.deepEqual(requests[2].args, { account: "B", chat: "300@g.us", favorite: true });
  session.activeAccount = "A";
  favorites.reset();
  requests[2].reject(new Error("old account error"));
  await pending;
  assert.deepEqual(errors, []);
  assert.equal(favorites.busy, null);

  const failed = favorites.toggle("100@s.whatsapp.net");
  requests[3].reject(new Error("offline"));
  await failed;
  assert.equal((errors[0] as Error).message, "offline");
  assert.deepEqual(favorites.chats, []);
  assert.equal(favorites.busy, null);

  const stale = favorites.refresh();
  const newest = favorites.refresh();
  requests[5].resolve(["100@s.whatsapp.net"]);
  await newest;
  requests[4].reject(new Error("stale reload error"));
  await stale;
  assert.deepEqual(favorites.chats, ["100@s.whatsapp.net"]);
  assert.equal(errors.length, 1);

  const removed = favorites.toggle("100@s.whatsapp.net");
  assert.deepEqual(requests[6].args, { account: "A", chat: "100@s.whatsapp.net", favorite: false });
  requests[6].resolve(undefined);
  await Promise.resolve();
  requests[7].resolve([]);
  await removed;
  assert.deepEqual(favorites.chats, []);
  assert.equal(favorites.busy, null);
});
