export const MAX_SHARED_CONTACTS = 50;
export const MAX_VCARD_BYTES = 16 * 1024;
export const MAX_CONTACT_BATCH_BYTES = 256 * 1024;

export type ParsedContact = { name: string; phones: string[] };
export type SharedContact = [name: string, phone: string];
export type ContactShareScope = { account: string; chat: string; generation: number };

export function contactPhone(value: string): string {
  const phone = value.trim().replace(/^tel:/i, "").replace(/[\s().-]/g, "").replace(/^\+/, "");
  if (!/^[1-9]\d{6,14}$/.test(phone)) throw new Error("Use an international phone number with its country code.");
  return phone;
}

export function contactLinkJid(value: string): string | null {
  if (value.length > 2048) throw new Error("Contact link is too long.");
  const link = value.trim();
  if (/^https:\/\/wa\.me\/qr\/[A-Za-z0-9_-]{1,256}$/.test(link)) return null;
  const number = /^https:\/\/wa\.me\/([1-9]\d{6,14})\/?$/.exec(link)?.[1];
  if (!number) throw new Error("Paste an https://wa.me/qr contact link or wa.me phone link.");
  return `${number}@s.whatsapp.net`;
}

export function isContactJid(value: string): boolean {
  return /^[1-9]\d{6,14}@s\.whatsapp\.net$/.test(value) || /^\d{1,32}@lid$/.test(value);
}

export function contactPhoneJid(value: string): string | null {
  try { return `${contactPhone(value)}@s.whatsapp.net`; } catch { return null; }
}

function escapeText(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/;/g, "\\;").replace(/,/g, "\\,");
}

function foldLine(value: string): string {
  let bytes = 0, folded = "";
  for (const character of value) {
    const size = new TextEncoder().encode(character).length;
    if (bytes + size > 75) { folded += "\r\n "; bytes = 1; }
    folded += character;
    bytes += size;
  }
  return folded;
}

export function makeVCard(name: string, phone: string): string {
  name = name.trim();
  if (!name || [...name].length > 160 || /[\u0000-\u001f\u007f-\u009f]/.test(name)) throw new Error("Contact name is empty, too long, or contains control characters.");
  const number = contactPhone(phone), escaped = escapeText(name);
  return `BEGIN:VCARD\r\nVERSION:3.0\r\n${foldLine(`FN:${escaped}`)}\r\n${foldLine(`N:;${escaped};;;`)}\r\nTEL;TYPE=CELL;waid=${number}:+${number}\r\nEND:VCARD\r\n`;
}

function unescapeText(value: string): string {
  return value.replace(/\\([nN,;\\])/g, (_, character: string) => /[nN]/.test(character) ? "\n" : character);
}

export function parseVCard(value: string, fallbackName = ""): ParsedContact {
  if (value.length > MAX_VCARD_BYTES || new TextEncoder().encode(value).length > MAX_VCARD_BYTES) throw new Error("Contact vCard is too large.");
  const lines = value.replace(/\r\n/g, "\n").replace(/\n[ \t]/g, "").trim().split("\n");
  if (lines.length > 1024 || /[\u0000-\u0008\u000b-\u001f\u007f]/.test(lines.join("\n"))
    || lines[0]?.toUpperCase() !== "BEGIN:VCARD" || lines.at(-1)?.toUpperCase() !== "END:VCARD") throw new Error("Invalid contact vCard.");
  let name = "", version = "";
  const phones: string[] = [];
  for (const line of lines.slice(1, -1)) {
    const colon = line.indexOf(":");
    if (colon < 0) { if (!line.trim()) continue; throw new Error("Invalid contact vCard line."); }
    const header = line.slice(0, colon), key = header.split(";")[0].split(".").at(-1)?.toUpperCase();
    const text = line.slice(colon + 1);
    if (key === "BEGIN" || key === "END") throw new Error("Share one vCard per contact.");
    if (key === "VERSION") { if (version) throw new Error("Duplicate vCard version."); version = text; }
    if (key !== "FN" && key !== "TEL") continue;
    if (/;ENCODING=/i.test(header)) throw new Error("This vCard text encoding cannot be read here.");
    const decoded = unescapeText(text);
    if (decoded.length > 1024) throw new Error("Contact field is too long.");
    if (key === "FN") { if (name) throw new Error("Duplicate contact name."); name = decoded; }
    else if (decoded.trim() && !phones.includes(decoded)) phones.push(decoded);
    if (phones.length > 10) throw new Error("Contact has too many phone numbers.");
  }
  if (!["2.1", "3.0", "4.0"].includes(version)) throw new Error("Unsupported vCard version.");
  return { name: name.trim() || fallbackName.slice(0, 1024) || "Contact", phones };
}

export function parseContactCards(cards: [string, string][]): ParsedContact[] {
  if (!cards.length || cards.length > MAX_SHARED_CONTACTS) throw new Error("Contact payload has too many contacts.");
  let bytes = 0;
  return cards.map(([name, value]) => {
    bytes += new TextEncoder().encode(name + value).length;
    if (name.length > 1024 || bytes > MAX_CONTACT_BATCH_BYTES) throw new Error("Contact payload is too large.");
    return parseVCard(value, name);
  });
}

export async function shareContacts(
  send: (contacts: SharedContact[], scope: ContactShareScope) => Promise<string>,
  current: () => ContactShareScope | null,
  scope: ContactShareScope,
  contacts: SharedContact[],
): Promise<string | null> {
  scope = { ...scope };
  const matches = () => { const active = current(); return active?.account === scope.account && active.chat === scope.chat && active.generation === scope.generation; };
  if (!matches()) throw new Error("Conversation changed before sharing contacts.");
  if (!contacts.length || contacts.length > MAX_SHARED_CONTACTS) throw new Error("Choose between 1 and 50 contacts.");
  const batch = contacts.map(([name, phone]) => { makeVCard(name, phone); return [name.trim(), contactPhone(phone)] as SharedContact; });
  if (new Set(batch.map(([, phone]) => phone)).size !== batch.length) throw new Error("The same phone number was selected twice.");
  const acknowledgement = await send(batch, { ...scope });
  if (!matches()) return null;
  if (typeof acknowledgement !== "string" || !acknowledgement.trim() || acknowledgement.length > 256 || /[\u0000-\u001f]/.test(acknowledgement)) throw new Error("Contact send was not acknowledged.");
  return acknowledgement;
}
