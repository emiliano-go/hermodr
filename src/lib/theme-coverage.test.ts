import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const theme = readFileSync(new URL("./utils/theme.svelte.ts", import.meta.url), "utf8");
const sources = readdirSync(root, { recursive: true, encoding: "utf8" }).filter((file) => file.endsWith(".svelte"))
  .map((file) => ({ file, source: readFileSync(join(root, file), "utf8") }));

test("component CSS variables have a registered token, source definition or fallback", () => {
  const defined = new Set([...theme.slice(0, theme.indexOf("] as const;")).matchAll(/key: "([\w-]+)"/g)]
    .map((match) => match[1]));
  for (const { source } of sources) {
    for (const match of source.matchAll(/--([\w-]+)\s*[:=]/g)) defined.add(match[1]);
  }
  const missing = sources.flatMap(({ file, source }) => [...source.matchAll(/var\(--([\w-]+)\s*([,)])/g)]
    .filter((match) => match[2] !== "," && !defined.has(match[1]))
    .map((match) => `${file}: --${match[1]}`));
  assert.deepEqual(missing, []);
});

test("Glass send button follows edited accent tokens", () => {
  const glass = theme.match(/const GLASS_CSS = `([\s\S]*?)`;/)![1];
  const send = glass.match(/^\.send\.ready[^\{]*\{([^}]+)\}/m)![1];
  assert.match(send, /background:\s*linear-gradient\(180deg, var\(--accent-hover\), var\(--accent\)\)/);
  assert.match(send, /color:\s*var\(--accent-ink\)/);
  assert.match(send, /color-mix\(in srgb, var\(--accent\) 45%, transparent\)/);
  assert.doesNotMatch(send, /#[\da-f]{3,8}\b|rgba?\(\s*10\s*,\s*132\s*,\s*255/i);
});

test("theme documentation lists every production component and registered token", () => {
  const review = readFileSync(new URL("../../docs/theme-review.md", import.meta.url), "utf8");
  const tokens = readFileSync(new URL("../../docs/theme-tokens.md", import.meta.url), "utf8");
  const listed = [...review.matchAll(/^\| \[[^\]]+\]\(\.\.\/src\/([^\)]+\.svelte)\) \|/gm)]
    .map((match) => match[1]).sort();
  assert.deepEqual(listed, sources.map(({ file }) => file.replaceAll("\\", "/")).sort());
  assert.deepEqual([...tokens.matchAll(/^\| `--([\w-]+)` \|/gm)].map((match) => match[1]).sort(),
    [...theme.slice(0, theme.indexOf("] as const;")).matchAll(/key: "([\w-]+)"/g)].map((match) => match[1]).sort());
});

test("native checkbox, radio and range focus keep the shared outline", () => {
  const page = sources.find(({ file }) => file.replaceAll("\\", "/") === "routes/+page.svelte")!.source;
  const reset = page.match(/:global\((input[^{}]+textarea:focus-visible)\)\s*\{([^}]+)\}/)![1];
  for (const type of ["checkbox", "radio", "range"]) assert.ok(reset.includes(`:not([type="${type}"])`));
  assert.ok(/:global\(:focus-visible\)\s*\{\s*outline: 2px solid var\(--accent\)/.test(page), "Shared focus outline missing");
  assert.ok(/:global\(input\[type="checkbox"\], input\[type="radio"\], input\[type="range"\]\)\s*\{\s*accent-color: var\(--accent\)/.test(page), "Native accent rule missing");
});

test("leaf fonts and alert colors preserve native transcription switch styling", () => {
  const cases = [
    ["lib/ui/BooleanProps.svelte", /button, input\[type="search"\]\s*\{([^}]+)\}/],
    ["lib/contacts/ContactEditor.svelte", /input:not\(\[type="checkbox"\]\)\s*\{([^}]+)\}/],
    ["lib/chat/GroupInviteLinks.svelte", /^\s*input\s*\{([^}]+)\}/m],
    ["lib/settings/TranscriptionSettings.svelte", /input:not\(\[type="checkbox"\]\), select\s*\{([^}]+)\}/],
  ] as const;
  for (const [file, selector] of cases) {
    const source = sources.find((s) => s.file.replaceAll("\\", "/") === file)!.source;
    const rule = source.match(selector);
    assert.ok(rule, `${file}: field rule missing`);
    assert.match(rule[1], /font:\s*inherit/, file);
  }
  const flags = sources.find((s) => s.file.replaceAll("\\", "/") === "lib/ui/BooleanProps.svelte")!.source;
  assert.ok(/\[role="alert"\]\s*\{\s*color:\s*var\(--danger\)/.test(flags), "BooleanProps alert must use danger token");
});

test("sender text consumers share theme text mixing without changing the hue", () => {
  const cases = [
    ["lib/media/TypingIndicator.svelte", /\.typer\s*\{([^}]+)\}/],
    ["lib/chat/ChatPreview.svelte", /\.sender\.themed\s*\{([^}]+)\}/],
    ["routes/+page.svelte", /:global\(\.sender\)\s*\{([^}]+)\}/],
  ] as const;
  for (const [file, selector] of cases) {
    const source = sources.find((s) => s.file.replaceAll("\\", "/") === file)!.source;
    const rule = source.match(selector);
    assert.ok(rule, `${file}: sender text rule missing`);
    assert.match(rule[1], /color:\s*color-mix\(in srgb, hsl\(var\(--hue\) 65% 68%\) 25%, var\(--text\)\)/, file);
  }
});

test("startup shell follows inherited theme tokens and preview popup follows shadow token", () => {
  const shell = readFileSync(new URL("../app.html", import.meta.url), "utf8");
  const body = shell.match(/html,\s*body\s*\{([^}]+)\}/)![1];
  for (const [property, value] of [
    ["background", "var(--bg, #111b21)"], ["color", "var(--text, #e9edef)"],
    ["font-family", "var(--font, system-ui, sans-serif)"], ["color-scheme", "var(--scheme, dark)"],
  ]) assert.ok(body.includes(`${property}: ${value};`), `Shell ${property} must follow its token`);
  const preview = sources.find((s) => s.file.replaceAll("\\", "/") === "lib/chat/ChatPreview.svelte")!.source;
  const popup = preview.match(/#chat-preview\s*\{([^}]+)\}/)![1];
  assert.match(popup, /box-shadow:\s*var\(--shadow\)/);
});
