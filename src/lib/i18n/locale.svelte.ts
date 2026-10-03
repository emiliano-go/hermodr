import { englishCatalog, installLocaleProvider, loadCatalog, LOCALE_STORAGE_KEY, resolveLocale, t,
  type Catalog, type LocaleLanguage, type LocalePreference, type MessageKey, type MessageParams } from "./localizer.ts";

export class LocaleState {
  preference = $state<LocalePreference>("system");
  resolved = $state("en");
  language = $state<LocaleLanguage>("en");
  dir = $state<"ltr" | "rtl">("ltr");
  catalog = $state.raw<Catalog>(englishCatalog);
  loading = $state(false);
  error = $state<{ code: MessageKey; params: MessageParams } | null>(null);
  private revision = 0;
  private initialized = false;
  private restoreProvider: (() => void) | undefined;

  get errorText() { return this.error ? t(this.error.code, this.error.params) : null; }

  private document() {
    if (typeof document === "undefined") return;
    document.documentElement.lang = this.language === this.resolved.split("-")[0] ? this.resolved : this.language;
    document.documentElement.dir = this.dir;
  }

  private async apply(preference: LocalePreference) {
    const request = ++this.revision;
    const choice = resolveLocale(preference, typeof navigator === "undefined" ? [] : navigator.languages);
    this.preference = preference;
    this.loading = true;
    try {
      const catalog = await loadCatalog(choice.language);
      if (request !== this.revision) return;
      this.catalog = catalog;
      this.resolved = choice.locale;
      this.language = choice.language;
      this.dir = choice.dir;
    } catch {
      if (request !== this.revision) return;
      this.catalog = englishCatalog;
      this.resolved = this.language = "en";
      this.dir = "ltr";
      this.error = { code: "locale.catalog_load_failed", params: {} };
    } finally {
      if (request === this.revision) { this.loading = false; this.document(); }
    }
  }

  private read(): LocalePreference {
    try {
      const saved = localStorage.getItem(LOCALE_STORAGE_KEY);
      if (saved === null || saved === "system") return "system";
      if (saved === "en" || saved === "ar") return saved;
      this.error = { code: "locale.preference_invalid", params: {} };
    } catch { this.error = { code: "locale.preference_read_failed", params: {} }; }
    return "system";
  }

  private storageChanged = (event: StorageEvent) => {
    if (event.key !== null && event.key !== LOCALE_STORAGE_KEY) return;
    this.error = null;
    void this.apply(this.read());
  };
  private languageChanged = () => {
    if (this.preference === "system") void this.apply("system");
  };

  async init() {
    if (this.initialized || typeof window === "undefined") return;
    this.initialized = true;
    this.restoreProvider = installLocaleProvider(() => ({ locale: this.resolved, catalog: this.catalog }));
    window.addEventListener("storage", this.storageChanged);
    window.addEventListener("languagechange", this.languageChanged);
    await this.apply(this.read());
  }

  async setPreference(preference: LocalePreference) {
    if (!["system", "en", "ar"].includes(preference)) {
      this.error = { code: "locale.preference_invalid", params: {} };
      return;
    }
    this.error = null;
    try { localStorage.setItem(LOCALE_STORAGE_KEY, preference); }
    catch { this.error = { code: "locale.preference_save_failed", params: {} }; }
    await this.apply(preference);
  }

  dispose() {
    this.revision++;
    this.loading = false;
    this.initialized = false;
    if (typeof window !== "undefined") {
      window.removeEventListener("storage", this.storageChanged);
      window.removeEventListener("languagechange", this.languageChanged);
    }
    this.restoreProvider?.();
    this.restoreProvider = undefined;
  }
}

export const locale = new LocaleState();
