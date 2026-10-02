<script lang="ts">
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
    error?: string | null; sendError?: string | null; draftError?: string | null; draftReady?: boolean;
    rules?: KeywordRules; rulesReady?: boolean; keywordError?: string | null; onretryrules?: () => void;
    ondraft: (value: string) => void; onopacity: (value: number) => void; onsend: () => Promise<void>;
    onolder: () => Promise<void>; onretry: () => Promise<void>; onretrydraft: () => void; onclose: () => Promise<void>;
  } = $props();
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
    const date = new Date(value * 1000);
    return Number.isFinite(date.getTime()) ? date.toLocaleString() : "Time unavailable";
  }
</script>

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
  <header><h1>{context?.title ?? "Floating chat"}</h1><button type="button" disabled={closing} aria-label="Close floating chat" onclick={() => void onclose()}>×</button></header>
  <div class="toolbar"><label>Background opacity <input type="range" min="0" max="1" step="0.05" value={opacity}
    oninput={(event) => onopacity(Number(event.currentTarget.value))} /></label><output>{Math.round(opacity * 100)}%</output></div>
  <p class="connection" role="status">{context?.connected ? "Connected" : "Offline — showing stored messages. Replies are unavailable."}</p>
  {#if error}<p class="error" role="alert">{error}<button type="button" disabled={loading} onclick={() => void onretry()}>Retry history</button></p>{/if}
  {#if keywordError}<p class="error" role="alert">{keywordError}{#if onretryrules}<button type="button" onclick={onretryrules}>Retry keyword rules</button>{/if}</p>{/if}
  <section bind:this={scroller} class="history" aria-label="Stored chat messages" aria-busy={loading}
    onscroll={() => { if (scroller) { scrollRevision++; pinnedBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40; } }}>
    {#if rows.length >= FLOAT_HISTORY_LIMIT}<p class="limit" role="status">Showing up to {FLOAT_HISTORY_LIMIT} stored messages in this window.</p>{/if}
    {#if hasMore}<button class="older" disabled={olderLoading || rows.length >= FLOAT_HISTORY_LIMIT} title={rows.length >= FLOAT_HISTORY_LIMIT ? "Floating history limit reached." : undefined}
      onclick={() => { if (rows.length < FLOAT_HISTORY_LIMIT) void onolder(); }}>{olderLoading ? "Loading…" : "Load older messages"}</button>{/if}
    {#if !rulesReady}<p role="status">Messages are hidden until keyword rules can be read.</p>
    {:else if loading && !rows.length}<p role="status">Loading messages…</p>
    {:else if !rows.length}<p>No stored messages.</p>
    {:else if !visibleRows.length}<p>All loaded messages are hidden by your keyword rules.</p>{/if}
    <ol>{#each visibleRows as message (message.id)}
      {@const content = floatContent(message)}
      <li class:mine={message.from_me} class:notice={content.notice} class:keyword-highlighted={keywordHighlighted(message, rules)}>
        <b>{message.from_me ? "You" : message.sender_name || message.sender || "Unknown sender"}</b>
        {#if content.media}<span class="media">{content.media}</span>{/if}
        <div class="text">
          {#each blocks(content.notice || content.media === "One-time media" || message.spoiler ? content.text : message.text.trim() === `[${message.media_kind}]` ? "" : message.text) as block}
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
  {#if sendError}<p class="error" role="alert">{sendError}</p>{/if}
  {#if draftError}<p class="error" role="alert">{draftError}<button type="button" onclick={onretrydraft}>Retry draft save</button></p>{/if}
  <form onsubmit={(event) => { event.preventDefault(); if (canSend) void onsend(); }}>
    <textarea aria-label="Reply to this chat" placeholder="Reply…" value={draft} disabled={!draftReady || sending || closing}
      oninput={(event) => ondraft(event.currentTarget.value)} onkeydown={key} rows="2"></textarea>
    <button type="submit" disabled={!canSend}>{sending ? "Sending…" : "Send"}</button>
  </form>
</main>

<style>
  :global(html), :global(body) { margin: 0; background: transparent; }
  .float-chat { display: flex; flex-direction: column; height: 100dvh; box-sizing: border-box; padding: 10px; gap: 8px; font: 13px system-ui, sans-serif; background: color-mix(in srgb, var(--bg, #111b21) var(--float-alpha), transparent); color: var(--text, #e9edef); }
  header, .toolbar, form { display: flex; align-items: center; gap: 8px; }
  header { justify-content: space-between; }
  h1 { margin: 0; font-size: 15px; overflow-wrap: anywhere; }
  button, textarea { font: inherit; color: inherit; }
  button { padding: 6px 10px; border: 1px solid var(--line-strong, #3b4a54); border-radius: 6px; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); cursor: pointer; }
  button:disabled { cursor: default; color: var(--muted, #8696a0); }
  header button { font-size: 19px; padding: 0 7px; }
  .toolbar { font-size: 11px; justify-content: space-between; }
  .toolbar label { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; }
  .toolbar input { flex: 1; min-width: 0; width: 80px; }
  .connection, time { margin: 0; font-size: 11px; color: var(--muted, #8696a0); }
  .history { flex: 1; min-height: 0; overflow-y: auto; overflow-anchor: none; }
  .history > ol { display: flex; flex-direction: column; align-items: flex-start; margin: 0; padding: 0; list-style: none; }
  .history > ol > li { max-width: 90%; padding: 8px 10px; margin: 4px 0; border-radius: 8px; overflow-wrap: anywhere; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); }
  .history > ol > li.mine { align-self: flex-end; background: color-mix(in srgb, var(--bubble-mine, #005c4b) var(--float-alpha), transparent); }
  .history > ol > li.notice { align-self: center; }
  .history > ol > li.keyword-highlighted { outline: 2px solid var(--mention, #f0b232); outline-offset: -2px; }
  b, .media { display: block; font-size: 11px; }
  .media { color: var(--muted, #8696a0); }
  .text { white-space: pre-wrap; }
  .text p { margin: 4px 0; }
  pre { margin: 4px 0; white-space: pre-wrap; }
  blockquote { margin: 4px 0; padding-left: 8px; border-left: 2px solid var(--muted, #8696a0); }
  .mention { color: var(--link, #53bdeb); }
  textarea { flex: 1; min-width: 0; resize: vertical; padding: 8px; border: 1px solid var(--line-strong, #3b4a54); border-radius: 6px; background: color-mix(in srgb, var(--surface, #202c33) var(--float-alpha), transparent); }
  .error { margin: 0; color: var(--danger, #ff6b6b); font-size: 12px; }
  .error button { margin-left: 8px; }
</style>
