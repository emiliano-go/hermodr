import { normalizeError } from "../i18n/errors.ts";
export type ContactDraft = { jid: string | null; phone: string; fullName: string; firstName: string; saveOnPhone: boolean };
type Call = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function contactAddress(jid: string | null, phone: string): string {
  if (jid) {
    if (!/^\d+@(s\.whatsapp\.net|lid)$/.test(jid)) throw normalizeError({ kind: "postal_error", code: "error.contact_address", params: {} });
    return jid;
  }
  const number = phone.trim().replace(/[\s()-]/g, "").replace(/^\+/, "");
  if (!/^[1-9]\d{6,14}$/.test(number)) throw normalizeError({ kind: "postal_error", code: "error.contact_phone", params: {} });
  return `${number}@s.whatsapp.net`;
}

export async function editContact(call: Call, account: string, current: () => string | null,
  draft: ContactDraft, remove = false): Promise<string> {
  if (current() !== account) throw normalizeError({ kind: "postal_error", code: "error.contact_account_before", params: {} });
  const jid = contactAddress(draft.jid, draft.phone);
  const fullName = draft.fullName.trim();
  if (!remove && !fullName) throw normalizeError({ kind: "postal_error", code: "error.contact_name", params: {} });
  await call(remove ? "remove_contact" : "save_contact", remove ? { account, jid } : {
    account, jid, fullName, firstName: draft.firstName.trim() || null, saveOnPrimaryAddressbook: draft.saveOnPhone,
  });
  if (current() !== account) throw normalizeError({ kind: "postal_error", code: "error.contact_account_during", params: {} });
  return jid;
}
