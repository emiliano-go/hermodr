import { groupAlbumRows } from "./albums.ts";
import type { StoredMessage } from "./wire";

export function albumTimeline(messages: readonly StoredMessage[], firstUnreadId: string | null,
  hidden: (message: StoredMessage) => boolean, dayKey: (timestamp: number) => string) {
  const parents = new Map(messages.map((message) => [message.id, message]));
  const parentIdOf = (message: StoredMessage) => {
    const id = message.album?.parent_id;
    if (!id) return null;
    const parent = parents.get(id);
    if (parent && (parent.chat !== message.chat || parent.sender !== message.sender || parent.from_me !== message.from_me
      || parent.media_kind !== "album" || parent.media_once_kind || parent.spoiler || parent.revoked || parent.deleted
      || parent.system_kind || hidden(parent))) return null;
    return id;
  };
  const firstChild = new Map<string, string>();
  for (const group of groupAlbumRows(messages, parentIdOf, { visible: (message) => !hidden(message) })) {
    if (group.parentId && !firstChild.has(group.parentId)) firstChild.set(group.parentId, group.messages[0].id);
  }
  const suppressed = new Set([...firstChild.keys()].filter((id) => parents.has(id)));
  const drawn = messages.filter((message) => !suppressed.has(message.id));
  const visible = (message: StoredMessage) => !hidden(message);
  const unread = firstUnreadId && firstChild.get(firstUnreadId) || firstUnreadId;
  const breakBefore = new Set<string>();
  let previous: StoredMessage | undefined;
  for (const message of drawn) {
    if (!visible(message)) continue;
    if (message.id === unread || (previous && dayKey(previous.timestamp) !== dayKey(message.timestamp))) breakBefore.add(message.id);
    previous = message;
  }
  const groups = groupAlbumRows(drawn, parentIdOf, { visible, breakBefore }).map((group) => {
    const first = group.messages[0];
    const parent = group.parentId ? parents.get(group.parentId) : undefined;
    return { ...group, parent, timestamp: parent && dayKey(parent.timestamp) === dayKey(first.timestamp) ? parent.timestamp : first.timestamp,
      beforeIds: [] as string[], afterIds: [] as string[],
      unreadId: group.messages.find((message) => message.id === unread)?.id ?? null };
  });
  const indices = new Map<string, number>();
  groups.forEach((group, index) => group.messages.forEach((message) => indices.set(message.id, index)));
  let pending: string[] = [], previousGroup: number | undefined;
  for (const message of messages) {
    if (suppressed.has(message.id)) { pending.push(message.id); continue; }
    const index = indices.get(message.id);
    if (index === undefined) continue;
    if (pending.length) {
      (previousGroup === index ? groups[index].afterIds : groups[index].beforeIds).push(...pending);
      pending = [];
    }
    previousGroup = index;
  }
  if (pending.length && previousGroup !== undefined) groups[previousGroup].afterIds.push(...pending);
  return groups;
}

export function visibleReadFrontier(messages: readonly Pick<StoredMessage, "id">[], visibleIds: Iterable<string>): string | null {
  const visible = new Set(visibleIds);
  let candidate: string | null = null;
  for (const message of messages) if (visible.has(message.id)) candidate = message.id;
  return candidate;
}
