import type { CachedMemberGroup, MemberBusinessHours, MemberProfileLocal, MemberProfileLive, Participant } from "./wire";

export type MemberScope = { account: string; group: string; jid: string; requestKey: string | number };
export function memberScopeMatches(scope: MemberScope | null, account: string | null, group: string, jid: string, requestKey: string | number): boolean {
  return scope !== null && scope.account === account && scope.group === group && scope.jid === jid && scope.requestKey === requestKey;
}
export type MemberAction = "promote" | "demote" | "remove" | "block" | "unblock" | "report";
export type MemberLocalView = MemberProfileLocal;
export function memberNoteError(notes: string, warnings: number | undefined): string {
  if (warnings === undefined || !Number.isInteger(warnings) || warnings < 0 || warnings > 100000) return "Warning count must be an integer from 0 to 100000.";
  if (notes.includes("\0")) return "Notes cannot contain NUL characters.";
  if (Array.from(notes).length > 4096) return "Notes cannot exceed 4096 characters.";
  if (new TextEncoder().encode(notes).length > 16384) return "Notes cannot exceed 16384 UTF-8 bytes.";
  return "";
}
export function memberLocalMatches(local: Pick<MemberProfileLocal, "jid" | "addresses" | "scope_chat"> | null, jid: string, group: string): boolean {
  return local !== null && local.scope_chat === group && (local.jid === jid || local.addresses.includes(jid));
}
export type MemberLiveView = MemberProfileLive;
type MemberField = MemberProfileLive["about"] | MemberProfileLive["device_count"] | MemberProfileLive["business"];
export function memberFieldText(field: MemberField | null, format: (value: NonNullable<MemberField["value"]>) => string = String): string {
  if (!field) return "Unavailable";
  if (field.state === "restricted") return "Access denied by server.";
  if (field.state === "error") return `${field.stale && field.value !== null ? `Cached value: ${format(field.value)}. ` : ""}Refresh failed: ${field.error || "Request failed"}`;
  if (field.state !== "available" || field.value === null) return "Not provided";
  return `${field.stale ? "Cached: " : ""}${format(field.value) || "(empty)"}`;
}
export function memberBusinessHours(hours: MemberBusinessHours): string {
  const time = (minutes: number) => `${Math.floor(minutes / 60).toString().padStart(2, "0")}:${(minutes % 60).toString().padStart(2, "0")}`;
  return `${hours.day}: ${hours.mode.replaceAll("_", " ")}${hours.open_minutes !== null ? ` ${time(hours.open_minutes)}` : ""}${hours.close_minutes !== null ? ` to ${time(hours.close_minutes)}` : ""}`;
}
export type MemberPermissions = { admin: boolean; connected: boolean; self: boolean; ready?: boolean; member: Pick<Participant | CachedMemberGroup, "admin" | "owner"> | null;
  blocked: boolean | null; supported: readonly MemberAction[] };

export function memberActionReason(action: MemberAction, state: MemberPermissions): string | null {
  if (state.ready === false) return "Current member data is not loaded.";
  if (!state.admin) return "Only group admins can use member actions.";
  if (!state.connected) return "Connect this account to use member actions.";
  if (state.self) return "This action cannot target your own account.";
  if (!state.supported.includes(action)) return "This action is unavailable in this client.";
  if (action === "promote" || action === "demote" || action === "remove") {
    if (!state.member) return "Membership is not known.";
    if (state.member.admin === null || state.member.owner === null) return "Member role is not known.";
    if (state.member.owner) return "The group owner cannot be changed here.";
    if (action === "promote" && state.member.admin) return "This member is already an admin.";
    if (action === "demote" && !state.member.admin) return "This member is not an admin.";
  }
  if (action === "block" || action === "unblock") {
    if (state.blocked === null) return "Block status is not available.";
    if (action === "block" && state.blocked) return "This member is already blocked.";
    if (action === "unblock" && !state.blocked) return "This member is not blocked.";
  }
  return null;
}

export function memberFresh(at: number | null | undefined, nowMs: number): boolean {
  if (at == null) return false;
  const age = nowMs - at * 1000;
  return age >= 0 && age < 30_000;
}

export function memberTyping(typing: { state: string; expires_at_ms: number } | null, nowMs: number): string | null {
  if (!typing || !Number.isFinite(typing.expires_at_ms) || typing.expires_at_ms <= nowMs || !["composing", "recording"].includes(typing.state)) return null;
  return `${typing.state === "recording" ? "Recording" : "Typing"}, expires in ${Math.ceil((typing.expires_at_ms - nowMs) / 1000)}s`;
}
