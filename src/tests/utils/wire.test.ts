import assert from "node:assert/strict";
import test from "node:test";
import { fixture } from "../../lib/utils/wire.fixture.ts";
import { DEFAULT_MESSAGE_WINDOW } from "../../lib/utils/message-window.ts";

test("Rust Serde fixture keeps flattened DTO fields and v1 plugin/archive keys", () => {
  assert.equal(fixture.message.from_me, true);
  assert.equal(fixture.message.sort_order, 7);
  assert.equal(fixture.message.media_kind, "image");
  assert.equal(fixture.message.reply_to_id, "quoted");
  assert.equal(fixture.message.preview_url, "https://example.invalid/");
  for (const internal of ["header", "media", "quote", "link", "local", "system", "locator", "history_shareable"])
    assert.equal(internal in fixture.message, false, internal);
  assert.equal(fixture.event.kind, "messageHint");
  assert.equal(fixture.event.from_me, true);
  assert.equal("fromMe" in fixture.event, false);
  assert.equal(fixture.plugin.type, "event");
  assert.deepEqual(fixture.plugin.event, fixture.event);
  assert.equal(fixture.settings.message_window_size, DEFAULT_MESSAGE_WINDOW);
  assert.equal(fixture.archive.missing_attachments, 0);
});
