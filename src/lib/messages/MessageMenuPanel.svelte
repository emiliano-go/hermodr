<script lang="ts" module>
  import type { IconName } from "$lib/ui/Icon.svelte";
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
  import Icon from "$lib/ui/Icon.svelte";

  let {
    element = $bindable(), left, top, closing = false,
    items, reactions, current, onreact, onmore, onclose,
  }: {
    element?: HTMLDivElement;
    left: number;
    top: number;
    closing?: boolean;
    items: MenuItem[];
    reactions: string[];
    current: string | null;
    onreact: (emoji: string) => void;
    onmore: () => void;
    onclose: () => void;
  } = $props();
</script>

<div class="menu" class:closing role="menu" bind:this={element} style="left: {left}px; top: {top}px">
  <div class="reactions">
    {#each reactions as emoji (emoji)}
      <button
        class="reaction"
        class:mine={current === emoji}
        role="menuitem"
        aria-label="React {emoji}"
        onclick={() => {
          onclose();
          onreact(current === emoji ? "" : emoji);
        }}>{emoji}</button>
    {/each}
    <button
      class="reaction more"
      role="menuitem"
      aria-label="More reactions"
      title="More reactions"
      onclick={() => {
        onclose();
        onmore();
      }}><Icon name="plus" size={18} /></button>
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
  .menu {
    position: fixed;
    z-index: 271;
    min-width: 230px;
    max-height: calc(100dvh - 16px);
    overflow-y: auto;
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
  .menu.closing {
    animation: pop-out calc(0.12s * var(--motion-scale)) ease-in forwards;
    pointer-events: none;
  }
  @keyframes pop-out {
    to {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  .reactions {
    flex-shrink: 0;
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
  .reaction.more {
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .reaction.more:hover {
    color: var(--text);
  }
  .item {
    flex-shrink: 0;
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
    flex-shrink: 0;
    height: 1px;
    margin: 4px 6px;
    background: var(--line-strong);
  }
</style>
