import { listen } from "@tauri-apps/api/event";
import { invoke } from "$lib/utils/ipc";
import type { TranscriptionView } from "$lib/utils/wire";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";

export class TranscriptionState {
  enabled = $state(false);
  #error = $state<LocalizedError | null>(null);
  get error(): string { return this.#error?.message ?? ""; }
  set error(value: unknown) { this.#error = value == null || value === "" ? null : normalizeError(value); }
  get diagnostic() { return this.#error?.diagnostic; }
  private version = 0;
  configure(view: TranscriptionView) {
    this.version++;
    const plugin = view.plugins.find((p) => p.id === view.settings.plugin_id);
    this.enabled = !!plugin?.enabled && !!plugin.contributes.transcription?.providers.some((p) => p.id === view.settings.provider);
    this.error = "";
  }
  async load() {
    const token = ++this.version;
    try {
      const view = await invoke<TranscriptionView>("transcription_settings");
      if (token === this.version) this.configure(view);
    } catch (failure) {
      if (token === this.version) { this.enabled = false; this.error = failure; }
    }
  }
  start() {
    let alive = true;
    let off: (() => void) | undefined;
    void this.load();
    void listen("transcription-settings-changed", () => { if (alive) void this.load(); }).then(
      (stop) => { if (alive) off = stop; else stop(); },
      (failure) => { if (alive) this.error = failure; },
    );
    return () => { alive = false; this.version++; off?.(); };
  }
}

export const transcription = new TranscriptionState();
