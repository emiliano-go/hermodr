<script lang="ts">
  import type { BroadcastList } from "$lib/utils/wire";

  let { info, loading, error, nameOf }: {
    info: BroadcastList | null;
    loading: boolean;
    error: string | null;
    nameOf: (jid: string) => string;
  } = $props();

  const recipients = $derived([...new Set(info?.recipients ?? [])]);
  const sourceDate = $derived.by(() => {
    const seconds = info?.source_timestamp ?? 0;
    if (!Number.isSafeInteger(seconds) || seconds <= 0) return null;
    const date = new Date(seconds * 1000);
    return Number.isNaN(date.getTime()) ? null : date;
  });
</script>

<section class="broadcast-info" aria-label="Broadcast recipients">
  <h3>Broadcast recipients</h3>
  {#if loading}
    <p role="status">Loading recipient details…</p>
  {:else if error}
    <p class="error" role="alert">Recipient details unavailable. {error}</p>
  {:else if !recipients.length}
    <p role="status">Recipient details unavailable.</p>
  {:else}
    <p class="source">Cached from a received message.
      {#if sourceDate}
        <time datetime={sourceDate.toISOString()}>{sourceDate.toLocaleString()}</time>
      {:else}
        <span>Message time unavailable.</span>
      {/if}
    </p>
    <ul>
      {#each recipients as jid (jid)}
        <li>{nameOf(jid)}</li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .broadcast-info { padding: 16px; }
  h3 { margin: 0 0 10px; font-size: 15px; }
  p { margin: 0; }
  .source { color: var(--muted); font-size: 12px; }
  .source time, .source span { display: block; margin-top: 4px; }
  .error { color: var(--danger); }
  ul { margin: 12px 0 0; padding: 0; list-style: none; max-height: 280px; overflow-y: auto; }
  li { padding: 8px 0; overflow-wrap: anywhere; }
  li + li { border-top: 1px solid var(--line); }
</style>
