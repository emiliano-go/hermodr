import { test } from "node:test";
import assert from "node:assert/strict";
import { formatDraft } from "../lib/utils/composer-format.ts";
import { inline, blocks } from "../lib/utils/format.ts";

test("formatting wraps selections and leaves surrounding whitespace outside markers", () => {
  const bold = formatDraft("hello world", 6, 11, "bold")!;
  assert.deepEqual(bold, { text: "hello *world*", start: 7, end: 12 });
  assert.equal(inline(bold.text)[1].kind, "bold");
  assert.equal(formatDraft("hello", 0, 5, "italic")?.text, "_hello_");
  assert.equal(formatDraft("hello", 0, 5, "strike")?.text, "~hello~");
  assert.equal(formatDraft("  hello  ", 0, 9, "bold")?.text, "  *hello*  ");
  assert.equal(blocks(formatDraft("one\ntwo", 0, 7, "mono")!.text)[0].kind, "pre");
});

test("empty selections place caret inside markers; repeated format removes existing markers", () => {
  assert.deepEqual(formatDraft("ab", 1, 1, "bold"), { text: "a**b", start: 2, end: 2 });
  assert.deepEqual(formatDraft("  ", 0, 2, "italic"), { text: "  __", start: 3, end: 3 });
  assert.deepEqual(formatDraft("*hello*", 1, 6, "bold"), { text: "hello", start: 0, end: 5 });
  assert.deepEqual(formatDraft("*hello*", 0, 7, "bold"), { text: "hello", start: 0, end: 5 });
  assert.equal(formatDraft("has*marker", 0, 10, "bold"), null);
  assert.equal(formatDraft("has`marker", 0, 10, "mono"), null);
  assert.equal(formatDraft("abc", -1, 2, "bold"), null);
});
