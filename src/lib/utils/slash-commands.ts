import { fuzzyScore } from "./quick-switcher.ts";

export type SlashCommandId = "poll" | "event" | "sticker" | "gif" | "location" | "mention-all" | "keep-in-chat";
export type SlashToken = { start: number; end: number; query: string; raw: string };
export type SlashSelection = { command: SlashCommandId; token: SlashToken; account: string; chat: string; generation: number };
export type SlashCommand = { id: SlashCommandId; label: string; description: string; aliases: string[] };
export const SLASH_COMMANDS: SlashCommand[] = [
  { id: "poll", label: "Poll", description: "Create a poll", aliases: ["vote", "question"] },
  { id: "event", label: "Event", description: "Create an event", aliases: ["calendar", "meeting"] },
  { id: "sticker", label: "Sticker", description: "Open your stickers", aliases: ["stickers"] },
  { id: "gif", label: "GIF", description: "Open GIFs", aliases: ["animation"] },
  { id: "location", label: "Location", description: "Share a location", aliases: ["map", "place"] },
  { id: "mention-all", label: "Mention all", description: "Mention everyone in this group", aliases: ["all", "everyone"] },
  { id: "keep-in-chat", label: "Keep in chat", description: "Choose a message to keep", aliases: ["keep", "disappearing"] },
];

export function slashToken(draft: string, caret: number, end = caret): SlashToken | null {
  if (!Number.isInteger(caret) || caret !== end || caret < 0 || caret > draft.length) return null;
  const before = draft.slice(0, caret), start = before.lastIndexOf("/");
  if (start < 0 || start > 0 && !/\s/.test(before[start - 1])) return null;
  const raw = before.slice(start), match = /^\/([\p{L}\p{N}_-]*)$/u.exec(raw);
  return match ? { start, end: caret, query: match[1], raw } : null;
}

export function replaceSlashToken(draft: string, token: SlashToken, replacement = ""): string | null {
  if (token.start < 0 || token.end < token.start || token.end > draft.length || draft.slice(token.start, token.end) !== token.raw) return null;
  return draft.slice(0, token.start) + replacement + draft.slice(token.end);
}

export function slashCommands(query: string): SlashCommand[] {
  return SLASH_COMMANDS.map((command, order) => {
    const scores = [command.id, command.label, ...command.aliases].map((value) => fuzzyScore(query, value)).filter((score): score is number => score !== null);
    return { command, order, score: scores.length ? Math.max(...scores) : null };
  }).filter((row): row is { command: SlashCommand; order: number; score: number } => row.score !== null)
    .sort((a, b) => b.score - a.score || a.order - b.order).map(({ command }) => command);
}

export function slashCommandKey(event: Pick<KeyboardEvent, "key" | "isComposing" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">,
  index: number, count: number): number | "choose" | "close" | null {
  if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey && (event.key === "Enter" || event.key === "Tab")) return null;
  if (event.key === "Escape") return "close";
  if ((event.key === "Enter" || event.key === "Tab") && count) return "choose";
  if ((event.key === "ArrowDown" || event.key === "ArrowUp") && count) return (index + (event.key === "ArrowDown" ? 1 : -1) + count) % count;
  return null;
}
