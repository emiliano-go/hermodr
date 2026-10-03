import { t, formatDate, formatTime, formatNumber } from "../i18n/localizer.ts";
import { LocalizedError } from "../i18n/errors.ts";
import type { CachedMemberGroup, MemberBusinessHours, MemberProfileLocal, MemberProfileLive, MemberProfileLiveView, MessageFailure, Participant } from "./wire";

export type MemberScope = { account: string; group: string; jid: string; requestKey: string | number };
export function memberScopeMatches(scope: MemberScope | null, account: string | null, group: string, jid: string, requestKey: string | number): boolean {
  return scope !== null && scope.account === account && scope.group === group && scope.jid === jid && scope.requestKey === requestKey;
}
export type MemberAction = "promote" | "demote" | "remove" | "block" | "unblock" | "report";
export type MemberLocalView = MemberProfileLocal;
export function memberNoteError(notes: string, warnings: number | undefined): string {
  if (warnings === undefined || !Number.isInteger(warnings) || warnings < 0 || warnings > 100000) return t("contact.warning_invalid");
  if (notes.includes("\0")) return t("contact.notes_nul");
  if (Array.from(notes).length > 4096) return t("contact.notes_length");
  if (new TextEncoder().encode(notes).length > 16384) return t("contact.notes_bytes");
  return "";
}
export function memberLocalMatches(local: Pick<MemberProfileLocal, "jid" | "addresses" | "scope_chat"> | null, jid: string, group: string): boolean {
  return local !== null && local.scope_chat === group && (local.jid === jid || local.addresses.includes(jid));
}
export type MemberLiveView = MemberProfileLiveView;
type MemberField = MemberProfileLive["about"] | MemberProfileLive["device_count"] | MemberProfileLive["business"];
export function memberFieldText(field: MemberField | null, format: (value: NonNullable<MemberField["value"]>) => string = (value) => typeof value === "number" ? formatNumber(value) : String(value), failure: MessageFailure | null = null): string {
  if (!field) return t("ui.unavailable");
  if (field.state === "restricted") return failure ? t(failure.code, failure.params) : t("contact.access_denied");
  if (field.state === "error") return t(field.stale && field.value !== null ? "contact.field_cached_failure" : "contact.field_refresh_failed", { value: field.value !== null ? format(field.value) : "", error: failure ? t(failure.code, failure.params) : t("error.member_field_query") });
  if (field.state !== "available" || field.value === null) return t("ui.not_provided");
  const value = format(field.value) || t("ui.empty_value");
  return field.stale ? t("contact.field_cached", { value }) : value;
}
export function memberBusinessHours(hours: MemberBusinessHours): string {
  const day = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"].indexOf(hours.day.length === 3 ? hours.day.toLowerCase() : ({ sunday: "sun", monday: "mon", tuesday: "tue", wednesday: "wed", thursday: "thu", friday: "fri", saturday: "sat" } as Record<string, string>)[hours.day.toLowerCase()] ?? "");
  const mode = ({ open: "contact.hours_open", closed: "contact.hours_closed", open_24h: "contact.hours_24", open_24_hours: "contact.hours_24", appointment_only: "contact.hours_appointment", by_appointment: "contact.hours_appointment", specific_hours: "contact.hours_open" } as Record<string, string>)[hours.mode.toLowerCase()] ?? "contact.hours_unknown";
  const time = (minutes: number) => formatTime(minutes * 60, { hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZone: "UTC" });
  return t("contact.business_hours_format", { day: day < 0 ? t("contact.day_unknown") : formatDate((3 + day) * 86400, { weekday: "long", timeZone: "UTC" }), mode: t(mode), opening: hours.open_minutes === null ? "" : t("contact.hours_opening", { time: time(hours.open_minutes) }), closing: hours.close_minutes === null ? "" : t("contact.hours_closing", { time: time(hours.close_minutes) }) });
}
export type MemberPermissions = { admin: boolean; connected: boolean; self: boolean; ready?: boolean; member: Pick<Participant | CachedMemberGroup, "admin" | "owner"> | null;
  blocked: boolean | null; supported: readonly MemberAction[] };

function memberActionCode(action: MemberAction, state: MemberPermissions): string | null {
  if (state.ready === false) return "contact.member_not_loaded";
  if (!state.admin) return "contact.member_admin_only";
  if (!state.connected) return "contact.member_actions_connect";
  if (state.self) return "contact.member_self_action";
  if (!state.supported.includes(action)) return "contact.member_action_unavailable";
  if (action === "promote" || action === "demote" || action === "remove") {
    if (!state.member) return "contact.membership_unknown";
    if (state.member.admin === null || state.member.owner === null) return "contact.role_unknown";
    if (state.member.owner) return "contact.owner_unchangeable";
    if (action === "promote" && state.member.admin) return "contact.already_admin";
    if (action === "demote" && !state.member.admin) return "contact.not_admin";
  }
  if (action === "block" || action === "unblock") {
    if (state.blocked === null) return "contact.block_unknown";
    if (action === "block" && state.blocked) return "contact.already_blocked";
    if (action === "unblock" && !state.blocked) return "contact.not_blocked";
  }
  return null;
}

export function memberActionReason(action: MemberAction, state: MemberPermissions): string | null {
  const code = memberActionCode(action, state);
  return code ? t(code) : null;
}

export function memberActionError(action: MemberAction, state: MemberPermissions): LocalizedError | null {
  const code = memberActionCode(action, state);
  return code ? new LocalizedError({ kind: "postal_error", code, params: {} }) : null;
}

export function memberFresh(at: number | null | undefined, nowMs: number): boolean {
  if (at == null) return false;
  const age = nowMs - at * 1000;
  return age >= 0 && age < 30_000;
}

export function memberTyping(typing: { state: string; expires_at_ms: number } | null, nowMs: number): string | null {
  if (!typing || !Number.isFinite(typing.expires_at_ms) || typing.expires_at_ms <= nowMs || !["composing", "recording"].includes(typing.state)) return null;
  return t("contact.typing_expires", { state: t(typing.state === "recording" ? "contact.recording" : "contact.typing"), count: Math.ceil((typing.expires_at_ms - nowMs) / 1000) });
}
