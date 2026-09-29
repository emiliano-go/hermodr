import { invoke } from "$lib/utils/ipc";
import { bare, captionOf } from "$lib/utils/message";
import type { ChatEvent, StoredMessage } from "$lib/utils/models";
import type { MenuItem } from "$lib/messages/MessageMenu.svelte";
import { chats } from "./chats.svelte";
import { composer } from "./composer.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";

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

export function menuItems(m: StoredMessage, openChat: (chat: string) => Promise<void>): MenuItem[] {
  const group = m.chat.endsWith("@g.us");
  const other = group && !m.from_me;
  const text = m.media_kind ? captionOf(m) : m.text;
  const items: MenuItem[] = [];
  // Under the quick-reaction row, and only once somebody has reacted: an entry
  // leading to an empty list is a dead end.
  if (messages.reactionsFor.get(m.id)?.length) {
    items.push({ label: "Reactions", icon: "smile", action: () => (ui.reactionsFor = m) });
  }
  items.push({
    label: "Reply",
    icon: "reply",
    action: () => {
      composer.editing = null;
      composer.replyingTo = m;
      composer.inputEl?.focus();
    },
  });
  if (m.from_me) {
    if (!m.revoked && !m.deleted && !m.media_kind && m.text.trim()) {
      items.push({ label: "Edit", icon: "edit", action: () => composer.startEditing(m) });
    }
    items.push({ label: "Message info", icon: "check", action: () => (ui.infoFor = m) });
  }
  // Admins can remove a member straight from their message; the owner can
  // never be removed, and we cannot remove ourselves this way.
  const senderParticipant = m.sender ? members.memberOf(m.sender) : undefined;
  if (other && members.isAdmin() && !senderParticipant?.owner) {
    items.push({
      label: `Remove ${members.senderLabel(m)} from group`,
      icon: "trash",
      danger: true,
      separated: true,
      action: () =>
        (ui.removeMember = { chat: m.chat, jid: bare(m.sender), name: members.senderLabel(m) }),
    });
  }
  if (other) {
    items.push(
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
    );
  }
  if (text && !m.revoked) {
    items.push({
      label: "Copy",
      icon: "copy",
      action: () => act(() => navigator.clipboard.writeText(text)),
    });
  }
  if (!m.revoked) {
    if (["image", "sticker", "video", "gif", "audio", "document"].includes(m.media_kind ?? "")) {
      const image = m.media_kind === "image" || m.media_kind === "sticker";
      const mediaAction = (action: string) => act(() => invoke("message_media_action", {
        chat: m.chat, id: m.id, action,
      }));
      if (image) items.push({ label: "Copy Image", icon: "copy", action: () => mediaAction("copy_image") });
      items.push(
        { label: image ? "Save Image…" : "Save Attachment…", icon: "download", action: () => mediaAction("save") },
        { label: image ? "Open Image" : "Open Attachment", icon: "external", action: () => mediaAction("open") },
      );
    }
    items.push(
      { label: "Forward", icon: "forward", action: () => (ui.forwarding = m) },
      {
        label: messages.marks.pinned === m.id ? "Unpin" : "Pin",
        icon: "pin",
        action: () =>
          act(() =>
            invoke("pin_message", { target: target(m), pinned: messages.marks.pinned !== m.id }),
          ),
      },
      {
        label: messages.starred.has(m.id) ? "Unstar" : "Star",
        icon: "star",
        action: () =>
          act(() => invoke("star", { target: target(m), starred: !messages.starred.has(m.id) })),
      },
    );
  }
  if (other) {
    items.push({
      label: "Report to admins",
      icon: "flag",
      separated: true,
      action: () => (ui.reporting = m),
    });
  }
  items.push({
    label: "Delete",
    icon: "trash",
    danger: true,
    separated: !other,
    action: () => (ui.deleting = m),
  });
  if (!m.revoked) {
    items.push({
      label: "Select messages",
      icon: "check",
      separated: true,
      action: () => (ui.picking = { [m.id]: true }),
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
      const m = messages.messages.find((row) => row.id === id);
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
  await act(async () => {
    await invoke("delete_messages", { chat, ids, everyone });
    ui.picking = null;
    await messages.reloadMessages(chat);
    await chats.refreshChats();
  });
}

/** Whether we may delete this message for everyone: ours, or ours to moderate. */
export function canDeleteForEveryone(m: StoredMessage) {
  if (m.revoked) return false;
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
