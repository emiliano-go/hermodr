// The optional Android companion that fetches one-time media in the background.
// The main link stays external; this instance never replaces it, it only adds a
// second, Android-style session with its own store.
import { invoke } from "$lib/ipc";
import type { OnceState } from "$lib/models";

class OnceInstance {
  paired = $state(false);
  running = $state(false);
  connected = $state(false);
  qrSvg = $state<string | null>(null);
  private code: string | null = null;

  async refresh() {
    try {
      const state = await invoke<OnceState>("once_state");
      this.paired = state.paired;
      this.running = state.running;
      this.connected = state.connected;
      if (state.qr !== this.code) {
        this.code = state.qr;
        this.qrSvg = state.qr ? await invoke<string>("qr_svg", { value: state.qr }) : null;
      }
    } catch {
      // No account is active yet; the companion is simply shown as stopped.
    }
  }
}

export const once = new OnceInstance();
