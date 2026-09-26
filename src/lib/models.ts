// Shared data shapes for the chat views. Moved out of +page.svelte so every
// view, bar and bubble can type its props without importing the page.
import type { Poll } from "$lib/PollCard.svelte";
import type { ChatEvent } from "$lib/EventCard.svelte";

export type StoredMessage = {
  chat: string;
  id: string;
  sender: string;
  sender_name: string | null;
  timestamp: number;
  from_me: boolean;
  text: string;
  media_kind: string | null;
  media_path: string | null;
  media_thumb: string | null;
  reply_to_id: string | null;
  reply_to_text: string | null;
  reply_to_sender: string | null;
  reply_to_chat: string | null;
  reply_to_kind: string | null;
  reply_to_thumb: string | null;
  read: boolean;
  revoked: boolean;
  mentioned: boolean;
  preview_url: string | null;
  preview_title: string | null;
  preview_desc: string | null;
  preview_thumb: string | null;
  preview_site: string | null;
  preview_color: string | null;
  status: string | null;
};
export type ChatSummary = {
  chat: string;
  display_name: string | null;
  last_message_at: number;
  last_text: string;
  last_from_me: boolean;
  last_sender_name: string | null;
  last_sender: string;
  last_media_kind: string | null;
  message_count: number;
  unread_count: number;
  mention_count: number;
  pinned: boolean;
};
export type Account = { id: string; label: string; jid: string | null };
export type SearchResult = {
  jid: string;
  name: string;
  number: string;
  kind: string;
  saved: boolean;
  has_messages: boolean;
};
export type GroupInfo = {
  subject: string | null;
  description: string | null;
  created_at: number | null;
  owner: string | null;
  owner_jid: string | null;
  participants: {
    jid: string;
    name: string;
    admin: boolean;
    owner: boolean;
    number: string | null;
    username: string | null;
    label: string | null;
  }[];
  allow_admin_reports: boolean;
  announce: boolean;
  locked: boolean;
  community: boolean;
  announcements: boolean;
  parent: string | null;
  parent_name: string | null;
  admin: boolean;
  can_send: boolean;
};
export type Retention = {
  max_age_hours: number | null;
  max_messages_per_chat: number | null;
};
export type UiSettings = {
  retention: Retention;
  accept_full_history: boolean;
  auto_download_media: boolean;
  warn_missing_video_preview: boolean;
  media_dir: string | null;
  send_typing: boolean;
  send_receipts: boolean;
  keep_history: boolean;
  skip_loading_screen: boolean;
};
export type ConnectionState = { started: boolean; connected: boolean; qr: string | null };

/** A file staged in the composer, before it is sent. */
export type PendingMedia = {
  id: number;
  file: File;
  url: string;
  kind: "image" | "video" | "other";
  caption: string;
};

export type ServiceEvent =
  | { kind: "qrCode"; code: string }
  | { kind: "connected" }
  | { kind: "disconnected" }
  | { kind: "loggedOut" }
  | { kind: "uploadProgress"; token: string; sent: number; total: number }
  | { kind: "message"; message: StoredMessage }
  | { kind: "messageHint"; chat: string; id: string; sender: string; from_me: boolean; fresh: boolean }
  | { kind: "retentionApplied"; removed: number }
  | { kind: "namesUpdated"; count: number }
  | { kind: "syncing"; pending: number; applied: number }
  | { kind: "initialSyncComplete"; messages: number; chats: number }
  | { kind: "synced" }
  | { kind: "historyLoaded"; chats: string[] }
  | { kind: "avatarChanged"; jid: string }
  | { kind: "typing"; chat: string; sender: string; state: string }
  | { kind: "presence"; jid: string; online: boolean; last_seen: number | null }
  | { kind: "memberLabel"; chat: string; jid: string; label: string }
  | { kind: "groupChanged"; chat: string }
  | { kind: "marks"; chat: string };

/** Group members for the @ autocomplete. */
export type Member = {
  jid: string;
  name: string;
  number: string | null;
  /** The member's reserved WhatsApp username, when they have one. */
  username: string | null;
  label: string | null;
  admin: boolean;
};

export type Marks = {
  reactions: { target: string; sender: string; emoji: string }[];
  starred: string[];
  pinned: string | null;
  polls: Poll[];
  events: ChatEvent[];
  view_once: { id: string; opened: boolean }[];
  forwarded: string[];
  edited: string[];
};

/** Files on their way out, drawn at the end of their chat until the sent message replaces them. */
export type Outgoing = {
  token: string;
  chat: string;
  kind: "image" | "video" | "other";
  url: string;
  name: string;
  caption: string;
  /** 0 to 1, from the core's upload progress. */
  progress: number;
};

/** The open chat's overrides of the typing and read receipt settings; `null` follows them. */
export type ChatPrivacy = { send_typing: boolean | null; send_receipts: boolean | null };

/** Chat list filter tabs. */
export type ChatFilter = "all" | "unread" | "groups";

/** Who an `@<user>` token names. */
export type MentionTarget = { jid: string; name: string; self: boolean };

/** One emoji with its count, and whether one of them is ours. */
export type Reaction = { emoji: string; count: number; mine: boolean };

/** Everything the list precomputes per message so the bubble stays dumb. */
export type BubbleVm = {
  first: boolean;
  showSender: boolean;
  senderText: string;
  senderHue: number;
  senderAvatar: string | null;
  memberTag: string | null;
  visual: boolean;
  caption: string;
  viewOnce: { opened: boolean } | null;
  inlineMeta: boolean;
  reactions: Reaction[] | undefined;
  isStarred: boolean;
  isEdited: boolean;
  isForwarded: boolean;
  isReplying: boolean;
  highlighted: boolean;
  forMe: boolean;
  menuOpen: boolean;
  poll: Poll | undefined;
  chatEvent: ChatEvent | undefined;
  downloading: boolean;
  onceAudioOpen: boolean;
  autoplay: boolean;
  voiceAvatar: string | null;
  quoteAuthor: string | null;
  quoteText: string | null;
  quoteChatName: string | null;
};

/** Page-owned helpers and actions the bubble calls back into. */
export type BubbleApi = {
  toWire: (text: string) => string;
  targetOf: (user: string) => MentionTarget;
  avatarOf: (jid: string) => string | null;
  onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
  onopenurl: (url: string) => void;
  formatTime: (ts: number) => string;
  namer: (jid: string) => string;
  onreplydraft: (m: StoredMessage) => void;
  onmenu: (e: MouseEvent, m: StoredMessage) => void;
  onjumpquoted: (m: StoredMessage) => void;
  ondownload: (m: StoredMessage) => void;
  onopenviewer: (m: StoredMessage) => void;
  onopenmedia: (path: string) => void;
  onvote: (m: StoredMessage, options: string[]) => unknown;
  onrespond: (m: StoredMessage, response: string) => unknown;
  oneditrequest: (m: StoredMessage) => void;
  oncancelevent: (m: StoredMessage) => void;
  onreact: (m: StoredMessage, emoji: string) => void;
  onmarkplayed: (m: StoredMessage) => void;
  onnextvoice: (m: StoredMessage) => void;
  onpausevoice: () => void;
  onreplymenu: (e: MouseEvent, m: StoredMessage) => void;
  ononce: (m: StoredMessage) => void;
  oncloseonce: () => void;
  oninviteopen: (jid: string) => void;
};

export type { Poll, ChatEvent };
