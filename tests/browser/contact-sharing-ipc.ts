import { makeVCard } from "../../src/lib/utils/vcard";

export const fixture = {
  calls: [] as { command: string; args?: Record<string, unknown> }[],
  failCards: false,
};

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  fixture.calls.push({ command, args });
  if (command === "own_contact_link") return `https://wa.me/qr/${args?.account === "other" ? "OWN_OTHER_77" : "OWN_FIRST_77"}` as T;
  if (command === "qr_svg") return '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/><rect x="10" y="10" width="30" height="30" fill="black"/></svg>' as T;
  if (command === "resolve_contact_link") return (String(args?.link).includes("/qr/") ? "9912345678901234@lid" : `${String(args?.link).split("/").at(-1)}@s.whatsapp.net`) as T;
  if (command === "message_contacts") {
    if (fixture.failCards) throw new Error("Synthetic card load failure");
    return [
      ["First", makeVCard("First <script>", "12025550101").replace("END:VCARD", "PHOTO:https://example.invalid/private\r\nEMAIL:private@example.invalid\r\nEND:VCARD")],
      ["Second", makeVCard("Second", "12025550102")],
    ] as T;
  }
  throw new Error(`Unexpected synthetic command: ${command}`);
}
