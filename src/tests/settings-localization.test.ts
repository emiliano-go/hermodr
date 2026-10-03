import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { parse } from "svelte/compiler";
import ts from "typescript";
import { englishCatalog, installLocaleProvider, parseCatalog, t, type Catalog } from "../lib/i18n/localizer.ts";
import arabic from "../lib/i18n/locales/ar.json" with { type: "json" };
import { messageText, normalizeError, LocalizedError } from "../lib/i18n/errors.ts";

const source = readFileSync(new URL("../lib/settings/Settings.svelte", import.meta.url), "utf8");
const ownedKeys = (catalog: Catalog): Catalog => Object.fromEntries(Object.entries(catalog)
  .filter(([key]) => key.startsWith("settings.main.") || key.startsWith("error.settings_") && source.includes(key)));
const fragment = { en: ownedKeys(englishCatalog), ar: ownedKeys(arabic) };
const keySource = readFileSync(new URL("../lib/utils/keybinds.svelte.ts", import.meta.url), "utf8");
const actionIds = [...keySource.match(/export const ACTIONS[^=]*= \[([\s\S]*?)\];/)![1].matchAll(/id: "([^"]+)"/g)].map((match) => match[1]);
const slots = (value: string | object) => [...JSON.stringify(value).matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)].map((match) => match[1]).sort();

function functions(context: Record<string, any>) {
  const script = source.match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("Settings.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((item) => item.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

test("Settings fragment covers every message/table/keybind with matching Arabic parameters", () => {
  const en = parseCatalog(fragment.en), ar = parseCatalog(fragment.ar);
  assert.deepEqual(Object.keys(en).sort(), Object.keys(ar).sort());
  const references = new Set([...source.matchAll(/settings\.main\.[A-Za-z0-9_.]+|error\.settings_[A-Za-z0-9_]+/g)].map((match) => match[0]));
  references.delete("settings.main.key_"); references.delete("settings.main.keybind.");
  for (const id of actionIds) for (const field of ["label", "description"]) references.add(`settings.main.keybind.${id}.${field}`);
  for (const key of ["space", "enter", "tab", "backspace", "delete", "escape"]) references.add(`settings.main.key_${key}`);
  for (const key of references) assert.ok(Object.hasOwn(en, key), `Missing ${key}`);
  for (const key of Object.keys(en)) {
    assert.ok(references.has(key), `Unused ${key}`);
    assert.deepEqual(slots(en[key]), slots(ar[key]), `Mismatched placeholders ${key}`);
  }
});

test("Settings has no untranslated visible text, attribute or expression labels", () => {
  const tree = parse(source, { modern: true });
  const human = new Set(["title", "aria-label", "placeholder", "label", "alt"]);
  const literals: string[] = [];
  function expression(value: any) {
    if (!value || typeof value !== "object") return;
    if (value.type === "CallExpression" && ["t", "localizedMessage", "bindingLabel"].includes(value.callee?.name)) return;
    if ((value.type === "Literal" || value.type === "StringLiteral") && typeof value.value === "string" && /[A-Za-z]/.test(value.value)) literals.push(value.value);
    for (const child of Object.values(value)) if (child && typeof child === "object") Array.isArray(child) ? child.forEach(expression) : expression(child);
  }
  function walk(value: any) {
    if (!value || typeof value !== "object") return;
    if (value.type === "Attribute" && !human.has(value.name)) return;
    if (value.type === "Text" && /[A-Za-z]/.test(value.data) && value.data.trim() !== "Ctrl+Alt+P") literals.push(value.data.trim());
    if (value.type === "ExpressionTag") { expression(value.expression); return; }
    for (const child of Object.values(value)) if (child && typeof child === "object") Array.isArray(child) ? child.forEach(walk) : walk(child);
  }
  walk(tree.fragment);
  assert.deepEqual(literals, []);
  assert.doesNotMatch(source, /(?:profileError|spaceError|desktopError|backfillError)\s*=\s*String\(/);
});

test("Settings structured errors survive locale changes and isolate identity/path parameters", async () => {
  const previous = new Map(Object.keys(fragment.en).map((key) => [key, englishCatalog[key]]));
  Object.assign(englishCatalog, fragment.en);
  const synthetic = "error.settings_synthetic_path";
  englishCatalog[synthetic] = "Could not open {path}.";
  let language = "en";
  const restore = installLocaleProvider(() => ({ locale: language, catalog: language === "ar"
    ? { ...fragment.ar, [synthetic]: "تعذر فتح {path}." } : englishCatalog }));
  try {
    const descriptor = { kind: "postal_error", code: "error.settings_account_changed", params: {}, diagnostic: "Synthetic diagnostic" };
    const context = functions({ spaceBusy: false, spaceError: null, spaceJson: "", normalizeError, messageText,
      onspaceexport: async () => { throw descriptor; } });
    await context.exportSpaces();
    assert.ok(context.spaceError instanceof LocalizedError);
    assert.equal(context.spaceError.diagnostic, descriptor.diagnostic);
    assert.equal(context.spaceError.message, "Account changed.");
    language = "ar";
    assert.equal(context.spaceError.message, fragment.ar[descriptor.code]);
    const message = { code: synthetic, params: { path: "F:\\Synthetic\\ملف.txt" } };
    assert.ok(context.localizedMessage(message).includes(message.params.path));
    assert.ok(source.includes("<bdi>{localizedMessage("));
    assert.ok(!/[\u2066-\u2069]/.test(context.localizedMessage(message)));
    assert.equal(message.params.path, "F:\\Synthetic\\ملف.txt");
    assert.ok(t("settings.main.ram_messages_hint", { min: 50, max: 2000, count: 150 }).includes(new Intl.NumberFormat(language).format(2000)));
  } finally {
    restore(); delete englishCatalog[synthetic];
    for (const [key, value] of previous) value === undefined ? delete englishCatalog[key] : englishCatalog[key] = value;
  }
});

test("Settings preserves locale/encryption controls and command contracts with logical bidi layout", () => {
  assert.ok(source.includes('bind:checked={draft.encrypt_databases}'));
  assert.ok(source.includes('locale.setPreference(event.currentTarget.value as LocalePreference)'));
  const commands = [...new Set([...source.matchAll(/invoke(?:<[^\n]+?>)?\("([^"]+)"/g)].map((match) => match[1]))].sort();
  assert.deepEqual(commands, ["backfill_history", "database_encryption_status", "get_desktop_status", "open_log", "profile", "resync_stickers", "set_about", "set_privacy", "set_profile_picture", "set_push_name", "sticker_library"].sort());
  assert.ok(source.includes('<bdi dir="auto">{account.label}</bdi>'));
  assert.ok(source.includes('<bdi dir="ltr">{number}</bdi>'));
  for (const text of [source, source.replace(/\r?\n/g, "\r\n")]) {
    assert.match(text, /dir="ltr"\s+placeholder=\{t\("settings\.main\.app_data_folder"\)\}/);
  }
  assert.doesNotMatch(source.slice(source.indexOf("<style>")), /(?:^|\n)\s*(?:left|right|padding-left|padding-right|margin-left|margin-right):/);
});
