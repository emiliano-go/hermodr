import assert from "node:assert/strict";
import test from "node:test";
import { wallpaperDataUrl } from "./utils/chat-wallpaper.ts";

const picture = "data:image/jpeg;base64,/9j/AA==";
const png = () => new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "synthetic.png", { type: "image/png" });
function browser(width: number, height: number, output = picture) {
  let closed = 0;
  const canvas = { width: 0, height: 0, getContext: () => ({ drawImage() {} }), toDataURL: () => output };
  const keys = ["createImageBitmap", "document"] as const;
  const previous = keys.map((key) => Object.getOwnPropertyDescriptor(globalThis, key));
  Object.defineProperty(globalThis, "createImageBitmap", { configurable: true, value: async () => ({ width, height, close() { closed++; } }) });
  Object.defineProperty(globalThis, "document", { configurable: true, value: { createElement: () => canvas } });
  return { canvas, closed: () => closed, restore() { keys.forEach((key, index) => {
    if (previous[index]) Object.defineProperty(globalThis, key, previous[index]!); else Reflect.deleteProperty(globalThis, key);
  }); } };
}

test("picture input rejects empty, oversized and disguised nonraster files before browser decoding", async () => {
  await assert.rejects(wallpaperDataUrl(new File([], "empty.png")), /smaller than 8 MB/);
  await assert.rejects(wallpaperDataUrl(new File([new Uint8Array(8 * 1024 * 1024 + 1)], "large.png")), /smaller than 8 MB/);
  await assert.rejects(wallpaperDataUrl(new File(["<svg xmlns='http://www.w3.org/2000/svg'></svg>"], "fake.png", { type: "image/png" })), /PNG, JPEG, WebP or GIF/);
});

test("decoded picture is downscaled without enlargement and releases its bitmap", async () => {
  for (const [width, height, expected] of [[2560, 1280, [1280, 640]], [100, 50, [100, 50]]] as const) {
    const mock = browser(width, height);
    try {
      assert.equal(await wallpaperDataUrl(png()), picture);
      assert.deepEqual([mock.canvas.width, mock.canvas.height], expected);
      assert.equal(mock.closed(), 1);
    } finally { mock.restore(); }
  }
});

test("oversized decoded or encoded images fail and still release their bitmap", async () => {
  for (const [width, height, output] of [[5000, 4000, picture], [100, 50, picture + "A".repeat(1024 * 1024)], [100, 50, "data:image/svg+xml;base64,PHN2Zz4="]] as const) {
    const mock = browser(width, height, output);
    try {
      await assert.rejects(wallpaperDataUrl(png()), /at most 16 megapixels|smaller or less detailed/);
      assert.equal(mock.closed(), 1);
    } finally { mock.restore(); }
  }
});
