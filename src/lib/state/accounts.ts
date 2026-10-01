import { invoke } from "$lib/utils/ipc";
import type { ConnectionState } from "$lib/utils/models";
import { chats } from "./chats.svelte";
import { composer } from "./composer.svelte";
import { favorites } from "./favorites.svelte";
import { labels } from "./labels.svelte";
import { refreshResolvedNames } from "./events";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { player } from "./player.svelte";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";

/** Clears everything tied to the current account before switching. */
function resetUi() {
  favorites.reset();
  labels.reset();
  chats.resetAccount();
  session.resetAccount();
  messages.resetAccount();
  members.resetAccount();
  composer.resetAccount();
  ui.resetAccount();
  player.stop();
}

/** Starts the account picked on the launch chooser. */
export async function chooseAccount(id: string) {
  session.choosingAccount = false;
  if (id === session.activeAccount) await connect();
  else await switchTo(id);
}

export async function switchTo(id: string) {
  if (id === session.activeAccount) return;
  try {
    resetUi();
    session.connected = false;
    await session.showQr(null);
    await invoke("switch_account", { id });
    await session.loadAccounts();
    await syncState();
  } catch (e) {
    ui.fail(e);
  }
}

export async function removeAccount(id: string) {
  try {
    if (id === session.activeAccount) {
      ui.showSettings = false;
      resetUi();
      session.connected = false;
      await session.showQr(null);
    }
    await invoke("remove_account", { id });
    await session.loadAccounts();
    await syncState();
  } catch (e) {
    ui.fail(e);
  }
}

export async function addAccount() {
  try {
    resetUi();
    session.connected = false;
    await session.showQr(null);
    await invoke("add_account", {});
    await session.loadAccounts();
    await syncState();
  } catch (e) {
    ui.fail(e);
  }
}

export async function syncState() {
  const state = await invoke<ConnectionState>("connection_state");
  session.started = state.started;
  session.connected = state.connected;
  await session.showQr(state.connected ? null : state.qr);
  if (session.connected) {
    // Already connected when the UI loaded without an explicit connect (e.g.
    // a webview reload): there is no fresh backlog to gate on, so do not hold
    // the loading screen. A cold start reaches here disconnected, then gates.
    if (!session.connectRequested && !session.gateDone) session.gateDone = true;
    await chats.refreshChats();
    void favorites.refresh();
    void labels.refresh();
    // Aliases need a live service, so they are read on every connect and on
    // every account switch rather than once at boot.
    await members.loadAliases();
  }
}

/** Connects, reusing a stored session when there is one. */
export async function connect() {
  session.connecting = true;
  ui.error = null;
  session.connectRequested = true;
  try {
    await invoke("connect");
    await syncState();
    void refreshResolvedNames();
  } catch (e) {
    ui.fail(e);
  } finally {
    session.connecting = false;
  }
}

/** Reconnects after the backend logged the account out. */
export async function reconnect() {
  session.connected = false;
  session.started = false;
  await session.showQr(null);
  await session.loadAccounts();
  await connect();
}
