import { invoke } from "$lib/utils/ipc";
import { broadcastSendReason, guardBroadcastSend } from "$lib/utils/broadcast";
import { session } from "./session.svelte";
import { chats } from "./chats.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";

import type { ScheduledMessage } from "$lib/utils/wire";
export type { ScheduledMessage } from "$lib/utils/wire";

export class ScheduledState {
  items = $state<ScheduledMessage[]>([]);
  account = $state<string | null>(null);
  open = $state(false);
  error = $state<string | null>(null);
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
      const items = await invoke<ScheduledMessage[]>("scheduled_messages", { account });
      if (version !== this.version || account !== this.account) return false;
      this.items = items;
      this.error = null;
      return true;
    } catch (error) {
      if (version === this.version && account === this.account) this.error = String(error);
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
      if (account === this.account) { this.error = String(error); ui.fail(error); }
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
    let blocked: string | null = null;
    try {
      if (!await this.refresh(account) || !current()) return;
      for (const item of this.items) {
        if (item.status !== "pending" || item.due_at > now()) continue;
        if (!current()) break;
        const reason = broadcastSendReason(item.chat);
        if (reason) { blocked = reason; continue; }
        try {
          sent = await enqueue(async (signal) => {
            guardBroadcastSend(item.chat);
            if (signal.aborted || !current()) throw new Error("Account changed before scheduled send");
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
      if (current()) { this.error = String(error); ui.fail(error); }
    } finally {
      if (version === this.version) this.busy = false;
    }
  }
}

export const scheduled = new ScheduledState();
