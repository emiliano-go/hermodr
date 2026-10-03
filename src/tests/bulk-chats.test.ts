import assert from "node:assert/strict";
import test from "node:test";
import { bulkReadError, formatBulkReadFailures } from "../lib/utils/bulk-chats.ts";
import { uiError } from "../lib/state/localized.ts";
import { englishCatalog, installLocaleProvider, loadCatalog, t, type Catalog } from "../lib/i18n/localizer.ts";

test("bulk read failures preserve every failed chat and ignore successful manual unread clears", () => {
  const results = [
    { chat: "archived", changed: 3, error: null },
    { chat: "failed", changed: null, error: uiError("error.operation_failed", {}, "synthetic receipt failure").descriptor },
    { chat: "manual", changed: 0, error: null },
    { chat: "second", changed: null, error: uiError("error.operation_failed", {}, "synthetic store failure").descriptor },
  ];
  assert.deepEqual(formatBulkReadFailures(results, (chat) => `Name ${chat}`), [
    "Name failed: Operation failed.", "Name second: Operation failed.",
  ]);
  assert.deepEqual(formatBulkReadFailures([], (chat) => chat), []);
  assert.deepEqual(formatBulkReadFailures(results.filter((result) => result.error === null), (chat) => chat), []);
  const failure = bulkReadError(results, (chat) => `Name ${chat}`)!;
  assert.equal(failure.message, "Name failed: Operation failed.\nName second: Operation failed.");
  assert.match(failure.diagnostic ?? "", /synthetic receipt failure/);
  assert.match(failure.diagnostic ?? "", /synthetic store failure/);
  assert.equal(bulkReadError([], (chat) => chat), null);
});

test("retained bulk failure retranslates every typed reason and keeps raw diagnostics separate", async () => {
  const arabic = await loadCatalog("ar");
  let snapshot: { locale: string; catalog: Catalog } = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => snapshot);
  try {
    const failure = bulkReadError([{ chat: "user-provided chat", changed: null,
      error: uiError("error.operation_failed", {}, "raw protocol text").descriptor }], (chat) => chat)!;
    assert.equal(failure.message, "user-provided chat: Operation failed.");
    snapshot = { locale: "ar", catalog: arabic };
    assert.equal(failure.message, `user-provided chat: ${t("error.operation_failed")}`);
    assert.ok(!failure.message.includes("[object Object]")); assert.ok(!failure.message.includes("raw protocol text"));
    assert.match(failure.diagnostic ?? "", /raw protocol text/);
  } finally { restore(); }
});
