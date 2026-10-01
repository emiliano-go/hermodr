import { invoke } from "$lib/utils/ipc";
import { bare, captionOf, isUnavailable } from "$lib/utils/message";
import { compareMessages } from "$lib/utils/message-window";
import type { ChatEvent, StoredMessage } from "$lib/utils/models";
import type { MenuItem } from "$lib/messages/MessageMenu.svelte";
import { chats } from "./chats.svelte";
import { composer } from "./composer.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";
import { session } from "./session.svelte";

export function target(m: StoredMessage) {
  return { chat: m.chat, id: m.id, sender: m.sender, fromMe: m.from_me };
}

/** Runs a message action, surfacing a failure instead of dropping it. */
export async function act(run: () => Promise<unknown>) {
  try {
    await run();
  } catch (e) {
    ui.fail(e);
  }
}

/** One slot in the message menu. Multi-item builders (dm-pair, media) stay glued
 * under a single id so reshuffling the order can never split a pair apart. */
export type MenuId =
  | "reactions"
  | "reply"
  | "edit"
  | "info"
  | "remove-member"
  | "dm-pair"
  | "copy"
  | "media"
  | "forward"
  | "pin"
  | "star"
  | "labels"
  | "report"
  | "delete"
  | "select";

export type MenuCtx = {
  m: StoredMessage;
  openChat: (chat: string) => Promise<void>;
  other: boolean;
  text: string;
  image: boolean;
  senderIsOwner: boolean;
};

type MenuBuilder = (ctx: MenuCtx) => MenuItem | MenuItem[] | null;

const MENU_BUILDERS: Record<MenuId, MenuBuilder> = {
  labels: ({ m }) => !m.revoked && !m.deleted && !m.spoiler && !m.system_kind && !m.media_once_kind
    && m.media_kind !== "view_once" && m.media_kind !== "unknown" && !isUnavailable(m)
    ? { label: "Labels", icon: "edit", action: () => { ui.labelTargets = [{ chat: m.chat, id: m.id }]; } } : null,
  // Under the quick-reaction row, and only once somebody has reacted: an entry
  // leading to an empty list is a dead end.
  reactions: ({ m }) =>
    messages.reactionsFor.get(m.id)?.length
      ? { label: "Reactions", icon: "smile", action: () => (ui.reactionsFor = m) }
      : null,
  reply: ({ m }) => ({
    label: "Reply",
    icon: "reply",
    action: () => {
      composer.editing = null;
      composer.replyingTo = m;
      composer.inputEl?.focus();
    },
  }),
  edit: ({ m }) =>
    m.from_me && !m.revoked && !m.deleted && !m.media_kind && m.text.trim()
      ? { label: "Edit", icon: "edit", action: () => composer.startEditing(m) }
      : null,
  info: ({ m }) =>
    m.from_me
      ? { label: "Message info", icon: "check", action: () => (ui.infoFor = m) }
      : null,
  // Admins can remove a member straight from their message; the owner can
  // never be removed, and we cannot remove ourselves this way.
  "remove-member": ({ m, other, senderIsOwner }) =>
    other && members.isAdmin() && !senderIsOwner
      ? {
          label: `Remove ${members.senderLabel(m)} from group`,
          icon: "trash",
          danger: true,
          action: () =>
            (ui.removeMember = { chat: m.chat, jid: bare(m.sender), name: members.senderLabel(m) }),
        }
      : null,
  "dm-pair": ({ m, other, openChat }) =>
    other
      ? [
          {
            label: "Reply privately",
            icon: "users",
            action: async () => {
              await openChat(bare(m.sender));
              composer.editing = null;
              composer.replyingTo = m;
              composer.inputEl?.focus();
            },
          },
          {
            label: `Message ${members.senderLabel(m)}`,
            icon: "message",
            action: () => openChat(bare(m.sender)),
          },
        ]
      : null,
  copy: ({ m, text }) =>
    text && !m.revoked
      ? {
          label: "Copy",
          icon: "copy",
          action: () => act(() => navigator.clipboard.writeText(text)),
        }
      : null,
  media: ({ m, image }) => {
    if (m.revoked) return null;
    if (!["image", "sticker", "video", "gif", "audio", "document"].includes(m.media_kind ?? "")) {
      return null;
    }
    const mediaAction = (action: string) =>
      act(() => invoke("message_media_action", { chat: m.chat, id: m.id, action }));
    const items: MenuItem[] = [];
    if (image) items.push({ label: "Copy Image", icon: "copy", action: () => mediaAction("copy_image") });
    items.push(
      {
        label: image ? "Save Image…" : "Save Attachment…",
        icon: "download",
        action: () => mediaAction("save"),
      },
      {
        label: image ? "Open Image" : "Open Attachment",
        icon: "external",
        action: () => mediaAction("open"),
      },
    );
    return items;
  },
  forward: ({ m }) =>
    !m.revoked
      ? { label: "Forward", icon: "forward", action: () => (ui.forwarding = [m]) }
      : null,
  pin: ({ m }) =>
    !m.revoked
      ? {
          label: messages.marks.pinned === m.id ? "Unpin" : "Pin",
          icon: "pin",
          action: () =>
            act(() =>
              invoke("pin_message", { target: target(m), pinned: messages.marks.pinned !== m.id }),
            ),
        }
      : null,
  star: ({ m }) =>
    !m.revoked
      ? {
          label: messages.starred.has(m.id) ? "Unstar" : "Star",
          icon: "star",
          action: () =>
            starMessages([m], !messages.starred.has(m.id)),
        }
      : null,
  report: ({ m, other }) =>
    other
      ? {
          label: "Report to admins",
          icon: "flag",
          action: () => (ui.reporting = m),
        }
      : null,
  delete: ({ m }) => ({
    label: "Delete",
    icon: "trash",
    danger: true,
    action: () => (ui.deleting = m),
  }),
  select: ({ m }) =>
    !m.revoked
      ? {
          label: "Select messages",
          icon: "check",
          action: () => (ui.picking = { [m.id]: m }),
        }
      : null,
};

/**
 * The message-menu order. To reorder the menu, reshuffle this array only —
 * {@link menuItems} builds each entry via {@link MENU_BUILDERS} and renders
 * them in this sequence.
 */
export const MESSAGE_MENU_ORDER: MenuId[] = [
  "info",
  "reply",
  "dm-pair",
  "copy",
  "select",
  "reactions",
  "forward",
  "edit",
  "pin",
  "star",
  "labels",
  "media",
  "report",
  "delete",
  "remove-member",
];

/**
 * The message-menu dividers. Each entry draws a divider above the named item —
 * to move a divider, move its line here. A divider renders only when its item
 * is visible (and never above the first item), so hiding conditional entries
 * can never strand one.
 */
export type DividerRule = MenuId | { id: MenuId; when: (ctx: MenuCtx) => boolean };
export const DIVIDER_BEFORE: DividerRule[] = [
  "remove-member",
  "report",
  "media",
  { id: "delete", when: ({ other }) => !other },
];

export function menuItems(m: StoredMessage, openChat: (chat: string) => Promise<void>): MenuItem[] {
  if (isUnavailable(m)) return [];
  const other = m.chat.endsWith("@g.us") && !m.from_me;
  const senderParticipant = m.sender ? members.memberOf(m.sender) : undefined;
  const ctx: MenuCtx = {
    m,
    openChat,
    other,
    text: m.media_kind ? captionOf(m) : m.text,
    image: m.media_kind === "image" || m.media_kind === "sticker",
    senderIsOwner: !!senderParticipant?.owner,
  };
  const divided = new Set<MenuId>();
  for (const rule of DIVIDER_BEFORE) {
    if (typeof rule === "string") divided.add(rule);
    else if (rule.when(ctx)) divided.add(rule.id);
  }
  const items: MenuItem[] = [];
  for (const id of MESSAGE_MENU_ORDER) {
    const built = MENU_BUILDERS[id](ctx);
    if (!built) continue;
    const list = Array.isArray(built) ? built : [built];
    list.forEach((item, i) => {
      if (i === 0 && divided.has(id) && items.length > 0) item.separated = true;
      items.push(item);
    });
  }
  return items;
}

/** Whether every picked message may be deleted for everyone. */
export function canDeletePickedForEveryone() {
  const ids = ui.bulkDelete ?? Object.keys(ui.picking ?? {});
  return (
    ids.length > 0 &&
    ids.every((id) => {
      const m = messages.messages.find((row) => row.id === id) ?? ui.picking?.[id];
      return !!m && canDeleteForEveryone(m);
    })
  );
}

/** Deletes the picked messages, for everyone or on this device only. */
export async function deleteSelected(everyone: boolean) {
  const ids = ui.bulkDelete ?? Object.keys(ui.picking ?? {});
  ui.bulkDelete = null;
  const chat = chats.selectedChat;
  if (!chat || ids.length === 0) return;
  const selection = ui.picking;
  const account = session.activeAccount;
  await act(async () => {
    await invoke("delete_messages", { chat, ids, everyone });
    if (session.activeAccount !== account) return;
    if (ui.picking === selection) ui.picking = null;
    if (chats.selectedChat === chat) await messages.reloadMessages(chat);
    await chats.refreshChats();
  });
}

/**
 * The media the viewer can page through: downloaded images, videos and GIFs,
 * including greyed-out ones whose local copy survived a delete. Messages
 * still marked one-time are excluded; their copy is shown behind the
 * one-time filter instead.
 */
export function viewableMessages(
  ordered: StoredMessage[],
  viewOnce: Set<string>,
): StoredMessage[] {
  return ordered.filter(
    (m) =>
      !isUnavailable(m) &&
      !!m.media_path &&
      !viewOnce.has(m.id) &&
      (m.media_kind === "image" || m.media_kind === "video" || m.media_kind === "gif"),
  );
}

/** The picked messages in the chat's own order, oldest first. */
export function pickedInOrder(
  picking: Record<string, StoredMessage> | null,
  ordered: StoredMessage[],
): StoredMessage[] {
  if (!picking) return [];
  const current = new Map(ordered.map((message) => [message.id, message]));
  return Object.values(picking).map((message) => current.get(message.id) ?? message).filter((message) => !isUnavailable(message)).sort(compareMessages);
}

export async function copyMessages(batch: StoredMessage[]) {
  const text = batch.filter((message) => !message.revoked && !isUnavailable(message)).map((message) => message.text).filter(Boolean).join("\n");
  if (text) await act(() => navigator.clipboard.writeText(text));
}

export async function starMessages(batch: StoredMessage[], starred: boolean) {
  await act(() => composer.enqueue(async (signal) => {
    for (const message of batch) {
      signal.throwIfAborted();
      if (!message.revoked && !isUnavailable(message)) await invoke("star", { target: target(message), starred });
    }
  }));
}

export async function reactMessages(batch: StoredMessage[], emoji: string) {
  await act(() => composer.enqueue(async (signal) => {
    for (const message of batch) {
      signal.throwIfAborted();
      if (!message.revoked && !isUnavailable(message)) await invoke("react", { target: target(message), emoji });
    }
  }));
}

/** Forwards one chat's messages to every chosen chat, in the batch's order. */
export async function forwardMessages(batch: StoredMessage[], targets: string[]) {
  batch = batch.filter((message) => !isUnavailable(message));
  if (batch.length === 0) return;
  const selection = ui.picking;
  await composer.enqueue(async (signal) => {
    for (const to of targets) {
      for (const m of batch) {
        signal.throwIfAborted();
        await invoke("forward_message", { chat: m.chat, id: m.id, to });
      }
    }
  });
  if (ui.picking === selection) ui.picking = null;
  await chats.refreshChats();
}

/** Whether we may delete this message for everyone: ours, or ours to moderate. */
export function canDeleteForEveryone(m: StoredMessage) {
  if (m.revoked || isUnavailable(m)) return false;
  if (m.from_me) return true;
  return members.isAdmin();
}

export async function deleteMessage(everyone: boolean) {
  const m = ui.deleting;
  ui.deleting = null;
  if (!m) return;
  await act(async () => {
    await invoke("delete_message", { target: target(m), everyone });
    await messages.reloadMessages(chats.selectedChat);
    await chats.refreshChats();
  });
}

export function eventFields(event: ChatEvent) {
  const { name, description, start, end, location, link } = event;
  return { name, description, start, end, location, link };
}

export async function saveEvent(chat: string, id: string, fields: object) {
  await composer.enqueue(() => invoke("edit_event", { chat, id, event: fields }));
  await messages.reloadMessages(chats.selectedChat);
  await messages.loadMarks(chats.selectedChat);
}
