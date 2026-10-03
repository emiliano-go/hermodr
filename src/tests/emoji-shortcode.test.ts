import { test } from "node:test";
import assert from "node:assert/strict";
import { emojiTokenAt, exactEmojiForToken, replaceEmojiToken, searchEmojis, type Emoji } from "../lib/utils/emoji.ts";

const emojis: Emoji[] = [
  { emoji: "😂", label: "face with tears of joy", shortcodes: ["joy"], tags: [], group: 0 },
  { emoji: "🔥", label: "fire", shortcodes: ["fire"], tags: [], group: 0 },
  { emoji: "💗", label: "heart", shortcodes: ["heart"], tags: [], group: 0 },
  { emoji: "❤️", label: "red heart", shortcodes: ["heart_red"], tags: [], group: 0 },
];

test("shortcode detection respects caret, whitespace and other composer tokens", () => {
  assert.deepEqual(emojiTokenAt("Hi :joy", 7), { query: "joy", raw: ":joy", start: 3, end: 7, closed: false });
  assert.deepEqual(emojiTokenAt("@Ada /poll :FiRe:", 17), { query: "fire", raw: ":FiRe:", start: 11, end: 17, closed: true });
  assert.equal(emojiTokenAt("foo:joy", 7), null);
  assert.equal(emojiTokenAt(":joy ", 5), null);
  assert.equal(emojiTokenAt(":joy", 2, 4), null);
  assert.equal(emojiTokenAt(":joy", 99), null);
});

test("known closed shortcode replaces only its token; unknown remains untouched", () => {
  const text = "Hi :joy: later";
  const token = emojiTokenAt(text, 8);
  assert.ok(token);
  const emoji = exactEmojiForToken(emojis, token);
  assert.equal(emoji, "😂");
  assert.deepEqual(replaceEmojiToken(text, token, emoji!), { text: "Hi 😂 later", caret: 5 });
  assert.equal(replaceEmojiToken("Hi :fire: later", token, emoji!), null);
  const unknown = emojiTokenAt(":not_known:", 11);
  assert.ok(unknown);
  assert.equal(exactEmojiForToken(emojis, unknown), null);
  assert.equal(exactEmojiForToken(emojis, emojiTokenAt(":joy", 4)!), null);
});

test("recent emoji wins within a match tier without displacing an exact shortcode", () => {
  assert.deepEqual(searchEmojis(emojis, "heart", 4, ["❤️"]).map((entry) => entry.emoji), ["💗", "❤️"]);
  const sameTier = emojis.filter((entry) => entry.shortcodes.some((code) => code.startsWith("heart")));
  assert.deepEqual(searchEmojis(sameTier, "hear", 4, ["❤️"]).map((entry) => entry.emoji), ["❤️", "💗"]);
});
