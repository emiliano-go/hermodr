import { t } from "../i18n/localizer.ts";
import { fuzzyScore } from "../utils/quick-switcher.ts";
import type { Space, SpaceInboxFilters, SpaceItem, SpaceTarget } from "../utils/wire.ts";

export type SpaceCandidate = { target: SpaceTarget; title: string; detail?: string };
export const SPACE_KINDS: Record<SpaceTarget["kind"], string> = {
  chat: "spaces.kind_chat", group: "spaces.kind_group", community: "spaces.kind_community", channel: "spaces.kind_channel", contact: "spaces.kind_contact",
  favorite_contact: "spaces.kind_favorite_contact", label: "spaces.kind_label", saved_message: "spaces.kind_saved_message",
  saved_search: "spaces.kind_saved_search", inbox_view: "spaces.kind_inbox_view",
};

export function emptyInboxFilters(): SpaceInboxFilters {
  return { unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" };
}

export function targetKey(target: SpaceTarget): string {
  if ("jid" in target) return JSON.stringify([target.kind, target.jid]);
  switch (target.kind) {
    case "label": return JSON.stringify([target.kind, target.label_id]);
    case "saved_message": return JSON.stringify([target.kind, target.chat, target.message_id]);
    case "saved_search": return JSON.stringify([target.kind, target.chat, target.query]);
    case "inbox_view": {
      const f = target.filters;
      return JSON.stringify([target.kind, f.unread, f.mentions, f.labelled, f.muted, f.archived, f.label, f.query]);
    }
  }
}

export function targetTitle(target: SpaceTarget): string {
  if ("jid" in target) return target.jid;
  switch (target.kind) {
    case "label": return t("labels.number", { id: target.label_id });
    case "saved_message": return `${target.chat} · ${target.message_id}`;
    case "saved_search": return target.query;
    case "inbox_view": {
      const f = target.filters;
      const parts = [f.unread && t("chat.unread"), f.mentions && t("chat.mentions"), f.labelled && t("chat.labelled"), f.muted && t("chat.muted"), f.archived && t("chat.archived"),
        f.label && t("labels.number", { id: f.label }), f.query].filter(Boolean);
      return parts.length ? parts.join(" · ") : t("spaces.inbox");
    }
  }
}

export function ordered<T extends { id: string; order: number }>(rows: readonly T[]): T[] {
  return [...rows].sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
}

export function movedIds(rows: readonly { id: string }[], id: string, direction: -1 | 1): string[] | null {
  const ids = rows.map((row) => row.id), index = ids.indexOf(id), next = index + direction;
  if (index < 0 || next < 0 || next >= ids.length) return null;
  [ids[index], ids[next]] = [ids[next], ids[index]];
  return ids;
}

export function spaceChildren(spaces: readonly Space[]) {
  const children = new Map<string | null, Space[]>();
  for (const space of ordered(spaces)) {
    const rows = children.get(space.parent_id) ?? [];
    rows.push(space);
    children.set(space.parent_id, rows);
  }
  return children;
}

export function descendants(spaces: readonly Space[], id: string): Set<string> {
  const children = spaceChildren(spaces), found = new Set<string>([id]), pending = [id];
  while (pending.length) for (const child of children.get(pending.pop()!) ?? []) {
    if (!found.has(child.id)) { found.add(child.id); pending.push(child.id); }
  }
  return found;
}

export function spaceTree(spaces: readonly Space[], collapsed: readonly string[] = []): { space: Space; depth: number }[] {
  const children = spaceChildren(spaces), hidden = new Set(collapsed), rows: { space: Space; depth: number }[] = [];
  function visit(parent: string | null, depth: number) {
    for (const space of children.get(parent) ?? []) {
      rows.push({ space, depth });
      if (!hidden.has(space.id)) visit(space.id, depth + 1);
    }
  }
  visit(null, 0);
  return rows;
}

export function directItems(items: readonly SpaceItem[], spaceId: string): SpaceItem[] {
  return ordered(items.filter((item) => item.space_id === spaceId));
}

export function matchingCandidates(catalog: readonly SpaceCandidate[], kind: SpaceTarget["kind"], query: string): SpaceCandidate[] {
  const seen = new Set<string>();
  return catalog.filter((row) => {
    const key = targetKey(row.target);
    if (row.target.kind !== kind || seen.has(key) || fuzzyScore(query.trim(), `${row.title} ${row.detail ?? ""} ${targetTitle(row.target)}`) === null) return false;
    seen.add(key);
    return true;
  });
}
