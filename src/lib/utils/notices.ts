import { formatDate, t } from "../i18n/localizer.ts";

export function unavailableLabel(): string { return t("message.unavailable"); }
export function unavailableExplanation(): string { return t("message.unavailable_explanation"); }

function permission(value: string | undefined, author: string | null, changed: string, on: string, off: string, unknown: string, enabled = "on", disabled = "off"): string {
  const selected = value === enabled || enabled === "on" && value === "true" ? on
    : value === disabled || disabled === "off" && value === "false" ? off : unknown;
  return (author ? t(changed, { author }) : "") + t(selected);
}

export function noticeText(kind: string, params: string[], name: (jid: string) => string, actor = "", isSelf: (jid: string) => boolean = (jid) => jid === "@me"): string | null {
  if (kind === "UNAVAILABLE_MESSAGE") return unavailableLabel() + ". " + unavailableExplanation();
  if (kind === "NEW_CHAT_MESSAGE_CAPPED") {
    const numeric = (value: string | undefined) => value && /^\d+$/.test(value) ? Number.isSafeInteger(Number(value)) ? Number(value) : value : null;
    const total = numeric(params[0]), used = numeric(params[1]), reset = Number(params[2]);
    const quota = total !== null ? t(used !== null ? "notice.quota_used" : "notice.quota", { total, used }) : t("notice.quota_unknown");
    const date = new Date(reset * 1000);
    const when = Number.isSafeInteger(reset) && reset > 0 && !Number.isNaN(date.getTime())
      ? t("notice.quota_reset", { when: formatDate(reset, { dateStyle: "medium", timeStyle: "medium" }) }) : t("notice.quota_reset_unknown");
    return t("notice.quota_limit", { quota, reset: when });
  }
  if (kind === "SILENCED_UNKNOWN_CALLER_AUDIO" || kind === "SILENCED_UNKNOWN_CALLER") return t("notice.silenced_voice");
  if (kind === "SILENCED_UNKNOWN_CALLER_VIDEO") return t("notice.silenced_video");
  const subjects = params.map((value) => kind.endsWith("CHANGE_NUMBER") && /^\d+$/.test(value) ? value + "@s.whatsapp.net" : value);
  const names = subjects.map((value) => value.includes("@") ? name(value) : value);
  const author = actor ? name(actor) : null, who = names.join(t("notice.names_separator"));
  const self = subjects.some(isSelf), plural = names.length > 1 || self;
  switch (kind) {
    case "E2E_IDENTITY_CHANGED": return t("notice.security_changed", { who: who || t("notice.contact") });
    case "DEVICE_ADDED": return t("notice.device_added", { who: who || t("notice.contact") });
    case "DEVICE_REMOVED": return t("notice.device_removed", { who: who || t("notice.contact") });
    case "GROUP_CREATE":
      return t(author ? params[0] ? "notice.group_created_by_named" : "notice.group_created_by" : params[0] ? "notice.group_created_named" : "notice.group_created", { author, title: params[0] ?? "" });
    case "GROUP_DELETE":
    case "COMMUNITY_PARENT_GROUP_DELETED": return t("notice.group_deleted");
    case "GROUP_DEACTIVATED": return t("notice.group_unavailable");
    case "GROUP_CHANGE_SUBJECT":
      return t(author ? params[0] ? "notice.subject_changed_by_named" : "notice.subject_changed_by" : params[0] ? "notice.subject_changed_named" : "notice.subject_changed", { author, title: params[0] ?? "" });
    case "GROUP_CHANGE_ICON": return t(author ? "notice.icon_changed_by" : "notice.icon_changed", { author });
    case "GROUP_CHANGE_DESCRIPTION": return t(author ? "notice.description_changed_by" : "notice.description_changed", { author });
    case "GROUP_CHANGE_INVITE_LINK": return t(author ? "notice.invite_reset_by" : "notice.invite_changed", { author });
    case "GROUP_CHANGE_RESTRICT":
      return permission(params[0], author, "notice.info_changed_by", "notice.info_admins", "notice.info_all", "notice.info_unknown");
    case "GROUP_CHANGE_ANNOUNCE":
      return permission(params[0], author, "notice.send_changed_by", "notice.send_admins", "notice.send_all", "notice.send_unknown");
    case "GROUP_PARTICIPANT_ADD":
    case "GROUP_PARTICIPANT_LINKED_GROUP_JOIN":
    case "GROUP_PARTICIPANT_JOINED_GROUP_AND_PARENT_GROUP":
      return who ? t(author ? "notice.added_by" : "notice.joined", { author, who }) : null;
    case "GROUP_PARTICIPANT_INVITE": return who ? t("notice.joined_link", { who }) : null;
    case "GROUP_PARTICIPANT_ACCEPT": return who ? t("notice.accepted_invite", { who }) : null;
    case "GROUP_PARTICIPANT_ADD_REQUEST_JOIN": return who ? t("notice.joined_approved", { who }) : null;
    case "GROUP_PARTICIPANT_REMOVE":
      return who ? t(author ? "notice.removed_by" : plural ? "notice.removed_many" : "notice.removed_one", { author, who }) : null;
    case "GROUP_PARTICIPANT_LEAVE": return who ? t("notice.left", { who }) : null;
    case "GROUP_PARTICIPANT_PROMOTE":
    case "COMMUNITY_PARTICIPANT_PROMOTE":
      return who ? t(author ? names.length > 1 ? "notice.promoted_by_many" : "notice.promoted_by_one" : names.length > 1 ? "notice.promoted_many" : self ? "notice.promoted_self" : "notice.promoted_one", { author, who }) : null;
    case "GROUP_PARTICIPANT_DEMOTE":
    case "COMMUNITY_PARTICIPANT_DEMOTE":
      return who ? t(author ? "notice.demoted_by" : names.length > 1 ? "notice.demoted_many" : self ? "notice.demoted_self" : "notice.demoted_one", { author, who }) : null;
    case "GROUP_PARTICIPANT_CHANGE_NUMBER":
    case "INDIVIDUAL_CHANGE_NUMBER":
      return names[0] ? t(isSelf(subjects[0]) ? names[1] ? "notice.number_changed_self_to" : "notice.number_changed_self" : names[1] ? "notice.number_changed_to" : "notice.number_changed", { who: names[0], number: names[1] ?? "" }) : t("notice.contact_number_changed");
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST":
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST_NON_ADMIN_ADD":
      return t("notice.join_requested", { who: who || author || t("notice.someone") });
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE":
      return permission(params[0], author, "notice.approval_changed_by", "notice.approval_on", "notice.approval_off", "notice.approval_unknown");
    case "GROUP_MEMBER_ADD_MODE":
      return t(params[0] === "admin_add" ? "notice.add_admins" : params[0] === "all_member_add" ? "notice.add_all" : "notice.add_unknown");
    case "GROUP_MEMBER_LINK_MODE":
      return permission(params[0]?.toLowerCase(), author, "notice.link_changed_by", "notice.link_admins", "notice.link_all", "notice.link_unknown", "admin_link", "all_member_link");
    case "GROUP_MEMBER_SHARE_GROUP_HISTORY_MODE":
      return permission(params[0]?.toLowerCase(), author, "notice.history_changed_by", "notice.history_admins", "notice.history_all", "notice.history_unknown", "admin_share", "all_member_share");
    case "GROUP_CHANGE_RECENT_HISTORY_SHARING":
      return permission(params[0], author, "notice.recent_history_changed_by", "notice.recent_history_on", "notice.recent_history_off", "notice.recent_history_unknown");
    case "CHANGE_EPHEMERAL_SETTING":
    case "DISAPPEARING_MODE": {
      const seconds = Number(params[0]);
      if (!params[0] || !Number.isFinite(seconds) || seconds < 0) return t("notice.disappearing_changed");
      if (seconds === 0) return t("notice.disappearing_off");
      const [divisor, unit] = seconds % 86400 === 0 ? [86400, "day"] : seconds % 3600 === 0 ? [3600, "hour"] : seconds % 60 === 0 ? [60, "minute"] : [1, "second"];
      return t(author ? "notice.disappearing_by" : "notice.disappearing", { author, duration: t("notice.duration_" + unit, { count: seconds / Number(divisor) }) });
    }
    case "EPHEMERAL_SETTING_NOT_APPLIED": return t("notice.disappearing_failed");
    case "EPHEMERAL_KEEP_IN_CHAT": return t("notice.disappearing_kept");
    case "COMMUNITY_CREATE": return t(author ? "notice.community_created_by" : "notice.community_created", { author });
    case "COMMUNITY_LINK_PARENT_GROUP":
    case "COMMUNITY_LINK_PARENT_GROUP_RICH":
    case "COMMUNITY_LINK_PARENT_GROUP_MEMBERSHIP_APPROVAL":
    case "COMMUNITY_LINK_SIBLING_GROUP":
    case "COMMUNITY_LINK_SUB_GROUP":
      return t(author ? "notice.community_linked_by" : who ? "notice.community_linked" : "notice.community_linked_generic", { author, who: who || t("notice.group_generic") });
    case "COMMUNITY_UNLINK_PARENT_GROUP":
    case "COMMUNITY_UNLINK_SIBLING_GROUP":
    case "COMMUNITY_UNLINK_SUB_GROUP":
    case "INTEGRITY_UNLINK_PARENT_GROUP":
      return t(author ? "notice.community_unlinked_by" : who ? "notice.community_unlinked" : "notice.community_unlinked_generic", { author, who: who || t("notice.group_generic") });
    case "COMMUNITY_CHANGE_DESCRIPTION": return t(author ? "notice.community_description_by" : "notice.community_description", { author });
    case "COMMUNITY_OWNER_CHANGED":
      return t(names.length === 2 ? "notice.community_owner_from_to" : "notice.community_owner", { previous: names[0] ?? "", next: names[1] ?? "" });
    case "CALL_MISSED": return t("notice.missed_call");
    case "CALL_MISSED_VOICE":
    case "CALL_MISSED_GROUP_VOICE": return t("notice.missed_voice");
    case "CALL_MISSED_VIDEO":
    case "CALL_MISSED_GROUP_VIDEO": return t("notice.missed_video");
    default: return null;
  }
}
