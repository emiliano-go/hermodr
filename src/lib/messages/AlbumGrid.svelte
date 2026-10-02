<script lang="ts">
  import type { Snippet } from "svelte";
  import type { StoredMessage } from "$lib/utils/wire";

  let { items, prev = undefined, timestamp, formatTime, children, header }: {
    items: readonly StoredMessage[];
    prev?: StoredMessage;
    timestamp: number;
    formatTime: (timestamp: number) => string;
    children: Snippet<[StoredMessage, StoredMessage | undefined]>;
    header?: Snippet;
  } = $props();
  const date = $derived(new Date(timestamp * 1000));
</script>

{#if items.length}
  <div class="album-grid" class:mine={items[0].from_me} role="group" aria-label="Media album">
    {#if header}{@render header()}{/if}
    <div class="tiles" class:single={items.length === 1}>
      {#each items as message, index (JSON.stringify([message.chat, message.id]))}
        <div class="tile">{@render children(message, index > 0 ? items[index - 1] : prev)}</div>
      {/each}
    </div>
    <footer><time datetime={Number.isFinite(date.getTime()) ? date.toISOString() : undefined}>{formatTime(timestamp)}</time></footer>
  </div>
{/if}

<style>
  .album-grid { width: min(480px, 100%); max-width: 100%; min-width: 0; align-self: flex-start; }
  .album-grid.mine { align-self: flex-end; }
  .tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
  .tiles.single { grid-template-columns: minmax(0, 1fr); }
  .tile { min-width: 0; max-width: 100%; }
  .tile:first-child:nth-last-child(3) { grid-column: 1 / -1; }
  footer { padding: 4px 6px; color: var(--muted); font-size: .8em; text-align: right; overflow-wrap: anywhere; }
</style>
