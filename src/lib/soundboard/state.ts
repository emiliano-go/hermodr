import { get, writable } from "svelte/store";
import { clipName, createSoundRepository, soundFile } from "./library.ts";
import type { SoundClip, SoundRepository, SoundScope } from "./library.ts";

export function createSoundboard(repository: SoundRepository = createSoundRepository()) {
  const state = writable({ clips: [] as SoundClip[], loading: false, busy: false, loaded: false, error: null as string | null });
  let revision = 0;
  async function load(force = false) {
    if (get(state).loading || get(state).busy || (!force && get(state).loaded)) return;
    const target = ++revision;
    state.update((value) => ({ ...value, loading: true, error: null }));
    try { const clips = await repository.list(); if (target === revision) state.update((value) => ({ ...value, clips, loaded: true })); }
    catch (error) { if (target === revision) state.update((value) => ({ ...value, error: `Could not load the soundboard. ${String(error)}` })); }
    finally { if (target === revision) state.update((value) => ({ ...value, loading: false })); }
  }
  async function change(work: () => Promise<SoundClip[]>): Promise<boolean> {
    if (get(state).busy) return false;
    const target = ++revision;
    state.update((value) => ({ ...value, busy: true, loading: false, error: null }));
    try { const clips = await work(); if (target === revision) state.update((value) => ({ ...value, clips, loaded: true })); return true; }
    catch (error) { if (target === revision) state.update((value) => ({ ...value, error: `Could not save the soundboard change. ${String(error)}` })); return false; }
    finally { if (target === revision) state.update((value) => ({ ...value, busy: false })); }
  }
  function add(file: File, name: string, shortcut: number | null) { return change(async () => repository.save(soundFile(file, name, shortcut))); }
  function update(id: string, name: string, shortcut: number | null) {
    return change(async () => repository.save({ ...await repository.read(id), name: clipName(name), shortcut }));
  }
  function remove(id: string) { return change(() => repository.remove(id)); }
  async function send(id: string, scope: SoundScope, current: () => boolean, callback: (file: File, scope: SoundScope) => Promise<void>) {
    if (!current()) throw new Error("The account or chat changed before sending.");
    const record = await repository.read(id);
    if (!current()) throw new Error("The account or chat changed before sending.");
    await callback(new File([record.blob], record.filename, { type: record.type }), scope);
  }
  return { subscribe: state.subscribe, load, add, update, remove, send };
}

export const soundboard = createSoundboard();
