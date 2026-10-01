import { test } from "node:test";
import assert from "node:assert/strict";
import { keywordTerms, keywordRules, keywordStorageKey, loadKeywordRules, saveKeywordRules,
  keywordHighlighted, keywordHidden, keywordMatch, type KeywordMessage } from "./utils/keywords.ts";

const message: KeywordMessage = { text: "A NEEDLE in a caption", from_me: false, deleted: false, revoked: false,
  system_kind: null, media_kind: null, media_once_kind: null, spoiler: false };

test("keywords match literal Unicode lowercase substrings with hide priority and privacy boundaries", () => {
  for (const [body, term, expected] of [["CAFÉ", "café", true], ["ΟΣ", "ος", true], ["İ", "i\u0307", true],
    ["STRASSE", "straße", false], ["A.*B", "a.*b", true], ["AxB", "a.*b", false], ["concatenate", "cat", true]] as const)
    assert.equal(keywordMatch(body, [term]), expected, `${body}/${term}`);
  const rules = { highlight: ["needle"], hide: ["secret"] };
  assert.equal(keywordHighlighted(message, rules), true);
  assert.equal(keywordHidden({ ...message, text: "NEEDLE SECRET" }, rules), true);
  assert.equal(keywordHighlighted({ ...message, text: "NEEDLE SECRET" }, rules), false);
  for (const guarded of [{ ...message, from_me: true }, { ...message, deleted: true }, { ...message, revoked: true },
    { ...message, spoiler: true }, { ...message, system_kind: "UNAVAILABLE_MESSAGE" }, { ...message, system_kind: "SYSTEM" },
    { ...message, media_kind: "view_once" }, { ...message, media_kind: "unknown" }, { ...message, media_once_kind: "image" }]) {
    assert.equal(keywordHighlighted(guarded, rules), false);
    assert.equal(keywordHidden(guarded, { highlight: [], hide: ["needle"] }), false);
  }
  assert.equal(keywordHighlighted({ ...message, text: "\uFEFF[image]", media_kind: "image" }, { highlight: ["image"], hide: [] }), false);
  assert.equal(keywordHighlighted({ ...message, media_kind: "image" }, rules), true);
  const untouched = { ...message, read: false, mentioned: true, reply_to_text: "secret" };
  keywordHighlighted(untouched, rules); keywordHidden(untouched, rules);
  assert.equal(untouched.read, false); assert.equal(untouched.mentioned, true);
});

test("keyword rules enforce list/codepoint bounds and stable normalization", () => {
  assert.deepEqual(keywordTerms([" Needle ", "needle", "", "\uFEFFCAFÉ\uFEFF", "café", "\u0085needle\u0085"]), ["Needle", "CAFÉ", "\u0085needle\u0085"]);
  assert.deepEqual(keywordRules({ highlight: [" Highlight "], hide: [" Hide "] }), { highlight: ["Highlight"], hide: ["Hide"] });
  assert.equal(keywordTerms(["🦀".repeat(100)])[0].length, 200);
  for (const bad of [["🦀".repeat(101)], Array(51).fill("same"), [7], null]) assert.throws(() => keywordTerms(bad));
  assert.throws(() => keywordRules({ highlight: [], hide: "needle" }));
});

test("keyword lists survive restart per account without overwriting corrupt or unwritable storage", () => {
  const saved = new Map<string, string>();
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); } };
  saveKeywordRules("account-a", { highlight: ["Needle"], hide: ["Secret"] }, storage);
  saveKeywordRules("account-b", { highlight: ["Other"], hide: [] }, storage);
  assert.deepEqual(loadKeywordRules("account-a", storage).rules, { highlight: ["Needle"], hide: ["Secret"] });
  assert.deepEqual(loadKeywordRules("account-b", storage).rules, { highlight: ["Other"], hide: [] });
  assert.deepEqual(loadKeywordRules("new-account", storage), { rules: { highlight: [], hide: [] }, error: null });
  for (const corrupt of ["[", JSON.stringify({ version: 2, highlight: [], hide: [] }), JSON.stringify({ version: 1, highlight: [7], hide: [] })]) {
    saved.set(keywordStorageKey("account-a"), corrupt);
    const loaded = loadKeywordRules("account-a", storage);
    assert.deepEqual(loaded.rules, { highlight: [], hide: [] }); assert.ok(loaded.error);
    assert.equal(saved.get(keywordStorageKey("account-a")), corrupt);
  }
  const denied = { getItem: () => { throw new Error("read denied"); }, setItem: () => { throw new Error("quota"); } };
  assert.match(loadKeywordRules("account-a", denied).error!, /read denied/);
  assert.throws(() => saveKeywordRules("account-a", { highlight: ["Needle"], hide: [] }, denied), /quota/);
});
