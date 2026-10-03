<script lang="ts">
  import { onMount } from "svelte";
  import { Channel, invoke as call, type InvokeArgs } from "@tauri-apps/api/core";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import FloatChat from "$lib/chat/FloatChat.svelte";
  import { FLOAT_HISTORY_LIMIT, floatDraftKey, mergeFloatPage, readFloatDraft, writeFloatDraft } from "$lib/utils/float-chat";
  import { cursorOf } from "$lib/utils/message-window";
  import type { FloatContext, MessagePage, StoredMessage } from "$lib/utils/wire";
  import { keywordStorageKey, loadKeywordRules, type KeywordRules } from "$lib/utils/keywords";

  let view = $state<ReturnType<typeof FloatChat>>();
  let context = $state.raw<FloatContext | null>(null);
  let rows = $state.raw<StoredMessage[]>([]);
  let draft = $state(""), opacity = $state(0.85);
  let loading = $state(false), olderLoading = $state(false), hasMore = $state(false);
  let sending = $state(false), closing = $state(false), draftReady = $state(false);
  let error = $state<LocalizedError | null>(null), sendError = $state<LocalizedError | null>(null), draftError = $state<LocalizedError | null>(null);
  let rules = $state.raw<KeywordRules>({ highlight: [], hide: [] });
  let rulesReady = $state(false), keywordError = $state<LocalizedError | null>(null);
  let alive = false, epoch = 0, refreshRequest = 0, olderRequest = 0, draftVersion = 0;
  let owner = "", subscribed = false, updates: Channel<void> | null = null;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let outbox = Promise.resolve();

  function current(scope = epoch) { return alive && scope === epoch; }

  async function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
    try { return await call<T>(command, args); } catch (cause) { throw normalizeError(cause); }
  }

  function failure(code: string, cause?: unknown): LocalizedError {
    const diagnostic = cause === undefined ? undefined : normalizeError(cause).diagnostic;
    return new LocalizedError({ kind: "postal_error", code, params: {}, ...(diagnostic ? { diagnostic } : {}) });
  }

  function loadRules() {
    if (!current() || !context) return;
    try {
      const loaded = loadKeywordRules(context.account_id, localStorage);
      keywordError = loaded.error ? failure("error.float_rules_read", loaded.error) : null;
      if (!loaded.error) { rules = loaded.rules; rulesReady = true; } else rulesReady = false;
    } catch (cause) { rulesReady = false; keywordError = failure("error.float_rules_read", cause); }
  }

  function rulesChanged(event: StorageEvent) {
    if (!current() || !context || event.key !== null && event.key !== keywordStorageKey(context.account_id)) return;
    loadRules();
  }

  function loadDraft() {
    if (!context) return;
    try {
      draft = readFloatDraft(localStorage, context);
      draftReady = true;
      draftError = null;
    } catch (cause) { draftError = failure("error.float_draft_read", cause); }
    try {
      const value = Number(localStorage.getItem(`${floatDraftKey(context)}.opacity`) ?? 0.85);
      opacity = Number.isFinite(value) && value >= 0 && value <= 1 ? value : 0.85;
    } catch (cause) { error = failure("error.float_opacity_read", cause); }
  }

  function persistDraft() {
    if (!context || !draftReady) throw failure("error.float_draft_unavailable");
    writeFloatDraft(localStorage, context, draft);
  }

  function changeDraft(value: string) {
    if (!current() || !draftReady) return;
    draft = value;
    draftVersion++;
    sendError = null;
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = failure("error.float_draft_save", cause); }
  }

  function retryDraft() {
    if (!draftReady) { loadDraft(); return; }
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = failure("error.float_draft_save", cause); }
  }

  function changeOpacity(value: number) {
    opacity = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0.85;
    if (!context) return;
    try { localStorage.setItem(`${floatDraftKey(context)}.opacity`, String(opacity)); }
    catch (cause) { error = failure("error.float_opacity_save", cause); }
  }

  async function refresh() {
    const scope = epoch, request = ++refreshRequest;
    olderRequest++;
    olderLoading = false;
    const valid = () => current(scope) && request === refreshRequest;
    let contextReady = false;
    loading = true;
    error = null;
    try {
      const next = await invoke<FloatContext>("float_context");
      if (!valid()) return;
      const key = JSON.stringify([next.account_id, next.chat]);
      if (owner && owner !== key) throw failure("error.float_binding_changed");
      const first = !owner;
      owner = key;
      context = next;
      contextReady = true;
      if (first) { loadDraft(); loadRules(); }
      const page = await invoke<MessagePage>("float_message_page", { limit: Math.min(FLOAT_HISTORY_LIMIT, Math.max(50, rows.length)) });
      if (!valid()) return;
      rows = mergeFloatPage(rows, page, next.chat);
      hasMore = page.has_more && rows.length < FLOAT_HISTORY_LIMIT;
    } catch (cause) {
      if (valid()) { error = normalizeError(cause); if (!contextReady && context) context = { ...context, connected: false }; }
    } finally { if (valid()) loading = false; }
  }

  async function refreshView() {
    if (!current()) return;
    if (view) await view.preserveViewport(refresh); else await refresh();
  }

  function invalidate() {
    if (!current() || refreshTimer !== undefined) return;
    refreshTimer = setTimeout(() => { refreshTimer = undefined; void refreshView(); }, 150);
  }

  async function start() {
    const scope = epoch;
    try {
      if (!subscribed) {
        updates ??= new Channel<void>();
        updates.onmessage = invalidate;
        await invoke("float_subscribe", { updates });
        if (!current(scope)) return;
        subscribed = true;
      }
      await refreshView();
    } catch (cause) { if (current(scope)) error = failure("error.float_updates_unavailable", cause); }
  }

  async function older() {
    if (!current() || !context || olderLoading || !hasMore || !rows.length || rows.length >= FLOAT_HISTORY_LIMIT) return;
    const scope = epoch, request = ++olderRequest, chat = context.chat, cursor = cursorOf(rows[0]);
    const valid = () => current(scope) && request === olderRequest && context?.chat === chat;
    olderLoading = true;
    try {
      const load = async () => {
        const page = await invoke<MessagePage>("float_message_page", { limit: 50, cursor });
        if (!valid()) return;
        rows = mergeFloatPage(rows, page, chat, true);
        hasMore = page.has_more && rows.length < FLOAT_HISTORY_LIMIT;
      };
      if (view) await view.preserveViewport(load, true); else await load();
    } catch (cause) { if (valid()) error = failure("error.float_history_load", cause); }
    finally { if (valid()) olderLoading = false; }
  }

  async function send() {
    if (!current() || !context?.connected || !draftReady || sending || closing || !draft.trim()) return;
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = failure("error.float_draft_save", cause); return; }
    const scope = epoch, version = draftVersion, text = draft.trim(), target = owner;
    sending = true;
    sendError = null;
    const run = outbox.then(async () => {
      if (!current(scope) || owner !== target || !context?.connected) throw failure("error.float_offline");
      await invoke("float_send_text", { text });
    });
    outbox = run.catch(() => {});
    try { await run; }
    catch (cause) { if (current(scope)) sendError = normalizeError(cause); return; }
    finally { if (current(scope)) sending = false; }
    if (!current(scope)) return;
    if (draftVersion === version) {
      draft = "";
      draftVersion++;
      try { persistDraft(); draftError = null; }
      catch (cause) { draftError = failure("error.float_sent_draft_uncleared", cause); }
    }
    void refreshView();
  }

  async function close() {
    if (!current() || closing) return;
    try {
      if (context && draftReady) persistDraft();
      else if (draft) throw failure("error.float_draft_save");
      draftError = null;
    }
    catch (cause) { draftError = failure("error.float_close_draft_save", cause); return; }
    const scope = epoch;
    closing = true;
    try { await invoke("close_float_chat"); }
    catch (cause) { if (current(scope)) error = normalizeError(cause); }
    finally { if (current(scope)) closing = false; }
  }

  onMount(() => {
    alive = true;
    epoch++;
    window.addEventListener("storage", rulesChanged);
    void start();
    return () => {
      alive = false;
      epoch++;
      refreshRequest++; olderRequest++;
      clearTimeout(refreshTimer);
      window.removeEventListener("storage", rulesChanged);
      if (updates) updates.onmessage = () => {};
    };
  });
</script>

<FloatChat bind:this={view} {context} {rows} {draft} {opacity} {loading} {olderLoading} {hasMore} {sending} {closing}
  {error} {sendError} {draftError} {draftReady} ondraft={changeDraft} onopacity={changeOpacity} onsend={send}
  {rules} {rulesReady} {keywordError} onretryrules={loadRules}
  onolder={older} onretry={start} onretrydraft={retryDraft} onclose={close} />
