<script lang="ts">
  import { onMount } from "svelte";
  import { Channel, invoke } from "@tauri-apps/api/core";
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
  let error = $state<string | null>(null), sendError = $state<string | null>(null), draftError = $state<string | null>(null);
  let rules = $state.raw<KeywordRules>({ highlight: [], hide: [] });
  let rulesReady = $state(false), keywordError = $state<string | null>(null);
  let alive = false, epoch = 0, refreshRequest = 0, olderRequest = 0, draftVersion = 0;
  let owner = "", subscribed = false, updates: Channel<void> | null = null;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let outbox = Promise.resolve();

  function current(scope = epoch) { return alive && scope === epoch; }

  function loadRules() {
    if (!current() || !context) return;
    try {
      const loaded = loadKeywordRules(context.account_id, localStorage);
      keywordError = loaded.error;
      if (!loaded.error) { rules = loaded.rules; rulesReady = true; } else rulesReady = false;
    } catch (cause) { rulesReady = false; keywordError = `Could not read keyword rules: ${String(cause)}`; }
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
    } catch (cause) { draftError = `Could not read the saved draft: ${String(cause)}`; }
    try {
      const value = Number(localStorage.getItem(`${floatDraftKey(context)}.opacity`) ?? 0.85);
      opacity = Number.isFinite(value) && value >= 0 && value <= 1 ? value : 0.85;
    } catch (cause) { error = `Could not read opacity: ${String(cause)}`; }
  }

  function persistDraft() {
    if (!context || !draftReady) throw new Error("The saved draft is unavailable.");
    writeFloatDraft(localStorage, context, draft);
  }

  function changeDraft(value: string) {
    if (!current() || !draftReady) return;
    draft = value;
    draftVersion++;
    sendError = null;
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = `Could not save the draft: ${String(cause)}`; }
  }

  function retryDraft() {
    if (!draftReady) { loadDraft(); return; }
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = `Could not save the draft: ${String(cause)}`; }
  }

  function changeOpacity(value: number) {
    opacity = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0.85;
    if (!context) return;
    try { localStorage.setItem(`${floatDraftKey(context)}.opacity`, String(opacity)); }
    catch (cause) { error = `Could not save opacity: ${String(cause)}`; }
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
      if (owner && owner !== key) throw new Error("The floating chat target changed.");
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
      if (valid()) { error = String(cause); if (!contextReady && context) context = { ...context, connected: false }; }
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
    } catch (cause) { if (current(scope)) error = `Live updates are unavailable: ${String(cause)}`; }
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
    } catch (cause) { if (valid()) error = `Could not load older messages: ${String(cause)}`; }
    finally { if (valid()) olderLoading = false; }
  }

  async function send() {
    if (!current() || !context?.connected || !draftReady || sending || closing || !draft.trim()) return;
    try { persistDraft(); draftError = null; }
    catch (cause) { draftError = `Could not save the draft: ${String(cause)}`; return; }
    const scope = epoch, version = draftVersion, text = draft.trim(), target = owner;
    sending = true;
    sendError = null;
    const run = outbox.then(async () => {
      if (!current(scope) || owner !== target || !context?.connected) throw new Error("This floating chat is offline or closed.");
      await invoke("float_send_text", { text });
    });
    outbox = run.catch(() => {});
    try { await run; }
    catch (cause) { if (current(scope)) sendError = String(cause); return; }
    finally { if (current(scope)) sending = false; }
    if (!current(scope)) return;
    if (draftVersion === version) {
      draft = "";
      draftVersion++;
      try { persistDraft(); draftError = null; }
      catch (cause) { draftError = `Message sent, but the saved draft could not be cleared: ${String(cause)}`; }
    }
    void refreshView();
  }

  async function close() {
    if (!current() || closing) return;
    try {
      if (context && draftReady) persistDraft();
      else if (draft) throw new Error("The draft could not be saved.");
      draftError = null;
    }
    catch (cause) { draftError = `Could not save the draft before closing: ${String(cause)}`; return; }
    const scope = epoch;
    closing = true;
    try { await invoke("close_float_chat"); }
    catch (cause) { if (current(scope)) error = String(cause); }
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
