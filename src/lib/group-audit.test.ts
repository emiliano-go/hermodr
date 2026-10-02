import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { AUDIT_KINDS, auditEntryLabel, auditFilters, mergeAuditEntries } from "./utils/group-audit.ts";
import type { AuditEntry } from "./utils/group-audit.ts";

test("local message controls remain sent requests while observed events keep their kind", () => {
  assert.equal(auditEntryLabel({ kind: "message_pin", source: "local" }), "Pin request sent");
  assert.equal(auditEntryLabel({ kind: "message_delete", source: "local" }), "Delete request sent");
  assert.equal(auditEntryLabel({ kind: "member_tag", source: "local" }), "Member tag request sent");
  assert.equal(auditEntryLabel({ kind: "message_pin", source: "message" }), "message pin");
  assert.equal(auditEntryLabel({ kind: "subject", source: "local" }), "subject");
});

test("audit date filters validate calendar dates and include complete local end day", () => {
  const result = auditFilters(" join ", " 100@lid ", "2024-02-29", "2024-02-29");
  assert.equal(result.error, "");
  assert.deepEqual(result.filters, { kind: "join", actor: "100@lid",
    since: new Date("2024-02-29T00:00:00").getTime() / 1000, until: new Date("2024-03-01T00:00:00").getTime() / 1000 - 1 });
  assert.equal(auditFilters("", "", "", "").filters?.since, null);
  assert.equal(auditFilters("unknown", "", "", "").filters, null);
  for (const value of ["2023-02-29", "2024-04-31", "not-a-date"]) assert.equal(auditFilters("", "", value, "").filters, null);
  assert.match(auditFilters("", "", "2026-10-02", "2026-10-01").error, /after/);
});

test("audit paging deduplicates overlapping rows without reversing server order", () => {
  const row = (id: number): AuditEntry => ({ id, chat: "100@g.us", kind: "create", actor: null, target: null, old_value: null, new_value: null, old_source: null,
    timestamp: 0, observed_at: 0, source: "history", message_id: null, jump_available: false });
  const previous = [row(2)];
  const merged = mergeAuditEntries(previous, [row(2), row(1), row(1)]);
  assert.deepEqual(merged.map((entry) => entry.id), [2, 1]);
  assert.equal(merged[0], previous[0]);
  assert.deepEqual(previous.map((entry) => entry.id), [2]);
});

test("audit selector offers every current native audit kind", () => {
  const source = readFileSync(new URL("../../crates/postal-core/src/store/group_audit.rs", import.meta.url), "utf8");
  const variants = source.match(/pub enum GroupAuditKind\s*\{([^}]+)\}/)![1].split(",").map((value) => value.trim()).filter(Boolean);
  const native = variants.map((value) => value.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase());
  assert.deepEqual([...AUDIT_KINDS].sort(), native.sort());
});
