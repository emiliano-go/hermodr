import { hue } from "./avatar";
import type { Account, BubbleApi, BubbleVm, ChatSummary, StoredMessage } from "./models";
import type { MenuItem } from "../messages/MessageMenuPanel.svelte";

export const noop = () => {};
export const account: Account = { id: "preview", label: "WhatsApp", jid: "598123456789@s.whatsapp.net", once_paired: false };
export const selectedChat = "design@g.us";

const rows = [
  ["Design team", "You: Will do 👍", "09:32", 0],
  ["Mom", "Dinner at 8?", "09:05", 2],
  ["Book club", "Laura: Photo", "08:47", 12],
  ["Diego Pérez", "You: See you tomorrow", "Yesterday", 0],
  ["Laura", "Thanks!", "Yesterday", 0],
  ["Work", "Ana: Standup moved to 10", "Mon", 0],
  ["Family", "Dad: 📍 Location", "Sun", 0],
] as const;

export const chats: ChatSummary[] = rows.map(([name, text, , unread], i) => ({
  chat: i === 0 ? selectedChat : `preview-${i}@g.us`,
  display_name: name,
  last_message_at: i,
  last_text: text,
  last_from_me: false,
  last_sender_name: null,
  last_sender: "",
  last_media_kind: null,
  message_count: 10,
  unread_count: unread,
  mention_count: i === 2 ? 1 : 0,
  pinned: false,
  archived: false,
  muted_until: 0,
  marked_unread: false,
}));

export const rowTime = (i: number) => rows[i]?.[2] ?? "";

function message(id: number, author: string, text: string, extra: Partial<StoredMessage> = {}): StoredMessage {
  return {
    spoiler: false,
    chat: selectedChat, id: String(id), sender: author, sender_name: author, sort_order: id,
    timestamp: 9 * 3600 + (12 + id * 4) * 60, from_me: author === "You", text,
    media_kind: null, media_path: null, media_thumb: null, media_duration: null,
    media_once_kind: null, reply_to_id: null, reply_to_text: null, reply_to_sender: null,
    reply_to_chat: null, reply_to_kind: null, reply_to_thumb: null, reply_to_path: null,
    reply_to_view_once: false, reply_to_recoverable: false,
    read: true, revoked: false, deleted: false, mentioned: false, preview_url: null, preview_title: null,
    preview_desc: null, preview_thumb: null, preview_site: null, preview_color: null,
    status: author === "You" ? "read" : null, system_kind: null, system_params: [],
    live_location: null, ...extra,
  };
}

export const messages = [
  message(0, "Ana", "Morning! The new mockups are up 🎨"),
  message(1, "Ana", "Can someone check the header spacing?"),
  message(2, "You", "On it, it looks tight on mobile", {
    reply_to_id: "1", reply_to_sender: "Ana", reply_to_text: "Can someone check the header spacing?",
  }),
  message(3, "Diego", "@598123456789 could you check the colors too?", { mentioned: true }),
  message(4, "Laura", "The brief is here: https://example.com/design"),
  message(5, "You", "Will do 👍", { status: "delivered" }),
];

export function bubbleView(m: StoredMessage, index: number, scene: string): BubbleVm {
  return {
    first: index !== 1, showSender: !m.from_me && index !== 1,
    senderText: m.sender_name ?? "", senderHue: hue(m.sender), senderAvatar: null,
    memberTag: null, visual: false, caption: "", viewOnce: null,
    onceKept: false, onceRevealed: false, inlineMeta: true,
    reactions: index === 0 ? [{ emoji: "👍", count: 2, mine: true }] : undefined,
    isStarred: false, isEdited: false, isForwarded: false, hasBody: true,
    isReplying: scene === "chat" && index === 1, highlighted: false, forMe: m.mentioned,
    menuOpen: scene === "menu" && index === 2, poll: undefined, chatEvent: undefined,
    downloading: false, downloadError: null, downloadGaveUp: false,
    picking: false, picked: false,
    onceAudioOpen: false, autoplay: false, voiceAvatar: null,
    quoteAuthor: m.reply_to_sender, quoteText: m.reply_to_text, quoteChatName: null,
  };
}

export const bubbleApi: BubbleApi = {
  toWire: (text) => text,
  targetOf: (user) => ({ jid: user, name: "You", self: true }),
  avatarOf: () => null,
  formatTime: (ts) => new Date(ts * 1000).toISOString().slice(11, 16),
  namer: (jid) => jid,
  onprofile: noop, onopenurl: noop, onreplydraft: noop, onmenu: noop, onpick: noop,
  onjumpquoted: noop, onrecoverquote: noop, recovering: {}, ondownload: noop,
  onopenviewer: noop, onopenmedia: noop, onopenquote: noop, onvote: noop,
  onrespond: noop, oneditrequest: noop, oncancelevent: noop, onreact: noop, onopenreactions: noop,
  onmarkplayed: noop, onnextvoice: noop, onpausevoice: noop, onreplymenu: noop,
  ononce: noop, oncloseonce: noop, onrevealonce: noop, oninviteopen: noop,
  oninvitejoin: async () => ({ jid: "preview@g.us", pending: false }),
};

export const menuItems: MenuItem[] = [
  { label: "Reply", icon: "reply", action: noop },
  { label: "Copy", icon: "copy", action: noop },
  { label: "Forward", icon: "forward", action: noop },
  { label: "Star", icon: "star", action: noop },
  { label: "Pin", icon: "pin", action: noop },
  { label: "Delete", icon: "trash", danger: true, separated: true, action: noop },
];
