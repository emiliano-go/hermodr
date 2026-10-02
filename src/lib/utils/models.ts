// Shared data shapes for the chat views. Moved out of +page.svelte so every
// view, bar and bubble can type its props without importing the page.
import type { StoredMessage, Poll, Joined, MediaQuality, Event as ChatEvent } from "./wire";
export type {
  Account, ChatRetention, ChatSummary, ConnectionState, DiskRetention, GroupInfo,
  GroupHistoryOffer, GroupHistoryResult, GroupMemberAddResult, LiveLocation, OnceState,
  GroupJoinRequest,
  ParticipantChange, Poll, RetentionLimit, SearchResult, ServiceEvent, Sticker,
  StickerLibrary, StickerPack, StickerResyncReport, StoredMessage, UiSettings,
  Event as ChatEvent, Participant as Member, ChatMarks as Marks,
} from "./wire";

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

/** A file staged in the composer, before it is sent. */
export type PendingMedia = {
  id: number;
  file: File;
  url: string;
  kind: "image" | "video" | "other";
  caption: string;
  /** Whether this attachment goes out as view once. */
  once: boolean;
  quality?: MediaQuality;
  retry?: AttachmentRetryContext;
};

export type AttachmentRetryContext = {
  accountId: string;
  accountSeq: number;
  generation: number;
  chatGeneration: number;
  chat: string;
  batch: string;
  album: boolean;
  parentId: string | null;
  reply: Pick<StoredMessage, "id" | "sender" | "text"> | null;
  mentions: string[];
};

export type AttachmentRecovery = {
  context: AttachmentRetryContext;
  retryable: PendingMedia[];
  uncertain: PendingMedia[];
  sentIds: string[];
  uncertainId: string | null;
  parentUncertain: boolean;
  error: string;
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
export type ChatPrivacy = Pick<import("./wire").ChatSettings, "send_typing" | "send_receipts">;

/** Chat list filter tabs. */
export type ChatFilter = "all" | "unread" | "groups" | "archived" | "favorites";

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
  /** The message still carries something to draw (text, media, a quote). */
  hasBody: boolean;
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

/** The list-wide inputs a row's own view model is built from. */
export type BubbleCtx = {
  isGroup: boolean;
  /** Messages picked for a bulk action, in the open chat; null when not picking. */
  picking: Record<string, StoredMessage> | null;
  dayKey: (ts: number) => string;
  senderLabel: (m: StoredMessage) => string;
  memberTagOf: (sender: string) => string | null;
  hue: (jid: string) => number;
  captionOf: (m: StoredMessage) => string;
  viewOnceMarks: { id: string; opened: boolean; available: boolean }[];
  reactionsFor: Map<string, Reaction[]>;
  starredSet: Set<string>;
  editedSet: Set<string>;
  forwardedSet: Set<string>;
  downloading: Record<string, true>;
  downloadErrors: Record<string, string>;
  downloadTries: Record<string, number>;
  replyingToId: string | null;
  highlightedId: string | null;
  menuId: string | null;
  polls: Poll[];
  events: ChatEvent[];
  avatars: Record<string, string | null>;
  revealedOnce: Record<string, true>;
  voiceAvatarOf: (m: StoredMessage) => string | null;
  quoteAuthorOf: (sender: string | null) => string;
  quoteTextOf: (m: StoredMessage) => string | null;
  quoteChatNameOf: (m: StoredMessage) => string | null;
  autoplayId: string | null;
  onceAudioOpenId: string | null;
};

/** Page-owned helpers and actions the bubble calls back into. */
export type BubbleApi = {
  toWire: (text: string) => string;
  targetOf: (user: string) => MentionTarget;
  avatarOf: (jid: string) => string | null;
  onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
  onopenurl: (url: string) => void;
  onopenchat?: (chat: string) => void | Promise<void>;
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
  onrespond: (m: StoredMessage, response: string, extraGuestCount?: number) => unknown;
  oneditrequest: (m: StoredMessage) => void;
  oncancelevent: (m: StoredMessage) => void;
  onreact: (m: StoredMessage, emoji: string) => void;
  /** Opens the Reactions dialog for a message, from its pill or menu. */
  onopenreactions: (m: StoredMessage) => void;
  onmarkplayed: (m: StoredMessage) => void;
  onnextvoice: (m: StoredMessage) => void;
  onpausevoice: () => void;
  onreplymenu: (e: MouseEvent, m: StoredMessage) => void;
  ononce: (m: StoredMessage) => void;
  oncloseonce: () => void;
  /** Dismisses the one-time filter on a kept copy; a second click opens it. */
  onrevealonce: (m: StoredMessage) => void;
  oninviteopen: (jid: string) => void;
  oninvitejoin: (message: StoredMessage, link: string | null) => Promise<Joined>;
};
