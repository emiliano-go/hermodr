<!-- The small confirm sheet shared by delete/report style actions. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    label,
    title,
    hint,
    actions,
    onclose,
  }: {
    label: string;
    title: string;
    hint: string;
    actions: Snippet;
    onclose: () => void;
  } = $props();
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="sheet-backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="sheet confirm" role="dialog" aria-modal="true" aria-label={label}>
    <h2>{title}</h2>
    <p class="hint">{hint}</p>
    <div class="confirm-actions">{@render actions()}</div>
  </div>
</div>

<style>
  .sheet-backdrop {
    position: fixed;
    inset: 0;
    z-index: 250;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 20px 22px;
    width: min(460px, 90vw);
    max-height: 86vh;
    overflow-y: auto;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .sheet.confirm {
    width: min(400px, 90vw);
  }
  .sheet.confirm h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    color: var(--faint);
    font-size: 12px;
    max-width: 44ch;
    text-wrap: balance;
  }
  .confirm-actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .confirm-actions :global(button) {
    padding: 8px 14px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--accent);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .confirm-actions :global(button:hover) {
    background: var(--raised);
  }
  .confirm-actions :global(button.danger) {
    color: var(--danger);
  }
</style>
