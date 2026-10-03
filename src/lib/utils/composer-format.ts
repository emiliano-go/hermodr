export type ComposerFormat = "bold" | "italic" | "strike" | "mono";
const markers: Record<ComposerFormat, string> = { bold: "*", italic: "_", strike: "~", mono: "```" };

export function formatDraft(text: string, start: number, end: number, kind: ComposerFormat) {
  if (start < 0 || end < start || end > text.length) return null;
  const marker = markers[kind];
  const selected = text.slice(start, end);
  if (selected.length >= marker.length * 2 && selected.startsWith(marker) && selected.endsWith(marker)) {
    const inner = selected.slice(marker.length, -marker.length);
    return { text: text.slice(0, start) + inner + text.slice(end), start, end: start + inner.length };
  }
  if (text.slice(start - marker.length, start) === marker && text.slice(end, end + marker.length) === marker) {
    return { text: text.slice(0, start - marker.length) + selected + text.slice(end + marker.length),
      start: start - marker.length, end: end - marker.length };
  }
  if (selected.includes(kind === "mono" ? "`" : marker)) return null;
  const leading = selected.match(/^\s*/)?.[0] ?? "", trailing = selected.match(/\s*$/)?.[0] ?? "";
  const inner = selected.trim();
  const value = inner ? leading + marker + inner + marker + trailing : selected + marker + marker;
  const caret = start + (inner ? leading.length : selected.length) + marker.length;
  return { text: text.slice(0, start) + value + text.slice(end), start: caret, end: caret + inner.length };
}
