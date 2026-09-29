/** Up to two letters for an avatar, or a digit pair for a bare number. */
export function initials(label: string) {
  const words = label.replace(/[^\p{L}\p{N}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "#";
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[1][0]).toUpperCase();
}

/** A stable hue per chat, so an avatar keeps its colour across sessions. */
export function hue(jid: string) {
  // A device suffix would give one person two colours across typing and messages.
  jid = jid.replace(/:\d+(?=@)/, "");
  let h = 0;
  for (const c of jid) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}
