import type { StorageFile, StorageReport, StorageCleanup } from "../../src/lib/storage";
import type { StoredMessage } from "../../src/lib/models";
import type { MessageCursor } from "../../src/lib/message-window";
export const windowFixture = {
  archive: Array.from({ length: 350 }, (_, n) => ({ chat: "window@s", id: String(n).padStart(4, "0"), timestamp: 100, text: `Message ${n}` }) as StoredMessage),
  phoneRequests: 0, failure: false, deferNext: false, pending: [] as (() => void)[],
};
export const mediaFixture = { calls: 0, failure: true, deferNext: false, pending: [] as (() => void)[] };
export const archiveFixture = { failure: false, cancelled: false, deferNext: false, pending: [] as (() => void)[],
  calls: [] as string[], accounts: [{ id: "existing", label: "Existing account" }] as { id: string; label: string }[] };
export const fixture = { updated: false, failure: false, calls: 0, savedRetention: null as unknown,
  storageFailure: false, storageCalls: 0, cacheBytes: 512,
  media: [
    { chat: "a@s", id: "photo", kind: "image", filename: "photo.jpg", timestamp: 200, quoted: false, bytes: 2048, available: true },
    { chat: "a@s", id: "doc", kind: "document", filename: "notes.pdf", timestamp: 100, quoted: false, bytes: 1024, available: true },
    { chat: "b@s", id: "video", kind: "video", filename: "clip.mp4", timestamp: 300, quoted: false, bytes: 4096, available: true },
  ] as StorageFile[],
};

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (command === "chats") return [{ chat: "archive@s", display_name: "Synthetic conversation" }] as T;
  if (command === "accounts") return { accounts: archiveFixture.accounts, active: "existing" } as T;
  if (command === "export_archive" || command === "restore_local_backup") {
    archiveFixture.calls.push(`${command}:${args?.chat ?? "all"}`);
    const complete = () => {
      if (archiveFixture.cancelled) return null as T;
      if (archiveFixture.failure) throw new Error("Synthetic archive write failure");
      if (command === "restore_local_backup") archiveFixture.accounts.push({ id: "restored", label: "Restored backup" });
      return { directory: "synthetic-output", messages: args?.chat ? 2 : 1003, attachments: 1, missing_attachments: 1 } as T;
    };
    if (archiveFixture.deferNext) {
      archiveFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => archiveFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "download_media") {
    mediaFixture.calls++;
    const complete = () => {
      if (mediaFixture.failure) throw new Error("Synthetic sender unavailable");
      const message = windowFixture.archive.find((m) => m.chat === args?.chat && m.id === args?.id);
      if (message) message.media_path = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
      return undefined as T;
    };
    if (mediaFixture.deferNext) {
      mediaFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => mediaFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "message_page") {
    if (windowFixture.failure) throw new Error("Synthetic page failure");
    const compare = (a: MessageCursor, b: MessageCursor) => a.timestamp - b.timestamp || (a.id === b.id ? 0 : a.id < b.id ? -1 : 1);
    const anchor = args?.anchorId ? windowFixture.archive.find((m) => m.chat === args?.chat && m.id === args.anchorId) : undefined;
    const cursor = anchor ?? args?.cursor as MessageCursor | undefined;
    const after = args?.direction === "after";
    const rows = windowFixture.archive.filter((m) => m.chat === args?.chat && (!args?.anchorId || anchor) &&
      (!cursor || (after ? compare(m, cursor) > 0 : args?.direction === "through" ? compare(m, cursor) <= 0 : compare(m, cursor) < 0)))
      .sort((a, b) => after ? compare(a, b) : compare(b, a));
    const limit = Number(args?.limit ?? 100);
    const messages = rows.slice(0, limit);
    if (after) messages.reverse();
    const result = { messages, has_more: rows.length > limit } as T;
    if (windowFixture.deferNext) {
      windowFixture.deferNext = false;
      return new Promise<T>((resolve) => windowFixture.pending.push(() => resolve(result)));
    }
    return result;
  }
  if (command === "load_older") { windowFixture.phoneRequests++; return undefined as T; }
  if (command === "marks") return { reactions: [], starred: [], pinned: null, polls: [], events: [], view_once: [], forwarded: [], edited: [] } as T;
  if (command === "storage_report") {
    let files = fixture.media.filter((file) => !args?.chat || file.chat === args.chat)
      .toSorted((a, b) => args?.order === "oldest" ? a.timestamp - b.timestamp : b.bytes - a.bytes);
    const total_files = files.length;
    const offset = Number(args?.offset ?? 0);
    files = files.slice(offset, offset + 50).map((file) => ({ ...file }));
    const report: StorageReport = {
      database_bytes: 65536, attachment_bytes: fixture.media.reduce((sum, file) => sum + file.bytes, 0),
      cache_bytes: fixture.cacheBytes, other_bytes: 128, total_files, files,
      chats: ["a@s", "b@s"].map((chat) => {
        const by_kind: Record<string, number> = {};
        for (const file of fixture.media.filter((file) => file.chat === chat)) by_kind[file.kind] = (by_kind[file.kind] ?? 0) + file.bytes;
        return { chat, name: chat === "a@s" ? "Synthetic A" : "Synthetic B", by_kind,
          bytes: Object.values(by_kind).reduce((sum, bytes) => sum + bytes, 0) };
      }),
    };
    return report as T;
  }
  if (command === "storage_cleanup") {
    fixture.storageCalls++;
    if (fixture.storageFailure) throw new Error("Synthetic filesystem failure");
    const action = args?.action as StorageCleanup;
    if (action.kind === "cache") {
      const bytes = fixture.cacheBytes;
      fixture.cacheBytes = 0;
      return { files: bytes ? 1 : 0, bytes } as T;
    }
    const removed = fixture.media.filter((file) => file.chat === action.chat &&
      (action.kind === "chat_media" || (file.id === action.id && file.quoted === action.quoted)));
    fixture.media = fixture.media.filter((file) => !removed.includes(file));
    return { files: removed.length, bytes: removed.reduce((sum, file) => sum + file.bytes, 0) } as T;
  }
  if (command === "chat_settings") return {
    auto_download: null,
    retention: { max_age_hours: { kind: "inherit" }, max_messages: { kind: "limited", value: 200 }, on_demand: true },
  } as T;
  if (command === "set_chat_retention") {
    fixture.savedRetention = JSON.parse(JSON.stringify(args?.retention));
    return undefined as T;
  }
  if (command !== "boolean_props") throw new Error(`No synthetic response for ${command}`);
  fixture.calls++;
  if (fixture.failure) throw new Error("Synthetic disconnected account");
  return [
    { name: "example_enabled", code: 1, default: false, value: !fixture.updated },
    { name: "example_disabled", code: 2, default: true, value: false },
    { name: "example_missing", code: 3, default: true, value: null },
  ] as T;
}
