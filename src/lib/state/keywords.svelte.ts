import { invoke } from "$lib/utils/ipc";
import { keywordHidden, keywordHighlighted, loadKeywordRules, saveKeywordRules,
  type KeywordMessage, type KeywordRules, type KeywordStorage } from "$lib/utils/keywords";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";
import { uiError } from "./localized.ts";

export class KeywordsState {
  account = $state<string | null>(null);
  rules = $state<KeywordRules>({ highlight: [], hide: [] });
  revision = $state(0);
  counts = $state.raw<Record<string, number>>({});
  #error = $state<LocalizedError | null>(null);
  get error(): string | null { return this.#error?.message ?? null; }
  set error(value: unknown) { this.#error = value == null ? null : normalizeError(value); }
  get diagnostic() { return this.#error?.diagnostic; }
  #countError = $state<LocalizedError | null>(null);
  get countError(): string | null { return this.#countError?.message ?? null; }
  set countError(value: unknown) { this.#countError = value == null ? null : normalizeError(value); }
  get countDiagnostic() { return this.#countError?.diagnostic; }
  private generation = 0;
  private request = 0;

  constructor(private readonly storage?: KeywordStorage) {}

  load(account: string | null) {
    ++this.generation;
    ++this.request;
    ++this.revision;
    this.account = account;
    this.counts = {};
    this.countError = null;
    const loaded = account ? loadKeywordRules(account, this.storage) : { rules: { highlight: [], hide: [] }, error: null };
    this.rules = loaded.rules;
    this.error = loaded.error ? uiError("error.state.keyword_load", {}, loaded.error) : null;
  }

  save(account: string, rules: KeywordRules): boolean {
    if (account !== this.account) { this.error = uiError("error.state.keyword_scope"); return false; }
    try {
      const saved = saveKeywordRules(account, rules, this.storage);
      this.rules = saved;
      ++this.revision;
      ++this.request;
      this.counts = {};
      this.error = this.countError = null;
      return true;
    } catch (error) {
      this.error = uiError("error.state.keyword_save", {}, error);
      return false;
    }
  }

  hidden(message: KeywordMessage) { return keywordHidden(message, this.rules); }
  highlighted(message: KeywordMessage) { return keywordHighlighted(message, this.rules); }

  async refreshCounts(account: string | null, currentGeneration: () => number): Promise<void> {
    if (!account || account !== this.account) return;
    const generation = this.generation, external = currentGeneration(), revision = this.revision, request = ++this.request;
    const current = () => this.account === account && generation === this.generation && revision === this.revision
      && request === this.request && external === currentGeneration();
    if (!this.rules.highlight.length) { this.counts = {}; this.countError = null; return; }
    try {
      const counts = await invoke<Record<string, number>>("keyword_mentions", { accountId: account,
        highlight: [...this.rules.highlight], hide: [...this.rules.hide] });
      if (current()) { this.counts = counts; this.countError = null; }
    } catch (error) {
      if (current()) { this.counts = {}; this.countError = uiError("error.state.keyword_counts", {}, error); }
    }
  }
}

export const keywords = new KeywordsState();
