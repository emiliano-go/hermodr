import { LocalizedError } from "../i18n/errors.ts";
export const MAX_SOUND_CLIPS = 24;
export const MAX_SOUND_BYTES = 8 * 1024 * 1024;
export const MAX_LIBRARY_BYTES = 32 * 1024 * 1024;
export const SOUND_FILE_ACCEPT = ".ogg,.opus,.mp3,.m4a,.aac,.wav";
const AUDIO_TYPES: Record<string, string> = { ogg: "audio/ogg", opus: "audio/ogg", mp3: "audio/mpeg", m4a: "audio/mp4", aac: "audio/aac", wav: "audio/wav" };

export type SoundClip = { id: string; name: string; filename: string; type: string; size: number; created_at: number; shortcut: number | null };
export type SoundRecord = SoundClip & { blob: Blob };
export type SoundScope = { account: string; chat: string; generation: number };
export type SoundRepository = {
  list(): Promise<SoundClip[]>;
  read(id: string): Promise<SoundRecord>;
  save(record: SoundRecord): Promise<SoundClip[]>;
  remove(id: string): Promise<SoundClip[]>;
};

export function clipName(name: string): string {
  const value = name.trim().replace(/\s+/g, " ");
  if (!value || value.length > 64 || /[\x00-\x1f\x7f]/.test(value)) throw new LocalizedError({ kind: "postal_error", code: "error.content.give_the_clip_a_name_of_1_64_characters", params: {} });
  return value;
}

export function soundFile(file: File, name: string, shortcut: number | null = null): SoundRecord {
  const filename = file.name.split(/[\\/]/).pop() ?? "";
  const dot = filename.lastIndexOf(".");
  const extension = dot > 0 ? filename.slice(dot + 1).toLowerCase() : "";
  // Keep formats aligned with media_kind_for in the attachment sender.
  const type = AUDIO_TYPES[extension];
  if (!type || (file.type && !file.type.toLowerCase().startsWith("audio/"))) throw new LocalizedError({ kind: "postal_error", code: "error.content.choose_an_ogg_opus_mp3_m4a_aac_or_wav_audio_file", params: {} });
  if (!file.size || file.size > MAX_SOUND_BYTES) throw new LocalizedError({ kind: "postal_error", code: "error.content.choose_an_audio_clip_smaller_than_8_mib", params: {} });
  if (!filename || filename.length > 256 || /[\x00-\x1f\x7f]/.test(filename)) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_audio_filename_is_invalid", params: {} });
  if (shortcut !== null && (!Number.isInteger(shortcut) || shortcut < 1 || shortcut > 9)) throw new LocalizedError({ kind: "postal_error", code: "error.content.choose_a_shortcut_from_1_9", params: {} });
  return { id: crypto.randomUUID(), name: clipName(name), filename, type, size: file.size, created_at: Date.now(), shortcut, blob: file };
}

export function clipMetadata(record: SoundRecord): SoundClip { const { blob: _, ...meta } = record; return meta; }

export function soundRecords(value: unknown): SoundRecord[] {
  if (!Array.isArray(value)) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_could_not_be_read", params: {} });
  return value.map((record) => {
    if (!record || typeof record.id !== "string" || !record.id || !(record.blob instanceof Blob)
      || typeof record.filename !== "string" || typeof record.name !== "string"
      || !Number.isSafeInteger(record.created_at) || record.created_at < 0 || record.created_at > 8_640_000_000_000_000) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_contains_an_invalid_clip", params: {} });
    const file = new File([record.blob], record.filename, { type: record.type });
    const checked = soundFile(file, record.name, record.shortcut);
    if (record.size !== checked.size || record.type !== checked.type || record.filename !== checked.filename) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_contains_an_invalid_clip", params: {} });
    return { ...checked, id: record.id, created_at: record.created_at };
  });
}

export function checkedLibrary(records: SoundRecord[], record: SoundRecord): SoundRecord[] {
  const next = [...records.filter((known) => known.id !== record.id), ...soundRecords([record])];
  if (next.length > MAX_SOUND_CLIPS) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_holds_up_to_24_clips_remove_one_before_adding_another", params: {} });
  if (next.reduce((bytes, clip) => bytes + clip.size, 0) > MAX_LIBRARY_BYTES) throw new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_holds_up_to_32_mib_remove_a_clip_before_adding_another", params: {} });
  if (record.shortcut !== null && next.some((known) => known.id !== record.id && known.shortcut === record.shortcut)) throw new LocalizedError({ kind: "postal_error", code: "error.content.that_shortcut_already_belongs_to_another_clip", params: {} });
  return next;
}

export function createSoundRepository(factory: IDBFactory | undefined = globalThis.indexedDB): SoundRepository {
  async function transaction<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore, done: (value: T) => void, fail: (error: unknown) => void) => void): Promise<T> {
    if (!factory) throw new LocalizedError({ kind: "postal_error", code: "error.content.local_sound_storage_is_unavailable", params: {} });
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      let abandoned = false;
      const open = factory.open("postal-soundboard", 1);
      open.onupgradeneeded = () => {
        const store = open.result.createObjectStore("clips", { keyPath: "id" });
        store.createIndex("shortcut", "shortcut", { unique: true });
      };
      open.onerror = () => reject(open.error);
      open.onblocked = () => { abandoned = true; reject(new LocalizedError({ kind: "postal_error", code: "error.content.close_another_soundboard_window_and_try_again", params: {} })); };
      open.onsuccess = () => { if (abandoned) open.result.close(); else resolve(open.result); };
    });
    return new Promise<T>((resolve, reject) => {
      let result: T, failure: unknown, tx: IDBTransaction | undefined;
      const fail = (error: unknown) => { failure = error; try { tx?.abort(); } catch {} };
      try {
        tx = db.transaction("clips", mode);
        tx.oncomplete = () => { db.close(); resolve(result); };
        tx.onerror = tx.onabort = () => { db.close(); reject(failure ?? tx?.error ?? new LocalizedError({ kind: "postal_error", code: "error.content.the_soundboard_change_was_not_saved", params: {} })); };
        work(tx.objectStore("clips"), (value) => { result = value; }, fail);
      } catch (error) { try { tx?.abort(); } catch {} db.close(); reject(error); }
    });
  }
  const metadata = (records: SoundRecord[]) => records.map(clipMetadata).sort((a, b) => b.created_at - a.created_at || a.id.localeCompare(b.id));
  return {
    list: () => transaction<SoundClip[]>("readonly", (store, done, fail) => {
      const read = store.getAll(); read.onsuccess = () => { try { done(metadata(soundRecords(read.result))); } catch (error) { fail(error); } };
    }),
    read: (id) => transaction<SoundRecord>("readonly", (store, done, fail) => {
      const read = store.get(id); read.onsuccess = () => { try { if (!read.result) throw new LocalizedError({ kind: "postal_error", code: "error.content.that_clip_is_no_longer_saved", params: {} }); done(soundRecords([read.result])[0]); } catch (error) { fail(error); } };
    }),
    save: (record) => transaction<SoundClip[]>("readwrite", (store, done, fail) => {
      const read = store.getAll(); read.onsuccess = () => {
        try { const next = checkedLibrary(soundRecords(read.result), record); store.put(record); done(metadata(next)); }
        catch (error) { fail(error); }
      };
    }),
    remove: (id) => transaction<SoundClip[]>("readwrite", (store, done, fail) => {
      const read = store.getAll(); read.onsuccess = () => {
        try { const next = soundRecords(read.result).filter((record) => record.id !== id); store.delete(id); done(metadata(next)); }
        catch (error) { fail(error); }
      };
    }),
  };
}
