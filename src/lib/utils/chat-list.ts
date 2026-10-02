import type { ChatSummary } from "./models";

/** Whether every field the list draws reads the same. */
function sameSummary(a: ChatSummary, b: ChatSummary) {
  return (
    a.chat === b.chat &&
    a.display_name === b.display_name &&
    a.last_message_at === b.last_message_at &&
    a.last_text === b.last_text &&
    a.last_from_me === b.last_from_me &&
    a.last_sender_name === b.last_sender_name &&
    a.last_sender === b.last_sender &&
    a.last_media_kind === b.last_media_kind &&
    a.message_count === b.message_count &&
    a.unread_count === b.unread_count &&
    a.mention_count === b.mention_count &&
    a.pinned === b.pinned &&
    a.archived === b.archived &&
    a.muted_until === b.muted_until &&
    a.mute_at_all === b.mute_at_all &&
    a.marked_unread === b.marked_unread
  );
}

/**
 * Reuses the previous summary object whenever nothing about it changed, so a
 * list refresh only re-renders the chats that actually moved or changed.
 */
export function mergeSummaries(previous: ChatSummary[], next: ChatSummary[]): ChatSummary[] {
  const known = new Map(previous.map((c) => [c.chat, c]));
  return next.map((summary) => {
    const old = known.get(summary.chat);
    return old && sameSummary(old, summary) ? old : summary;
  });
}
