// Shared data shapes for the chat views. Moved out of +page.svelte so every
// view, bar and bubble can type its props without importing the page.
export type Poll = {
  id: string;
  name: string;
  options: string[];
  multi: boolean;
  votes: { voter: string; options: string[] }[];
};

export type ChatEvent = {
  id: string;
  name: string;
  description: string | null;
  start: number | null;
  end: number | null;
  location: string | null;
  link: string | null;
  canceled: boolean;
  responses: { responder: string; response: string }[];
};

export type FoundItem = {
  chat: string;
  id: string;
  /** The chat's name, shown when results span several chats. */
  where: string | null;
  author: string;
  text: string;
  timestamp: number;
  unread: boolean;
};

export type StarredItem = {
  chat: string;
  id: string;
  where: string;
  author: string;
  text: string;
  timestamp: number;
  /** What the star action needs to name the message. */
  sender: string;
  fromMe: boolean;
};

export type ChatRetention = {
  max_age_hours: RetentionLimit;
  max_messages: RetentionLimit;
  on_demand: boolean;
};

export type StoredMessage = {
  sort_order?: number;
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
  media_duration: number | null;
  media_once_kind: string | null;
  reply_to_id: string | null;
  reply_to_text: string | null;
  reply_to_sender: string | null;
  reply_to_chat: string | null;
  reply_to_kind: string | null;
  reply_to_thumb: string | null;
  /** The quoted message was view-once, the one copy a linked device is sent. */
  reply_to_view_once: boolean;
  /** This account sent that view-once, so it may take the quoted copy. */
  reply_to_recoverable: boolean;
  /** Where a recovered copy was written, once taken. */
  reply_to_path: string | null;
  read: boolean;
  revoked: boolean;
  /** Deleted on this device only: kept, plus greyed out in the chat. */
  deleted: boolean;
  mentioned: boolean;
  preview_url: string | null;
  preview_title: string | null;
  preview_desc: string | null;
  preview_thumb: string | null;
  preview_site: string | null;
  preview_color: string | null;
  status: string | null;
  /** Set on a system line (group change, security notice) instead of a message. */
  system_kind: string | null;
  system_params: string[];
  /** The last position of a live location, updated in place as edits arrive. */
  live_location: LiveLocation | null;
};

/** A live location share as last seen by this device. */
export type LiveLocation = {
  lat: number;
  lng: number;
  /** The sender's accuracy estimate, in metres. */
  accuracy: number | null;
  /** Movement speed in metres per second. */
  speed: number | null;
  /** Travel direction, degrees clockwise from magnetic north. */
  heading: number | null;
  /** The sender's update counter; higher is newer. */
  sequence: number | null;
  /** When the share started, Unix seconds. */
  started_at: number;
  /** When the last position arrived, Unix seconds. */
  updated_at: number;
  /** When the share is expected to end, when the message carried one. */
  expires_at: number | null;
  /** Stopped by the sender or expired; the last position is kept. */
  ended: boolean;
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
  archived: boolean;
  /** Unix seconds; -1 indefinitely, 0 not muted. */
  muted_until: number;
  marked_unread: boolean;
};
export type Account = { id: string; label: string; jid: string | null };
export type SearchResult = {
  jid: string;
  name: string;
  number: string;
  kind: string;
  saved: boolean;
  has_messages: boolean;
  /** The contact's local aliases, which a query may have matched. */
  aliases: string[];
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
  /** Members may add participants, not just admins. */
  members_can_add: boolean;
};

/** The server's answer for one person of a group member change. */
export type ParticipantChange = {
  jid: string;
  ok: boolean;
  /** The server's code, such as `403` or `409`, when it refused. */
  code: string | null;
  error: string | null;
  /** The add was accepted but still needs an admin's approval. */
  pending: boolean;
};
export type RetentionLimit = { kind: "inherit" } | { kind: "unlimited" } | { kind: "limited"; value: number };
export type DiskRetention = {
  max_age_hours: RetentionLimit;
  max_messages_per_chat: RetentionLimit;
};
export type UiSettings = {
  retention: DiskRetention;
  message_window_size: number;
  request_full_history: boolean;
  auto_download_media: boolean;
  warn_missing_video_preview: boolean;
  media_dir: string | null;
  /** Cold storage for the message archive; empty keeps it in the app data folder. */
  history_dir: string | null;
  send_typing: boolean;
  send_receipts: boolean;
  keep_history: boolean;
  skip_loading_screen: boolean;
  keep_archived: boolean;
  android_instance: boolean;
  /** Global kill switch for desktop notifications; muted chats never notify. */
  notifications_enabled: boolean;
  /** Keep the chat list order while the pointer hovers over it; reorder on leave/open. */
  freeze_chat_list_on_hover: boolean;
  /** Log the library's keepalive pings and transport frames; applies next start. */
  verbose_whatsapp_logs: boolean;
};
export type ConnectionState = { started: boolean; connected: boolean; qr: string | null };
export type OnceState = { paired: boolean; pairing: boolean; running: boolean; connected: boolean; qr: string | null };

/** A file staged in the composer, before it is sent. */
export type PendingMedia = {
  id: number;
  file: File;
  url: string;
  kind: "image" | "video" | "other";
  caption: string;
  /** Whether this attachment goes out as view once. */
  once: boolean;
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
  | { kind: "chatStateChanged"; chat: string }
  | { kind: "syncing"; pending: number; applied: number }
  | { kind: "initialSyncComplete"; messages: number; chats: number }
  | { kind: "synced" }
  | { kind: "historyLoaded"; chats: string[] }
  | { kind: "backfill"; done: number; total: number }
  | { kind: "historyProgress"; percent: number }
  | { kind: "avatarChanged"; jid: string }
  | { kind: "typing"; chat: string; sender: string; state: string }
  | { kind: "presence"; jid: string; online: boolean; last_seen: number | null }
  | { kind: "memberLabel"; chat: string; jid: string; label: string }
  | { kind: "groupChanged"; chat: string }
  | { kind: "marks"; chat: string }
  | { kind: "stickerLibraryChanged"; packs: boolean; favorites: boolean; recents: boolean };

/** A sticker in a pack, or one kept from a message. */
export type Sticker = {
  filehash: string;
  pack_id: string | null;
  path: string | null;
  animated: boolean;
  lottie: boolean;
  emojis: string[];
  favorite: boolean;
  recent_at: number | null;
  updated_at: number;
};

/** A sticker pack received from the phone or fetched by id. */
export type StickerPack = {
  pack_id: string;
  name: string | null;
  publisher: string | null;
  tray_path: string | null;
  origin: string | null;
  updated_at: number;
};

export type StickerLibrary = {
  packs: StickerPack[];
  favorites: Sticker[];
  recent: Sticker[];
};

export type StickerResyncReport = { packs: number; stickers: number };

/** Group members for the @ autocomplete. */
export type Member = {
  jid: string;
  name: string;
  number: string | null;
  /** The member's reserved WhatsApp username, when they have one. */
  username: string | null;
  label: string | null;
  admin: boolean;
  /** The group's creator, who cannot be removed or demoted. */
  owner: boolean;
};

export type Marks = {
  reactions: { target: string; sender: string; emoji: string }[];
  starred: string[];
  pinned: string | null;
  polls: Poll[];
  events: ChatEvent[];
  view_once: { id: string; opened: boolean; available: boolean }[];
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
export type ChatFilter = "all" | "unread" | "groups" | "archived";

/** Who an `@<user>` token names. */
export type MentionTarget = { jid: string; name: string; self: boolean };

/** One emoji with its count, and whether one of them is ours. */
export type Reaction = { emoji: string; count: number; mine: boolean };

/** Who reacted to a message, grouped by the emoji they used. */
export type ReactionGroup = { emoji: string; senders: string[] };

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
  viewOnce: { opened: boolean; available: boolean } | null;
  /** A kept one-time copy, still behind its dismissable one-time filter. */
  onceKept: boolean;
  /** The filter was dismissed for this visit to the chat. */
  onceRevealed: boolean;
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
  /** Why the last download failed, shown until it is tried again. */
  downloadError: string | null;
  /** Downloading kept failing, so the retry is no longer offered. */
  downloadGaveUp: boolean;
  /** The message can be picked for a bulk action. */
  picking: boolean;
  /** It is picked. */
  picked: boolean;
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
  /** Picks or unpicks a message while a bulk selection is open. */
  onpick: (m: StoredMessage) => void;
  onjumpquoted: (m: StoredMessage) => void;
  /** Takes back the view-once a reply quotes, then opens the recovered copy. */
  onrecoverquote: (m: StoredMessage) => void;
  /** A view-once copy is being recovered from this reply. */
  recovering: Record<string, true>;
  ondownload: (m: StoredMessage) => void;
  onopenviewer: (m: StoredMessage) => void;
  onopenmedia: (path: string) => void;
  /** Shows the view-once copy a reply carries in the built-in viewer. */
  onopenquote: (m: StoredMessage) => void;
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
  /** Dismisses the one-time filter on a kept copy; a second click opens it. */
  onrevealonce: (m: StoredMessage) => void;
  oninviteopen: (jid: string) => void;
};
