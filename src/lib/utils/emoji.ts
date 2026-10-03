export type Emoji = {
  emoji: string;
  label: string;
  /** Without colons, most familiar first: `joy`, `face_with_tears_of_joy`. */
  shortcodes: string[];
  tags: string[];
  group: number;
};

export type EmojiToken = { query: string; raw: string; start: number; end: number; closed: boolean };

export function emojiTokenAt(text: string, caret: number, selectionEnd = caret): EmojiToken | null {
  if (!Number.isInteger(caret) || caret !== selectionEnd || caret < 0 || caret > text.length) return null;
  const match = /(?:^|\s)(:([a-z0-9_+-]{2,})(:)?)$/i.exec(text.slice(0, caret));
  return match ? { query: match[2].toLowerCase(), raw: match[1], start: caret - match[1].length, end: caret, closed: !!match[3] } : null;
}

export function replaceEmojiToken(text: string, token: EmojiToken, emoji: string): { text: string; caret: number } | null {
  if (text.slice(token.start, token.end) !== token.raw) return null;
  return { text: text.slice(0, token.start) + emoji + text.slice(token.end), caret: token.start + emoji.length };
}

export function exactEmojiForToken(all: Emoji[], token: EmojiToken): string | null {
  return token.closed ? all.find((entry) => entry.shortcodes.includes(token.query))?.emoji ?? null : null;
}

/** Emojibase groups in picker order; 2 is skin-tone components and is left out. */
export const GROUPS: { id: number; label: string; icon: string }[] = [
  { id: 0, label: "Smileys & emotion", icon: "😀" },
  { id: 1, label: "People & body", icon: "👋" },
  { id: 3, label: "Animals & nature", icon: "🐻" },
  { id: 4, label: "Food & drink", icon: "🍔" },
  { id: 5, label: "Travel & places", icon: "✈️" },
  { id: 6, label: "Activities", icon: "⚽" },
  { id: 7, label: "Objects", icon: "💡" },
  { id: 8, label: "Symbols", icon: "❤️" },
  { id: 9, label: "Flags", icon: "🏁" },
];

type Row = { hexcode: string; label: string; unicode: string; group?: number; tags?: string[] };
type Codes = Record<string, string | string[]>;

let loading: Promise<Emoji[]> | null = null;

/** The emoji table, loaded on first use so it stays out of the startup bundle. */
export function loadEmojis(): Promise<Emoji[]> {
  loading ??= Promise.all([
    import("emojibase-data/en/compact.json"),
    import("emojibase-data/en/shortcodes/github.json"),
    import("emojibase-data/en/shortcodes/emojibase.json"),
  ]).then(([data, github, emojibase]) => {
    const list = (x: string | string[] | undefined) => (x === undefined ? [] : [x].flat());
    return (data.default as Row[])
      .filter((row) => row.group !== undefined && row.group !== 2)
      .map((row) => ({
        emoji: row.unicode,
        label: row.label,
        shortcodes: [
          ...new Set([
            ...list((github.default as Codes)[row.hexcode]),
            ...list((emojibase.default as Codes)[row.hexcode]),
          ]),
        ],
        tags: row.tags ?? [],
        group: row.group!,
      }));
  });
  return loading;
}

/** Best matches for a `:query`: shortcode prefix, then shortcode word, then label and tags. */
export function searchEmojis(all: Emoji[], query: string, limit = 24, recent: readonly string[] = []): Emoji[] {
  const q = query.toLowerCase().replace(/^:|:$/g, "");
  if (!q) return [];
  const rank = (e: Emoji) => {
    if (e.shortcodes.some((c) => c === q)) return 0;
    if (e.shortcodes.some((c) => c.startsWith(q))) return 1;
    if (e.shortcodes.some((c) => c.split(/[_-]/).some((w) => w.startsWith(q)))) return 2;
    if (e.label.toLowerCase().includes(q)) return 3;
    if (e.tags.some((t) => t.startsWith(q))) return 4;
    return 9;
  };
  return all
    .map((e) => ({ e, r: rank(e), recent: recent.indexOf(e.emoji) }))
    .filter((x) => x.r < 9)
    .sort((a, b) => a.r - b.r || (a.recent < 0 ? recent.length : a.recent) - (b.recent < 0 ? recent.length : b.recent))
    .slice(0, limit)
    .map((x) => x.e);
}

const RECENT_KEY = "postal.recentEmoji";

export function recentEmojis(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

export function rememberEmoji(emoji: string) {
  const next = [emoji, ...recentEmojis().filter((e) => e !== emoji)].slice(0, 32);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Recents are a convenience; losing them is harmless.
  }
}
