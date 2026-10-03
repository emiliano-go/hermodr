const VERSION = 1;

export function draftKey(account: string): string {
  return `postal.drafts.${JSON.stringify(account)}`;
}

export function readDrafts(storage: Pick<Storage, "getItem">, account: string): Map<string, string> {
  const raw = storage.getItem(draftKey(account));
  if (!raw) return new Map();
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return new Map();
    const record = parsed as { version?: unknown; drafts?: unknown };
    if (record.version !== VERSION || !record.drafts || typeof record.drafts !== "object" || Array.isArray(record.drafts)) return new Map();
    return new Map(Object.entries(record.drafts).filter(([chat, text]) => chat.length > 0 && typeof text === "string" && text.length > 0) as [string, string][]);
  } catch {
    return new Map();
  }
}

export function writeDrafts(storage: Pick<Storage, "setItem" | "removeItem">, account: string, drafts: Map<string, string>): void {
  const key = draftKey(account);
  if (drafts.size) storage.setItem(key, JSON.stringify({ version: VERSION, drafts: Object.fromEntries(drafts) }));
  else storage.removeItem(key);
}

export function draftPreview(text: string): string {
  return text.trim().replace(/\s+/g, " ");
}
