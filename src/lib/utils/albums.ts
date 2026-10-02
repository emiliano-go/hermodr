import type { StoredMessage } from "./wire";

export type AlbumGroup = { parentId: string | null; messages: StoredMessage[]; prev: StoredMessage | undefined };

export function groupAlbumRows(messages: readonly StoredMessage[], parentIdOf: (message: StoredMessage) => string | null,
  options: { visible?: (message: StoredMessage) => boolean; breakBefore?: ReadonlySet<string> } = {}): AlbumGroup[] {
  const groups: AlbumGroup[] = [];
  let active: AlbumGroup | undefined;
  let previous: StoredMessage | undefined;
  for (const message of messages) {
    if (options.visible && !options.visible(message)) { active = undefined; continue; }
    const eligible = (message.media_kind === "image" || message.media_kind === "video") && !message.spoiler
      && !message.media_once_kind && !message.reply_to_view_once && !message.revoked && !message.deleted && !message.system_kind;
    const association = eligible ? parentIdOf(message) : null;
    const parentId = association?.trim() && association !== message.id ? association : null;
    const first = active?.messages[0];
    if (parentId && active?.parentId === parentId && first?.chat === message.chat && first.sender === message.sender
      && first.from_me === message.from_me && !options.breakBefore?.has(message.id)) {
      active.messages.push(message);
    } else {
      active = { parentId, messages: [message], prev: previous };
      groups.push(active);
    }
    previous = message;
  }
  return groups;
}
