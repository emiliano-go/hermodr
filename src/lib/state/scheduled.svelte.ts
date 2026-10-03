import { invoke } from "$lib/utils/ipc";
import { broadcastSendError, guardBroadcastSend } from "$lib/utils/broadcast";
import { session } from "./session.svelte";
import { chats } from "./chats.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";
import { uiError } from "./localized.ts";

import type { ScheduledMessageView } from "$lib/utils/wire";
export type { ScheduledMessageView as ScheduledMessage } from "$lib/utils/wire";

export class ScheduledState {
  items = $state<ScheduledMessageView[]>([]);
  account = $state<string | null>(null);
  open = $state(false);
  #error = $state<LocalizedError | null>(null);
  get error(): string | null { return this.#error?.message ?? null; }
  set error(value: unknown) { this.#error = value == null ? null : normalizeError(value); }
  get diagnostic() { return this.#error?.diagnostic; }
  private version = 0;
  private busy = false;

  selectAccount(account: string | null) {
    if (this.account === account) return;
    this.version++;
    this.busy = false;
    this.account = account;
    this.items = [];
    this.error = null;
    this.open = false;
  }

  async refresh(account = this.account): Promise<boolean> {
    if (!account) return false;
    const version = this.version;
    try {
      const items = await invoke<ScheduledMessageView[]>("scheduled_messages", { account });
      if (version !== this.version || account !== this.account) return false;
      this.items = items;
      this.error = null;
      return true;
    } catch (error) {
      if (version === this.version && account === this.account) this.error = error;
      return false;
    }
  }

  async change(command: "update_scheduled_message" | "cancel_scheduled_message" | "retry_scheduled_message",
    args: { id: string; text?: string; dueAt?: number }): Promise<boolean> {
    const account = this.account;
    if (!account) return false;
    try {
      await invoke(command, { ...args, account });
      await this.refresh(account);
      return true;
    } catch (error) {
      if (account === this.account) { this.error = error; ui.fail(error); }
      return false;
    }
  }

  async tick(enqueue: <T>(task: (signal: AbortSignal) => Promise<T>) => Promise<T>, now = () => Math.floor(Date.now() / 1000)) {
    const account = this.account;
    if (this.busy || !account || account !== session.activeAccount || !session.connected) return;
    this.busy = true;
    const version = this.version;
    const current = () => version === this.version && account === session.activeAccount && session.connected;
    let sent = false;
    let blocked: LocalizedError | null = null;
    try {
      if (!await this.refresh(account) || !current()) return;
      for (const item of this.items) {
        if (item.status !== "pending" || item.due_at > now()) continue;
        if (!current()) break;
        const reason = broadcastSendError(item.chat);
        if (reason) { blocked = reason; continue; }
        try {
          sent = await enqueue(async (signal) => {
            guardBroadcastSend(item.chat);
            if (signal.aborted || !current()) throw uiError("error.state.scheduled_scope");
            return await invoke<boolean>("send_scheduled_message", { account, id: item.id });
          }) || sent;
        } catch (error) {
          if (current()) ui.fail(error);
        }
      }
      if (!current()) return;
      await this.refresh(account);
      if (current() && blocked) this.error = blocked;
      if (sent && current()) {
        await messages.reloadMessages(chats.selectedChat);
        await chats.refreshChats();
      }
    } catch (error) {
      if (current()) { this.error = error; ui.fail(error); }
    } finally {
      if (version === this.version) this.busy = false;
    }
  }
}

export const scheduled = new ScheduledState();
