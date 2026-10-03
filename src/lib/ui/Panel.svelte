<!-- Discord-style full-window panel: searchable section list left, section right. -->
<script lang="ts" generics="S extends string">
  import { t } from "$lib/i18n/localizer";
  import type { Snippet } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import { cubicOut } from "svelte/easing";
  import Icon from "$lib/ui/Icon.svelte";
  import { onMount } from "svelte";

  let {
    label,
    nav,
    section = $bindable(),
    header,
    pageHead,
    children,
    footer,
    onclose,
  }: {
    label: string;
    nav: { id: S; label: string; group: string }[];
    section: S;
    header: Snippet;
    pageHead?: Snippet;
    children: Snippet;
    footer?: Snippet;
    onclose: () => void;
  } = $props();

  let query = $state("");
  const shown = $derived(
    nav.filter((n) => n.label.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const groups = $derived([...new Set(shown.map((n) => n.group))]);

  // Focus management (WCAG 2.4.3/2.4.7): trap Tab inside the dialog, focus the
  // dialog on open, and restore focus to the opener on close.
  let modal: HTMLDivElement | undefined = $state();
  let opener: Element | null = null;
  onMount(() => {
    opener = document.activeElement;
    // Focus the search field first so keyboard users land on a control.
    const first = modal?.querySelector<HTMLElement>("input, button:not(.close), [tabindex]") ?? modal;
    first?.focus({ preventScroll: true });
    return () => {
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    };
  });

  function trapTab(e: KeyboardEvent) {
    if (e.key !== "Tab" || !modal) return;
    const items = [...modal.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])',
    )].filter((el) => el.offsetParent !== null || el === document.activeElement);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={(e) => {
  if (e.key === "Escape") onclose();
  else trapTab(e);
}} />

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
    bind:this={modal}
    transition:scale|global={{ start: 0.94, duration: motion(200), easing: cubicOut }}>
    <nav>
      {@render header()}

      <label class="nav-search">
        <Icon name="search" size={14} />
        <input placeholder={t("ui.search")} bind:value={query} />
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
      <div class="topbar" class:bare={!pageHead}>
        <div class="titles">{@render pageHead?.()}</div>
        <button class="close" title={t("ui.close")} aria-label="{t("ui.close")} {label}" onclick={onclose}>
          <Icon name="x" size={18} />
          <span>{t("ui.escape_key")}</span>
        </button>
      </div>
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
    padding: 7px 10px;
    height: 32px;
    background: var(--bg);
    border-radius: var(--radius-lg);
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
    font-size: 0.8125rem;
  }
  .nav-group {
    margin: 12px 2px 4px;
    font-size: 0.7188rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .nav-item {
    text-align: start;
    background: transparent;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: 0.9062rem;
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
  .topbar {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 24px clamp(24px, 6%, 72px) 14px 40px;
    border-bottom: 1px solid var(--line);
    background: var(--bg);
  }
  .topbar.bare {
    border-bottom: 0;
    padding: 18px 22px 0;
    justify-content: flex-end;
  }
  .topbar.bare .titles {
    display: none;
  }
  .titles {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .titles :global(h2) {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
  }
  .titles :global(.lede) {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .close {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 0;
    color: var(--muted);
    font: inherit;
    font-size: 0.6875rem;
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
    padding: 20px clamp(24px, 6%, 72px) 96px 40px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 760px;
  }

  /* Shared building blocks for every panel's sections. */
  .content :global(h2) {
    margin: 0 0 6px;
    font-size: 1.25rem;
    font-weight: 600;
  }
  .content :global(.lede) {
    margin: 0 0 16px;
    color: var(--muted);
    font-size: 0.875rem;
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
    font-size: 0.9375rem;
  }
  .content :global(.setting-desc),
  .content :global(.muted) {
    font-size: 0.8125rem;
    color: var(--muted);
  }
  .content :global(.field) {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 7px 10px;
    color: inherit;
    font: inherit;
    font-size: 0.875rem;
  }
  .content :global(.tag) {
    font-size: 0.75rem;
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
    inset-inline-start: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--text);
    transition: transform calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .content :global(.switch:checked) {
    background: var(--accent);
  }
  .content :global(.switch:checked::after) {
    background: var(--accent-ink);
    transform: translateX(16px);
  }
  main :global(.button) {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--raised);
    border: 0;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
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
    font-size: 0.8125rem;
  }
  :global([dir="rtl"]) .content :global(.switch:checked::after) { transform: translateX(-16px); }
</style>
