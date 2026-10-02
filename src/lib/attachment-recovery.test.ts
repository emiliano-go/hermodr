import { test } from "node:test";
import assert from "node:assert/strict";
import { findRecovery, recoveryKey } from "./utils/attachment-recovery.ts";
import type { AttachmentRecovery } from "./utils/models";

test("recovery buttons cannot select another account, chat, generation or reused batch", () => {
  const record = { context: { accountId: "account", accountSeq: 1, generation: 2, chatGeneration: 3, chat: "a@s", batch: "one" },
    retryable: [], uncertain: [], sentIds: [], error: "synthetic" } as unknown as AttachmentRecovery;
  const key = recoveryKey(record);
  assert.equal(findRecovery([record], key), record);
  for (const change of [{ accountId: "other" }, { accountSeq: 2 }, { generation: 3 }, { chatGeneration: 4 }, { chat: "b@s" }, { batch: "two" }]) {
    const replacement = { ...record, context: { ...record.context, ...change } };
    assert.equal(findRecovery([replacement], key), undefined);
  }
  assert.equal(findRecovery([], key), undefined);
});
