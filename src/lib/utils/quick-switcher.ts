import type { SearchResult } from "./wire.ts";

export type QuickChat = Pick<SearchResult, "jid" | "name" | "number" | "kind" | "aliases">;
export type QuickSwitchTarget = { chat: string; label: string; messageId?: string };

export function messageSnippet(text: string, query: string): string {
  const at = text.toLowerCase().indexOf(query.trim().toLowerCase());
  const start = Math.max(0, at - 48), end = start + 180;
  return `${start ? "…" : ""}${text.slice(start, end)}${end < text.length ? "…" : ""}`;
}

function normalized(value: string): string {
  return value.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase().replace(/\s+/g, "");
}

export function fuzzyScore(query: string, value: string): number | null {
  const needle = normalized(query), text = normalized(value);
  if (!needle) return 0;
  let cursor = 0, gaps = 0;
  for (const character of needle) {
    const next = text.indexOf(character, cursor);
    if (next < 0) return null;
    gaps += next - cursor;
    cursor = next + character.length;
  }
  const score = text === needle ? 3000 : text.startsWith(needle) ? 2000 : text.includes(needle) ? 1000 : 500 - gaps;
  return score - text.length / 1000;
}

export function quickChats(recent: QuickChat[], directory: QuickChat[], query: string): QuickChat[] {
  if (!query.trim()) return recent.slice(0, 20);
  const known = new Map(recent.map((row) => [row.jid, row]));
  for (const row of directory) known.set(row.jid, row);
  return [...known.values()].map((row, order) => {
    const address = query.includes("@") ? row.jid : row.jid.split("@")[0];
    const scores = [row.name, row.number, address, ...row.aliases].map((value) => fuzzyScore(query, value))
      .filter((score): score is number => score !== null);
    return { row, order, score: scores.length ? Math.max(...scores) : null };
  }).filter((match): match is { row: QuickChat; order: number; score: number } => match.score !== null)
    .sort((a, b) => b.score - a.score || a.order - b.order).slice(0, 30).map(({ row }) => row);
}

export function quickSwitcherKey(key: string, selected: number, count: number): number | "choose" | "close" | null {
  if (key === "Escape") return "close";
  if (key === "Enter") return count ? "choose" : null;
  if ((key === "ArrowDown" || key === "ArrowUp") && count) {
    return (selected + (key === "ArrowDown" ? 1 : -1) + count) % count;
  }
  return null;
}
