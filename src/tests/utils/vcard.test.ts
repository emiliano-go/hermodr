import assert from "node:assert/strict";
import test from "node:test";
import { contactLinkJid, contactPhoneJid, isContactJid, makeVCard, parseVCard, parseContactCards, shareContacts, MAX_VCARD_BYTES } from "../../lib/utils/vcard.ts";

test("vCard names escape punctuation and fold UTF-8 without leaking extra fields", () => {
  for (const name of ["Comma, Semi; Slash\\", "é界".repeat(60)]) {
    const card = makeVCard(name, "+598 12345678");
    assert.ok(card.split("\r\n").every((line) => new TextEncoder().encode(line).length <= 75));
    assert.deepEqual(parseVCard(card), { name, phones: ["+59812345678"] });
    assert.doesNotMatch(card, /EMAIL:|PHOTO:|ADR:|NOTE:|ORG:/);
  }
  assert.throws(() => makeVCard("Name\nPHOTO:secret", "59812345678"));
  assert.throws(() => makeVCard("Name", "123;PHOTO:secret"));
});

test("vCard parser reads grouped and URI telephone fields but ignores private and remote properties", () => {
  const card = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Name\\, Test\r\nitem1.TEL;VALUE=uri:tel:+59812345678\r\nTEL:local ext 5\r\nPHOTO:https://example.invalid/secret\r\nEMAIL:hidden@example.invalid\r\nNOTE:private\r\nEND:VCARD\r\n";
  assert.deepEqual(parseVCard(card), { name: "Name, Test", phones: ["tel:+59812345678", "local ext 5"] });
  assert.equal(contactPhoneJid("tel:+59812345678"), "59812345678@s.whatsapp.net");
  assert.equal(contactPhoneJid("local ext 5"), null);
  assert.throws(() => parseVCard(card + card));
  assert.throws(() => parseVCard("x".repeat(MAX_VCARD_BYTES + 1)));
  assert.throws(() => parseContactCards(Array(51).fill(["Name", makeVCard("Name", "59812345678")])));
  assert.deepEqual(parseContactCards([["First", makeVCard("First", "59812345678")], ["Second", makeVCard("Second", "59812345679")]]).map((contact) => contact.name), ["First", "Second"]);
});

test("contact links accept opaque QR tokens and secondary phone links, never arbitrary targets", () => {
  assert.equal(contactLinkJid(" https://wa.me/59812345678/ "), "59812345678@s.whatsapp.net");
  assert.equal(contactLinkJid("https://wa.me/qr/SCAN_TOKEN-77"), null);
  assert.equal(isContactJid("9912345678901234@lid"), true);
  assert.equal(isContactJid("12025550101@s.whatsapp.net"), true);
  for (const jid of ["12025550101@g.us", "person@evil.invalid", "12025550101:3@s.whatsapp.net"]) assert.equal(isContactJid(jid), false);
  for (const link of ["http://wa.me/59812345678", "https://wa.me.evil/59812345678", "https://wa.me@evil/59812345678",
    "https://wa.me:443/59812345678", "https://wa.me/qr/", "https://wa.me/qr/CODE%0a", "https://wa.me/qr/CODE/extra", "https://wa.me/qr/CODE?text=secret", "https://wa.me/+59812345678", "https://wa.me/59812345678?text=x",
    "https://wa.me/59812345678#x", "https://wa.me/59812345678/../1234567", "https://wa.me/0000000", "https://wa.me/59812345678%0a", "x".repeat(2049)]) assert.throws(() => contactLinkJid(link), link);
});

test("multi-contact sharing binds scope, requires acknowledgement and ignores stale completion", async () => {
  const scope = { account: "synthetic-account", chat: "synthetic-chat", generation: 1 };
  let active: typeof scope | null = scope;
  const cards: [string, string][] = [["First", "59812345678"], ["Second", "59812345679"]];
  let calls = 0;
  const send = async (batch: [string, string][], captured: typeof scope) => { calls++; assert.deepEqual(batch, cards); assert.deepEqual(captured, scope); return "synthetic-id"; };
  assert.equal(await shareContacts(send, () => active, scope, cards), "synthetic-id");
  await assert.rejects(shareContacts(async () => "", () => active, scope, cards), /not acknowledged/);
  await assert.rejects(shareContacts(async () => { throw new Error("send failed"); }, () => active, scope, cards), /send failed/);
  assert.deepEqual(cards, [["First", "59812345678"], ["Second", "59812345679"]]);
  assert.equal(await shareContacts(async () => { active = { ...scope, generation: 2 }; return "late-id"; }, () => active, scope, cards), null);
  active = null;
  await assert.rejects(shareContacts(send, () => active, scope, cards), /Conversation changed/);
  assert.equal(calls, 1);
});
