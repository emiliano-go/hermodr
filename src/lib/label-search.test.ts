import { test } from "node:test";
import assert from "node:assert/strict";
import { labelSearch } from "./utils/label-search.ts";

test("label search keeps surrounding text and names containing spaces", () => {
  assert.deepEqual(labelSearch('invoice Label:"To do" overdue'), { name: "To do", query: "invoice  overdue" });
  assert.deepEqual(labelSearch("Label:TODO"), { name: "TODO", query: "" });
  assert.deepEqual(labelSearch("label:'Other label' test"), { name: "Other label", query: "test" });
});

test("ordinary text and URLs do not become label filters", () => {
  assert.equal(labelSearch("https://example.invalid/label:TODO"), null);
  assert.equal(labelSearch("mylabel:TODO"), null);
  assert.equal(labelSearch("label:"), null);
});
