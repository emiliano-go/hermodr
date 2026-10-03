import { invoke } from "$lib/utils/ipc";
import type { QuickRepliesView, QuickReply } from "$lib/utils/wire";
import type { QuickReplyScope } from "$lib/utils/quick-replies";
import { session } from "./session.svelte";
import { messages } from "./messages.svelte";
import { chats } from "./chats.svelte";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";
import { uiError } from "./localized.ts";

export class QuickRepliesState {
  replies = $state.raw<QuickReply[]>([]);
  loading = $state(false);
  syncing = $state(false);
  #error = $state<LocalizedError | null>(null);
  get error(): string { return this.#error?.message ?? ""; }
  set error(value: unknown) { this.#error = value == null || value === "" ? null : normalizeError(value); }
  get diagnostic() { return this.#error?.diagnostic; }
  key = $state(0);
  private account: string | null = null;
  private generation = -1;
  private request = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;

  reset() {
    ++this.request; ++this.key;
    clearTimeout(this.timer); this.timer = undefined;
    this.account = null; this.generation = -1;
    this.replies = []; this.loading = this.syncing = false; this.error = "";
  }

  scope(chat: string): QuickReplyScope | null {
    return this.account && this.account === session.activeAccount && this.generation === messages.accountGeneration
      ? { account: this.account, chat, generation: this.generation, requestKey: this.key } : null;
  }

  current(scope: QuickReplyScope) {
    if (scope.account !== session.activeAccount || scope.generation !== messages.accountGeneration
      || scope.chat !== chats.selectedChat || scope.requestKey !== this.key) throw uiError("error.state.quick_reply_scope");
  }

  async refresh() {
    const account = session.activeAccount, generation = messages.accountGeneration, request = ++this.request;
    if (!account) { this.reset(); return; }
    const current = () => account === session.activeAccount && generation === messages.accountGeneration && request === this.request;
    if (this.account !== account || this.generation !== generation) this.replies = [];
    this.account = account; this.generation = generation;
    this.loading = true; this.error = "";
    try {
      const view = await invoke<QuickRepliesView>("quick_replies_view", { accountId: account });
      if (!current()) return;
      this.account = account; this.generation = generation; this.replies = view.replies;
    } catch (error) { if (current()) this.error = error; }
    finally { if (current()) this.loading = false; }
  }

  queueRefresh() {
    if (this.timer) return;
    this.timer = setTimeout(() => { this.timer = undefined; void this.refresh(); }, 200);
  }

  async sync(scope: QuickReplyScope) {
    this.current(scope);
    if (!session.connected || this.syncing) return;
    this.syncing = true; this.error = "";
    try {
      await invoke<void>("sync_quick_replies", { accountId: scope.account });
      this.current(scope);
      await this.refresh();
    } catch (error) {
      try { this.current(scope); this.error = error; } catch {}
    } finally {
      if (scope.account === session.activeAccount && scope.generation === messages.accountGeneration
        && scope.requestKey === this.key) this.syncing = false;
    }
  }
}

export const quickReplies = new QuickRepliesState();
