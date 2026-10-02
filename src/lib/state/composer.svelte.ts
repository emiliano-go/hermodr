// Composer domain: drafts, staged files, autocomplete, the send pipeline and
// per-chat send privacy. Moved out of +page.svelte. Reads chats (selection),
// messages (reloads, history source), members (roster, names), session
// (settings) and ui (notices); element access (focus, scrolling) arrives via
// host callbacks the route registers, so flows move verbatim.
import { tick } from "svelte";
import { invoke } from "$lib/utils/ipc";
import { broadcastSendReason, guardBroadcastSend } from "$lib/utils/broadcast";
import { loadEmojis, rememberEmoji, searchEmojis, type Emoji } from "$lib/utils/emoji";
import type { PickerTab } from "$lib/composer/ExpressionPicker.svelte";
import { base64Of, imagePreview, rasterizeSvg } from "$lib/utils/files";
import { cancelStagedAttachment, isAlbumMedia, isAlbumSelection, sendAttachment, stageAttachment } from "$lib/utils/upload";
import { canChooseMediaQuality } from "$lib/utils/media-quality";
import { keybinds, matches, matchesDraftHistory } from "$lib/utils/keybinds.svelte";
import { ComposerHistory, type DraftSnapshot } from "$lib/utils/composer-history";
import type { Recording } from "$lib/composer/VoiceRecorder.svelte";
import type { AttachmentRecovery, AttachmentRetryContext, ChatPrivacy, Outgoing, PendingMedia, StoredMessage } from "$lib/utils/models";
import type { AlbumSendResult } from "$lib/utils/wire";
import { isUnavailable } from "$lib/utils/message";
import { chats } from "./chats.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";
import { scheduled } from "./scheduled.svelte";

export class ComposerState {
  private draftText = $state("");
  private draftUndo = new ComposerHistory();
  private undoChat: string | null = null;
  private beforeInput: { snapshot: DraftSnapshot; kind: string } | null = null;

  get draft() { return this.draftText; }
  set draft(text: string) {
    const before = this.beforeInput?.snapshot ?? this.draftSnapshot();
    const kind = this.beforeInput?.kind ?? "";
    this.beforeInput = null;
    this.draftText = text;
    if (chats.selectedChat) this.drafts[chats.selectedChat] = text;
    if (this.undoChat !== chats.selectedChat) this.resetUndo();
    else this.draftUndo.record(before, this.draftSnapshot(), kind);
  }
  /** Per-chat composer text, so switching chats does not lose what was typed. */
  drafts: Record<string, string> = $state({});
  replyingTo = $state<StoredMessage | null>(null);
  /** Our own message being edited in the composer, if any. */
  editing = $state<{ chat: string; id: string; original: string } | null>(null);

  /** Files staged for review before they are sent, shown above the composer. */
  private pendingItems = $state<PendingMedia[]>([]);
  private stagingTickets = new Map<number, object>();
  get pending() { return this.pendingItems; }
  set pending(items: PendingMedia[]) {
    if (items.length === 0) this.stagingTickets.clear();
    const ids = new Set(items.map((item) => item.id));
    for (const item of this.pendingItems) {
      if (!ids.has(item.id)) this.stagingTickets.delete(item.id);
    }
    this.pendingItems = items;
  }
  pendingSeq = 0;
  attachmentRecoveries = $state<AttachmentRecovery[]>([]);
  private attachmentBatchSeq = 0;
  private attachmentChatGenerations = new Map<string, number>();
  recording = $state(false);
  outgoing = $state<Outgoing[]>([]);

  /** Texts we sent this session, newest first, recalled with the history keybind. */
  sentHistory = $state<string[]>([]);
  /** Position while recalling `sentHistory`; -1 means not browsing. */
  historyIndex = $state(-1);
  /** The draft to restore when arrowing forward past the newest sent message. */
  historyDraft = "";

  /** Open mention query, or null while the autocomplete is closed. */
  mentionQuery = $state<string | null>(null);
  mentionIndex = $state(0);
  /** Mentions picked from the autocomplete, used to convert the text on send. */
  chosenMentions = $state<{ name: string; jid: string }[]>([]);

  // `:name` completion, as Discord does it. Replaced once, then read only.
  emojiTable = $state.raw<Emoji[]>([]);
  emojiToken = $state<{ query: string; start: number } | null>(null);
  emojiIndex = $state(0);
  pickerTab = $state<PickerTab | null>(null);

  chatPrivacy = $state<ChatPrivacy>({ send_typing: null, send_receipts: null });

  // Our own typing: announced at most every five seconds, withdrawn after four
  // idle ones or when the message goes out.
  typingSentAt = 0;
  typingIdle: ReturnType<typeof setTimeout> | undefined = undefined;

  /**
   * Every outgoing message goes through here, one at a time, so they reach the
   * chat in the order they were sent even when an earlier one is slow.
   */
  outbox: Promise<unknown> = Promise.resolve();
  private accountSeq = 0;
  private uploadsAbort = new AbortController();

  /** The textarea element, synced from the route; read at event time only. */
  inputEl: HTMLTextAreaElement | undefined = undefined;
  /** View callbacks the route registers (focus, scrolling). */
  host: { scrollToBottom(): void; focusComposer(): void } = {
    scrollToBottom() {},
    focusComposer() {},
  };

  private draftSnapshot(): DraftSnapshot {
    return { text: this.draft, start: this.inputEl?.selectionStart ?? this.draft.length,
      end: this.inputEl?.selectionEnd ?? this.draft.length,
      mentions: this.chosenMentions.map((mention) => ({ ...mention })) };
  }

  onComposerBeforeInput(event: InputEvent) {
    this.beforeInput = { snapshot: this.draftSnapshot(), kind: event.inputType };
  }

  resetUndo() {
    this.draftUndo.reset();
    this.beforeInput = null;
    this.undoChat = chats.selectedChat;
  }

  async undoDraft(redo = false) {
    const chat = chats.selectedChat;
    if (this.undoChat !== chat) { this.resetUndo(); return; }
    const snapshot = redo ? this.draftUndo.redo(this.draftSnapshot()) : this.draftUndo.undo(this.draftSnapshot());
    if (!snapshot) return;
    this.beforeInput = null;
    this.draftText = snapshot.text;
    this.chosenMentions = snapshot.mentions;
    if (chat) this.drafts[chat] = snapshot.text;
    this.mentionQuery = null;
    this.emojiToken = null;
    this.resetHistory();
    await tick();
    if (chats.selectedChat !== chat || this.draft !== snapshot.text) return;
    this.inputEl?.focus();
    this.inputEl?.setSelectionRange(snapshot.start, snapshot.end);
  }

  /**
   * Autocomplete rows for the `@` being typed, capped at eight. A member is
   * found by display name, by one of their local aliases, by their reserved
   * username, or by number. `token` is what choosing the row puts in the draft:
   * the alias that matched when one did, otherwise the name. Both are
   * convertible on send, so the row reads the way the user typed it.
   */
  mentionMatches = $derived.by(() => {
    const query = this.mentionQuery;
    if (query === null) return [];
    const needle = query.toLowerCase();
    // `@all` is a group mention (respects mutes); `@all-override` also lists
    // every member, which notifies them even with the chat muted.
    const all = chats.selectedChat?.endsWith("@g.us")
      ? [
          { jid: "@all", name: "all", username: null, number: null, aliases: [], token: "all" },
          {
            jid: "@all-override",
            name: "all-override",
            username: null,
            number: null,
            aliases: [],
            token: "all-override",
          },
        ]
      : [];
    const memberList = members.participants.map((p) => {
      const name = members.displayName(p.name, p.jid);
      return {
        jid: p.jid,
        name,
        username: p.username,
        number: p.number,
        aliases: members.aliasesFor(p.jid),
        token: name,
      };
    });
    return [...all, ...memberList]
      .filter((p) =>
        [p.name, p.username, p.number, ...p.aliases].some((f) => f?.toLowerCase().includes(needle)),
      )
      .map((p) => ({
        ...p,
        token: p.aliases.find((a) => a.toLowerCase().includes(needle)) ?? p.name.replace(/^@/, ""),
      }))
      .slice(0, 8);
  });

  emojiMatches = $derived(
    this.emojiToken ? searchEmojis(this.emojiTable, this.emojiToken.query, 12) : [],
  );

  chatSendsTyping = $derived(this.chatPrivacy.send_typing ?? session.settings.send_typing);
  chatSendsReceipts = $derived(this.chatPrivacy.send_receipts ?? session.settings.send_receipts);
  typingHidden = $derived(!this.chatSendsTyping);
  receiptsHidden = $derived(!this.chatSendsReceipts);

  enqueue<T>(task: (signal: AbortSignal) => Promise<T>, account = this.accountSeq): Promise<T> {
    const signal = this.uploadsAbort.signal;
    const start = () => {
      if (account !== this.accountSeq) throw new Error("Account changed before sending");
      return task(signal);
    };
    const run = this.outbox.then(start, start);
    this.outbox = run.catch(() => {});
    return run;
  }

  noteUploadProgress(token: string, sent: number, total: number) {
    const upload = this.outgoing.find((o) => o.token === token);
    if (upload) upload.progress = total > 0 ? sent / total : 0;
  }

  /** Sets one of the two per-chat privacy overrides, dropping it when it matches the default. */
  async setChatPrivacy(patch: Partial<ChatPrivacy>) {
    const chat = chats.selectedChat;
    if (!chat) return;
    const next: ChatPrivacy = {
      send_typing: patch.send_typing ?? this.chatPrivacy.send_typing,
      send_receipts: patch.send_receipts ?? this.chatPrivacy.send_receipts,
    };
    if (next.send_typing === session.settings.send_typing) next.send_typing = null;
    if (next.send_receipts === session.settings.send_receipts) next.send_receipts = null;
    try {
      await invoke("set_chat_privacy", {
        chat,
        typing: next.send_typing,
        receipts: next.send_receipts,
      });
      if (chats.selectedChat === chat) this.chatPrivacy = next;
      if (!(next.send_typing ?? session.settings.send_typing)) this.stopTyping(chat);
    } catch (e) {
      ui.fail(e);
    }
  }

  toggleChatTyping() {
    void this.setChatPrivacy({ send_typing: this.typingHidden });
  }

  toggleChatReceipts() {
    void this.setChatPrivacy({ send_receipts: this.receiptsHidden });
  }

  reportTyping() {
    const chat = chats.selectedChat;
    if (!chat || !this.chatSendsTyping || broadcastSendReason(chat)) return;
    if (Date.now() - this.typingSentAt > 5000) {
      this.typingSentAt = Date.now();
      invoke("send_typing", { chat, typing: true }).catch(() => {});
    }
    clearTimeout(this.typingIdle);
    this.typingIdle = setTimeout(() => this.stopTyping(chat), 4000);
  }

  stopTyping(chat = chats.selectedChat) {
    clearTimeout(this.typingIdle);
    if (!chat || !this.typingSentAt) return;
    this.typingSentAt = 0;
    if (broadcastSendReason(chat)) return;
    invoke("send_typing", { chat, typing: false }).catch(() => {});
  }

  /** The `@…` token immediately before the caret, if the user is typing one. */
  currentMentionQuery(): { query: string; start: number } | null {
    const input = this.inputEl;
    if (!input) return null;
    const caret = input.selectionStart ?? this.draft.length;
    const before = this.draft.slice(0, caret);
    const at = before.lastIndexOf("@");
    if (at === -1) return null;
    if (at > 0 && !/\s/.test(before[at - 1])) return null;
    const query = before.slice(at + 1);
    if (/\s/.test(query)) return null;
    return { query, start: at };
  }

  /** A `:word` right before the caret, at least two letters long. */
  currentEmojiQuery() {
    const caret = this.inputEl?.selectionStart ?? this.draft.length;
    const match = /(?:^|\s)(:([a-z0-9_+-]{2,}))$/i.exec(this.draft.slice(0, caret));
    return match ? { query: match[2], start: caret - match[1].length } : null;
  }

  /** Puts text at the caret, or in place of the characters from `start` to it. */
  async insertAtCaret(text: string, start?: number) {
    const input = this.inputEl;
    const caret = input?.selectionStart ?? this.draft.length;
    const from = start ?? caret;
    this.draft = this.draft.slice(0, from) + text + this.draft.slice(start === undefined ? input?.selectionEnd ?? caret : caret);
    await tick();
    const position = from + text.length;
    input?.focus();
    input?.setSelectionRange(position, position);
  }

  selectEmoji(emoji: string) {
    const token = this.emojiToken;
    this.emojiToken = null;
    rememberEmoji(emoji);
    void this.insertAtCaret(emoji, token?.start);
  }

  onComposerInput(event: Event) {
    this.draft = (event.currentTarget as HTMLTextAreaElement).value;
    // Typing ends a history recall, so the next Up starts from the newest again.
    this.historyIndex = -1;
    if (this.draft.trim()) this.reportTyping();
    else this.stopTyping();
    // A complete `:shortcode:` turns into its emoji the moment it is closed.
    const caret = this.inputEl?.selectionStart ?? this.draft.length;
    const closed = /(?:^|\s)(:([a-z0-9_+-]+):)$/i.exec(this.draft.slice(0, caret));
    const exact =
      closed && this.emojiTable.find((e) => e.shortcodes.includes(closed[2].toLowerCase()));
    if (closed && exact) {
      this.emojiToken = null;
      rememberEmoji(exact.emoji);
      void this.insertAtCaret(exact.emoji, caret - closed[1].length);
      return;
    }
    this.emojiToken = this.currentEmojiQuery();
    this.emojiIndex = 0;
    if ((this.emojiToken || this.draft.includes(":")) && this.emojiTable.length === 0) {
      loadEmojis().then((list) => (this.emojiTable = list));
    }
    const token = this.currentMentionQuery();
    if (token && members.participants.length > 0) {
      this.mentionQuery = token.query;
      this.mentionIndex = 0;
    } else {
      this.mentionQuery = null;
    }
  }

  /** Puts a chosen mention in the draft and remembers the token to convert on send. */
  async selectMention(person: { jid: string; name: string; token: string }) {
    const input = this.inputEl;
    const token = this.currentMentionQuery();
    if (!input || !token) return;
    const caret = input.selectionStart ?? this.draft.length;
    this.draft = this.draft.slice(0, token.start) + `@${person.token} ` + this.draft.slice(caret);
    this.mentionQuery = null;
    // Deduped by the text that went in, not by the person: the same contact
    // addressed once by name and once by alias leaves two tokens, and dropping
    // either one would send it as plain text.
    if (!this.chosenMentions.some((m) => m.name === person.token)) {
      this.chosenMentions = [...this.chosenMentions, { name: person.token, jid: person.jid }];
    }
    await tick();
    const position = token.start + person.token.length + 2;
    input.focus();
    input.setSelectionRange(position, position);
  }

  /**
   * Loads a chosen message into the composer for editing, defaulting to our
   * last editable text message. The loaded list is newest-first, so the first
   * match is the most recent one.
   */
  startEditing(message?: StoredMessage) {
    const chat = chats.selectedChat;
    if (!chat) return;
    const candidate =
      message ??
      messages.messages.find(
        (m) => m.from_me && !m.media_kind && !m.deleted && m.text.trim() && !m.revoked && !isUnavailable(m),
      );
    if (!candidate || isUnavailable(candidate)) return;
    const reason = broadcastSendReason(chat) ?? broadcastSendReason(candidate.chat);
    if (reason) { ui.fail(reason); return; }
    this.editing = { chat, id: candidate.id, original: candidate.text };
    this.replyingTo = null;
    this.draft = candidate.text;
    this.resetUndo();
    this.resetHistory();
    this.host.focusComposer();
  }

  cancelEditing() {
    if (!this.editing) return;
    this.editing = null;
    this.draft = "";
    this.resetUndo();
    this.host.focusComposer();
  }

  resetHistory() {
    this.historyIndex = -1;
    this.historyDraft = "";
  }

  /** Recalls older sent messages; returns false to leave the caret alone. */
  recallPrev(): boolean {
    if (this.sentHistory.length === 0) return false;
    if (this.historyIndex === -1) {
      if (this.draft !== "" || this.editing) return false;
      this.historyDraft = this.draft;
    }
    this.historyIndex = Math.min(this.historyIndex + 1, this.sentHistory.length - 1);
    this.draft = this.sentHistory[this.historyIndex];
    return true;
  }

  recallNext(): boolean {
    if (this.historyIndex === -1) return false;
    this.historyIndex -= 1;
    this.draft = this.historyIndex === -1 ? this.historyDraft : this.sentHistory[this.historyIndex];
    return true;
  }

  onComposerKey(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (matchesDraftHistory(event, "undoDraft") || matchesDraftHistory(event, "redoDraft")) {
      event.preventDefault();
      void this.undoDraft(matchesDraftHistory(event, "redoDraft"));
      return;
    }
    if (this.emojiToken && this.emojiMatches.length > 0) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        this.emojiIndex = (this.emojiIndex + step + this.emojiMatches.length) % this.emojiMatches.length;
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        this.selectEmoji(this.emojiMatches[this.emojiIndex].emoji);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        this.emojiToken = null;
        return;
      }
    }
    if (this.mentionQuery !== null && this.mentionMatches.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        this.mentionIndex = (this.mentionIndex + 1) % this.mentionMatches.length;
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        this.mentionIndex =
          (this.mentionIndex - 1 + this.mentionMatches.length) % this.mentionMatches.length;
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        void this.selectMention(this.mentionMatches[this.mentionIndex]);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        this.mentionQuery = null;
        return;
      }
    }
    // Configurable composer shortcuts, only once the popups above are out of the way.
    if (matches(event, keybinds.cancelReply)) {
      if (this.editing || this.replyingTo) {
        event.preventDefault();
        if (this.editing) this.cancelEditing();
        else this.replyingTo = null;
      }
      return;
    }
    if (matches(event, keybinds.editLast)) {
      event.preventDefault();
      this.startEditing();
      return;
    }
    if (matches(event, keybinds.historyPrev) && this.recallPrev()) {
      event.preventDefault();
      return;
    }
    if (matches(event, keybinds.historyNext) && this.recallNext()) {
      event.preventDefault();
      return;
    }
    // Enter sends; Shift+Enter keeps the newline the textarea just added.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void this.send();
    }
  }

  /**
   * Turns the display text into wire text, naming mentions by number. A
   * mention goes out as `@<number>`, which every client renders as the
   * contact's real name, so neither an alias nor a display name ever leaves
   * this machine as itself.
   */
  mentionPayload() {
    let text = this.draft.trim();
    const jids: string[] = [];
    for (const mention of this.chosenMentions) {
      const token = `@${mention.name}`;
      if (!text.includes(token)) continue;
      if (mention.jid === "@all") {
        jids.push("@all");
        continue;
      }
      if (mention.jid === "@all-override") {
        // Everyone is also listed explicitly, which bypasses their mute.
        text = text.replace("@all-override", "@all");
        jids.push("@all");
        for (const person of members.participants) jids.push(person.jid);
        continue;
      }
      const user = mention.jid.split("@")[0].split(":")[0];
      text = text.replace(token, `@${user}`);
      jids.push(mention.jid);
    }
    // An alias typed without picking it from the list still means the mention,
    // so it is converted the same way.
    for (const { alias, jid } of members.groupAliases) {
      const token = `@${alias}`;
      if (!text.includes(token)) continue;
      text = text.split(token).join(`@${jid.split("@")[0]}`);
      jids.push(jid);
    }
    // The same person reached twice, once by name and once by alias, is still
    // one notified participant.
    return { text, jids: [...new Set(jids)] };
  }

  async schedule(dueAt: number): Promise<boolean> {
    const chat = chats.selectedChat;
    const account = session.activeAccount;
    if (!chat || !account || this.editing || this.replyingTo || this.pending.length || this.recording || !this.draft.trim()) return false;
    const typed = this.draft;
    const sequence = this.accountSeq;
    const { text, jids } = this.mentionPayload();
    try {
      guardBroadcastSend(chat);
      await invoke("schedule_message", { account, chat, text, mentions: jids, dueAt });
      if (sequence !== this.accountSeq) return true;
      if (chat === chats.selectedChat && typed === this.draft) {
        this.draft = "";
        delete this.drafts[chat];
        this.chosenMentions = [];
        this.mentionQuery = null;
        this.resetUndo();
        this.stopTyping(chat);
      }
      scheduled.selectAccount(account);
      await scheduled.refresh(account);
      return true;
    } catch (error) {
      if (sequence === this.accountSeq) ui.fail(error);
      return false;
    }
  }

  async send() {
    const selectedChat = chats.selectedChat;
    if (!selectedChat) return;
    const reason = broadcastSendReason(this.editing?.chat ?? selectedChat);
    if (reason) { ui.fail(reason); return; }
    // Editing replaces an existing message rather than sending a new one.
    if (this.editing) {
      const current = this.editing;
      const text = this.draft.trim();
      if (!text) return;
      this.draft = "";
      delete this.drafts[selectedChat];
      this.editing = null;
      this.resetUndo();
      this.stopTyping();
      this.resetHistory();
      this.host.focusComposer();
      try {
        await this.enqueue(() => {
          guardBroadcastSend(current.chat);
          return invoke("edit_message", { chat: current.chat, id: current.id, text });
        });
        await messages.reloadMessages(chats.selectedChat);
        await chats.refreshChats();
      } catch (e) {
        ui.fail(e);
      }
      return;
    }
    // With attachments staged, the typed text goes out as their caption.
    if (this.pending.length > 0) {
      if (this.pending[0].retry) {
        await this.sendPending();
        return;
      }
      // Mentions in a caption go out as `@<number>` with their JIDs, as in text.
      const { text: caption, jids } = this.mentionPayload();
      this.draft = "";
      delete this.drafts[selectedChat];
      this.chosenMentions = [];
      this.mentionQuery = null;
      this.resetUndo();
      this.stopTyping();
      await this.sendPending(caption, jids.filter((j) => j !== "@all"));
      return;
    }
    if (!this.draft.trim()) return;
    const chat = selectedChat;
    this.stopTyping();
    const typed = this.draft;
    const { text, jids } = this.mentionPayload();
    const reply = this.replyingTo;
    this.draft = "";
    delete this.drafts[chat];
    this.replyingTo = null;
    this.chosenMentions = [];
    this.mentionQuery = null;
    this.resetUndo();
    // Keep the typed text for the history keybind, newest first, without dupes.
    this.sentHistory = [typed, ...this.sentHistory.filter((t) => t !== typed)].slice(0, 100);
    this.resetHistory();
    this.host.focusComposer();
    try {
      await this.enqueue(() => {
        guardBroadcastSend(chat);
        return reply
          ? invoke("send_reply", {
              chat,
              text,
              replyToId: reply.id,
              replyToSender: reply.sender,
              replyToText: reply.text,
              mentions: jids,
              // A group message answered privately quotes across chats.
              replyToChat: reply.chat,
            })
          : invoke("send_text", { chat, text, mentions: jids });
      });
      await messages.reloadMessages(chats.selectedChat);
      await chats.refreshChats();
      this.host.scrollToBottom();
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Stages a file for review rather than sending it straight away. */
  async stageFile(file: File) {
    const account = this.accountSeq, owner = session.activeAccount;
    const chat = chats.selectedChat, generation = messages.accountGeneration;
    if (!owner || !chat) return;
    const reason = broadcastSendReason(chat);
    if (reason) { ui.fail(reason); return; }
    const id = this.pendingSeq++, ticket = {};
    this.stagingTickets.set(id, ticket);
    const current = () => account === this.accountSeq && owner === session.activeAccount
      && chat === chats.selectedChat && generation === messages.accountGeneration
      && this.stagingTickets.get(id) === ticket;
    try {
      if (file.type === "image/svg+xml" || /\.svg$/i.test(file.name)) file = await rasterizeSvg(file);
      if (!current()) return;
      const kind = file.type.startsWith("image/")
        ? "image"
        : file.type.startsWith("video/")
          ? "video"
          : "other";

      // Staged before the preview is drawn, so Enter can send it straight away.
      const items = [...this.pending];
      const next = items.findIndex((item) => item.id > id);
      items.splice(next < 0 ? items.length : next, 0,
        { id, file, url: kind === "video" ? URL.createObjectURL(file) : "", kind, caption: "", once: false,
          quality: canChooseMediaQuality(file) ? session.settings.media_quality : undefined });
      this.pending = items;
      this.host.focusComposer();
      if (kind === "image") {
        const url = await imagePreview(file);
        if (!current() || !this.pending.some((item) => item.id === id && item.file === file)) {
          if (url.startsWith("blob:")) URL.revokeObjectURL(url);
          return;
        }
        this.pending = this.pending.map((p) => (p.id === id ? { ...p, url } : p));
      }
    } catch (e) {
      // Staging must never take the chat down with it.
      if (current()) ui.fail(`Could not preview that file: ${e}`);
    } finally {
      if (this.stagingTickets.get(id) === ticket) this.stagingTickets.delete(id);
    }
  }

  /** `text` from the composer becomes the first attachment's caption, unless it has its own. */
  async sendPending(text = "", mentions: string[] = []) {
    const account = this.accountSeq, owner = session.activeAccount, chat = chats.selectedChat;
    if (!chat || this.pending.length === 0) return;
    guardBroadcastSend(chat);
    const retry = this.pending[0].retry;
    if (retry && (!this.attachmentScopeCurrent(retry) || retry.chat !== chat)) {
      ui.fail("These attachments belong to another conversation or account.");
      return;
    }
    const items = (retry ? this.pending.filter((item) => item.retry?.batch === retry.batch) : this.pending)
      .map((item) => ({ ...item }));
    const ordinary = items.every(isAlbumMedia);
    if (retry?.album && !ordinary) {
      ui.fail("Album retries require ordinary photos and videos.");
      return;
    }
    const album = !!owner && (retry ? retry.album && ordinary && items.length <= 8
      && (items.length >= 2 || !!retry.parentId) : isAlbumSelection(items));
    const captioned = !retry && !!text && !items[0].caption.trim();
    if (captioned) items[0].caption = text;
    const firstId = items[0].id;
    const context: AttachmentRetryContext = retry ? { ...retry, album } : {
      accountId: owner ?? "", accountSeq: account, generation: messages.accountGeneration, chat,
      chatGeneration: this.attachmentChatGenerations.get(chat) ?? 0,
      batch: `attachments-${account}-${this.attachmentBatchSeq++}`, album, parentId: null,
      reply: this.replyingTo ? { id: this.replyingTo.id, sender: this.replyingTo.sender, text: this.replyingTo.text } : null,
      mentions: captioned ? [...mentions] : [],
    };
    const reply = context.reply, signal = this.uploadsAbort.signal;
    // The tray empties at once; each file waits in the chat as a bubble instead.
    this.pending = retry ? this.pending.filter((item) => item.retry?.batch !== retry.batch) : [];
    if (!retry) this.replyingTo = null;
    const batch: Outgoing[] = items.map((item) => ({
      token: `upload-${item.id}-${Date.now()}`,
      chat,
      kind: item.kind,
      url: item.url,
      name: item.file.name,
      caption: item.caption.trim(),
      progress: 0,
    }));
    this.outgoing = [...this.outgoing, ...batch];
    this.host.scrollToBottom();
    const finish = (token: string, release = true) => {
      const done = this.outgoing.find((o) => o.token === token);
      if (release && done?.url.startsWith("blob:")) URL.revokeObjectURL(done.url);
      this.outgoing = this.outgoing.filter((o) => o.token !== token);
    };
    await this.enqueue(async () => {
      if (album) {
        await this.sendPendingAlbum(items, batch, context, signal, finish);
        return;
      }
      for (const [i, item] of items.entries()) {
        const { token } = batch[i];
        try {
          guardBroadcastSend(chat);
          const warning = await sendAttachment(item.file, {
            chat,
            caption: item.caption.trim() || null,
            replyToId: reply?.id ?? null,
            replyToSender: reply?.sender ?? null,
            replyToText: reply?.text ?? null,
            viewOnce: item.once,
            mentions: item.id === firstId ? context.mentions : [],
            progress: token,
            quality: item.quality ?? null,
          }, signal);
          if (account !== this.accountSeq) return;
          if (warning && session.settings.warn_missing_video_preview) ui.notify(warning);
          if (chats.selectedChat === chat) await messages.reloadMessages(chat).catch((e) => ui.fail(e));
          finish(token);
          if (chats.selectedChat === chat) this.host.scrollToBottom();
        } catch (e) {
          if (account !== this.accountSeq) return;
          ui.fail(e);
          for (const rest of batch.slice(i)) {
            finish(rest.token, !this.attachmentScopeCurrent(context));
          }
          this.recoverAttachments(context, items.slice(i), [], String(e));
          break;
        }
      }
    }, account).catch((error) => {
      for (const item of batch) finish(item.token, !this.attachmentScopeCurrent(context));
      this.recoverAttachments(context, items, [], String(error));
    });
    if (this.attachmentScopeCurrent(context)) await chats.refreshChats();
  }

  private attachmentScopeCurrent(context: AttachmentRetryContext) {
    return context.accountSeq === this.accountSeq && context.accountId === (session.activeAccount ?? "")
      && context.generation === messages.accountGeneration
      && context.chatGeneration === (this.attachmentChatGenerations.get(context.chat) ?? 0);
  }

  get currentAttachmentRecoveries() {
    return this.attachmentRecoveries.filter((recovery) => recovery.context.chat === chats.selectedChat
      && this.attachmentScopeCurrent(recovery.context));
  }

  restoreKnownUnsent(chat: string) {
    if (chat !== chats.selectedChat) return false;
    const restored: PendingMedia[] = [], remaining: AttachmentRecovery[] = [];
    for (const recovery of this.attachmentRecoveries) {
      if (recovery.context.chat !== chat || !this.attachmentScopeCurrent(recovery.context) || !recovery.retryable.length) {
        remaining.push(recovery);
        continue;
      }
      restored.push(...recovery.retryable);
      if (recovery.uncertain.length || recovery.parentUncertain) remaining.push({ ...recovery, retryable: [] });
    }
    if (!restored.length) return false;
    this.attachmentRecoveries = remaining;
    this.pending = [...restored, ...this.pending];
    return true;
  }

  discardAttachmentRecovery(recovery: AttachmentRecovery) {
    if (!this.attachmentRecoveries.includes(recovery) || !this.attachmentScopeCurrent(recovery.context)
      || recovery.context.chat !== chats.selectedChat) return;
    for (const item of [...recovery.retryable, ...recovery.uncertain]) {
      if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    }
    this.attachmentRecoveries = this.attachmentRecoveries.filter((item) => item !== recovery);
  }

  forgetRecovery(chat: string) {
    this.attachmentChatGenerations.set(chat, (this.attachmentChatGenerations.get(chat) ?? 0) + 1);
    for (const item of [...this.pending]) {
      if (item.retry?.chat === chat) this.removePending(item.id);
    }
    for (const recovery of this.attachmentRecoveries) {
      if (recovery.context.chat !== chat) continue;
      for (const item of [...recovery.retryable, ...recovery.uncertain]) {
        if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
      }
    }
    this.attachmentRecoveries = this.attachmentRecoveries.filter((item) => item.context.chat !== chat);
  }

  private recoverAttachments(context: AttachmentRetryContext, retryable: PendingMedia[], uncertain: PendingMedia[],
    error: string, result?: AlbumSendResult) {
    if (!this.attachmentScopeCurrent(context)) return;
    this.attachmentRecoveries = [...this.attachmentRecoveries, {
      context, retryable: retryable.map((item) => ({ ...item, retry: context })), uncertain,
      sentIds: result?.sent_ids ?? [], uncertainId: result?.uncertain_id ?? null,
      parentUncertain: result?.parent_uncertain ?? false, error,
    }];
    this.restoreKnownUnsent(context.chat);
    if (chats.selectedChat === context.chat) ui.fail(error);
  }

  private async sendPendingAlbum(items: PendingMedia[], batch: Outgoing[], context: AttachmentRetryContext,
    signal: AbortSignal, finish: (token: string, release?: boolean) => void) {
    const uploads: string[] = [];
    let dispatched = false;
    try {
      for (const item of items) {
        guardBroadcastSend(context.chat);
        if (!this.attachmentScopeCurrent(context) || chats.selectedChat !== context.chat) throw new Error("Conversation changed before album dispatch.");
        uploads.push(await stageAttachment(item.file, signal, context.accountId));
      }
      signal.throwIfAborted();
      guardBroadcastSend(context.chat);
      if (!this.attachmentScopeCurrent(context) || chats.selectedChat !== context.chat) throw new Error("Conversation changed before album dispatch.");
      dispatched = true;
      const result = await invoke<AlbumSendResult>("send_album", {
        accountId: context.accountId, chat: context.chat, parentId: context.parentId,
        items: items.map((item, i) => ({ upload: uploads[i], caption: item.caption.trim() || null,
          quality: item.quality ?? null, progress: batch[i].token })),
        replyToId: context.reply?.id ?? null, replyToSender: context.reply?.sender ?? null,
        replyToText: context.reply?.text ?? null, mentions: context.mentions,
      });
      const preflight = result && "preflight_failed" in result && result.preflight_failed === true;
      const uncertain = result?.uncertain_index;
      if (!result || result.account_id !== context.accountId || result.chat !== context.chat
        || !Array.isArray(result.sent_ids) || !result.sent_ids.every((id) => typeof id === "string" && id.length > 0)
        || new Set(result.sent_ids).size !== result.sent_ids.length
        || !Array.isArray(result.warnings) || !result.warnings.every((warning) => typeof warning === "string")
        || typeof result.parent_uncertain !== "boolean" || (result.error !== null && typeof result.error !== "string")
        || (result.uncertain_id !== null && (typeof result.uncertain_id !== "string" || !result.uncertain_id))
        || (uncertain === null && result.uncertain_id !== null)
        || !Number.isInteger(result.next_index) || result.next_index < 0 || result.next_index > items.length
        || (uncertain !== null && (!Number.isInteger(uncertain) || uncertain < 0 || uncertain >= items.length
          || uncertain !== result.sent_ids.length || result.next_index !== uncertain + 1 || !result.uncertain_id))
        || (uncertain === null && result.next_index !== result.sent_ids.length)
        || (preflight && (result.next_index !== 0 || result.parent_uncertain || uncertain !== null))
        || (result.parent_uncertain && (result.next_index !== 0 || uncertain !== null))
        || typeof result.parent_id !== "string" || (!preflight && !result.parent_id)
        || (context.parentId && !preflight && result.parent_id !== context.parentId)) {
        throw new Error("Album response did not match the submitted batch; outcome unknown.");
      }
      if (!this.attachmentScopeCurrent(context)) {
        for (const item of batch) finish(item.token);
        return;
      }
      const nextContext = { ...context, parentId: result.parent_id || context.parentId,
        mentions: result.next_index === 0 ? context.mentions : [] };
      const tail = items.slice(result.next_index);
      const unknown = uncertain === null ? [] : [items[uncertain]];
      const held = new Set([...tail, ...unknown].map((item) => item.id));
      for (const [i, item] of batch.entries()) finish(item.token, !held.has(items[i].id));
      if (tail.length || unknown.length || result.parent_uncertain) {
        this.recoverAttachments(nextContext, tail, unknown,
          result.error ?? "Album outcome requires review before retrying.", result);
      } else if (result.error && chats.selectedChat === context.chat) ui.fail(result.error);
      if (result.warnings.length && chats.selectedChat === context.chat) ui.notify(result.warnings.join("\n"));
    } catch (error) {
      const keep = this.attachmentScopeCurrent(context);
      for (const item of batch) finish(item.token, !keep);
      this.recoverAttachments(context, dispatched ? [] : items, dispatched ? items : [],
        dispatched ? `Album outcome unknown; review before retrying. ${error}` : String(error));
    } finally {
      await Promise.all(uploads.map((token) => cancelStagedAttachment(token, context.accountId)));
    }
    if (this.attachmentScopeCurrent(context) && chats.selectedChat === context.chat) {
      await messages.reloadMessages(context.chat).catch((error) => {
        if (this.attachmentScopeCurrent(context) && chats.selectedChat === context.chat) ui.fail(error);
      });
      if (this.attachmentScopeCurrent(context) && chats.selectedChat === context.chat) this.host.scrollToBottom();
    }
  }

  async sendSoundClip(file: File, scope: { account: string; chat: string; generation: number }) {
    guardBroadcastSend(scope.chat);
    const account = this.accountSeq;
    const current = () => account === this.accountSeq && scope.account === session.activeAccount
      && scope.chat === chats.selectedChat && scope.generation === messages.accountGeneration;
    const allowed = () => current() && session.connected && !this.editing && !this.recording
      && (!members.chatGroup || members.chatGroup.can_send);
    if (!allowed()) throw new Error("Conversation changed or cannot send audio clips.");
    await this.enqueue(async (signal) => {
      guardBroadcastSend(scope.chat);
      if (!allowed()) throw new Error("Conversation changed before sending the audio clip.");
      await sendAttachment(file, { chat: scope.chat }, signal);
    }, account);
    if (!current()) return;
    await Promise.all([messages.reloadMessages(scope.chat), chats.refreshChats()]).catch((error) => {
      if (current()) ui.fail(error);
    });
    if (current()) this.host.scrollToBottom();
  }

  async sendVoice(note: Recording) {
    const account = this.accountSeq;
    const selectedChat = chats.selectedChat;
    if (!selectedChat) return;
    guardBroadcastSend(selectedChat);
    this.recording = false;
    const chat = selectedChat;
    const reply = this.replyingTo;
    this.replyingTo = null;
    try {
      await this.enqueue(async (signal) => {
        guardBroadcastSend(chat);
        const data = await base64Of(note.blob);
        signal.throwIfAborted();
        return invoke("send_voice", {
          chat,
          data,
          seconds: note.seconds,
          waveform: note.waveform,
          replyToId: reply?.id ?? null,
          replyToSender: reply?.sender ?? null,
          replyToText: reply?.text ?? null,
          viewOnce: note.viewOnce,
        });
      }, account);
      if (account !== this.accountSeq) return;
      await messages.reloadMessages(chats.selectedChat);
      await chats.refreshChats();
      this.host.scrollToBottom();
    } catch (e) {
      if (account !== this.accountSeq) return;
      ui.fail(e);
    }
  }

  removePending(id: number) {
    const item = this.pending.find((p) => p.id === id);
    if (item?.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    this.stagingTickets.delete(id);
    this.pendingItems = this.pendingItems.filter((p) => p.id !== id);
  }

  /** Flips one staged attachment between view-once and ordinary. */
  toggleOnce(id: number) {
    this.pending = this.pending.map((p) =>
      p.id === id && p.kind !== "other" ? { ...p, once: !p.once } : p,
    );
  }

  /** Mirrors resetUi: drafts, tray, replies, edits and history are dropped. */
  resetAccount() {
    this.accountSeq++;
    this.stagingTickets.clear();
    for (const recovery of this.attachmentRecoveries) {
      for (const item of [...recovery.retryable, ...recovery.uncertain]) {
        if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
      }
    }
    this.attachmentRecoveries = [];
    this.attachmentChatGenerations.clear();
    this.uploadsAbort.abort();
    this.uploadsAbort = new AbortController();
    this.outbox = Promise.resolve();
    for (const item of [...this.pending, ...this.outgoing]) {
      if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    }
    this.outgoing = [];
    this.draft = "";
    this.drafts = {};
    this.pending = [];
    this.replyingTo = null;
    this.editing = null;
    this.sentHistory = [];
    this.chosenMentions = [];
    this.resetUndo();
    this.resetHistory();
  }
}

export const composer = new ComposerState();
