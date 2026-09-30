// Composer domain: drafts, staged files, autocomplete, the send pipeline and
// per-chat send privacy. Moved out of +page.svelte. Reads chats (selection),
// messages (reloads, history source), members (roster, names), session
// (settings) and ui (notices); element access (focus, scrolling) arrives via
// host callbacks the route registers, so flows move verbatim.
import { tick } from "svelte";
import { invoke } from "$lib/utils/ipc";
import { loadEmojis, rememberEmoji, searchEmojis, type Emoji } from "$lib/utils/emoji";
import type { PickerTab } from "$lib/composer/ExpressionPicker.svelte";
import { base64Of, imagePreview, rasterizeSvg } from "$lib/utils/files";
import { sendAttachment } from "$lib/utils/upload";
import { keybinds, matches } from "$lib/utils/keybinds.svelte";
import type { Recording } from "$lib/composer/VoiceRecorder.svelte";
import type { ChatPrivacy, Outgoing, PendingMedia, StoredMessage } from "$lib/utils/models";
import { chats } from "./chats.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";
import { scheduled } from "./scheduled.svelte";

export class ComposerState {
  draft = $state("");
  /** Per-chat composer text, so switching chats does not lose what was typed. */
  drafts: Record<string, string> = $state({});
  replyingTo = $state<StoredMessage | null>(null);
  /** Our own message being edited in the composer, if any. */
  editing = $state<{ chat: string; id: string; original: string } | null>(null);

  /** Files staged for review before they are sent, shown above the composer. */
  pending = $state<PendingMedia[]>([]);
  pendingSeq = 0;
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
    if (!chat || !this.chatSendsTyping) return;
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
    this.draft = this.draft.slice(0, from) + text + this.draft.slice(caret);
    if (chats.selectedChat) this.drafts[chats.selectedChat] = this.draft;
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
    if (chats.selectedChat) this.drafts[chats.selectedChat] = this.draft;
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
        (m) => m.from_me && !m.media_kind && !m.deleted && m.text.trim() && !m.revoked,
      );
    if (!candidate) return;
    this.editing = { chat, id: candidate.id, original: candidate.text };
    this.replyingTo = null;
    this.draft = candidate.text;
    this.resetHistory();
    this.host.focusComposer();
  }

  cancelEditing() {
    if (!this.editing) return;
    this.editing = null;
    this.draft = "";
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
      await invoke("schedule_message", { account, chat, text, mentions: jids, dueAt });
      if (sequence !== this.accountSeq) return true;
      if (chat === chats.selectedChat && typed === this.draft) {
        this.draft = "";
        delete this.drafts[chat];
        this.chosenMentions = [];
        this.mentionQuery = null;
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
    // Editing replaces an existing message rather than sending a new one.
    if (this.editing) {
      const current = this.editing;
      const text = this.draft.trim();
      if (!text) return;
      this.draft = "";
      delete this.drafts[selectedChat];
      this.editing = null;
      this.stopTyping();
      this.resetHistory();
      this.host.focusComposer();
      try {
        await this.enqueue(() => invoke("edit_message", { chat: current.chat, id: current.id, text }));
        await messages.reloadMessages(chats.selectedChat);
        await chats.refreshChats();
      } catch (e) {
        ui.fail(e);
      }
      return;
    }
    // With attachments staged, the typed text goes out as their caption.
    if (this.pending.length > 0) {
      // Mentions in a caption go out as `@<number>` with their JIDs, as in text.
      const { text: caption, jids } = this.mentionPayload();
      this.draft = "";
      delete this.drafts[selectedChat];
      this.chosenMentions = [];
      this.mentionQuery = null;
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
    // Keep the typed text for the history keybind, newest first, without dupes.
    this.sentHistory = [typed, ...this.sentHistory.filter((t) => t !== typed)].slice(0, 100);
    this.resetHistory();
    this.host.focusComposer();
    try {
      await this.enqueue(() =>
        reply
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
          : invoke("send_text", { chat, text, mentions: jids }),
      );
      await messages.reloadMessages(chats.selectedChat);
      await chats.refreshChats();
      this.host.scrollToBottom();
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Stages a file for review rather than sending it straight away. */
  async stageFile(file: File) {
    try {
      if (file.type === "image/svg+xml" || /\.svg$/i.test(file.name)) file = await rasterizeSvg(file);
      const kind = file.type.startsWith("image/")
        ? "image"
        : file.type.startsWith("video/")
          ? "video"
          : "other";

      // Staged before the preview is drawn, so Enter can send it straight away.
      const id = this.pendingSeq++;
      this.pending = [
        ...this.pending,
        { id, file, url: kind === "video" ? URL.createObjectURL(file) : "", kind, caption: "", once: false },
      ];
      this.host.focusComposer();
      if (kind === "image") {
        const url = await imagePreview(file);
        this.pending = this.pending.map((p) => (p.id === id ? { ...p, url } : p));
      }
    } catch (e) {
      // Staging must never take the chat down with it.
      ui.fail(`Could not preview that file: ${e}`);
    }
  }

  /** `text` from the composer becomes the first attachment's caption, unless it has its own. */
  async sendPending(text = "", mentions: string[] = []) {
    const account = this.accountSeq;
    const signal = this.uploadsAbort.signal;
    const selectedChat = chats.selectedChat;
    if (!selectedChat || this.pending.length === 0) return;
    const captioned = !!text && !this.pending[0].caption.trim();
    if (captioned) this.pending[0].caption = text;
    const firstId = this.pending[0].id;
    const chat = selectedChat;
    const items = [...this.pending];
    const reply = this.replyingTo;
    // The tray empties at once; each file waits in the chat as a bubble instead.
    this.pending = [];
    this.replyingTo = null;
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
    const finish = (token: string) => {
      const done = this.outgoing.find((o) => o.token === token);
      if (done?.url.startsWith("blob:")) URL.revokeObjectURL(done.url);
      this.outgoing = this.outgoing.filter((o) => o.token !== token);
    };
    await this.enqueue(async () => {
      for (const [i, item] of items.entries()) {
        const { token } = batch[i];
        try {
          const warning = await sendAttachment(item.file, {
            chat,
            caption: item.caption.trim() || null,
            replyToId: reply?.id ?? null,
            replyToSender: reply?.sender ?? null,
            replyToText: reply?.text ?? null,
            viewOnce: item.once,
            mentions: captioned && item.id === firstId ? mentions : [],
            progress: token,
          }, signal);
          if (account !== this.accountSeq) return;
          if (warning && session.settings.warn_missing_video_preview) ui.notify(warning);
          if (chats.selectedChat === chat) await messages.reloadMessages(chat).catch((e) => ui.fail(e));
          finish(token);
          if (chats.selectedChat === chat) this.host.scrollToBottom();
        } catch (e) {
          if (account !== this.accountSeq) return;
          ui.fail(e);
          // Unsent files return to the tray for an explicit retry.
          for (const rest of batch.slice(i)) {
            this.outgoing = this.outgoing.filter((o) => o.token !== rest.token);
          }
          this.pending = [...items.slice(i), ...this.pending];
          break;
        }
      }
    }, account).catch((e) => { if (account === this.accountSeq) ui.fail(e); });
    if (account !== this.accountSeq) return;
    await chats.refreshChats();
  }

  async sendVoice(note: Recording) {
    const account = this.accountSeq;
    this.recording = false;
    const selectedChat = chats.selectedChat;
    if (!selectedChat) return;
    const chat = selectedChat;
    const reply = this.replyingTo;
    this.replyingTo = null;
    try {
      await this.enqueue(async (signal) => {
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
    this.pending = this.pending.filter((p) => p.id !== id);
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
    this.resetHistory();
  }
}

export const composer = new ComposerState();
