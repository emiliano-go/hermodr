import { t } from "../i18n/localizer.ts";
import type { GroupAuditEntry, GroupAuditFilter, GroupAuditPage, GroupAuditKind } from "./wire";

export type AuditEntry = GroupAuditEntry;
export type AuditScope = { account: string; group: string; member: string | null; requestKey: string | number };
export type AuditFilters = Pick<GroupAuditFilter, "kind" | "actor" | "since" | "until">;
export type AuditPage = GroupAuditPage;
export function auditEntryRequested(entry: Pick<AuditEntry, "kind" | "source">): boolean {
  return entry.source === "local" && ["message_edit", "message_delete", "message_pin", "message_unpin", "member_tag"].includes(entry.kind);
}
export function auditEntryLabel(entry: Pick<AuditEntry, "kind" | "source">): string {
  return t(`group.audit_${auditEntryRequested(entry) ? "request" : "kind"}_${entry.kind}`);
}
export const AUDIT_KINDS: readonly GroupAuditKind[] = ["join", "leave", "remove", "promote", "demote", "subject", "description", "locked", "announce", "ephemeral", "join_approval", "member_add_mode", "forwarding", "invite_change", "create", "delete", "picture", "message_edit", "message_delete", "message_pin", "message_unpin", "member_tag", "member_link_mode", "member_share_history_mode", "history_sharing", "owner_change"];

function day(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const [year, month, date] = value.split("-").map(Number);
  const result = new Date(`${value}T00:00:00`);
  return Number.isFinite(result.getTime()) && result.getFullYear() === year && result.getMonth() + 1 === month && result.getDate() === date ? result : null;
}

export function auditFilters(kind: string, actor: string, from: string, to: string): { filters: AuditFilters | null; error: string } {
  const start = from ? day(from) : null, end = to ? day(to) : null;
  if (from && !start || to && !end) return { filters: null, error: t("group.audit_dates_invalid") };
  if (kind.trim() && !AUDIT_KINDS.includes(kind.trim() as GroupAuditKind)) return { filters: null, error: t("group.audit_kind_invalid") };
  if (start && end && start > end) return { filters: null, error: t("group.audit_date_order") };
  if (end) end.setDate(end.getDate() + 1);
  return { filters: { kind: (kind.trim() || null) as GroupAuditKind | null, actor: actor.trim() || null,
    since: start ? Math.floor(start.getTime() / 1000) : null, until: end ? Math.floor(end.getTime() / 1000) - 1 : null }, error: "" };
}

export function mergeAuditEntries(previous: AuditEntry[], next: AuditEntry[]): AuditEntry[] {
  const known = new Set(previous.map((entry) => entry.id));
  return [...previous, ...next.filter((entry) => { if (known.has(entry.id)) return false; known.add(entry.id); return true; })];
}
