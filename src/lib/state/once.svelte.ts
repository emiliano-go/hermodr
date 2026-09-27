// The optional Android companion that fetches one-time media. The main link
// stays external; this instance only wakes when a one-time message arrives,
// fetches it, and goes dormant. Pairing is its own short session, since
// enabling the companion requires an existing link.
import { invoke } from "$lib/ipc";
import type { OnceState } from "$lib/models";
import { ui } from "./ui.svelte";

class OnceInstance {
  paired = $state(false);
  pairing = $state(false);
  running = $state(false);
  connected = $state(false);
  qrSvg = $state<string | null>(null);
  private code: string | null = null;

  async refresh() {
    try {
      const state = await invoke<OnceState>("once_state");
      this.paired = state.paired;
      this.pairing = state.pairing;
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

  /** Runs the companion just long enough to scan its QR. */
  async pair() {
    await this.setPairing(true);
  }

  /** Gives up on the pairing session; the link is not affected. */
  async cancelPair() {
    await this.setPairing(false);
  }

  private async setPairing(on: boolean) {
    try {
      await invoke("set_pairing", { pairing: on });
    } catch (e) {
      ui.fail(e);
    } finally {
      await this.refresh();
    }
  }
}

export const once = new OnceInstance();
