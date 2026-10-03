import assert from "node:assert/strict";
import test from "node:test";

const stateGlobal = globalThis as unknown as { $state?: (value: unknown) => unknown };
const originalState = stateGlobal.$state;
stateGlobal.$state = (value) => value;
const a11y = await import("../lib/utils/accessibility.svelte.ts");
if (originalState) stateGlobal.$state = originalState; else delete stateGlobal.$state;

const {
  accessibility,
  DEFAULTS,
  ACCESSIBLE_VALUES,
  enableAccessibilityMode,
  disableAccessibilityMode,
  wantsReducedMotion,
  wantsHighContrast,
  contrastRatio,
  ensureContrast,
  highContrastTokens,
  announceMessage,
  announceTyping,
  announceStatus,
  onAnnouncement,
  mediaAltText,
} = a11y;

function reset() {
  Object.assign(accessibility, { ...DEFAULTS, snapshot: null });
}

test("defaults follow the OS and keep motion, contrast and autoplay on", () => {
  reset();
  assert.equal(accessibility.enabled, false);
  assert.equal(accessibility.promptSeen, false);
  assert.equal(accessibility.reduceMotion, "system");
  assert.equal(accessibility.highContrast, "system");
  assert.equal(accessibility.textScale, 100);
  assert.equal(accessibility.announcements, "concise");
  assert.equal(accessibility.autoplayVideos, true);
  assert.equal(accessibility.charShortcutsEnabled, true);
});

test("enabling Accessibility mode turns every feature on and disabling restores prior values", () => {
  reset();
  accessibility.textScale = 150;
  accessibility.reduceMotion = "off";
  enableAccessibilityMode();
  assert.equal(accessibility.enabled, true);
  assert.equal(accessibility.promptSeen, true);
  for (const [key, value] of Object.entries(ACCESSIBLE_VALUES)) {
    assert.deepEqual(accessibility[key as keyof typeof ACCESSIBLE_VALUES], value);
  }
  // Every preference stays individually editable while the mode is on.
  accessibility.textScale = 175;
  assert.equal(accessibility.textScale, 175);
  accessibility.autoplayVideos = true;
  assert.equal(accessibility.autoplayVideos, true);
  disableAccessibilityMode();
  assert.equal(accessibility.enabled, false);
  assert.equal(accessibility.textScale, 150);
  assert.equal(accessibility.reduceMotion, "off");
  assert.equal(accessibility.snapshot, null);
  reset();
});

test("reduce motion honours the explicit toggle over the OS preference", () => {
  reset();
  accessibility.reduceMotion = "on";
  assert.equal(wantsReducedMotion(), true);
  accessibility.reduceMotion = "off";
  assert.equal(wantsReducedMotion(), false);
  reset();
});

test("high contrast is off unless requested", () => {
  reset();
  accessibility.highContrast = "on";
  assert.equal(wantsHighContrast(), true);
  accessibility.highContrast = "off";
  assert.equal(wantsHighContrast(), false);
  reset();
});

test("contrast helpers measure ratios and repair them", () => {
  assert.ok(Math.abs((contrastRatio("#ffffff", "#000000") ?? 0) - 21) < 0.01);
  assert.equal(contrastRatio("not-a-colour", "#000000"), null);
  const fixed = ensureContrast("#777777", "#ffffff", 4.5);
  assert.ok((contrastRatio(fixed, "#ffffff") ?? 0) >= 4.5);
});

test("high-contrast transform reaches 4.5:1 text and 3:1 non-text over the dark theme", () => {
  const tokens = {
    bg: "#111b21",
    "chat-bg": "#0b141a",
    surface: "#202c33",
    bubble: "#202c33",
    "bubble-mine": "#005c4b",
    text: "#e9edef",
    muted: "#8696a0",
    faint: "#667781",
    link: "#53bdeb",
    accent: "#00a884",
    "accent-hover": "#06cf9c",
    "accent-text": "#00a884",
    danger: "#f15c6d",
    mention: "#f0b232",
    "mention-pill": "#53bdeb",
    replying: "#00a884",
    line: "#222d34",
    "line-soft": "#1d282f",
    "line-strong": "#3b4a54",
    scheme: "dark",
  };
  const out = highContrastTokens(tokens);
  const bg = out["chat-bg"];
  for (const key of ["text", "muted", "faint", "link", "accent-text", "danger", "mention", "mention-pill"]) {
    assert.ok((contrastRatio(out[key], bg) ?? 0) >= 4.5, `${key} fails 4.5:1`);
  }
  for (const key of ["accent", "accent-hover", "replying", "line", "line-strong"]) {
    assert.ok((contrastRatio(out[key], bg) ?? 0) >= 3, `${key} fails 3:1`);
  }
  // Translucent glass tokens become opaque so contrast is guaranteed.
  assert.ok(!out.bg.startsWith("rgba"));
});

test("announcements are suppressed during the initial sync backlog", () => {
  reset();
  const heard: string[] = [];
  const stop = onAnnouncement((a) => heard.push(a.text));
  try {
    announceMessage("Alice", "hello", false);
    assert.equal(heard.length, 0);
    announceMessage("Alice", "hello", true);
    assert.equal(heard.length, 1);
    announceTyping("Alice", false);
    assert.equal(heard.length, 1);
    accessibility.announcements = "detailed";
    announceTyping("Alice", true);
    assert.equal(heard.length, 2);
    announceStatus("Connected", false);
    assert.equal(heard.length, 2);
    announceStatus("Connected", true);
    assert.equal(heard.length, 3);
    accessibility.announcements = "off";
    announceMessage("Alice", "hello again", true);
    assert.equal(heard.length, 3);
  } finally {
    stop();
    reset();
  }
});

test("concise mode skips bodies unless mentioned; detailed keeps them", () => {
  reset();
  const heard: string[] = [];
  const stop = onAnnouncement((a) => heard.push(a.text));
  try {
    announceMessage("Alice", "the full body", true);
    assert.match(heard[0], /Alice/);
    assert.doesNotMatch(heard[0], /full body/);
    announceMessage("Bob", "the full body", true, { mention: true });
    assert.match(heard[1], /full body/);
  } finally {
    stop();
    reset();
  }
});

test("media alt text names sender, kind and caption", () => {
  const alt = mediaAltText("Alice", "video", "at the beach");
  assert.match(alt, /Alice/);
  assert.match(alt, /video/);
  assert.match(alt, /at the beach/);
  assert.match(mediaAltText("", "gif", null), /GIF/);
});

test("text scale defaults and preset stay within 100–200", () => {
  assert.ok(DEFAULTS.textScale >= 100 && DEFAULTS.textScale <= 200);
  assert.ok(ACCESSIBLE_VALUES.textScale >= 100 && ACCESSIBLE_VALUES.textScale <= 200);
  assert.equal(DEFAULTS.textScale, 100);
  assert.equal(ACCESSIBLE_VALUES.textScale, 125);
});

test("preferences persist per device as JSON in localStorage", () => {
  reset();
  const store = globalThis as unknown as { localStorage?: Storage };
  const realStorage = store.localStorage;
  const mem = new Map<string, string>();
  store.localStorage = {
    getItem: (k: string) => mem.get(k) ?? null,
    setItem: (k: string, v: string) => { mem.set(k, v); },
    removeItem: (k: string) => { mem.delete(k); },
    clear: () => mem.clear(),
    key: () => null,
    length: 0,
  } as unknown as Storage;
  try {
    accessibility.textScale = 150;
    accessibility.announcements = "detailed";
    a11y.save();
    const saved = JSON.parse(mem.get("postal.accessibility") ?? "{}");
    assert.equal(saved.textScale, 150);
    assert.equal(saved.announcements, "detailed");
  } finally {
    if (realStorage) store.localStorage = realStorage; else delete store.localStorage;
    reset();
  }
});
