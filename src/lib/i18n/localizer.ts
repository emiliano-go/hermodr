import en from "./locales/en.json" with { type: "json" };

export type MessageKey = keyof typeof en;
export type MessageParams = Record<string, string | number | boolean | null>;
export type PluralMessage = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };
export type Catalog = Record<string, string | PluralMessage>;
export type LocalePreference = "system" | "en" | "ar";
export type LocaleLanguage = "en" | "ar";
export type LocaleSnapshot = { locale: string; catalog: Catalog };
export const LOCALE_STORAGE_KEY = "postal.locale";
export const englishCatalog: Catalog = en;
export const localeNames = { en: "English", ar: "العربية" } as const;
let provider: () => LocaleSnapshot = () => ({ locale: "en", catalog: englishCatalog });

export function installLocaleProvider(getter: () => LocaleSnapshot): () => void {
  const previous = provider;
  provider = getter;
  return () => { if (provider === getter) provider = previous; };
}

export function hasMessage(code: string): code is MessageKey {
  return Object.hasOwn(englishCatalog, code);
}

export function parseCatalog(value: unknown): Catalog {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid locale catalog.");
  const catalog: Catalog = Object.create(null);
  for (const [key, message] of Object.entries(value)) {
    if (typeof message === "string" && message.trim()) { catalog[key] = message; continue; }
    if (!message || typeof message !== "object" || Array.isArray(message) || typeof message.other !== "string") throw new Error("Invalid plural message.");
    for (const [form, text] of Object.entries(message)) {
      if (!["zero", "one", "two", "few", "many", "other"].includes(form) || typeof text !== "string" || !text.trim()) throw new Error("Invalid plural form.");
    }
    catalog[key] = message as PluralMessage;
  }
  return catalog;
}

export async function loadCatalog(language: LocaleLanguage): Promise<Catalog> {
  return language === "en" ? englishCatalog : parseCatalog((await import("./locales/ar.json", { with: { type: "json" } })).default);
}

export function resolveLocale(preference: LocalePreference, languages: readonly string[]): { locale: string; language: LocaleLanguage; dir: "ltr" | "rtl" } {
  if (preference !== "system") return { locale: preference, language: preference, dir: preference === "ar" ? "rtl" as const : "ltr" as const };
  let first: string | undefined;
  for (const requested of languages) {
    try {
      const locale = Intl.getCanonicalLocales(requested)[0];
      first ??= locale;
      const base = locale.split("-")[0];
      if (base === "en" || base === "ar") return { locale, language: base, dir: base === "ar" ? "rtl" as const : "ltr" as const };
    } catch {}
  }
  return { locale: first ?? "en", language: "en" as const, dir: "ltr" as const };
}

function template(value: string | PluralMessage | undefined, params: MessageParams, locale: string): string | undefined {
  if (typeof value === "string") return value;
  if (!value || typeof params.count !== "number" || !Number.isFinite(params.count)) return undefined;
  return value[new Intl.PluralRules(locale).select(params.count)] ?? value.other;
}

function interpolate(value: string | undefined, params: MessageParams, locale: string, catalog: Catalog): string | undefined {
  if (value === undefined) return undefined;
  let missing = false;
  const result = value.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_token, name: string) => {
    if (!Object.hasOwn(params, name)) { missing = true; return ""; }
    const parameter = params[name];
    if (typeof parameter === "number" && !Number.isFinite(parameter)) { missing = true; return ""; }
    if (typeof parameter === "boolean") {
      const code = parameter ? "common.yes" : "common.no";
      return Object.hasOwn(catalog, code) && typeof catalog[code] === "string" ? catalog[code] : en[code];
    }
    return parameter === null ? "" : typeof parameter === "number" ? new Intl.NumberFormat(locale).format(parameter) : String(parameter);
  });
  return missing ? undefined : result;
}

export function t(code: string, params: MessageParams = {}): string {
  const state = provider();
  const english = hasMessage(code) ? englishCatalog[code] : undefined;
  try {
    const translated = hasMessage(code) && Object.hasOwn(state.catalog, code) ? state.catalog[code] : undefined;
    return interpolate(template(translated, params, state.locale), params, state.locale, state.catalog)
      ?? interpolate(template(english, params, "en"), params, state.locale, state.catalog)
      ?? (typeof state.catalog["locale.text_unavailable"] === "string" ? state.catalog["locale.text_unavailable"] : en["locale.text_unavailable"]);
  } catch { return en["locale.text_unavailable"]; }
}

export const formatMessage = t;

export function formatDate(seconds: number, options: Intl.DateTimeFormatOptions = { dateStyle: "medium" }): string {
  try { return new Intl.DateTimeFormat(provider().locale, options).format(new Date(seconds * 1000)); }
  catch { return t("locale.text_unavailable"); }
}

export function formatTime(seconds: number, options: Intl.DateTimeFormatOptions = { hour: "numeric", minute: "2-digit" }): string {
  return formatDate(seconds, options);
}

export function formatNumber(value: number, options: Intl.NumberFormatOptions = {}): string {
  if (!Number.isFinite(value)) return t("locale.text_unavailable");
  try { return new Intl.NumberFormat(provider().locale, options).format(value); }
  catch { return t("locale.text_unavailable"); }
}

export function formatRelative(value: number, unit: Intl.RelativeTimeFormatUnit, options: Intl.RelativeTimeFormatOptions = { numeric: "auto" }): string {
  try { return new Intl.RelativeTimeFormat(provider().locale, options).format(value, unit); }
  catch { return t("locale.text_unavailable"); }
}
