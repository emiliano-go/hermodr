// The line a system row draws, from its stub kind and the JIDs it names.
// Null draws nothing: E2E_ENCRYPTED is the chat's fixed header instead, and a
// kind without a sentence here is kept but not shown.

export function noticeText(kind: string, params: string[], name: (jid: string) => string): string | null {
  const names = params.map((p) => (p.includes("@") ? name(p) : p));
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
      return params[0] ? `Group “${params[0]}” was created.` : "The group was created.";
    case "GROUP_DELETE":
    case "COMMUNITY_PARENT_GROUP_DELETED":
      return "This group was deleted.";
    case "GROUP_DEACTIVATED":
      return "This group is no longer available.";
    case "GROUP_CHANGE_SUBJECT":
      return params[0] ? `The group name changed to “${params[0]}”.` : "The group name changed.";
    case "GROUP_CHANGE_ICON":
      return "The group icon changed.";
    case "GROUP_CHANGE_DESCRIPTION":
      return "The group description changed.";
    case "GROUP_PARTICIPANT_ADD":
    case "GROUP_PARTICIPANT_LINKED_GROUP_JOIN":
    case "GROUP_PARTICIPANT_JOINED_GROUP_AND_PARENT_GROUP":
      return who ? `${who} joined.` : null;
    case "GROUP_PARTICIPANT_INVITE":
      return who ? `${who} joined using the group's invite link.` : null;
    case "GROUP_PARTICIPANT_ACCEPT":
      return who ? `${who} accepted the invite to join.` : null;
    case "GROUP_PARTICIPANT_ADD_REQUEST_JOIN":
      return who ? `${who} joined after their request was approved.` : null;
    case "GROUP_PARTICIPANT_REMOVE":
      return who ? `${who} ${was} removed.` : null;
    case "GROUP_PARTICIPANT_LEAVE":
      return who ? `${who} left.` : null;
    case "GROUP_PARTICIPANT_PROMOTE":
      return who ? `${who} ${is} now ${names.length > 1 ?"admins" : "an admin"}.` : null;
    case "GROUP_PARTICIPANT_DEMOTE":
      return who ? `${who} ${is} no longer ${names.length > 1 ?"admins" : "an admin"}.` : null;
    case "GROUP_PARTICIPANT_CHANGE_NUMBER": {
      // The stub names the old number and then the new one; the new one is who they are now.
      const now = names.at(-1);
      return now ? `${now} changed ${now === "You" ? "your" : "their"} phone number.` : null;
    }
    case "CALL_MISSED":
      return "Missed call";
    case "CALL_MISSED_VOICE":
    case "CALL_MISSED_GROUP_VOICE":
      return "Missed voice call";
    case "CALL_MISSED_VIDEO":
    case "CALL_MISSED_GROUP_VIDEO":
      return "Missed video call";
    // The caller's number stays out of it: the point of silencing is that they are unknown.
    case "SILENCED_UNKNOWN_CALLER_AUDIO":
    case "SILENCED_UNKNOWN_CALLER":
      return "Silenced voice call from an unknown number";
    case "SILENCED_UNKNOWN_CALLER_VIDEO":
      return "Silenced video call from an unknown number";
    default:
      return null;
  }
}
