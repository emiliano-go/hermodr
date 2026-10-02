// The optional Android companion that fetches one-time media. The main link
// stays external; this instance only wakes when a one-time message arrives,
// fetches it, and goes dormant. Pairing is its own short session, since
// enabling the companion requires an existing link.
import { invoke } from "$lib/utils/ipc";
import type { OnceState, ServiceEvent } from "$lib/utils/models";
import { ui } from "./ui.svelte";

/** Why the last phone-number attempt produced no code. */
export type OncePairCodeError = { message: string; throttled: boolean; unavailable: boolean };

class OnceInstance {
  paired = $state(false);
  pairing = $state(false);
  running = $state(false);
  connected = $state(false);
  qrSvg = $state<string | null>(null);
  private code: string | null = null;

  /** Phone-number linking, an alternative to the QR shown beside it. */
  pairCode = $state<string | null>(null);
  pairCodeExpiresAt = $state<number | null>(null);
  pairCodeError = $state<OncePairCodeError | null>(null);
  pairCodeManual = $state(false);
  pairCodeBusy = $state(false);
  pairingPhone = $state<string | null>(null);

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

  /** Routes one companion event: pairing updates the code, anything else the snapshot. */
  async noteEvent(event: ServiceEvent) {
    switch (event.kind) {
      case "pairingCode":
        this.pairCode = event.code;
        this.pairCodeExpiresAt = Date.now() + event.timeout_secs * 1000;
        this.pairCodeError = null;
        this.pairCodeManual = false;
        return;
      case "pairingCodeRefresh":
        if (event.force_manual) {
          this.pairCode = null;
          this.pairCodeExpiresAt = null;
          this.pairCodeManual = true;
        } else {
          await this.refreshPairCode();
        }
        return;
      case "pairingCodeError":
        this.pairCode = null;
        this.pairCodeExpiresAt = null;
        this.pairCodeManual = false;
        this.pairCodeError = {
          message: event.message,
          throttled: event.throttled,
          unavailable: event.unavailable,
        };
        return;
      case "connected":
      case "disconnected":
      case "loggedOut":
        this.clearPairCode();
        await this.refresh();
        return;
      default:
        await this.refresh();
    }
  }

  /** Asks WhatsApp to mint a companion pairing code for `phone` (E.164 digits). */
  async requestPairCode(phone: string) {
    this.pairingPhone = phone;
    this.pairCode = null;
    this.pairCodeExpiresAt = null;
    this.pairCodeError = null;
    this.pairCodeManual = false;
    this.pairCodeBusy = true;
    try {
      await invoke("request_pair_code", { phone, companion: true });
    } catch (e) {
      this.pairCodeError = { message: String(e), throttled: false, unavailable: false };
    } finally {
      this.pairCodeBusy = false;
    }
  }

  /** Mints another code for the same number, after a refresh or an expiry. */
  async refreshPairCode() {
    if (this.pairingPhone) await this.requestPairCode(this.pairingPhone);
  }

  /** Drops the outstanding code and falls back to the QR. */
  async cancelPairCode() {
    this.clearPairCode();
    try {
      await invoke("cancel_pair_code", { companion: true });
    } catch {
      // The QR flow is untouched either way.
    }
  }

  clearPairCode() {
    this.pairCode = null;
    this.pairCodeExpiresAt = null;
    this.pairCodeError = null;
    this.pairCodeManual = false;
    this.pairingPhone = null;
  }

  /** Runs the companion just long enough to scan its QR. */
  async pair() {
    this.clearPairCode();
    await this.setPairing(true);
  }

  /** Gives up on the pairing session; the link is not affected. */
  async cancelPair() {
    await this.cancelPairCode();
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
