import { strict as assert } from "node:assert";
import { test } from "node:test";
import { base64Of } from "../lib/utils/files.ts";

test("attachment encoding preserves empty and binary data across chunk boundaries", async () => {
  for (const size of [0, 1, 255, 32768, 32769, 65537]) {
    const bytes = Uint8Array.from({ length: size }, (_, i) => i % 256);
    assert.equal(await base64Of(new Blob([bytes])), Buffer.from(bytes).toString("base64"));
  }
});
