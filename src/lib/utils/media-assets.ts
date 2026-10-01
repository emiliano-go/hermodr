type Authorize = (paths: string[]) => Promise<Record<string, string | null>>;
const FIELDS = new Set(["media_path", "reply_to_path", "reply_to_thumb", "media_thumb", "path", "tray_path", "picture", "avatar"]);
const SCALARS = new Set(["avatar", "save_sticker", "download_sticker", "playable_audio", "playable_video"]);
const DTO_COMMANDS = new Set(["messages", "message_page", "starred_messages", "pings", "search_messages", "switcher_messages",
  "gallery_page", "sticker_library", "sticker_pack", "invite_info", "admin_reports"]);

function absolutePath(value: unknown): value is string {
  return typeof value === "string" && /^(?:[a-z]:[\\/]|\\\\|\/)/i.test(value);
}

/** Canonicalizes known returned media paths through native file authorization. */
export function createMediaAssetPreparer(authorize: Authorize) {
  const jobs = new Map<string, { promise: Promise<string | null>; resolve: (path: string | null) => void }>();
  const pending: string[] = [];
  let draining = false;

  async function drain() {
    while (pending.length) {
      const batch = pending.splice(0, 512);
      let granted: Record<string, string | null> = {};
      try { granted = await authorize(batch); } catch {}
      for (const path of batch) {
        const canonical = granted?.[path];
        jobs.get(path)!.resolve(absolutePath(canonical) ? canonical : null);
        jobs.delete(path);
      }
    }
    draining = false;
  }

  function preparePath(path: string): Promise<string | null> {
    const existing = jobs.get(path);
    if (existing) return existing.promise;
    let resolve!: (path: string | null) => void;
    const promise = new Promise<string | null>((done) => { resolve = done; });
    jobs.set(path, { promise, resolve });
    pending.push(path);
    if (!draining) { draining = true; queueMicrotask(() => void drain()); }
    return promise;
  }

  return async function prepare<T>(command: string, value: T): Promise<T> {
    if (!DTO_COMMANDS.has(command) && !SCALARS.has(command) && command !== "media_library") return value;
    let result: unknown = value;
    const targets: { path: string; set: (path: string | null) => void }[] = [];
    function visit(value: unknown) {
      if (Array.isArray(value)) { for (const row of value) visit(row); return; }
      if (!value || typeof value !== "object" || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) return;
      const row = value as Record<string, unknown>;
      for (const [key, field] of Object.entries(row)) {
        if (FIELDS.has(key) && absolutePath(field)) targets.push({ path: field, set: (path) => { row[key] = path; } });
        else visit(field);
      }
    }
    if (SCALARS.has(command) && absolutePath(result)) {
      targets.push({ path: result, set: (path) => { result = path; } });
    } else if (command === "media_library" && Array.isArray(result)) {
      const rows = result;
      rows.forEach((path, index) => { if (absolutePath(path)) targets.push({ path, set: (value) => { rows[index] = value; } }); });
    }
    if (DTO_COMMANDS.has(command)) visit(result);
    await Promise.all(targets.map(async ({ path, set }) => set(await preparePath(path))));
    if (SCALARS.has(command) && command !== "avatar" && result === null) throw new Error(`Media file authorization failed for ${command}`);
    if (command === "media_library" && Array.isArray(result)) result = result.filter((path) => path !== null);
    return result as T;
  };
}
