import { get, writable } from "svelte/store";
import { appendHistory, clearHistory, loadHistory, saveHistory } from "./history.ts";
import type { HistorySnapshot, HistoryStorage, NotificationHistoryEntry } from "./history.ts";

export type HistoryState = HistorySnapshot & { account: string | null };
const empty = (): HistorySnapshot => ({ entries: [], error: null, writable: true });

export function createNotificationHistory(storage: () => HistoryStorage) {
  const state = writable<HistoryState>({ account: null, ...empty() });
  const accounts = new Map<string, HistorySnapshot>();

  function publish(account: string, snapshot: HistorySnapshot) {
    accounts.set(account, snapshot);
    state.set({ account, ...snapshot });
  }

  function load(account: string | null, current = () => true): HistorySnapshot | null {
    if (!current()) return null;
    if (!account) { state.set({ account: null, ...empty() }); return empty(); }
    let snapshot = accounts.get(account);
    if (!snapshot) {
      try { snapshot = loadHistory(account, storage()); }
      catch { snapshot = { entries: [], error: "Notification history storage is unavailable. New entries are temporary.", writable: false }; }
    }
    if (!current()) return null;
    publish(account, snapshot);
    return snapshot;
  }

  function record(account: string, entry: NotificationHistoryEntry, current: () => boolean): boolean {
    const snapshot = load(account, current);
    if (!snapshot || !current()) return false;
    const entries = appendHistory(snapshot.entries, entry);
    if (entries === snapshot.entries) return !snapshot.error;
    let next: HistorySnapshot = { ...snapshot, entries };
    if (snapshot.writable) {
      if (!current()) return false;
      try { const local = storage(); if (!current()) return false; next = saveHistory(account, entries, local); }
      catch { next.error = "Notification history storage is unavailable. New entries are temporary."; }
    }
    if (!current() || get(state).account !== account) return false;
    publish(account, next);
    return !next.error;
  }

  function clear(account: string, current: () => boolean): boolean {
    const snapshot = load(account, current);
    if (!snapshot || !current()) return false;
    let next: HistorySnapshot;
    try { const local = storage(); if (!current()) return false; next = clearHistory(account, snapshot, local); }
    catch { next = { ...snapshot, error: "Notification history could not be cleared. Local storage is unavailable." }; }
    if (!current() || get(state).account !== account) return false;
    publish(account, next);
    return !next.error;
  }

  return { subscribe: state.subscribe, load, record, clear };
}

export const notificationHistory = createNotificationHistory(() => localStorage);
