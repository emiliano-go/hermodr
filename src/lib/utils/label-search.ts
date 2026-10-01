export function labelSearch(query: string): { name: string; query: string } | null {
  const match = /(?:^|\s)label:(?:"([^"]+)"|'([^']+)'|(\S+))/i.exec(query);
  if (!match) return null;
  return { name: (match[1] ?? match[2] ?? match[3]).trim(), query: (query.slice(0, match.index) + " " + query.slice(match.index + match[0].length)).trim() };
}
