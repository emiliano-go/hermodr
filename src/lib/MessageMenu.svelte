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
  import { motion } from "$lib/theme.svelte";

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
  let closing = $state(false);

  /**
   * Fades out, then tells the parent to drop the menu.
   *
   * The removal is deferred past the event that dismissed it (a right-click
   * here crashed WebKitGTK when the overlay vanished mid-dispatch), so the wait
   * is a frame plus the fade rather than a bare timeout.
   */
  function close() {
    if (closing) return;
    closing = true;
    const ms = Math.max(1, motion(120));
    requestAnimationFrame(() => setTimeout(onclose, ms));
  }

  // Position after paint, then focus. Doing either during the opening click
  // can race the engine's own focus/context-menu handling.
  onMount(() => {
    requestAnimationFrame(() => {
      const rect = menu?.getBoundingClientRect();
      if (rect) {
        pos = {
          left: Math.max(8, Math.min(x, window.innerWidth - rect.width - 8)),
          top: Math.max(8, Math.min(y, window.innerHeight - rect.height - 8)),
        };
      }
      menu?.querySelector("button")?.focus();
    });

    // Outside dismissal runs in the capture phase so the second right-click is
    // consumed before it reaches the message behind the menu.
    const outside = (target: EventTarget | null) =>
      menu ? !menu.contains(target as Node) : true;
    const onPointerDown = (event: PointerEvent) => {
      if (outside(event.target)) close();
    };
    const onContextMenu = (event: MouseEvent) => {
      if (menu?.contains(event.target as Node)) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
    };
    const onResize = () => close();

    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("contextmenu", onContextMenu, true);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("contextmenu", onContextMenu, true);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

<div class="menu" class:closing role="menu" bind:this={menu} style="left: {pos.left}px; top: {pos.top}px">
  <div class="reactions">
    {#each reactions as emoji (emoji)}
      <button
        class="reaction"
        class:mine={current === emoji}
        role="menuitem"
        aria-label="React {emoji}"
        onclick={() => {
          close();
          onreact(current === emoji ? "" : emoji);
        }}>{emoji}</button>
    {/each}
  </div>
  {#each items as item (item.label)}
    {#if item.separated}<div class="sep"></div>{/if}
    <button
      class="item"
      class:danger={item.danger}
      role="menuitem"
      onclick={() => {
        close();
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
