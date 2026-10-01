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
  if (!value || value.length > 64 || /[\x00-\x1f\x7f]/.test(value)) throw new Error("Give the clip a name of 1–64 characters.");
  return value;
}

export function soundFile(file: File, name: string, shortcut: number | null = null): SoundRecord {
  const filename = file.name.split(/[\\/]/).pop() ?? "";
  const dot = filename.lastIndexOf(".");
  const extension = dot > 0 ? filename.slice(dot + 1).toLowerCase() : "";
  // Keep formats aligned with media_kind_for in the attachment sender.
  const type = AUDIO_TYPES[extension];
  if (!type || (file.type && !file.type.toLowerCase().startsWith("audio/"))) throw new Error("Choose an OGG, OPUS, MP3, M4A, AAC or WAV audio file.");
  if (!file.size || file.size > MAX_SOUND_BYTES) throw new Error("Choose an audio clip smaller than 8 MiB.");
  if (!filename || filename.length > 256 || /[\x00-\x1f\x7f]/.test(filename)) throw new Error("The audio filename is invalid.");
  if (shortcut !== null && (!Number.isInteger(shortcut) || shortcut < 1 || shortcut > 9)) throw new Error("Choose a shortcut from 1–9.");
  return { id: crypto.randomUUID(), name: clipName(name), filename, type, size: file.size, created_at: Date.now(), shortcut, blob: file };
}

export function clipMetadata(record: SoundRecord): SoundClip { const { blob: _, ...meta } = record; return meta; }

export function soundRecords(value: unknown): SoundRecord[] {
  if (!Array.isArray(value)) throw new Error("The soundboard could not be read.");
  return value.map((record) => {
    if (!record || typeof record.id !== "string" || !record.id || !(record.blob instanceof Blob)
      || typeof record.filename !== "string" || typeof record.name !== "string"
      || !Number.isSafeInteger(record.created_at) || record.created_at < 0 || record.created_at > 8_640_000_000_000_000) throw new Error("The soundboard contains an invalid clip.");
    const file = new File([record.blob], record.filename, { type: record.type });
    const checked = soundFile(file, record.name, record.shortcut);
    if (record.size !== checked.size || record.type !== checked.type || record.filename !== checked.filename) throw new Error("The soundboard contains an invalid clip.");
    return { ...checked, id: record.id, created_at: record.created_at };
  });
}

export function checkedLibrary(records: SoundRecord[], record: SoundRecord): SoundRecord[] {
  const next = [...records.filter((known) => known.id !== record.id), ...soundRecords([record])];
  if (next.length > MAX_SOUND_CLIPS) throw new Error("The soundboard holds up to 24 clips. Remove one before adding another.");
  if (next.reduce((bytes, clip) => bytes + clip.size, 0) > MAX_LIBRARY_BYTES) throw new Error("The soundboard holds up to 32 MiB. Remove a clip before adding another.");
  if (record.shortcut !== null && next.some((known) => known.id !== record.id && known.shortcut === record.shortcut)) throw new Error("That shortcut already belongs to another clip.");
  return next;
}

export function createSoundRepository(factory: IDBFactory | undefined = globalThis.indexedDB): SoundRepository {
  async function transaction<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore, done: (value: T) => void, fail: (error: unknown) => void) => void): Promise<T> {
    if (!factory) throw new Error("Local sound storage is unavailable.");
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      let abandoned = false;
      const open = factory.open("postal-soundboard", 1);
      open.onupgradeneeded = () => {
        const store = open.result.createObjectStore("clips", { keyPath: "id" });
        store.createIndex("shortcut", "shortcut", { unique: true });
      };
      open.onerror = () => reject(open.error);
      open.onblocked = () => { abandoned = true; reject(new Error("Close another soundboard window and try again.")); };
      open.onsuccess = () => { if (abandoned) open.result.close(); else resolve(open.result); };
    });
    return new Promise<T>((resolve, reject) => {
      let result: T, failure: unknown, tx: IDBTransaction | undefined;
      const fail = (error: unknown) => { failure = error; try { tx?.abort(); } catch {} };
      try {
        tx = db.transaction("clips", mode);
        tx.oncomplete = () => { db.close(); resolve(result); };
        tx.onerror = tx.onabort = () => { db.close(); reject(failure ?? tx?.error ?? new Error("The soundboard change was not saved.")); };
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
      const read = store.get(id); read.onsuccess = () => { try { if (!read.result) throw new Error("That clip is no longer saved."); done(soundRecords([read.result])[0]); } catch (error) { fail(error); } };
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
