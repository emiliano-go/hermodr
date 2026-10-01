// Session state; cross-domain connection flows live in accounts.ts.
import { invoke } from "$lib/utils/ipc";
import type { Account, ConnectionState, UiSettings } from "$lib/utils/models";
import { ui } from "./ui.svelte";

/** Device memory of the notification kill switch, backing backends that drop it. */
const NOTIFICATIONS_KEY = "postal.notifications_enabled";

function readLocalNotifications(): boolean | null {
  try {
    const raw = localStorage.getItem(NOTIFICATIONS_KEY);
    return raw === null ? null : raw === "1";
  } catch {
    return null;
  }
}

function writeLocalNotifications(enabled: boolean) {
  try {
    localStorage.setItem(NOTIFICATIONS_KEY, enabled ? "1" : "0");
  } catch {
    // The backend copy is authoritative when present; this only fills its gaps.
  }
}

function storedZoom() {
  try {
    const saved = Number(localStorage.getItem("postal.zoom"));
    return Number.isFinite(saved) && saved ? Math.min(2, Math.max(0.6, saved)) : 1;
  } catch {
    return 1;
  }
}

export class SessionState {
  connected = $state(false);
  connecting = $state(false);
  started = $state(false);
  /** Launch chooser, shown when several linked accounts could be signed into. */
  choosingAccount = $state(false);
  qrSvg = $state<string | null>(null);
  /** Set once an explicit connect starts, so a reload without one still reveals. */
  connectRequested = false;

  /** Offline-backlog progress: how many the server announced and how many stored. */
  syncPending = $state(0);
  syncApplied = $state(0);
  /** A running full-history backfill, in chats. */
  backfill = $state<{ done: number; total: number } | null>(null);
  /** The phone's history sync after pairing, until it reaches 100%. */
  historyPercent = $state<number | null>(null);
  /** Backlog applied, waiting for the first chat/message paint to land. */
  finalizing = $state(false);
  /** The loading screen may be left. Survives reconnects for this launch. */
  gateDone = $state(false);
  /** The gate hit its cap and revealed; sync keeps going in the background. */
  syncTimedOut = $state(false);
  gateTimer: ReturnType<typeof setTimeout> | undefined = undefined;

  accountList = $state<Account[]>([]);
  activeAccount = $state<string | null>(null);
  /** Our own JID, for the account panel's picture and number. */
  me = $state<string | null>(null);
  /** Bumped when we replace our picture, which keeps its file name. */
  meVersion = $state(0);
  profileVersion = $state(0);

  /** WhatsApp privacy categories to values, for who can see us online. */
  privacy = $state<Record<string, string>>({});

  settings = $state<UiSettings>({
    message_window_size: 500,
    retention: { max_age_hours: { kind: "unlimited" }, max_messages_per_chat: { kind: "unlimited" } },
    request_full_history: false,
    auto_download_media: true,
    auto_download_types: { image: false, video: false, audio: false, document: false, sticker: false, gif: false },
    auto_transcribe: false,
    warn_missing_video_preview: true,
    media_dir: null,
    history_dir: null,
    send_typing: true,
    send_receipts: true,
    keep_history: true,
    skip_loading_screen: false,
    keep_archived: true,
    android_instance: false,
    notifications_enabled: true,
    freeze_chat_list_on_hover: false,
    chat_preview: true,
    verbose_whatsapp_logs: true,
  });

  /** Interface scale, persisted under `postal.zoom`; Ctrl +/-/0 adjust it. */
  zoom = $state(storedZoom());

  activeLabel = $derived(this.accountList.find((a) => a.id === this.activeAccount)?.label ?? "WhatsApp");

  syncPercent = $derived(
    this.syncPending > 0 ? Math.min(100, Math.round((this.syncApplied / this.syncPending) * 100)) : 0,
  );

  /** The chat UI may be shown and refreshed: the gate opened, or the user opted out of it. */
  uiUnlocked = $derived(this.gateDone || this.settings.skip_loading_screen);

  /**
   * Who sees us as online. "Same as last seen" defers to last seen, so with
   * last seen hidden we are effectively invisible, as Discord shows it.
   */
  visibility = $derived.by(() => {
    if (!this.connected) return "offline";
    const audience = this.privacy.online === "all" ? "all" : (this.privacy.last ?? "all");
    return audience === "none" ? "invisible" : audience === "all" ? "online" : "contacts";
  });

  setZoom(next: number) {
    this.zoom = Math.min(2, Math.max(0.6, Math.round(next * 10) / 10));
  }

  /** Applies the scale to the document; called from a route effect so it also runs on change. */
  applyZoom() {
    document.documentElement.style.zoom = String(this.zoom);
    try {
      localStorage.setItem("postal.zoom", String(this.zoom));
    } catch {
      // The scale lasts this session then.
    }
  }

  /** Caps how long the loading screen can hold, so a stuck sync never hangs the app. */
  startGateTimeout() {
    clearTimeout(this.gateTimer);
    this.gateTimer = setTimeout(() => {
      if (!this.gateDone) {
        this.syncTimedOut = true;
        this.gateDone = true;
      }
    }, 60_000);
  }

  stopGateTimeout() {
    clearTimeout(this.gateTimer);
  }

  async showQr(code: string | null) {
    this.qrSvg = code ? await invoke<string>("qr_svg", { value: code }) : null;
  }

  async loadAccounts() {
    const view = await invoke<import("$lib/utils/wire").AccountsView>("accounts");
    this.accountList = view.accounts;
    this.activeAccount = view.active;
  }

  async renameAccount(id: string, label: string) {
    try {
      await invoke("rename_account", { id, label });
      await this.loadAccounts();
    } catch (e) {
      ui.fail(e);
    }
  }

  async loadSettings() {
    this.settings = await invoke<UiSettings>("get_settings");
    // Off must behave as if every chat were muted, even when the backend
    // cannot persist the toggle: a stale backend drops the field, and a
    // fresh one defaults it on for older settings files. An explicit
    // backend `false` always wins; otherwise a stored off wins.
    if (this.settings.notifications_enabled !== false && readLocalNotifications() === false) {
      this.settings.notifications_enabled = false;
    }
  }

  async saveSettings(next: UiSettings) {
    // Device memory of the toggle, so the choice survives backends that drop it.
    writeLocalNotifications(next.notifications_enabled);
    try {
      await invoke("set_settings", { settings: next });
      this.settings = next;
    } catch (e) {
      ui.fail(e);
    }
  }

  async loadPrivacy() {
    try {
      const profile = await invoke<import("$lib/utils/wire").Profile>("profile");
      this.privacy = profile.privacy;
    } catch {
      // Privacy stays unknown; presence still works.
    }
  }

  /** Online while the window has focus, as WhatsApp Web does; typing only arrives then. */
  setOnline(online: boolean) {
    if (this.connected) invoke("set_online", { online }).catch(() => {});
  }

  /** Mirrors resetUi: the session-owned share of an account switch. */
  resetAccount() {
    this.me = null;
    // A switch starts a fresh catch-up, so the loading gate applies again.
    clearTimeout(this.gateTimer);
    this.gateDone = false;
    this.finalizing = false;
    this.syncTimedOut = false;
    this.syncPending = 0;
    this.syncApplied = 0;
    this.backfill = null;
    this.historyPercent = null;
  }
}

export const session = new SessionState();

export type { ConnectionState };
