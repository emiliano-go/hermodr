<script lang="ts" module>
  import type { IconName } from "$lib/Icon.svelte";
  export type MenuItem = {
    label: string;
    icon: IconName;
    action: () => void;
    danger?: boolean;
    /** Draws a divider above the item. */
    separated?: boolean;
  };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    x,
    y,
    items,
    reactions,
    current,
    onreact,
    onclose,
  }: {
    x: number;
    y: number;
    items: MenuItem[];
    /** Quick reactions, shown as a row above the items. */
    reactions: string[];
    /** Our current reaction, which a second click takes back. */
    current: string | null;
    onreact: (emoji: string) => void;
    onclose: () => void;
  } = $props();

  let menu: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  // Opens at the pointer but never past the window's edges.
  onMount(() => {
    const rect = menu!.getBoundingClientRect();
    pos = {
      left: Math.min(x, window.innerWidth - rect.width - 8),
      top: Math.min(y, window.innerHeight - rect.height - 8),
    };
    menu!.querySelector("button")?.focus();
  });
</script>

<svelte:window
  onkeydown={(e) => e.key === "Escape" && onclose()}
  onblur={onclose}
  onresize={onclose} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="catcher"
  role="presentation"
  onclick={onclose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onclose();
  }}></div>
<div class="menu" role="menu" bind:this={menu} style="left: {pos.left}px; top: {pos.top}px">
  <div class="reactions">
    {#each reactions as emoji (emoji)}
      <button
        class="reaction"
        class:mine={current === emoji}
        role="menuitem"
        aria-label="React {emoji}"
        onclick={() => onreact(current === emoji ? "" : emoji)}>{emoji}</button>
    {/each}
  </div>
  {#each items as item (item.label)}
    {#if item.separated}<div class="sep"></div>{/if}
    <button
      class="item"
      class:danger={item.danger}
      role="menuitem"
      onclick={() => {
        onclose();
        item.action();
      }}>
      <Icon name={item.icon} size={18} />
      {item.label}
    </button>
  {/each}
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 270;
  }
  .menu {
    position: fixed;
    z-index: 271;
    min-width: 230px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
    animation: pop calc(0.12s * var(--motion-scale)) ease-out;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  .reactions {
    display: flex;
    gap: 2px;
    padding: 2px 2px 6px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--line-strong);
  }
  .reaction {
    flex: 1;
    height: 36px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    font-size: 20px;
    cursor: pointer;
    transition: transform calc(0.1s * var(--motion-scale)) var(--ease);
  }
  .reaction:hover {
    background: var(--raised);
    transform: scale(1.15);
  }
  .reaction.mine {
    background: var(--accent-soft);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    text-align: left;
    cursor: pointer;
  }
  .item :global(svg) {
    color: var(--muted);
  }
  .item:hover,
  .item:focus-visible {
    background: var(--raised);
    outline: none;
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--danger);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line-strong);
  }
</style>
