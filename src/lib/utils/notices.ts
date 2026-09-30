// The line a system row draws, from its stub kind and the JIDs it names.
// Null draws nothing: E2E_ENCRYPTED is the chat's fixed header instead, and a
// kind without a sentence here is kept but not shown.

export function noticeText(kind: string, params: string[], name: (jid: string) => string, actor = ""): string | null {
  if (kind === "NEW_CHAT_MESSAGE_CAPPED") {
    const total = /^\d+$/.test(params[0] ?? "") ? params[0] : null;
    const used = /^\d+$/.test(params[1] ?? "") ? params[1] : null;
    const reset = Number(params[2]);
    const date = new Date(reset * 1000);
    const quota = total !== null ? `Quota this cycle: ${total} new chats${used !== null ? `, ${used} used` : ""}.`
      : "WhatsApp did not provide this cycle's quota.";
    const when = Number.isSafeInteger(reset) && reset > 0 && !Number.isNaN(date.getTime())
      ? `Resets at ${date.toLocaleString()}.` : "WhatsApp did not provide a reset time.";
    return `This account has reached WhatsApp's limit for starting new one-to-one chats. ${quota} ${when}`;
  }
  if (kind === "SILENCED_UNKNOWN_CALLER_AUDIO" || kind === "SILENCED_UNKNOWN_CALLER") return "Silenced voice call from an unknown number";
  if (kind === "SILENCED_UNKNOWN_CALLER_VIDEO") return "Silenced video call from an unknown number";
  const names = params.map((p) => p.includes("@") ? name(p) : kind.endsWith("CHANGE_NUMBER") && /^\d+$/.test(p) ? name(`${p}@s.whatsapp.net`) : p);
  const author = actor ? name(actor) : null;
  const who = names.join(", ");
  // "You" and several people take the plural verb.
  const plural = names.length > 1 || names[0] === "You";
  const was = plural ? "were" : "was";
  const is = plural ? "are" : "is";
  switch (kind) {
    case "E2E_IDENTITY_CHANGED":
      return `${who || "This contact"}'s security code changed.`;
    case "DEVICE_ADDED":
      return `${who || "This contact"} linked a new device.`;
    case "DEVICE_REMOVED":
      return `${who || "This contact"} removed a device.`;
    case "GROUP_CREATE":
      return author ? `${author} created ${params[0] ? `group "${params[0]}"` : "the group"}.`
        : params[0] ? `Group "${params[0]}" was created.` : "The group was created.";
    case "GROUP_DELETE":
    case "COMMUNITY_PARENT_GROUP_DELETED":
      return "This group was deleted.";
    case "GROUP_DEACTIVATED":
      return "This group is no longer available.";
    case "GROUP_CHANGE_SUBJECT":
      return author ? `${author} changed the group name${params[0] ? ` to "${params[0]}"` : ""}.`
        : params[0] ? `The group name changed to "${params[0]}".` : "The group name changed.";
    case "GROUP_CHANGE_ICON":
      return author ? `${author} changed the group icon.` : "The group icon changed.";
    case "GROUP_CHANGE_DESCRIPTION":
      return author ? `${author} changed the group description.` : "The group description changed.";
    case "GROUP_CHANGE_INVITE_LINK":
      return author ? `${author} reset the group invite link.` : "The group invite link changed.";
    case "GROUP_CHANGE_RESTRICT":
      return `${author ? `${author} changed who can edit group info. ` : ""}${params[0] === "on" || params[0] === "true" ? "Only admins can edit group info." : params[0] === "off" || params[0] === "false" ? "All members can edit group info." : "Group info permissions changed."}`;
    case "GROUP_CHANGE_ANNOUNCE":
      return `${author ? `${author} changed who can send messages. ` : ""}${params[0] === "on" || params[0] === "true" ? "Only admins can send messages." : params[0] === "off" || params[0] === "false" ? "All members can send messages." : "Group message permissions changed."}`;
    case "GROUP_PARTICIPANT_ADD":
    case "GROUP_PARTICIPANT_LINKED_GROUP_JOIN":
    case "GROUP_PARTICIPANT_JOINED_GROUP_AND_PARENT_GROUP":
      return who ? author ? `${author} added ${who}.` : `${who} joined.` : null;
    case "GROUP_PARTICIPANT_INVITE":
      return who ? `${who} joined using the group's invite link.` : null;
    case "GROUP_PARTICIPANT_ACCEPT":
      return who ? `${who} accepted the invite to join.` : null;
    case "GROUP_PARTICIPANT_ADD_REQUEST_JOIN":
      return who ? `${who} joined after their request was approved.` : null;
    case "GROUP_PARTICIPANT_REMOVE":
      return who ? author ? `${author} removed ${who}.` : `${who} ${was} removed.` : null;
    case "GROUP_PARTICIPANT_LEAVE":
      return who ? `${who} left.` : null;
    case "GROUP_PARTICIPANT_PROMOTE":
    case "COMMUNITY_PARTICIPANT_PROMOTE":
      if (author && who) return `${author} made ${who} ${names.length > 1 ? "admins" : "an admin"}.`;
      return who ? `${who} ${is} now ${names.length > 1 ?"admins" : "an admin"}.` : null;
    case "GROUP_PARTICIPANT_DEMOTE":
    case "COMMUNITY_PARTICIPANT_DEMOTE":
      if (author && who) return `${author} removed ${who}'s admin role.`;
      return who ? `${who} ${is} no longer ${names.length > 1 ?"admins" : "an admin"}.` : null;
    case "GROUP_PARTICIPANT_CHANGE_NUMBER":
    case "INDIVIDUAL_CHANGE_NUMBER":
      return names[0] ? `${names[0]} changed ${names[0] === "You" ? "your" : "their"} phone number${names[1] ? ` to ${names[1]}` : ""}.` : "This contact changed their phone number.";
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST":
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST_NON_ADMIN_ADD":
      return `${who || author || "Someone"} requested to join the group.`;
    case "GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE":
      return `${author ? `${author} changed join approval. ` : ""}${params[0] === "on" || params[0] === "true" ? "Admins must approve new members." : params[0] === "off" || params[0] === "false" ? "New members can join without approval." : "Join approval settings changed."}`;
    case "GROUP_MEMBER_ADD_MODE":
      return params[0] === "admin_add" ? "Only admins can add members." : params[0] === "all_member_add" ? "All members can add members." : "Who can add group members changed.";
    case "GROUP_MEMBER_LINK_MODE":
      return `${author ? `${author} changed invite-link sharing. ` : ""}${params[0]?.toLowerCase() === "admin_link" ? "Only admins can share the group invite link." : params[0]?.toLowerCase() === "all_member_link" ? "All members can share the group invite link." : "Who can share the group invite link changed."}`;
    case "GROUP_MEMBER_SHARE_GROUP_HISTORY_MODE":
      return `${author ? `${author} changed history-sharing permissions. ` : ""}${params[0]?.toLowerCase() === "admin_share" ? "Only admins can share history with new members." : params[0]?.toLowerCase() === "all_member_share" ? "All members can share history with new members." : "Who can share history with new members changed."}`;
    case "GROUP_CHANGE_RECENT_HISTORY_SHARING":
      return `${author ? `${author} changed recent-history sharing. ` : ""}${params[0] === "on" || params[0] === "true" ? "Recent history sharing is enabled." : params[0] === "off" || params[0] === "false" ? "Recent history sharing is disabled." : "Recent history sharing settings changed."}`;
    case "CHANGE_EPHEMERAL_SETTING":
    case "DISAPPEARING_MODE": {
      const seconds = Number(params[0]);
      if (!params[0] || !Number.isFinite(seconds) || seconds < 0) return "Disappearing message settings changed.";
      if (seconds === 0) return "Disappearing messages were turned off.";
      const [divisor, unit] = seconds % 86400 === 0 ? [86400, "day"] : seconds % 3600 === 0 ? [3600, "hour"] : seconds % 60 === 0 ? [60, "minute"] : [1, "second"];
      const count = seconds / Number(divisor);
      return `${author ? `${author} set disappearing messages` : "Disappearing messages were set"} to ${count} ${unit}${count === 1 ? "" : "s"}.`;
    }
    case "EPHEMERAL_SETTING_NOT_APPLIED":
      return "The disappearing message setting could not be applied.";
    case "EPHEMERAL_KEEP_IN_CHAT":
      return "A disappearing message was kept in the chat.";
    case "COMMUNITY_CREATE":
      return author ? `${author} created the community.` : "The community was created.";
    case "COMMUNITY_LINK_PARENT_GROUP":
    case "COMMUNITY_LINK_PARENT_GROUP_RICH":
    case "COMMUNITY_LINK_PARENT_GROUP_MEMBERSHIP_APPROVAL":
    case "COMMUNITY_LINK_SIBLING_GROUP":
    case "COMMUNITY_LINK_SUB_GROUP":
      return author ? `${author} linked ${who || "a group"} to the community.` : who ? `${who} was linked to the community.` : "A group was linked to the community.";
    case "COMMUNITY_UNLINK_PARENT_GROUP":
    case "COMMUNITY_UNLINK_SIBLING_GROUP":
    case "COMMUNITY_UNLINK_SUB_GROUP":
    case "INTEGRITY_UNLINK_PARENT_GROUP":
      return author ? `${author} unlinked ${who || "a group"} from the community.` : who ? `${who} was unlinked from the community.` : "A group was unlinked from the community.";
    case "COMMUNITY_CHANGE_DESCRIPTION":
      return author ? `${author} changed the community description.` : "The community description changed.";
    case "COMMUNITY_OWNER_CHANGED":
      return names.length === 2 ? `Community ownership changed from ${names[0]} to ${names[1]}.` : "Community ownership changed.";
    case "CALL_MISSED":
      return "Missed call";
    case "CALL_MISSED_VOICE":
    case "CALL_MISSED_GROUP_VOICE":
      return "Missed voice call";
    case "CALL_MISSED_VIDEO":
    case "CALL_MISSED_GROUP_VIDEO":
      return "Missed video call";
    default:
      return null;
  }
}
