import assert from "node:assert/strict";
import test from "node:test";
import { limitKey, limitValue, parseLimit } from "./utils/retention.ts";

test("retention keeps inheritance, unlimited and a zero limit distinct", () => {
  for (const key of ["inherit", "unlimited", "0", "24", "500", "4294967295"]) {
    assert.equal(limitKey(parseLimit(key)), key);
  }
  assert.equal(limitValue(parseLimit("0")), 0);
  assert.equal(limitValue(parseLimit("")), null);
  assert.notDeepEqual(parseLimit("inherit"), parseLimit("unlimited"));
  for (const key of ["-1", "NaN", "0.5", "4294967296"]) assert.throws(() => parseLimit(key));
});
