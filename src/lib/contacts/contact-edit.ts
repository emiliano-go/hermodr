export type ContactDraft = { jid: string | null; phone: string; fullName: string; firstName: string; saveOnPhone: boolean };
type Call = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function contactAddress(jid: string | null, phone: string): string {
  if (jid) {
    if (!/^\d+@(s\.whatsapp\.net|lid)$/.test(jid)) throw new Error("Contact address must be a bare phone number or LID.");
    return jid;
  }
  const number = phone.trim().replace(/[\s()-]/g, "").replace(/^\+/, "");
  if (!/^[1-9]\d{6,14}$/.test(number)) throw new Error("Enter an international phone number with its country code.");
  return `${number}@s.whatsapp.net`;
}

export async function editContact(call: Call, account: string, current: () => string | null,
  draft: ContactDraft, remove = false): Promise<string> {
  if (current() !== account) throw new Error("Account changed before editing contact.");
  const jid = contactAddress(draft.jid, draft.phone);
  const fullName = draft.fullName.trim();
  if (!remove && !fullName) throw new Error("Contact name cannot be empty.");
  await call(remove ? "remove_contact" : "save_contact", remove ? { account, jid } : {
    account, jid, fullName, firstName: draft.firstName.trim() || null, saveOnPrimaryAddressbook: draft.saveOnPhone,
  });
  if (current() !== account) throw new Error("Account changed during contact edit.");
  return jid;
}
