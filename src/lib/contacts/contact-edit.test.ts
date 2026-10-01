import { test } from "node:test";
import assert from "node:assert/strict";
import { contactAddress, editContact, type ContactDraft } from "./contact-edit.ts";

const draft: ContactDraft = { jid: null, phone: "+598 (919) 54564", fullName: " Saved name ", firstName: " First ", saveOnPhone: true };

test("contact editor validates phone input and preserves typed LID addressing", () => {
  assert.equal(contactAddress(null, draft.phone), "59891954564@s.whatsapp.net");
  assert.equal(contactAddress("77@lid", ""), "77@lid");
  for (const phone of ["123", "001234567", "name", "+1234567890123456", "1234567@g.us"]) assert.throws(() => contactAddress(null, phone));
  for (const jid of ["77:2@lid", "77@g.us", "@lid", "77@newsletter"]) assert.throws(() => contactAddress(jid, ""));
});

test("contact editor sends account-bound provider options, waits for success and propagates failures", async () => {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let account = "account-a";
  let release!: () => void;
  const pending = new Promise<void>((resolve) => { release = resolve; });
  let settled = false;
  const save = editContact(async (command, args) => { calls.push({ command, args }); await pending; }, account, () => account, draft)
    .then((jid) => { settled = true; return jid; });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(settled, false);
  assert.deepEqual(calls, [{ command: "save_contact", args: { account: "account-a", jid: "59891954564@s.whatsapp.net",
    fullName: "Saved name", firstName: "First", saveOnPrimaryAddressbook: true } }]);
  release(); assert.equal(await save, "59891954564@s.whatsapp.net");
  const removals: { command: string; args: Record<string, unknown> }[] = [];
  await editContact(async (command, args) => { removals.push({ command, args }); }, account, () => account, { ...draft, jid: "77@lid", firstName: "", saveOnPhone: false }, true);
  assert.deepEqual(removals, [{ command: "remove_contact", args: { account: "account-a", jid: "77@lid" } }]);
  await assert.rejects(editContact(async () => { throw new Error("provider rejected"); }, account, () => account, draft), /provider rejected/);
  let invoked = false;
  await assert.rejects(editContact(async () => { invoked = true; }, "account-b", () => account, draft), /Account changed before/);
  assert.equal(invoked, false);
  await assert.rejects(editContact(async () => { account = "account-b"; }, "account-a", () => account, draft), /Account changed during/);
});
