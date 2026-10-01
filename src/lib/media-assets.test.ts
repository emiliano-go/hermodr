import test from "node:test";
import assert from "node:assert/strict";
import { createMediaAssetPreparer } from "./utils/media-assets.ts";

test("returned DTO media paths canonicalize together while nonmedia and web resources stay unchanged", async () => {
  const calls: string[][] = [];
  const prepare = createMediaAssetPreparer(async (paths) => {
    calls.push(paths);
    return Object.fromEntries(paths.map((path) => [path, path === "/media/missing.png" ? null : path.replace("/media/./", "/media/")]));
  });
  const response = { messages: [{ text: "/private/session.db", media_path: "/media/./photo.jpg", media_thumb: "data:image/png;base64,AA==",
    reply_to_path: "/media/quote.png", reply_to_thumb: "https://example.test/preview.png" }],
    pack: { tray_path: "/media/./photo.jpg", stickers: [{ path: "/media/missing.png" }, { path: "relative.png" }] },
    thumbnail: { media_thumb: "blob:preview", picture: "/media/./photo.jpg", avatar: "/media/missing.png" },
    config: { session: "C:\\account\\session.db" }, strings: ["/media/unrelated.png"] };
  assert.equal(await prepare("messages", response), response);
  assert.deepEqual(calls, [["/media/./photo.jpg", "/media/quote.png", "/media/missing.png"]]);
  assert.equal(response.messages[0].media_path, "/media/photo.jpg");
  assert.equal(response.pack.tray_path, "/media/photo.jpg");
  assert.equal(response.pack.stickers[0].path, null);
  assert.equal(response.pack.stickers[1].path, "relative.png");
  assert.equal(response.messages[0].text, "/private/session.db");
  assert.equal(response.messages[0].media_thumb, "data:image/png;base64,AA==");
  assert.equal(response.messages[0].reply_to_thumb, "https://example.test/preview.png");
  assert.equal(response.thumbnail.media_thumb, "blob:preview");
  assert.equal(response.thumbnail.picture, "/media/photo.jpg");
  assert.equal(response.thumbnail.avatar, null);
  assert.equal(response.config.session, "C:\\account\\session.db");
  assert.deepEqual(response.strings, ["/media/unrelated.png"]);
});

test("scalar file commands and media-library arrays authorize only absolute returned paths", async () => {
  const calls: string[][] = [];
  const prepare = createMediaAssetPreparer(async (paths) => {
    calls.push(paths);
    return Object.fromEntries(paths.map((path) => [path, path.replace("\\alias\\", "\\canonical\\")]));
  });
  for (const command of ["avatar", "save_sticker", "download_sticker", "playable_audio", "playable_video"]) {
    assert.equal(await prepare(command, "C:\\alias\\photo.png"), "C:\\canonical\\photo.png");
  }
  assert.equal(await prepare("about", "C:\\private\\session.db"), "C:\\private\\session.db");
  assert.equal(await prepare("send_media", "/warning/message"), "/warning/message");
  assert.equal(await prepare("download_media", undefined), undefined);
  assert.equal(await prepare("recover_quote_media", undefined), undefined);
  assert.equal(await prepare("avatar", null), null);
  assert.equal(await prepare("avatar", "https://example.test/avatar"), "https://example.test/avatar");
  assert.deepEqual(await prepare("media_library", ["\\\\server\\share\\photo.png", "/media/gif.gif", "blob:preview"]),
    ["\\\\server\\share\\photo.png", "/media/gif.gif", "blob:preview"]);
  assert.equal(calls.length, 6);
});

test("coalesced authorization deduplicates simultaneous responses and bounds native batches to 512", async () => {
  const calls: string[][] = [];
  const prepare = createMediaAssetPreparer(async (paths) => {
    calls.push(paths);
    return Object.fromEntries(paths.map((path) => [path, path]));
  });
  const rows = Array.from({ length: 1025 }, (_, index) => ({ media_path: `/media/${index}.png` }));
  const avatar = prepare("avatar", "/media/0.png");
  await Promise.all([prepare("messages", rows), avatar]);
  assert.deepEqual(calls.map((batch) => batch.length), [512, 512, 1]);
  assert.equal(calls.flat().filter((path) => path === "/media/0.png").length, 1);
  assert.equal(rows[1024].media_path, "/media/1024.png");
});

test("missing, malformed and failed grants close media paths without failing messages or caching rejections", async () => {
  let failed = true;
  const prepare = createMediaAssetPreparer(async () => {
    if (failed) throw new Error("synthetic authorization failure");
    return { "/media/a.png": "/media/a.png", "/media/b.png": "https://example.test/not-a-file" };
  });
  const rows = [{ media_path: "/media/a.png", text: "kept" }, { media_path: "/media/b.png" }, { media_path: "/media/c.png" }];
  assert.equal(await prepare("messages", rows), rows);
  assert.deepEqual(rows.map((row) => row.media_path), [null, null, null]);
  assert.equal(rows[0].text, "kept");
  failed = false;
  assert.equal(await prepare("avatar", "/media/a.png"), "/media/a.png");
  assert.equal(await prepare("avatar", "/media/b.png"), null);
  assert.equal(await prepare("avatar", "/media/c.png"), null);
  assert.deepEqual(await prepare("media_library", ["/media/a.png", "/media/b.png", "/media/c.png"]), ["/media/a.png"]);
});
