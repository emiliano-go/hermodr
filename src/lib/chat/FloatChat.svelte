<script lang="ts">
  import { t, formatDate, formatNumber } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { tick } from "svelte";
  import { blocks, type Inline } from "$lib/utils/format";
  import { FLOAT_HISTORY_LIMIT, floatContent } from "$lib/utils/float-chat";
  import type { FloatContext, StoredMessage } from "$lib/utils/wire";
  import { keywordHidden, keywordHighlighted, type KeywordRules } from "$lib/utils/keywords";

  let { context, rows, draft, opacity = 0.85, loading = false, olderLoading = false, hasMore = false, sending = false,
    closing = false, error = null, sendError = null, draftError = null, draftReady = false,
    rules = { highlight: [], hide: [] }, rulesReady = true, keywordError = null, onretryrules,
    ondraft, onopacity, onsend, onolder, onretry, onretrydraft, onclose }: {
    context: FloatContext | null; rows: StoredMessage[]; draft: string; opacity?: number;
    loading?: boolean; olderLoading?: boolean; hasMore?: boolean; sending?: boolean; closing?: boolean;
    error?: LocalizedError | string | null; sendError?: LocalizedError | string | null; draftError?: LocalizedError | string | null; draftReady?: boolean;
    rules?: KeywordRules; rulesReady?: boolean; keywordError?: LocalizedError | string | null; onretryrules?: () => void;
    ondraft: (value: string) => void; onopacity: (value: number) => void; onsend: () => Promise<void>;
    onolder: () => Promise<void>; onretry: () => Promise<void>; onretrydraft: () => void; onclose: () => Promise<void>;
  } = $props();
  let dismissed = $state.raw<(LocalizedError | string)[]>([]);
  $effect(() => { error; sendError; draftError; keywordError; context?.account_id; context?.chat; dismissed = []; });
  let scroller = $state<HTMLElement>();
  let seen = 0;
  let pinnedBottom = true;
  let scrollRevision = 0;
  const canSend = $derived(!!context?.connected && draftReady && !sending && !closing && !!draft.trim());
  const visibleRows = $derived(rulesReady ? rows.filter((message) => !keywordHidden(message, rules)) : []);

  $effect(() => {
    rows;
    const initial = seen === 0;
    seen = rows.length;
    if (!initial || !rows.length) return;
    let active = true;
    void tick().then(() => { if (active && scroller?.isConnected) { scroller.scrollTop = scroller.scrollHeight; pinnedBottom = true; } });
    return () => { active = false; };
  });

  export async function preserveViewport(task: () => Promise<void>, older = false) {
    const el = scroller, height = el?.scrollHeight ?? 0, top = el?.scrollTop ?? 0, follow = pinnedBottom, revision = scrollRevision;
    await task();
    await tick();
    if (!el?.isConnected || revision !== scrollRevision) return;
    if (older) el.scrollTop = top + el.scrollHeight - height;
    else if (follow) el.scrollTop = el.scrollHeight;
    else el.scrollTop = top;
  }

  function key(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    if (canSend) void onsend();
  }

  function time(value: number) {
    return Number.isFinite(value) ? formatDate(value, { dateStyle: "medium", timeStyle: "short" }) : t("chat.time_unavailable");
  }
</script>


{#snippet problem(value: LocalizedError | string)}
  {#if !dismissed.includes(value)}
    {@const failure = normalizeError(value)}
    <div class="error-notice" role="alert">
      <p class="error">{failure.message}</p>
      {#if failure.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{failure.diagnostic}</pre></details>{/if}
      <button type="button" onclick={() => dismissed = [...dismissed, value]}>{t("action.dismiss")}</button>
    </div>
  {/if}
{/snippet}

{#snippet runs(nodes: Inline[])}
  {#each nodes as node}
    {#if node.kind === "text"}{node.text}
    {:else if node.kind === "link"}{node.url}
    {:else if node.kind === "mention"}<span class="mention">@{node.user}</span>
    {:else if node.kind === "code"}<code>{node.text}</code>
    {:else if node.kind === "bold"}<strong>{@render runs(node.children)}</strong>
    {:else if node.kind === "italic"}<em>{@render runs(node.children)}</em>
    {:else}<s>{@render runs(node.children)}</s>{/if}
  {/each}
{/snippet}
{#snippet lines(items: Inline[][])}{#each items as line, index}{#if index}<br />{/if}{@render runs(line)}{/each}{/snippet}

<main class="float-chat" style:--float-alpha={`${Math.max(0, Math.min(1, opacity)) * 100}%`}>
  <header><h1><bdi>{context?.title ?? t("chat.float_title")}</bdi></h1><button type="button" disabled={closing} aria-label={t("chat.float_close")} onclick={() => void onclose()}>×</button></header>
  <div class="toolbar"><label>{t("chat.float_opacity")} <input type="range" min="0" max="1" step="0.05" value={opacity}
    oninput={(event) => onopacity(Number(event.currentTarget.value))} /></label><output>{formatNumber(opacity, { style: "percent", maximumFractionDigits: 0 })}</output></div>
  <p class="connection" role="status">{context?.connected ? t("settings.connected") : t("chat.float_offline")}</p>
  {#if error}<div class="error-actions">{@render problem(error)}<button type="button" disabled={loading} onclick={() => void onretry()}>{t("chat.history_retry")}</button></div>{/if}
  {#if keywordError}<div class="error-actions">{@render problem(keywordError)}{#if onretryrules}<button type="button" onclick={onretryrules}>{t("chat.keywords_retry")}</button>{/if}</div>{/if}
  <section bind:this={scroller} class="history" aria-label={t("chat.stored_messages")} aria-busy={loading}
    onscroll={() => { if (scroller) { scrollRevision++; pinnedBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40; } }}>
    {#if rows.length >= FLOAT_HISTORY_LIMIT}<p class="limit" role="status">{t("chat.float_limit", { count: FLOAT_HISTORY_LIMIT })}</p>{/if}
    {#if hasMore}<button class="older" disabled={olderLoading || rows.length >= FLOAT_HISTORY_LIMIT} title={rows.length >= FLOAT_HISTORY_LIMIT ? t("chat.float_limit_reached") : undefined}
      onclick={() => { if (rows.length < FLOAT_HISTORY_LIMIT) void onolder(); }}>{olderLoading ? t("ui.loading") : t("chat.messages_older")}</button>{/if}
    {#if !rulesReady}<p role="status">{t("chat.float_keywords_hidden")}</p>
    {:else if loading && !rows.length}<p role="status">{t("settings.messages_loading")}</p>
    {:else if !rows.length}<p>{t("chat.stored_empty")}</p>
    {:else if !visibleRows.length}<p>{t("chat.keywords_all_hidden")}</p>{/if}
    <ol>{#each visibleRows as message (message.id)}
      {@const content = floatContent(message)}
      <li class:mine={message.from_me} class:notice={content.notice} class:keyword-highlighted={keywordHighlighted(message, rules)}>
        <b><bdi>{message.from_me ? t("chat.you") : message.sender_name || message.sender || t("chat.sender_unknown")}</bdi></b>
        {#if content.media}<span class="media">{content.media}</span>{/if}
        <div class="text" dir="auto">
          {#each blocks(content.notice || (message.media_kind === "view_once" || !!message.media_once_kind) || message.spoiler ? content.text : message.text.trim() === `[${message.media_kind}]` ? "" : message.text) as block}
            {#if block.kind === "pre"}<pre>{block.text}</pre>
            {:else if block.kind === "quote"}<blockquote>{@render lines(block.lines)}</blockquote>
            {:else if block.kind === "list"}{#if block.ordered}<ol>{#each block.items as item}<li>{@render runs(item)}</li>{/each}</ol>
              {:else}<ul>{#each block.items as item}<li>{@render runs(item)}</li>{/each}</ul>{/if}
            {:else}<p>{@render lines(block.lines)}</p>{/if}
          {/each}
        </div>
        <time>{time(message.timestamp)}</time>
      </li>
    {/each}</ol>
  </section>
  {#if sendError}{@render problem(sendError)}{/if}
  {#if draftError}<div class="error-actions">{@render problem(draftError)}<button type="button" onclick={onretrydraft}>{t("chat.draft_retry")}</button></div>{/if}
  <form onsubmit={(event) => { event.preventDefault(); if (canSend) void onsend(); }}>
    <textarea aria-label={t("chat.reply_label")} dir="auto" placeholder={t("chat.reply_placeholder")} value={draft} disabled={!draftReady || sending || closing}
      oninput={(event) => ondraft(event.currentTarget.value)} onkeydown={key} rows="2"></textarea>
    <button type="submit" disabled={!canSend}>{sending ? t("ui.sending") : t("ui.send")}</button>
  </form>
</main>

<style>
  :global(html), :global(body) { margin: 0; background: transparent; }
  .float-chat { display: flex; flex-direction: column; height: 100dvh; box-sizing: border-box; padding: 10px; gap: 8px; font: 0.8125rem system-ui, sans-serif; background: color-mix(in srgb, var(--bg, #111b21) var(--float-alpha), transparent); color: var(--text, #e9edef); }
  header, .toolbar, form { display: flex; align-items: center; gap: 8px; }
  header { justify-content: space-between; }
  h1 { margin: 0; font-size: 0.9375rem; overflow-wrap: anywhere; }
  button, textarea { font: inherit; color: inherit; }
  button { padding: 6px 10px; border: 1px solid var(--line-strong, #3b4a54); border-radius: 6px; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); cursor: pointer; }
  button:disabled { cursor: default; color: var(--muted, #8696a0); }
  header button { font-size: 1.1875rem; padding: 0 7px; }
  .toolbar { font-size: 0.6875rem; justify-content: space-between; }
  .toolbar label { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; }
  .toolbar input { flex: 1; min-width: 0; width: 80px; }
  .connection, time { margin: 0; font-size: 0.6875rem; color: var(--muted, #8696a0); }
  .history { flex: 1; min-height: 0; overflow-y: auto; overflow-anchor: none; }
  .history > ol { display: flex; flex-direction: column; align-items: flex-start; margin: 0; padding: 0; list-style: none; }
  .history > ol > li { max-width: 90%; padding: 8px 10px; margin: 4px 0; border-radius: 8px; overflow-wrap: anywhere; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); }
  .history > ol > li.mine { align-self: flex-end; background: color-mix(in srgb, var(--bubble-mine, #005c4b) var(--float-alpha), transparent); }
  .history > ol > li.notice { align-self: center; }
  .history > ol > li.keyword-highlighted { outline: 2px solid var(--mention, #f0b232); outline-offset: -2px; }
  b, .media { display: block; font-size: 0.6875rem; }
  .media { color: var(--muted, #8696a0); }
  .text { white-space: pre-wrap; }
  .text p { margin: 4px 0; }
  pre { margin: 4px 0; white-space: pre-wrap; }
  blockquote { margin: 4px 0; padding-inline-start: 8px; border-inline-start: 2px solid var(--muted, #8696a0); }
  .mention { color: var(--link, #53bdeb); }
  textarea { flex: 1; min-width: 0; resize: vertical; padding: 8px; border: 1px solid var(--line-strong, #3b4a54); border-radius: 6px; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); }
  .error { margin: 0; color: var(--danger, #ff6b6b); font-size: 0.75rem; }
  .error-actions { display: grid; gap: 6px; }
  .error-notice { display: grid; gap: 6px; }
  .error-notice p { margin: 0; }
  .error-notice summary { cursor: pointer; }
  .error-notice pre { max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; }
  .error-notice button { justify-self: start; }
</style>
