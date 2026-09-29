<!-- Replaces the composer while messages are picked for a bulk action. -->
<script lang="ts">
  import Icon from "$lib/Icon.svelte";

  let {
    count,
    ondelete,
    oncancel,
  }: { count: number; ondelete: () => void; oncancel: () => void } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && oncancel()} />

<div class="selection-bar">
  <button class="icon" aria-label="Cancel selection" title="Cancel" onclick={oncancel}>
    <Icon name="x" size={18} />
  </button>
  <span class="count">{count} selected</span>
  <button class="delete" disabled={count === 0} onclick={ondelete}>
    <Icon name="trash" size={16} /> Delete
  </button>
</div>

<style>
  .selection-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 16px;
    min-height: 62px;
    box-sizing: border-box;
    background: var(--surface);
    flex: none;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--raised);
    color: var(--text);
  }
  .count {
    flex: 1;
    color: var(--text);
    font-size: 14px;
  }
  .delete {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: 0;
    border-radius: 999px;
    background: var(--raised);
    color: var(--danger);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .delete:hover:not(:disabled) {
    background: var(--danger-soft, var(--raised));
  }
  .delete:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
