// WhatsApp's message formatting, parsed into a tree the UI renders directly,
// so nothing from a message is ever treated as HTML.

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "link"; url: string }
  | { kind: "code"; text: string }
  | { kind: "mention"; user: string }
  | { kind: "bold" | "italic" | "strike"; children: Inline[] };

export type Block =
  | { kind: "para"; lines: Inline[][] }
  | { kind: "quote"; lines: Inline[][] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  | { kind: "pre"; text: string };

// A marker only opens after a non-word character and closes before one, with
// no space just inside it, which is how WhatsApp keeps `2*3*4` literal.
const SPANS: { kind: "bold" | "italic" | "strike"; re: RegExp }[] = [
  { kind: "bold", re: /(^|[^\p{L}\p{N}*])\*(?=\S)([^*\n]*?\S)\*(?![\p{L}\p{N}*])/u },
  { kind: "italic", re: /(^|[^\p{L}\p{N}_])_(?=\S)([^_\n]*?\S)_(?![\p{L}\p{N}_])/u },
  { kind: "strike", re: /(^|[^\p{L}\p{N}~])~(?=\S)([^~\n]*?\S)~(?![\p{L}\p{N}~])/u },
];
const CODE = /`([^`\n]+)`/;
const URL = /https?:\/\/[^\s<>()[\]{}"']+[^\s<>()[\]{}"'.,;:!?]/;
// The wire names a mention by the user part of its JID: a phone number or a LID.
const MENTION = /(^|[^\p{L}\p{N}_])@(\d{6,20})(?!\p{N})/u;

export function inline(text: string): Inline[] {
  const out: Inline[] = [];
  let rest = text;
  while (rest) {
    // Earliest construct wins; code and links are taken whole, spans recurse.
    let best: { at: number; length: number; node: Inline } | null = null;
    const consider = (at: number, length: number, node: Inline) => {
      if (!best || at < best.at) best = { at, length, node };
    };
    const code = CODE.exec(rest);
    if (code) consider(code.index, code[0].length, { kind: "code", text: code[1] });
    const url = URL.exec(rest);
    if (url) consider(url.index, url[0].length, { kind: "link", url: url[0] });
    const mention = MENTION.exec(rest);
    if (mention) {
      const at = mention.index + mention[1].length;
      consider(at, mention[0].length - mention[1].length, { kind: "mention", user: mention[2] });
    }
    for (const { kind, re } of SPANS) {
      const m = re.exec(rest);
      if (m) consider(m.index + m[1].length, m[0].length - m[1].length, { kind, children: inline(m[2]) });
    }
    const found = best as { at: number; length: number; node: Inline } | null;
    if (!found) {
      out.push({ kind: "text", text: rest });
      break;
    }
    if (found.at > 0) out.push({ kind: "text", text: rest.slice(0, found.at) });
    out.push(found.node);
    rest = rest.slice(found.at + found.length);
  }
  return out;
}

const blockCache = new Map<string, Block[]>();

export function blocks(text: string): Block[] {
  const cached = blockCache.get(text);
  if (cached) return cached;
  const out: Block[] = [];
  const parts = text.split(/```/);
  parts.forEach((part, i) => {
    // Odd parts sat between a pair of fences; an unpaired last fence stays literal.
    if (i % 2 === 1 && i < parts.length - 1) {
      out.push({ kind: "pre", text: part.replace(/^\n|\n$/g, "") });
      return;
    }
    const chunk = i % 2 === 1 ? "```" + part : part;
    for (const line of chunk.split("\n")) {
      const quote = /^>\s?(.*)$/.exec(line);
      const bullet = /^[-*•]\s+(.+)$/.exec(line);
      const number = /^\d+\.\s+(.+)$/.exec(line);
      const last = out[out.length - 1];
      if (quote) {
        if (last?.kind === "quote") last.lines.push(inline(quote[1]));
        else out.push({ kind: "quote", lines: [inline(quote[1])] });
      } else if (bullet || number) {
        const ordered = !!number;
        const item = inline((bullet ?? number)![1]);
        if (last?.kind === "list" && last.ordered === ordered) last.items.push(item);
        else out.push({ kind: "list", ordered, items: [item] });
      } else if (last?.kind === "para") {
        last.lines.push(inline(line));
      } else {
        out.push({ kind: "para", lines: [inline(line)] });
      }
    }
  });
  // Blank lines around fences leave empty paragraphs behind.
  const result = out.filter((b) => b.kind !== "para" || b.lines.some((l) => l.length > 0));
  // Parsing is pure; keep the last thousand texts so re-renders do not re-parse
  // a chat full of long messages.
  if (blockCache.size >= 1000) blockCache.clear();
  blockCache.set(text, result);
  return result;
}

/** The text without its formatting marks, for previews and notifications. */
export function plain(text: string, mentionName: (user: string) => string = (user) => user): string {
  const flat = (nodes: Inline[]): string =>
    nodes
      .map((n) =>
        n.kind === "text" || n.kind === "code"
          ? n.text
          : n.kind === "link"
            ? n.url
            : n.kind === "mention"
              ? `@${mentionName(n.user)}`
              : flat(n.children),
      )
      .join("");
  return blocks(text)
    .map((b) =>
      b.kind === "pre" ? b.text : b.kind === "list" ? b.items.map(flat).join(" ") : b.lines.map(flat).join(" "),
    )
    .join(" ");
}

// Self-check, run with `node --experimental-strip-types src/lib/format.ts`.
const argv = (globalThis as { process?: { argv?: string[] } }).process?.argv;
if (argv?.[1]?.endsWith("format.ts")) {
  const assert = (ok: boolean, what: string) => {
    if (!ok) throw new Error(what);
  };
  const json = (x: unknown) => JSON.stringify(x);
  assert(json(inline("*hi*")) === json([{ kind: "bold", children: [{ kind: "text", text: "hi" }] }]), "bold");
  assert(json(inline("2*3*4")) === json([{ kind: "text", text: "2*3*4" }]), "math stays literal");
  assert(inline("a _b ~c~_ d")[1].kind === "italic", "nesting");
  assert(inline("see https://x.io/a.").some((n) => n.kind === "link" && n.url === "https://x.io/a"), "url drops trailing dot");
  assert(blocks("> a\n> b\nc")[0].kind === "quote" && blocks("> a\n> b\nc")[1].kind === "para", "quote block");
  assert(blocks("```\nx *y*\n```")[0].kind === "pre", "fenced code");
  assert(blocks("- a\n- b")[0].kind === "list", "bullets");
  assert(plain("*bold* and _it_") === "bold and it", "plain");
  assert(json(inline("hi @59891954564!")[1]) === json({ kind: "mention", user: "59891954564" }), "mention");
  assert(inline("mail a@123456789").every((n) => n.kind === "text"), "no mention inside a word");
  assert(plain("*@123456789* ok", () => "Ana") === "@Ana ok", "mention in bold, named");
  console.log("format.ts ok");
}
