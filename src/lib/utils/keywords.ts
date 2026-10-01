import type { StoredMessage } from "./models";

export type KeywordRules = { highlight: string[]; hide: string[] };
export type KeywordStorage = Pick<Storage, "getItem" | "setItem">;
export type KeywordMessage = Pick<StoredMessage, "text" | "from_me" | "deleted" | "revoked" | "system_kind" | "media_kind" | "media_once_kind" | "spoiler">;
export const MAX_KEYWORDS = 50;
export const MAX_KEYWORD_LENGTH = 100;
const empty = (): KeywordRules => ({ highlight: [], hide: [] });

export function keywordTerms(value: unknown): string[] {
  if (!Array.isArray(value) || value.length > MAX_KEYWORDS) throw new Error("Each keyword list supports up to 50 entries.");
  const result: string[] = [], seen = new Set<string>();
  for (const valueTerm of value) {
    if (typeof valueTerm !== "string") throw new Error("Keywords must be text.");
    const term = valueTerm.trim();
    if ([...term].length > MAX_KEYWORD_LENGTH) throw new Error("Each keyword can contain up to 100 characters.");
    if (term && !seen.has(term.toLowerCase())) { seen.add(term.toLowerCase()); result.push(term); }
  }
  return result;
}

export function keywordRules(value: unknown): KeywordRules {
  if (!value || typeof value !== "object" || !("highlight" in value) || !("hide" in value)) throw new Error("Invalid keyword lists.");
  return { highlight: keywordTerms(value.highlight), hide: keywordTerms(value.hide) };
}

export function keywordStorageKey(account: string): string {
  if (!account) throw new Error("Select an account before changing keyword rules.");
  return `postal.keywords.${JSON.stringify(account)}`;
}

export function loadKeywordRules(account: string, storage?: KeywordStorage): { rules: KeywordRules; error: string | null } {
  try {
    const saved = (storage ?? localStorage).getItem(keywordStorageKey(account));
    if (saved === null) return { rules: empty(), error: null };
    if (saved.length > 65536) throw new Error("Saved keyword rules are too large.");
    const value: unknown = JSON.parse(saved);
    if (!value || typeof value !== "object" || !("version" in value) || value.version !== 1) throw new Error("Unsupported keyword rules version.");
    return { rules: keywordRules(value), error: null };
  } catch (error) {
    return { rules: empty(), error: `Could not load keyword rules; existing stored data was kept. ${String(error)}` };
  }
}

export function saveKeywordRules(account: string, value: KeywordRules, storage: KeywordStorage = localStorage): KeywordRules {
  const rules = keywordRules(value);
  storage.setItem(keywordStorageKey(account), JSON.stringify({ version: 1, ...rules }));
  return rules;
}

export function keywordBody(message: KeywordMessage): string {
  if (message.from_me || message.deleted || message.revoked || message.spoiler || message.system_kind !== null
    || message.media_kind === "view_once" || message.media_kind === "unknown" || message.media_once_kind !== null) return "";
  if (message.media_kind && message.text.trim() === `[${message.media_kind}]`) return "";
  return message.text;
}

export function keywordMatch(text: string, terms: readonly string[]): boolean {
  if (!text || !terms.length) return false;
  const body = text.toLowerCase();
  return terms.some((term) => !!term && body.includes(term.toLowerCase()));
}

export function keywordHidden(message: KeywordMessage, rules: KeywordRules): boolean {
  return keywordMatch(keywordBody(message), rules.hide);
}

export function keywordHighlighted(message: KeywordMessage, rules: KeywordRules): boolean {
  const body = keywordBody(message);
  return !keywordMatch(body, rules.hide) && keywordMatch(body, rules.highlight);
}
