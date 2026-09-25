<!-- Discord-style full-window panel: searchable section list left, section right. -->
<script lang="ts" generics="S extends string">
  import type { Snippet } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import { cubicOut } from "svelte/easing";
  import Icon from "$lib/Icon.svelte";

  let {
    label,
    nav,
    section = $bindable(),
    header,
    children,
    footer,
    onclose,
  }: {
    label: string;
    nav: { id: S; label: string; group: string }[];
    section: S;
    header: Snippet;
    children: Snippet;
    footer?: Snippet;
    onclose: () => void;
  } = $props();

  let query = $state("");
  const shown = $derived(
    nav.filter((n) => n.label.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const groups = $derived([...new Set(shown.map((n) => n.group))]);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(160) }}
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-label={label}
    transition:scale|global={{ start: 0.94, duration: motion(200), easing: cubicOut }}>
    <nav>
      {@render header()}

      <label class="nav-search">
        <Icon name="search" size={14} />
        <input placeholder="Search" bind:value={query} />
      </label>

      {#each groups as group (group)}
        <span class="nav-group">{group}</span>
        {#each shown.filter((n) => n.group === group) as item (item.id)}
          <button
            class="nav-item"
            class:active={section === item.id}
            onclick={() => (section = item.id)}>{item.label}</button>
        {/each}
      {/each}
    </nav>

    <main>
      <button class="close" title="Close" aria-label="Close {label}" onclick={onclose}>
        <Icon name="x" size={18} />
        <span>ESC</span>
      </button>
      <div class="content">{@render children()}</div>
      {@render footer?.()}
    </main>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 260;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }
  .modal {
    display: grid;
    grid-template-columns: 250px 1fr;
    width: min(1100px, 94vw);
    height: min(820px, 92vh);
    overflow: hidden;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 12px;
    background: var(--surface);
    overflow-y: auto;
  }
  .nav-search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    padding: 0 10px;
    height: 32px;
    background: var(--bg);
    border-radius: 6px;
    color: var(--muted);
  }
  .nav-search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    outline: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
  }
  .nav-group {
    margin: 12px 10px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .nav-item {
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--muted);
    font: inherit;
    font-size: 14.5px;
    padding: 7px 10px;
    cursor: pointer;
  }
  .nav-item:hover {
    background: var(--raised);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--raised-2);
    color: var(--text);
  }
  main {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .close {
    position: absolute;
    top: 18px;
    right: 22px;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 0;
    color: var(--muted);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .close :global(svg) {
    padding: 6px;
    border: 2px solid currentColor;
    border-radius: 50%;
  }
  .close:hover {
    color: var(--text);
  }
  .content {
    flex: 1;
    overflow-y: auto;
    padding: 32px clamp(24px, 6%, 72px) 96px 40px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 760px;
  }

  /* Shared building blocks for every panel's sections. */
  .content :global(h2) {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 600;
  }
  .content :global(.lede) {
    margin: 0 0 16px;
    color: var(--muted);
    font-size: 14px;
  }
  .content :global(.card) {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border-radius: var(--radius);
  }
  .content :global(.actions-row) {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .content :global(.setting) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px 0;
    border-bottom: 1px solid var(--line);
  }
  .content :global(.setting.stack) {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .content :global(.setting > div) {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .content :global(.setting-title) {
    font-size: 15px;
  }
  .content :global(.setting-desc),
  .content :global(.muted) {
    font-size: 13px;
    color: var(--muted);
  }
  .content :global(.field) {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 7px 10px;
    color: inherit;
    font: inherit;
    font-size: 14px;
  }
  .content :global(.tag) {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    padding: 0 8px;
  }
  .content :global(.switch) {
    appearance: none;
    flex: none;
    position: relative;
    width: 40px;
    height: 24px;
    margin: 0;
    border-radius: 999px;
    background: var(--raised-2);
    cursor: pointer;
    transition: background-color calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .content :global(.switch::after) {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #fff;
    transition: transform calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .content :global(.switch:checked) {
    background: var(--accent);
  }
  .content :global(.switch:checked::after) {
    transform: translateX(16px);
  }
  main :global(.button) {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--raised);
    border: 0;
    border-radius: 6px;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    padding: 8px 14px;
    cursor: pointer;
    white-space: nowrap;
  }
  main :global(.button:hover:not(:disabled)) {
    background: var(--raised-2);
  }
  main :global(.button.primary) {
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }
  main :global(.button.primary:hover:not(:disabled)) {
    background: var(--accent-hover);
  }
  main :global(.button.danger) {
    color: var(--danger);
  }
  main :global(.button:disabled) {
    opacity: 0.6;
    cursor: default;
  }
  .content :global(.error-text) {
    margin: 8px 0 0;
    color: var(--danger);
    font-size: 13px;
  }
</style>
