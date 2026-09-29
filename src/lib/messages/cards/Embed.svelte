<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * One card for everything drawn inside a bubble: link embeds, quotes, and
   * message kinds without a view of their own (location, contacts…).
   * `compact` is the quote form; clicking anywhere runs `onclick`. The full
   * form links its title and image to `onopen`.
   */
  let {
    label,
    title = null,
    text = null,
    image = null,
    icon = null,
    color = null,
    compact = false,
    tooltip,
    onclick,
    onopen,
    children,
  }: {
    label?: string | null;
    title?: string | null;
    text?: string | null;
    /** A picture URL, already resolved. */
    image?: string | null;
    /** A glyph shown where the picture would be, when there is none. */
    icon?: string | null;
    /** The side bar's colour; the accent when unset. */
    color?: string | null;
    compact?: boolean;
    tooltip?: string;
    onclick?: () => void;
    onopen?: () => void;
    children?: Snippet;
  } = $props();

  // As Discord draws embeds: a small image sits beside the text, a large one under it.
  let wide = $state(false);
</script>

{#snippet body()}
  <span class="embed-body">
    {#if label}<span class="embed-label">{label}</span>{/if}
    {#if title}
      {#if onopen}
        <button type="button" class="embed-title link" title={tooltip} onclick={onopen}>{title}</button>
      {:else}
        <span class="embed-title">{title}</span>
      {/if}
    {/if}
    {#if text}<span class="embed-text">{text}</span>{/if}
    {@render children?.()}
  </span>
  {#if image}
    {#if onopen && !compact}
      <button type="button" class="embed-image" title={tooltip} onclick={onopen}>
        <img
          src={image}
          alt=""
          onload={(e) => (wide = (e.currentTarget as HTMLImageElement).naturalWidth >= 300)} />
      </button>
    {:else}
      <span class="embed-image"><img src={image} alt="" /></span>
    {/if}
  {:else if icon}
    <span class="embed-icon">{icon}</span>
  {/if}
{/snippet}

{#if compact && onclick}
  <button type="button" class="embed quote compact" title={tooltip} style:--embed-color={color} {onclick}>
    {@render body()}
  </button>
{:else}
  <div class="embed" class:compact class:wide style:--embed-color={color}>{@render body()}</div>
{/if}

<style>
  .embed {
    display: flex;
    gap: 16px;
    max-width: 432px;
    margin-top: 4px;
    padding: 10px 14px 14px 12px;
    box-sizing: border-box;
    background: rgba(0, 0, 0, 0.18);
    border: 0;
    border-left: 4px solid var(--embed-color, var(--accent));
    border-radius: 4px;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .embed.wide {
    flex-direction: column;
    gap: 10px;
  }
  .embed-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .embed-label {
    font-size: 12px;
    color: var(--muted);
  }
  .embed-title {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    text-align: left;
  }
  .embed-title.link {
    color: var(--link);
    cursor: pointer;
  }
  .embed-title.link:hover {
    text-decoration: underline;
  }
  .embed-text {
    font-size: 13.5px;
    line-height: 18px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .embed-image {
    flex: none;
    align-self: flex-start;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    overflow: hidden;
  }
  button.embed-image {
    cursor: pointer;
  }
  .embed-image img {
    display: block;
    width: 80px;
    height: 80px;
    object-fit: cover;
  }
  .embed-icon {
    flex: none;
    font-size: 14px;
  }
  .embed.wide .embed-image img {
    width: auto;
    height: auto;
    max-width: 100%;
    max-height: 300px;
  }

  /* The quote form, as quotes have always looked. */
  .embed.compact {
    align-items: center;
    gap: 8px;
    max-width: 100%;
    margin: 0 0 2px;
    padding: 5px 8px 6px;
    border-radius: 6px;
    font-size: 13px;
    line-height: 18px;
    color: var(--muted);
    overflow: hidden;
  }
  button.embed.compact {
    cursor: pointer;
  }
  button.embed.compact:hover {
    background: rgba(0, 0, 0, 0.26);
  }
  .compact .embed-body {
    gap: 0;
  }
  .compact .embed-label {
    font-size: inherit;
    font-weight: 500;
    color: var(--embed-color, var(--accent));
  }
  .compact .embed-text {
    font-size: inherit;
    line-height: inherit;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .compact .embed-image img {
    width: 42px;
    height: 42px;
  }
</style>
