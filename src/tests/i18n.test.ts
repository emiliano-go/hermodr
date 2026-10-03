import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { englishCatalog, formatDate, formatMessage, formatNumber, formatRelative, formatTime, hasMessage, installLocaleProvider,
  loadCatalog, LOCALE_STORAGE_KEY, parseCatalog, resolveLocale, t, type Catalog } from "../lib/i18n/localizer.ts";
import type { LocaleState } from "../lib/i18n/locale.svelte.ts";

const arabic = await loadCatalog("ar");
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));

test("pure Node localizer has readable English fallback and never exposes unknown/prototype keys", () => {
  assert.equal(t("locale.language"), "Language"); assert.equal(formatMessage("error.operation_failed"), "Operation failed.");
  for (const code of ["missing.key", "toString", "constructor", "__proto__"]) assert.equal(t(code), "Text unavailable.");
  assert.equal(hasMessage("locale.language"), true); assert.equal(hasMessage("missing.key"), false);
});

test("missing translations and parameters use source English rather than keys or unresolved tokens", () => {
  const restore = installLocaleProvider(() => ({ locale: "ar", catalog: { "locale.text_unavailable": "النص غير متاح.", "unknown.key": "Not source text" } }));
  try {
    assert.equal(t("locale.selected", { language: "<English>" }), "Language: <English>");
    assert.equal(t("locale.selected"), "النص غير متاح."); assert.equal(t("unknown.key"), "النص غير متاح.");
    assert.equal(t("common.items", { count: NaN }), "النص غير متاح.");
  } finally { restore(); }
});

test("Arabic foundation keys cover English and use all six native plural categories", () => {
  assert.deepEqual(Object.keys(arabic).sort(), Object.keys(englishCatalog).sort());
  const restore = installLocaleProvider(() => ({ locale: "ar", catalog: arabic }));
  try {
    const number = new Intl.NumberFormat("ar");
    for (const [count, expected] of [[0, "لا عناصر"], [1, "عنصر واحد"], [2, "عنصران"], [3, `${number.format(3)} عناصر`], [11, `${number.format(11)} عنصرًا`], [102, `${number.format(102)} عنصر`]] as const)
      assert.equal(t("common.items", { count }), expected);
    assert.equal(t("locale.selected", { language: "العربية" }), "اللغة: العربية");
  } finally { restore(); }
});

test("catalog validation rejects corrupt values and preserves scalar interpolation as plain text", () => {
  for (const value of [null, [], { a: 1 }, { a: "" }, { a: { one: "One" } }, { a: { other: "Other", wrong: "Invalid" } }])
    assert.throws(() => parseCatalog(value));
  assert.equal(t("locale.selected", { language: "<img src=x>" }), "Language: <img src=x>");
  assert.equal(t("locale.selected", { language: true }), "Language: Yes");
  assert.equal(t("locale.selected", { language: null }), "Language: ");
});

test("Intl formatting follows live provider and seconds remain unchanged", () => {
  let state = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => state);
  try {
    const seconds = 0, options = { dateStyle: "medium", timeZone: "UTC" } as const;
    assert.equal(formatDate(seconds, options), new Intl.DateTimeFormat("en", options).format(new Date(0)));
    assert.equal(formatTime(seconds, { hour: "2-digit", minute: "2-digit", timeZone: "UTC" }), new Intl.DateTimeFormat("en", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" }).format(new Date(0)));
    assert.equal(formatRelative(-1, "day"), new Intl.RelativeTimeFormat("en", { numeric: "auto" }).format(-1, "day"));
    state = { locale: "ar", catalog: arabic };
    assert.equal(formatNumber(1234.5), new Intl.NumberFormat("ar").format(1234.5)); assert.equal(t("locale.language"), "اللغة");
    assert.equal(formatDate(NaN), "النص غير متاح."); assert.equal(formatNumber(Infinity), "النص غير متاح.");
  } finally { restore(); }
});

test("System resolves regional languages and retains valid unsupported format locale with English text", () => {
  assert.deepEqual(resolveLocale("system", ["ar-SA"]), { locale: "ar-SA", language: "ar", dir: "rtl" });
  assert.deepEqual(resolveLocale("system", ["es-UY"]), { locale: "es-UY", language: "en", dir: "ltr" });
  assert.deepEqual(resolveLocale("system", ["invalid_locale"]), { locale: "en", language: "en", dir: "ltr" });
  assert.deepEqual(resolveLocale("en", ["ar-SA"]), { locale: "en", language: "en", dir: "ltr" });
});

const source = readFileSync(new URL("../lib/i18n/locale.svelte.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const compiled = compileModule(js, { generate: "server", filename: "locale.svelte.js" }).js.code.replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
function fixture() {
  const saved = new Map<string, string>(), listeners = new Map<string, (event?: any) => unknown>();
  const document = { documentElement: { lang: "en", dir: "ltr" } }, navigator = { languages: ["en-US"] };
  const window = { addEventListener: (name: string, callback: (event?: any) => unknown) => { listeners.set(name, callback); }, removeEventListener: (name: string) => { listeners.delete(name); } };
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); } };
  let loader: (language: string) => Promise<Catalog> = async (language) => language === "ar" ? arabic : englishCatalog;
  const State = new Function("englishCatalog", "installLocaleProvider", "loadCatalog", "LOCALE_STORAGE_KEY", "resolveLocale", "t", "window", "document", "navigator", "localStorage", `${compiled}\nreturn LocaleState;`)
    (englishCatalog, installLocaleProvider, (language: string) => loader(language), LOCALE_STORAGE_KEY, resolveLocale, t, window, document, navigator, storage) as new () => LocaleState;
  const state = new State();
  return { state, saved, storage, listeners, document, navigator, load: (next: typeof loader) => { loader = next; } };
}

test("client bootstrap applies persisted Arabic lang/dir and reuses listener lifecycle", async () => {
  const f = fixture(); f.saved.set(LOCALE_STORAGE_KEY, "ar");
  try {
    await f.state.init(); await f.state.init();
    assert.equal(f.state.preference, "ar"); assert.equal(t("locale.language"), "اللغة");
    assert.deepEqual(f.document.documentElement, { lang: "ar", dir: "rtl" }); assert.equal(f.listeners.size, 2);
    await f.state.setPreference("en"); assert.equal(f.saved.get(LOCALE_STORAGE_KEY), "en"); assert.equal(f.document.documentElement.dir, "ltr");
  } finally { f.state.dispose(); }
  assert.equal(f.listeners.size, 0); assert.equal(t("locale.language"), "Language");
});

test("late Arabic catalog cannot overwrite newer English preference or document", async () => {
  const f = fixture(); f.saved.set(LOCALE_STORAGE_KEY, "en"); await f.state.init();
  const pending = deferred<Catalog>(); f.load(async (language) => language === "ar" ? pending.promise : englishCatalog);
  try {
    const old = f.state.setPreference("ar"); await f.state.setPreference("en"); pending.resolve(arabic); await old;
    assert.equal(f.state.preference, "en"); assert.equal(f.state.language, "en"); assert.equal(f.state.loading, false);
    assert.deepEqual(f.document.documentElement, { lang: "en", dir: "ltr" }); assert.equal(f.saved.get(LOCALE_STORAGE_KEY), "en");
  } finally { f.state.dispose(); }
});

test("storage and language changes synchronize System mode without unrelated-key work", async () => {
  const f = fixture(); await f.state.init();
  try {
    f.saved.set(LOCALE_STORAGE_KEY, "ar"); f.listeners.get("storage")!({ key: "unrelated" }); await tick(); assert.equal(f.state.language, "en");
    f.listeners.get("storage")!({ key: LOCALE_STORAGE_KEY }); await tick(); assert.equal(f.state.language, "ar");
    f.saved.clear(); f.navigator.languages = ["ar-EG"]; f.listeners.get("storage")!({ key: null }); await tick();
    assert.equal(f.state.preference, "system"); assert.equal(f.state.resolved, "ar-EG");
    f.navigator.languages = ["en-GB"]; f.listeners.get("languagechange")!(); await tick(); assert.equal(f.state.resolved, "en-GB");
  } finally { f.state.dispose(); }
});

test("read/save/catalog errors stay visible with English fallback and session-only choice", async () => {
  const f = fixture(); f.storage.getItem = () => { throw new Error("Denied"); };
  try {
    await f.state.init(); assert.equal(f.state.error?.code, "locale.preference_read_failed"); assert.ok(f.state.errorText);
    f.storage.setItem = () => { throw new Error("Quota"); }; await f.state.setPreference("ar");
    assert.equal(f.state.language, "ar"); assert.equal(f.state.error?.code, "locale.preference_save_failed"); assert.equal(f.saved.size, 0);
    f.load(async () => { throw new Error("Missing catalog"); }); await f.state.setPreference("ar");
    assert.equal(f.state.language, "en"); assert.equal(f.state.dir, "ltr"); assert.equal(f.state.error?.code, "locale.catalog_load_failed");
    assert.equal(t("locale.language"), "Language");
  } finally { f.state.dispose(); }
});

test("disposing cancels pending locale commit and restores pure provider", async () => {
  const f = fixture(); await f.state.init(); const pending = deferred<Catalog>(); f.load(() => pending.promise);
  const old = f.state.setPreference("ar"); f.state.dispose(); pending.resolve(arabic); await old;
  assert.deepEqual(f.document.documentElement, { lang: "en-US", dir: "ltr" }); assert.equal(f.state.loading, false); assert.equal(t("locale.language"), "Language");
});
