// The words a group member change's server answer turns into.
import type { GroupHistoryOffer, GroupHistoryResult, ParticipantChange } from "./models";
import { t } from "../i18n/localizer.ts";
import { LocalizedError, messageText } from "../i18n/errors.ts";

/** Why the server refused one person, or that the add needs approval. `null`
 * when it simply worked. */
export function changeText(change: ParticipantChange): string | null {
  if (change.pending) return t("group_action.pending");
  if (change.ok) return null;
  const code = change.code ?? "";
  if (code === "403") return t("group_action.forbidden");
  if (code === "409") return t("group_action.not_member");
  if (code === "404" || code === "406") return t("group_action.number_unknown");
  if (code === "408") return t("group_action.unreachable");
  return change.error ? t("group_action.refused_code", { code: code || t("group_action.error") }) : t("group_action.refused");
}

export function historyReceivers(jids: string[], requested: boolean, offer: GroupHistoryOffer | null): string[] {
  if (!requested) return [];
  if (!offer?.enabled) throw new LocalizedError({ kind: "postal_error",
    code: offer?.reason_ref?.code ?? "error.group_history_unavailable", params: offer?.reason_ref?.params ?? {},
    ...(offer?.reason ? { diagnostic: offer.reason } : {}) });
  return [...jids];
}

export function historyResultText(result: GroupHistoryResult): string {
  if (result.state === "shared") return t("group_action.history_accepted");
  return result.message_ref ? messageText(result.message_ref) : t("group_action.history_legacy_result");
}
