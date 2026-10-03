import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { changeText, historyReceivers, historyResultText } from "../lib/utils/group-actions.ts";
import type { GroupHistoryOffer, ParticipantChange } from "../lib/utils/models.ts";
import { LocalizedError } from "../lib/i18n/errors.ts";
import { t } from "../lib/i18n/localizer.ts";

const offer: GroupHistoryOffer = { enabled: true, reason: null, max_messages: 100, time_window_seconds: 7 * 86400 };

test("history recipients require explicit opt-in and an available offer", () => {
  const selected = ["100@s.whatsapp.net", "200@lid"];
  assert.deepEqual(historyReceivers(selected, false, offer), []);
  assert.deepEqual(historyReceivers(selected, false, null), []);
  assert.deepEqual(historyReceivers(selected, true, offer), selected);
  assert.notEqual(historyReceivers(selected, true, offer), selected);
  assert.throws(() => historyReceivers(selected, true, null), /unavailable/);
  assert.throws(() => historyReceivers(selected, true, { ...offer, enabled: false, reason: "WhatsApp disabled group history." }),
    (error) => error instanceof LocalizedError && error.code === "error.group_history_unavailable"
      && error.diagnostic === "WhatsApp disabled group history.");
});

test("participant outcomes remain independent from history acceptance", () => {
  const added: ParticipantChange = { jid: "100@s.whatsapp.net", ok: true, code: null, error: null, pending: false };
  assert.equal(changeText(added), null);
  assert.equal(changeText({ ...added, pending: true }), "Sent for approval.");
  assert.equal(changeText({ ...added, ok: false, code: "403" }), "Not allowed (admin rights or privacy).");
  const legacy = { state: "upload_failed", message: "The upload failed; membership was added.", retry_id: "same-bundle" };
  assert.equal(historyResultText(legacy), t("group_action.history_legacy_result"));
  assert.equal(legacy.message, "The upload failed; membership was added.");
  const shared = historyResultText({ state: "shared", message: "Delivered to everyone", retry_id: null });
  assert.match(shared, /accepted/);
  assert.match(shared, /not been confirmed/);
  assert.doesNotMatch(shared, /Delivered to everyone/);
});

test("the member picker renders history off and unavailable until policy is loaded", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/group-history", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: AddMembers } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/AddMembers.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const result = render(AddMembers, { props: { chat: "1@g.us", title: "Synthetic group", members: [], avatars: {}, me: null,
      onavatar() {}, onclose() {}, async onadd() { throw new Error("render must not add members"); },
      async onretryhistory() { throw new Error("render must not retry history"); } } });
    const checkbox = result.body.match(/<input[^>]*type="checkbox"[^>]*>/)?.[0];
    assert.ok(checkbox);
    assert.match(checkbox, /disabled/);
    assert.doesNotMatch(checkbox, /checked/);
    assert.match(result.body, /Checking history sharing availability/);
    assert.match(result.body, /Share recent history with selected people/);
  } finally { await server.close(); }
});
