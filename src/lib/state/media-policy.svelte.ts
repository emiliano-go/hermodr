import { invoke } from "$lib/utils/ipc";
import { emptyMediaOverrides } from "$lib/utils/auto-download";
import type { MediaAutoDownload, MediaAutoDownloadOverrides } from "$lib/utils/wire";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";

export class MediaPolicyState {
  value = $state<MediaAutoDownloadOverrides>(emptyMediaOverrides());
  busy = $state(false);
  #error = $state<LocalizedError | null>(null);
  get error(): string { return this.#error?.message ?? ""; }
  set error(value: unknown) { this.#error = value == null || value === "" ? null : normalizeError(value); }
  get diagnostic() { return this.#error?.diagnostic; }
  private generation = 0;
  private accountId = "";
  private chat = "";

  constructor(private rpc: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke) {}

  async load(accountId: string, chat: string) {
    const token = ++this.generation;
    this.accountId = accountId; this.chat = chat;
    this.value = emptyMediaOverrides(); this.error = ""; this.busy = true;
    try {
      const value = await this.rpc("chat_media_auto_download", { accountId, chat }) as MediaAutoDownloadOverrides;
      if (token === this.generation) this.value = value;
    } catch (failure) {
      if (token === this.generation) this.error = failure;
    } finally {
      if (token === this.generation) this.busy = false;
    }
  }

  async change(kind: keyof MediaAutoDownload, enabled: boolean | null) {
    if (this.busy || !this.accountId || !this.chat) return;
    const token = this.generation;
    const overrides = { ...this.value, [kind]: enabled };
    this.busy = true; this.error = "";
    try {
      await this.rpc("set_chat_media_auto_download", { accountId: this.accountId, chat: this.chat, overrides });
      if (token === this.generation) this.value = overrides;
    } catch (failure) {
      if (token === this.generation) this.error = failure;
    } finally {
      if (token === this.generation) this.busy = false;
    }
  }
}
