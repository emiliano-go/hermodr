import { invoke } from "$lib/utils/ipc";
import { favoriteRows } from "$lib/utils/favorite-chats";
import type { ChatSummary } from "$lib/utils/models";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";

export class FavoritesState {
  chats = $state<string[]>([]);
  busy = $state<string | null>(null);
  private generation = 0;
  private request = 0;

  reset() {
    this.generation++;
    this.request++;
    this.chats = [];
    this.busy = null;
  }

  rows(chats: ChatSummary[]) { return favoriteRows(chats, this.chats); }

  async refresh() {
    const account = session.activeAccount;
    const generation = this.generation;
    const request = ++this.request;
    if (!account) { this.reset(); return; }
    try {
      const chats = await invoke<string[]>("favorite_chats", { account });
      if (session.activeAccount === account && generation === this.generation && request === this.request)
        this.chats = chats;
    } catch (error) {
      if (session.activeAccount === account && generation === this.generation && request === this.request) ui.fail(error);
    }
  }

  async toggle(chat: string) {
    if (this.busy || !session.activeAccount) return;
    const account = session.activeAccount;
    const generation = this.generation;
    this.busy = chat;
    try {
      await invoke("set_favorite", { account, chat, favorite: !this.chats.includes(chat) });
      if (session.activeAccount === account && generation === this.generation) await this.refresh();
    } catch (error) {
      if (session.activeAccount === account && generation === this.generation) ui.fail(error);
    } finally {
      if (session.activeAccount === account && generation === this.generation) this.busy = null;
    }
  }
}

export const favorites = new FavoritesState();
