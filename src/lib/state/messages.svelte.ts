// Messages domain: the open conversation's scrollback, marks, recall,
// downloads, typing state flows and autoplay. Moved out of +page.svelte.
// Depends only on ui (failure reporting); cross-domain flows (openChat, send,
// event dispatch) live in the route and state/events.ts.
import { tick } from "svelte";
import { invoke } from "$lib/ipc";
import type { Marks, Reaction, StoredMessage } from "$lib/models";
import { ui } from "./ui.svelte";

const PAGE = 200;

export class MessagesState {
  /** How many messages the open chat shows; "load older" raises it. */
  messageLimit = $state(PAGE);
  loadingOlder = $state(false);
  olderTimer: ReturnType<typeof setTimeout> | undefined = undefined;
  /** Set while a "load older" answer is in flight; its `historyLoaded` is the flush. */
  historyActive = $state(false);
  /** Whether reaching the top of the open chat asks the phone for more. */
  loadOnScroll = $state(true);
  /** Set once the phone had nothing older, so scrolling stops asking. */
  olderExhausted = $state(false);
  /**
   * A recall walks back about a day, 50 messages per request, since the phone
   * answers by count and not by time.
   */
  recall: { chat: string; until: number; rounds: number; auto: boolean } | null = null;

  messages: StoredMessage[] = $state([]);
  /** Newest-first request id; a slow `messages` response must not win over a newer one. */
  messagesSeq = 0;

  /** Invalidates in-flight reloads; the holder compares its id against {@link messagesSeq}. */
  nextSeq() {
    return ++this.messagesSeq;
  }
  /** Oldest first, the order the conversation is drawn in. */
  ordered = $derived(this.messages.slice().reverse());

  /** Reactions, stars, the pinned message, polls and events of the open chat. */
  marks = $state<Marks>(structuredClone(NO_MARKS));

  /** Per message: each emoji with its count, and whether one of them is ours. */
  reactionsFor = $derived.by(() => {
    const byMessage = new Map<string, Reaction[]>();
    for (const r of this.marks.reactions) {
      const list = byMessage.get(r.target) ?? [];
      const entry = list.find((e) => e.emoji === r.emoji);
      if (entry) {
        entry.count += 1;
        entry.mine ||= r.sender === "@me";
      } else {
        list.push({ emoji: r.emoji, count: 1, mine: r.sender === "@me" });
      }
      byMessage.set(r.target, list);
    }
    return byMessage;
  });
  starred = $derived(new Set(this.marks.starred));
  edited = $derived(new Set(this.marks.edited));
  forwarded = $derived(new Set(this.marks.forwarded));
  pinnedMessage = $derived(
    this.marks.pinned ? (this.messages.find((m) => m.id === this.marks.pinned) ?? null) : null,
  );

  /** Media downloads in flight, so a second click does not start another. */
  downloading = $state<Record<string, true>>({});
  /** Voice note to play next, set when the previous one ends on its own. */
  autoplayId = $state<string | null>(null);
  /** Unread mentions in the open chat, oldest first, for jump-to-mention. */
  mentionQueue = $state<string[]>([]);
  mentionCursor = $state(0);

  /** Resolved whenever a recall ends, however it ends. */
  recallWaiters: (() => void)[] = [];

  /**
   * Reloads a conversation without touching the unread state.
   *
   * `keepPlace` holds the view on the same message when older ones are added
   * above it, instead of letting them push it down.
   */
  async reloadMessages(chat: string | null, keepPlace = false, el: HTMLDivElement | null = null) {
    if (!chat) return;
    const seq = ++this.messagesSeq;
    const fromBottom = el ? el.scrollHeight - el.scrollTop : 0;
    let loaded: StoredMessage[];
    try {
      loaded = await invoke<StoredMessage[]>("messages", { chat, limit: this.messageLimit });
    } catch (e) {
      ui.fail(e);
      return;
    }
    // A slow response must not overwrite a newer conversation.
    if (seq !== this.messagesSeq) return;
    this.messages = loaded;
    if (keepPlace && el) {
      await tick();
      el.scrollTop = el.scrollHeight - fromBottom;
    }
  }

  async loadMarks(chat: string | null) {
    if (!chat) return;
    try {
      this.marks = await invoke<Marks>("marks", { chat });
    } catch {
      this.marks = structuredClone(NO_MARKS);
    }
  }

  /** `quiet` for background fetches, whose failures only matter once clicked. */
  async downloadMedia(chat: string | null, message: StoredMessage, quiet = false) {
    if (!chat || this.downloading[message.id]) return;
    this.downloading[message.id] = true;
    try {
      await invoke("download_media", { chat, id: message.id });
      await this.reloadMessages(chat);
    } catch (e) {
      if (!quiet) ui.fail(e);
    } finally {
      delete this.downloading[message.id];
    }
  }

  /** Tells the sender a voice note was heard or view-once media opened; the core honours the receipts setting. */
  markPlayed(message: StoredMessage) {
    if (message.from_me) return;
    invoke("mark_played", { chat: message.chat, id: message.id, sender: message.sender }).catch(
      () => {},
    );
  }

  /** The note after `finished` in the conversation, so the next one can autoplay. */
  playNextVoice(finished: StoredMessage) {
    const at = this.ordered.findIndex((m) => m.id === finished.id);
    this.autoplayId =
      this.ordered.slice(at + 1).find((m) => m.media_kind === "audio" && m.media_path)?.id ?? null;
  }

  settleRecall() {
    const waiters = this.recallWaiters;
    this.recallWaiters = [];
    for (const done of waiters) done();
  }

  /** Asks the phone for the chat's previous day and waits until it has landed or given up. */
  recallDay(chat: string | null): Promise<void> {
    if (!chat || this.olderExhausted) return Promise.resolve();
    const done = new Promise<void>((resolve) => this.recallWaiters.push(resolve));
    if (!this.loadingOlder) void this.loadOlder(chat);
    return done;
  }

  /** Asks the phone for about a day of older messages in the open chat. */
  async loadOlder(chat: string | null, auto = false) {
    if (!chat || this.loadingOlder) return;
    const oldest = this.messages.at(-1)?.timestamp ?? Math.floor(Date.now() / 1000);
    this.recall = { chat, until: oldest - 86_400, rounds: 0, auto };
    await this.requestOlder(chat);
  }

  async requestOlder(chat: string) {
    if (!this.recall) return;
    this.loadingOlder = true;
    const auto = this.recall.auto;
    // The phone answers asynchronously, or not at all when it has nothing
    // older or is offline, so the spinner gives up on its own.
    clearTimeout(this.olderTimer);
    this.olderTimer = setTimeout(() => {
      this.loadingOlder = false;
      this.historyActive = false;
      this.recall = null;
      this.olderExhausted = true;
      if (!auto) ui.fail("Your phone did not answer. It has to be online for older messages to load.");
      this.settleRecall();
    }, 15000);
    try {
      this.historyActive = true;
      await invoke("load_older", { chat, count: 50 });
    } catch (e) {
      this.loadingOlder = false;
      this.historyActive = false;
      this.recall = null;
      ui.fail(e);
      this.settleRecall();
    }
  }

  /** After a batch lands: keep walking back until the day is covered. */
  continueRecall(chat: string | null, added: number) {
    const oldest = this.messages.at(-1)?.timestamp;
    if (!this.recall || this.recall.chat !== chat) {
      this.settleRecall();
      return;
    }
    if (added === 0) this.olderExhausted = true;
    if (added > 0 && oldest && oldest > this.recall.until && ++this.recall.rounds < 10) {
      void this.requestOlder(this.recall.chat);
    } else {
      this.recall = null;
      this.settleRecall();
    }
  }

  /** Readies a fresh chat: pager reset, recall cancelled, autoplay cleared. */
  prepareChat() {
    this.messageLimit = PAGE;
    this.messagesSeq++;
    this.recall = null;
    this.olderExhausted = false;
    this.loadOnScroll = true;
    this.autoplayId = null;
  }

  /** Mirrors resetUi: the list and the mention queue are dropped. */
  resetAccount() {
    this.messages = [];
    this.mentionQueue = [];
  }
}

const NO_MARKS: Marks = {
  reactions: [],
  starred: [],
  pinned: null,
  polls: [],
  events: [],
  view_once: [],
  forwarded: [],
  edited: [],
};

export const messages = new MessagesState();
