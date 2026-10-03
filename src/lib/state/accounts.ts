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

let transitionRevision = 0;
let transitionQueue = Promise.resolve();
type Current = () => boolean;

function transition(run: (current: Current) => Promise<void>) {
  const revision = ++transitionRevision;
  const current = () => revision === transitionRevision;
  const pending = transitionQueue.then(async () => {
    try { await run(current); }
    catch (error) { if (current()) ui.fail(error); }
    finally { if (current()) session.connecting = false; }
  });
  transitionQueue = pending;
  return pending;
}

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
  await transition(async (current) => {
    const view = await session.loadAccounts(current, resetUi);
    if (id === view.active) await connectCurrent(current);
    else await switchCurrent(id, current);
  });
}

export function switchTo(id: string) {
  return transition((current) => switchCurrent(id, current));
}

async function switchCurrent(id: string, current: Current) {
  const view = await session.loadAccounts(current, resetUi);
  if (id === view.active) return;
  if (current()) {
    resetUi();
    session.connected = false;
    await session.showQr(null, current);
  }
  await invoke("switch_account", { id });
  if (!current()) return;
  await session.loadAccounts(current, resetUi);
  if (current()) await syncState(current);
}

export function removeAccount(id: string) {
  return transition(async (current) => {
    const view = await session.loadAccounts(current, resetUi);
    if (id === view.active && current()) {
      ui.showSettings = false;
      resetUi();
      if (current()) {
        session.connected = false;
        await session.showQr(null, current);
      }
    }
    await invoke("remove_account", { id });
    if (!current()) return;
    await session.loadAccounts(current, resetUi);
    if (current()) await syncState(current);
  });
}

export function addAccount() {
  return transition(async (current) => {
    if (current()) {
      resetUi();
      session.connected = false;
      await session.showQr(null, current);
    }
    await invoke("add_account", {});
    if (!current()) return;
    await session.loadAccounts(current, resetUi);
    if (current()) await syncState(current);
  });
}

export async function syncState(owner: Current = () => true) {
  const revision = transitionRevision;
  const current = () => revision === transitionRevision && owner();
  if (!current()) return;
  const state = await invoke<ConnectionState>("connection_state");
  if (!current()) return;
  session.started = state.started;
  session.connected = state.connected;
  await session.showQr(state.connected ? null : state.qr, current);
  if (!current()) return;
  if (session.connected) {
    // Already connected when the UI loaded without an explicit connect (e.g.
    // a webview reload): there is no fresh backlog to gate on, so do not hold
    // the loading screen. A cold start reaches here disconnected, then gates.
    if (!session.connectRequested && !session.gateDone) session.gateDone = true;
    await chats.refreshChats();
    if (!current()) return;
    void favorites.refresh();
    void labels.refresh();
    // Aliases need a live service, so they are read on every connect and on
    // every account switch rather than once at boot.
    await members.loadAliases();
  }
}

/** Connects, reusing a stored session when there is one. */
export async function connect() {
  await transition(connectCurrent);
}

async function connectCurrent(current: Current) {
  await session.loadAccounts(current, resetUi);
  if (current()) {
    session.connecting = true;
    ui.error = null;
    session.connectRequested = true;
  }
  await invoke("connect");
  if (!current()) return;
  await syncState(current);
  if (current()) void refreshResolvedNames();
}

/** Reconnects after the backend logged the account out. */
export function reconnect() {
  return transition(async (current) => {
    if (current()) {
      session.connected = false;
      session.started = false;
      await session.showQr(null, current);
    }
    await connectCurrent(current);
  });
}
