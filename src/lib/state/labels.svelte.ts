import { invoke } from "$lib/utils/ipc";
import type { LabelsView } from "$lib/utils/wire";
import { composer } from "./composer.svelte";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";

export class LabelsState {
  account = $state<string | null>(null);
  view = $state.raw<LabelsView>({ labels: [], chats: [], messages: [], complete: false });
  loaded = $state(false);
  loading = $state(false);
  busy = $state(false);
  error = $state<string | null>(null);
  private request = 0;
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;

  reset() {
    clearTimeout(this.refreshTimer);
    this.refreshTimer = undefined;
    this.request++;
    this.account = null;
    this.view = { labels: [], chats: [], messages: [], complete: false };
    this.loaded = false;
    this.loading = false;
    this.busy = false;
    this.error = null;
  }

  chatIds(chat: string) {
    return this.view.chats.filter((row) => row.chat === chat).map((row) => row.label_id);
  }

  messageIds(chat: string, id: string) {
    return this.view.messages.filter((row) => row.chat === chat && row.message_id === id).map((row) => row.label_id);
  }

  queueRefresh() {
    if (this.refreshTimer !== undefined) return;
    this.refreshTimer = setTimeout(() => { this.refreshTimer = undefined; void this.refresh(); }, 200);
  }

  async refresh() {
    const account = session.activeAccount;
    if (!account) { this.reset(); return; }
    if (this.account !== account) this.reset();
    this.account = account;
    const generation = messages.accountGeneration;
    const request = ++this.request;
    const current = () => account === session.activeAccount && generation === messages.accountGeneration && request === this.request;
    this.loading = true;
    this.error = null;
    try {
      const view = await invoke<LabelsView>("labels_view", { accountId: account });
      if (current()) { this.view = view; this.loaded = true; }
    } catch (error) {
      if (current()) this.error = String(error);
    } finally {
      if (current()) this.loading = false;
    }
  }

  async mutate(command: string, args: Record<string, unknown> | Record<string, unknown>[]) {
    const account = session.activeAccount;
    const generation = messages.accountGeneration;
    const current = () => account === session.activeAccount && generation === messages.accountGeneration && this.account === account;
    if (!account || !current() || this.busy || !session.connected) throw new Error("Labels are unavailable for this account.");
    this.busy = true;
    this.error = null;
    try {
      await composer.enqueue(async (signal) => {
        for (const item of Array.isArray(args) ? args : [args]) {
          if (signal.aborted || !session.connected || !current()) throw new Error("Account changed or disconnected before label operation.");
          await invoke(command, { ...item, accountId: account });
        }
      });
      if (!current()) throw new Error("Account changed during label operation.");
      await this.refresh();
    } catch (error) {
      if (current()) { await this.refresh(); if (current()) this.error = String(error); }
      throw error;
    } finally {
      if (current()) this.busy = false;
    }
  }

  save(labelId: string, name: string, color: number) {
    const create = !labelId;
    if (!labelId) labelId = BigInt(`0x${crypto.randomUUID().replaceAll("-", "")}`).toString();
    return this.mutate("save_label", { labelId, name, color, create });
  }

  delete(labelId: string) { return this.mutate("delete_label", { labelId }); }

  applyChat(labelId: string, chat: string, labeled: boolean) {
    return this.mutate("label_chat", { labelId, chat, labeled });
  }

  applyMessage(labelId: string, chat: string, messageId: string, labeled: boolean) {
    return this.mutate("label_message", { labelId, chat, messageId, labeled });
  }

  applyMessages(labelId: string, targets: { chat: string; id: string }[], labeled: boolean) {
    return this.mutate("label_message", targets.map(({ chat, id }) => ({ labelId, chat, messageId: id, labeled })));
  }
}

export const labels = new LabelsState();
