import { listen } from "@tauri-apps/api/event";
import { invoke } from "$lib/utils/ipc";
import type { TranscriptionView } from "$lib/utils/wire";

export class TranscriptionState {
  enabled = $state(false);
  error = $state("");
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
      if (token === this.version) { this.enabled = false; this.error = String(failure); }
    }
  }
  start() {
    let alive = true;
    let off: (() => void) | undefined;
    void this.load();
    void listen("transcription-settings-changed", () => { if (alive) void this.load(); }).then(
      (stop) => { if (alive) off = stop; else stop(); },
      (failure) => { if (alive) this.error = String(failure); },
    );
    return () => { alive = false; this.version++; off?.(); };
  }
}

export const transcription = new TranscriptionState();
