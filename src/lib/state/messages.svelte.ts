// Messages domain: the open conversation's scrollback, marks, recall,
// downloads, typing state flows and autoplay. Moved out of +page.svelte.
// Depends only on ui (failure reporting); cross-domain flows (openChat, send,
// event dispatch) live in the route and state/events.ts.
import { tick } from "svelte";
import { invoke } from "$lib/utils/ipc";
import type { Marks, Reaction, ReactionGroup, StoredMessage } from "$lib/utils/models";
import { ui } from "./ui.svelte";
import { MessageWindow, DEFAULT_MESSAGE_WINDOW, cursorOf, type MessagePage } from "$lib/utils/message-window";
import { isUnavailable } from "$lib/utils/message";

const PAGE = 200;
export const MAX_DOWNLOAD_TRIES = 3;

export class MessagesState {
  messageLimit = $state(DEFAULT_MESSAGE_WINDOW);
  atLatest = $state(true);
  private window = new MessageWindow();
  private chat: string | null = null;
  private refreshPending = false;
  private marksSeq = 0;
  private accountSeq = 0;
  private rowRequestSeq = 0;
  private rowRequests = new Map<string, number>();
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

  /**
   * The open chat's rows, newest first. Raw: replaced wholesale on every
   * update, so deep proxying only costs time on a list this size.
   */
  messages: StoredMessage[] = $state.raw([]);
  /** Newest-first request id; a slow `messages` response must not win over a newer one. */
  messagesSeq = 0;

  get accountGeneration() { return this.accountSeq; }

  /** Invalidates in-flight reloads; the holder compares its id against {@link messagesSeq}. */
  nextSeq() {
    return ++this.messagesSeq;
  }
  /** Oldest first, the order the conversation is drawn in. */
  ordered = $derived(this.messages.slice().reverse());

  /**
   * Reactions, stars, the pinned message, polls and events of the open chat.
   * Replaced wholesale, so raw like the message list.
   */
  marks = $state.raw<Marks>(structuredClone(NO_MARKS));

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
  /**
   * Who reacted to each message, by emoji: the sender addresses the counts
   * above fold away, which is what the "Reactions" list reads. Groups keep the
   * order the emoji first arrived in, and ours ("@me") leads its own group.
   */
  reactorsFor = $derived.by(() => {
    const byMessage = new Map<string, ReactionGroup[]>();
    for (const r of this.marks.reactions) {
      const list = byMessage.get(r.target) ?? [];
      const group = list.find((g) => g.emoji === r.emoji);
      if (group) group.senders.push(r.sender);
      else list.push({ emoji: r.emoji, senders: [r.sender] });
      byMessage.set(r.target, list);
    }
    for (const list of byMessage.values()) {
      for (const group of list) {
        group.senders.sort((a, b) => Number(b === "@me") - Number(a === "@me"));
      }
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
  /** Why a message's last download failed, until it is tried again. */
  downloadErrors = $state<Record<string, string>>({});
  /** Failed downloads per message; at `MAX_DOWNLOAD_TRIES` the retry is withdrawn. */
  downloadTries = $state<Record<string, number>>({});
  /** View-once copies being recovered from a reply, keyed by that reply. */
  recovering = $state<Record<string, true>>({});
  /** Kept one-time media whose filter was dismissed, keyed by message. */
  revealedOnce = $state<Record<string, true>>({});

  /** Lifts the one-time filter from a kept copy, for this visit to the chat. */
  revealOnce(id: string) {
    this.revealedOnce = { ...this.revealedOnce, [id]: true };
  }
  /** Voice note to play next, set when the previous one ends on its own. */
  autoplayId = $state<string | null>(null);
  /** Unread mentions in the open chat, oldest first, for jump-to-mention. */
  mentionQueue = $state<string[]>([]);
  mentionCursor = $state(0);

  /** Oldest unread incoming message when the open chat was entered; shows the divider. */
  firstUnreadId = $state<string | null>(null);
  /**
   * Newest unread at that same moment. The divider stays until this one is
   * read too, so it survives scrolling through the run instead of vanishing
   * with the first message.
   */
  lastUnreadId = $state<string | null>(null);
  /** Last message marked read while scrolling, so marking only happens on change. */
  lastMarkedId: string | null = null;
  readMarkTimer: ReturnType<typeof setTimeout> | undefined = undefined;

  /** Resolved whenever a recall ends, however it ends. */
  recallWaiters: (() => void)[] = [];

  /**
   * Reloads a conversation without touching the unread state.
   *
   * `keepPlace` holds the view on the same message when older ones are added
   * above it, instead of letting them push it down.
   */
  async reloadMessages(chat: string | null, keepPlace = false, el: HTMLDivElement | null = null) {
    if (!chat || chat !== this.chat) return false;
    if (this.loadingOlder) { this.refreshPending = true; return false; }
    const seq = ++this.messagesSeq;
    let loaded: StoredMessage[];
    try {
      const cursor = !this.atLatest && this.messages[0] ? cursorOf(this.messages[0]) : null;
      loaded = (await invoke<MessagePage>("message_page", { chat, limit: this.messageLimit, cursor, direction: "through" })).messages;
    } catch (e) {
      if (seq === this.messagesSeq) ui.fail(e);
      return false;
    }
    // A slow response must not overwrite a newer conversation.
    if (seq !== this.messagesSeq) return false;
    await this.paint(loaded, "replace", keepPlace ? el : null);
    if (seq !== this.messagesSeq) return false;
    // The divider only makes sense while its message is still loaded.
    if (this.firstUnreadId && !loaded.some((m) => m.id === this.firstUnreadId)) {
      this.firstUnreadId = null;
    }
    if (this.lastUnreadId && !loaded.some((m) => m.id === this.lastUnreadId)) {
      this.lastUnreadId = null;
    }
    return true;
  }

  acceptMessages(rows: StoredMessage[]) {
    this.rowRequests.clear();
    this.messages = this.window.replace(rows);
  }

  /** Folds one changed row in, so only its bubble re-renders. */
  patch(row: StoredMessage) {
    if (row.chat !== this.chat) return;
    const key = JSON.stringify([row.chat, row.id]);
    const next = this.window.patch(row);
    if (next === this.messages) return;
    this.rowRequests.delete(key);
    this.messagesSeq++;
    this.messages = next;
  }

  /** Applies a delivery-state change to a loaded row without a refetch. */
  setStatus(chat: string, id: string, status: string) {
    const row = this.messages.find((m) => m.chat === chat && m.id === id);
    if (!row || row.status === status) return;
    const next = this.window.patch({ ...row, status });
    if (next === this.messages) return;
    this.messagesSeq++;
    this.messages = next;
  }

  /** Adds a row that just arrived, or refreshes it when already loaded. */
  append(row: StoredMessage) {
    if (row.chat !== this.chat) return;
    if (!this.atLatest && !this.messages.some((loaded) => loaded.chat === row.chat && loaded.id === row.id)) return;
    this.rowRequests.delete(JSON.stringify([row.chat, row.id]));
    this.messagesSeq++;
    this.messages = this.window.insert(row);
  }

  /**
   * Fetches one row and folds it in without reloading the window. A full
   * window reload replaces every object and so re-renders every bubble; on a
   * busy account that happens per message. `mayAppend` is for a fresh arrival
   * only: an old row the window never held is left to the next full reload.
   */
  async refreshRow(chat: string, id: string, mayAppend: boolean) {
    if (chat !== this.chat) return;
    const known = this.messages.some((m) => m.chat === chat && m.id === id);
    if (!known && !(mayAppend && this.atLatest)) return;
    const key = JSON.stringify([chat, id]);
    const request = ++this.rowRequestSeq;
    const account = this.accountSeq;
    const window = this.window;
    const status = this.messages.find((m) => m.chat === chat && m.id === id)?.status;
    this.rowRequests.set(key, request);
    this.messagesSeq++;
    try {
      const page = await invoke<MessagePage>("message_page", {
        chat,
        limit: 1,
        anchorId: id,
        direction: "through",
      });
      if (account !== this.accountSeq || window !== this.window || chat !== this.chat || this.rowRequests.get(key) !== request) return;
      let row = page.messages.find((m) => m.chat === chat && m.id === id);
      const current = this.messages.find((m) => m.chat === chat && m.id === id);
      if (!row) {
        if (current && isUnavailable(current)) {
          this.messagesSeq++;
          this.messages = this.window.replace(this.messages.filter((m) => m.chat !== chat || m.id !== id));
        }
        return;
      }
      if (current && current.status !== status) row = { ...row, status: current.status };
      if (known) this.patch(row);
      else this.append(row);
    } catch {
      // A later full reload covers a failed one-row fetch.
    } finally {
      if (this.rowRequests.get(key) === request) this.rowRequests.delete(key);
    }
  }

  private async flushRefresh(chat: string, el: HTMLDivElement | null = null) {
    if (!this.refreshPending || chat !== this.chat) return;
    this.refreshPending = false;
    await this.reloadMessages(chat, true, el);
  }

  resizeWindow(limit: number) {
    this.messageLimit = limit;
    this.window = new MessageWindow(limit);
    this.rowRequests.clear();
    this.messages = this.window.retain(this.messages, this.atLatest ? "newer" : "older");
  }

  private async paint(rows: StoredMessage[], mode: "replace" | "older" | "newer", el: HTMLDivElement | null) {
    const anchor = el ? [...el.querySelectorAll<HTMLElement>("[data-id]")]
      .find((row) => row.getBoundingClientRect().bottom > el.getBoundingClientRect().top) : undefined;
    const id = anchor?.dataset.id;
    const top = anchor?.getBoundingClientRect().top ?? 0;
    if (mode === "replace") this.rowRequests.clear();
    this.messages = mode === "replace" ? this.window.replace(rows) : this.window.retain(rows, mode);
    if (el && id) {
      await tick();
      const row = [...el.querySelectorAll<HTMLElement>("[data-id]")].find((row) => row.dataset.id === id);
      if (row) el.scrollTop += row.getBoundingClientRect().top - top;
    }
  }

  private async loadLocalOlder(chat: string, el: HTMLDivElement | null): Promise<number | null> {
    const oldest = this.messages.at(-1);
    if (!oldest) return 0;
    const seq = ++this.messagesSeq;
    const page = await invoke<MessagePage>("message_page", {
      chat, limit: Math.min(PAGE, Math.floor(this.messageLimit / 2)), cursor: cursorOf(oldest), direction: "before",
    });
    if (seq !== this.messagesSeq || chat !== this.chat) return null;
    if (page.messages.length) {
      this.atLatest = false;
      await this.paint(page.messages, "older", el);
      await this.loadMarks(chat);
    }
    return page.messages.length;
  }

  async loadNewer(chat: string | null, el: HTMLDivElement | null = null) {
    if (!chat || chat !== this.chat || this.loadingOlder || !this.messages[0]) return;
    const seq = ++this.messagesSeq;
    this.loadingOlder = true;
    try {
      const page = await invoke<MessagePage>("message_page", {
        chat, limit: Math.min(PAGE, Math.floor(this.messageLimit / 2)), cursor: cursorOf(this.messages[0]), direction: "after",
      });
      if (seq !== this.messagesSeq) return;
      this.atLatest = !page.has_more;
      this.olderExhausted = false;
      await this.paint(page.messages, "newer", el);
      await this.loadMarks(chat);
    } catch (e) { ui.fail(e); }
    finally {
      if (seq === this.messagesSeq) { this.loadingOlder = false; await this.flushRefresh(chat, el); }
    }
  }

  async showLatest(chat: string | null) {
    if (!chat || chat !== this.chat) return false;
    const wasLatest = this.atLatest;
    this.atLatest = true;
    this.refreshPending = false;
    this.recall = null;
    this.loadingOlder = false;
    this.historyActive = false;
    this.olderExhausted = false;
    clearTimeout(this.olderTimer);
    this.settleRecall();
    const seq = this.messagesSeq + 1;
    const loaded = await this.reloadMessages(chat);
    if (!loaded && seq === this.messagesSeq) this.atLatest = wasLatest;
    await this.loadMarks(chat);
    return loaded;
  }

  async showStoredMessage(chat: string, id: string): Promise<boolean> {
    if (chat !== this.chat) return false;
    this.loadingOlder = false;
    this.historyActive = false;
    this.recall = null;
    clearTimeout(this.olderTimer);
    this.settleRecall();
    const seq = ++this.messagesSeq;
    const page = await invoke<MessagePage>("message_page", { chat, limit: this.messageLimit, anchorId: id, direction: "through" });
    if (seq !== this.messagesSeq || chat !== this.chat || !page.messages.some((m) => m.id === id)) return false;
    this.atLatest = false;
    this.acceptMessages(page.messages);
    await this.loadMarks(chat);
    return true;
  }

  async loadMarks(chat: string | null) {
    if (!chat || chat !== this.chat) return;
    const seq = ++this.marksSeq;
    try {
      const marks = await invoke<Marks>("marks", { chat, ids: this.messages.map((m) => m.id) });
      if (chat === this.chat && seq === this.marksSeq) this.marks = marks;
    } catch {
      if (chat === this.chat && seq === this.marksSeq) this.marks = structuredClone(NO_MARKS);
    }
  }

  /** `quiet` for background fetches, whose failures only matter once clicked. */
  async downloadMedia(chat: string | null, message: StoredMessage, quiet = false) {
    const account = this.accountSeq;
    const tries = this.downloadTries[message.id] ?? 0;
    if (!chat || isUnavailable(message) || this.downloading[message.id] || tries >= MAX_DOWNLOAD_TRIES) return;
    this.downloading[message.id] = true;
    delete this.downloadErrors[message.id];
    try {
      await invoke("download_media", { chat, id: message.id });
      if (account !== this.accountSeq) return;
      delete this.downloadTries[message.id];
      // Fold the fetched path into its row instead of reloading the window;
      // a chat full of stickers used to reload once per file.
      await this.refreshRow(chat, message.id, true);
    } catch (e) {
      // The core already asked the sender to upload it again; what is left is shown on the message.
      if (account !== this.accountSeq) return;
      this.downloadErrors[message.id] = String(e);
      this.downloadTries[message.id] = tries + 1;
      if (!quiet) ui.fail(e);
    } finally {
      if (account === this.accountSeq) delete this.downloading[message.id];
    }
  }

  /** Takes back the view-once a reply quotes, returning where the copy landed. */
  async recoverQuote(chat: string | null, message: StoredMessage): Promise<string | null> {
    if (!chat || isUnavailable(message) || this.recovering[message.id]) return null;
    this.recovering[message.id] = true;
    delete this.downloadErrors[message.id];
    try {
      await invoke("recover_quote_media", { chat, id: message.id });
      await this.reloadMessages(chat);
      return this.ordered.find((m) => m.id === message.id)?.reply_to_path ?? null;
    } catch (e) {
      this.downloadErrors[message.id] = String(e);
      ui.fail(e);
      return null;
    } finally {
      delete this.recovering[message.id];
    }
  }

  /** Tells the sender a voice note was heard or view-once media opened; the core honours the receipts setting. */
  markPlayed(message: StoredMessage) {
    if (message.from_me || isUnavailable(message)) return;
    invoke("mark_played", { chat: message.chat, id: message.id, sender: message.sender }).catch(
      () => {},
    );
  }

  /** Queues the note after `finished`; true when one was found to play next. */
  playNextVoice(finished: StoredMessage): boolean {
    const at = this.ordered.findIndex((m) => m.id === finished.id);
    const next = this.ordered.slice(at + 1).find((m) => m.media_kind === "audio" && m.media_path);
    this.autoplayId = next?.id ?? null;
    return !!next;
  }

  settleRecall() {
    const waiters = this.recallWaiters;
    this.recallWaiters = [];
    for (const done of waiters) done();
  }

  /** Asks the phone for the chat's previous day and waits until it has landed or given up. */
  recallDay(chat: string | null, el: HTMLDivElement | null = null): Promise<void> {
    if (!chat || this.olderExhausted) return Promise.resolve();
    const done = new Promise<void>((resolve) => this.recallWaiters.push(resolve));
    if (!this.loadingOlder) void this.loadOlder(chat, false, el);
    return done;
  }

  async loadOlder(chat: string | null, auto = false, el: HTMLDivElement | null = null) {
    if (!chat || chat !== this.chat || this.loadingOlder) return;
    this.loadingOlder = true;
    try {
      const added = await this.loadLocalOlder(chat, el);
      if (added === null) return;
      if (added > 0) {
        this.loadingOlder = false;
        await this.flushRefresh(chat, el);
        this.settleRecall();
        return;
      }
      const oldest = this.messages.at(-1)?.timestamp ?? Math.floor(Date.now() / 1000);
      this.recall = { chat, until: oldest - 86_400, rounds: 0, auto };
      await this.requestOlder(chat);
    } catch (e) {
      if (chat !== this.chat) return;
      this.loadingOlder = false;
      await this.flushRefresh(chat, el);
      this.settleRecall();
      ui.fail(e);
    }
  }

  async finishOlder(chat: string, el: HTMLDivElement | null) {
    if (chat !== this.chat) return;
    clearTimeout(this.olderTimer);
    try {
      const added = await this.loadLocalOlder(chat, el);
      if (added !== null) {
        this.loadingOlder = false;
        await this.flushRefresh(chat, el);
        this.continueRecall(chat, added);
      }
    } catch (e) {
      if (chat !== this.chat) return;
      this.loadingOlder = false; this.recall = null; this.settleRecall(); ui.fail(e);
    }
  }

  async requestOlder(chat: string) {
    if (!this.recall || chat !== this.chat) return;
    const request = this.recall;
    this.loadingOlder = true;
    const auto = this.recall.auto;
    // The phone answers asynchronously, or not at all when it has nothing
    // older or is offline, so the spinner gives up on its own.
    clearTimeout(this.olderTimer);
    this.olderTimer = setTimeout(() => {
      if (this.recall !== request) return;
      this.loadingOlder = false;
      this.historyActive = false;
      this.recall = null;
      this.olderExhausted = true;
      void this.flushRefresh(chat);
      if (!auto) ui.fail("Your phone did not answer. It has to be online for older messages to load.");
      this.settleRecall();
    }, 15000);
    try {
      this.historyActive = true;
      await invoke("load_older", { chat, count: 50 });
    } catch (e) {
      if (this.recall !== request) return;
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
  prepareChat(chat: string, limit = DEFAULT_MESSAGE_WINDOW) {
    this.chat = chat;
    this.messageLimit = limit;
    this.window = new MessageWindow(limit);
    this.rowRequests.clear();
    this.messages = [];
    this.atLatest = true;
    this.refreshPending = false;
    this.marksSeq++;
    this.messagesSeq++;
    this.recall = null;
    this.loadingOlder = false;
    this.historyActive = false;
    clearTimeout(this.olderTimer);
    this.settleRecall();
    this.olderExhausted = false;
    this.loadOnScroll = true;
    this.autoplayId = null;
    this.revealedOnce = {};
    // The previous chat's divider must not flash over the new rows.
    this.firstUnreadId = null;
    this.lastUnreadId = null;
  }

  /** Mirrors resetUi: the list and the mention queue are dropped. */
  resetAccount() {
    this.accountSeq++;
    this.chat = null;
    this.refreshPending = false;
    this.marksSeq++;
    this.messagesSeq++;
    this.window.evict();
    this.rowRequests.clear();
    this.recall = null;
    this.loadingOlder = false;
    this.historyActive = false;
    clearTimeout(this.olderTimer);
    this.settleRecall();
    this.messages = [];
    this.marks = structuredClone(NO_MARKS);
    this.downloadErrors = {};
    this.downloadTries = {};
    this.downloading = {};
    this.recovering = {};
    this.mentionQueue = [];
    this.revealedOnce = {};
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
