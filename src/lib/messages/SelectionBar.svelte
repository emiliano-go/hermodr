<!-- Replaces the composer while messages are picked for a bulk action. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import Icon from "$lib/ui/Icon.svelte";

  let {
    count,
    onforward,
    ondelete,
    oncopy,
    onstar,
    onlabel = () => {},
    onreact,
    reactionReason = null,
    allStarred = false,
    oncancel,
  }: {
    count: number;
    onforward: () => void;
    ondelete: () => void;
    oncopy: () => unknown;
    onstar: () => unknown;
    onlabel?: () => unknown;
    onreact: (event: MouseEvent) => void;
    reactionReason?: string | null;
    allStarred?: boolean;
    oncancel: () => void;
  } = $props();
  let busy = $state(false);

  async function run(action: () => unknown) {
    if (busy) return;
    busy = true;
    try { await action(); } finally { busy = false; }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && oncancel()} />

<div class="selection-bar">
  <button class="icon" aria-label={t("content.cancel_selection")} title={t("content.cancel")} onclick={oncancel}>
    <Icon name="x" size={18} />
  </button>
  <span class="count">{t("content.selected_count", { count })}</span>
  <button class="action" disabled={count === 0 || busy} onclick={() => run(onlabel)}>{t("content.labels")}</button>
  <button class="forward" disabled={count === 0 || busy} onclick={onforward}>
    <Icon name="forward" size={16} /> {t("content.forward")}
  </button>
  <button class="action" disabled={count === 0 || busy} onclick={() => run(oncopy)}>
    <Icon name="copy" size={16} /> {t("content.copy")}
  </button>
  <button class="action" disabled={count === 0 || busy} onclick={() => run(onstar)}>
    <Icon name="star" size={16} /> {allStarred ? t("content.unstar") : t("content.star")}
  </button>
  <button class="action" disabled={count === 0 || busy || !!reactionReason} title={reactionReason ?? undefined}
    onclick={(event) => { if (!busy && !reactionReason) onreact(event); }}>
    <Icon name="smile" size={16} /> {t("content.react")}
  </button>
  <button class="delete" disabled={count === 0 || busy} onclick={ondelete}>
    <Icon name="trash" size={16} /> {t("content.delete")}
  </button>
</div>

<style>
  .selection-bar {
    display: flex;
    flex-wrap: wrap;
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
  .forward,
  .action,
  .delete {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: 0;
    border-radius: 999px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .forward:hover:not(:disabled) {
    background: var(--raised-2);
  }
  .action:hover:not(:disabled) { background: var(--raised-2); }
  .delete {
    color: var(--danger);
  }
  .delete:hover:not(:disabled) {
    background: var(--danger-soft, var(--raised));
  }
  .forward:disabled,
  .action:disabled,
  .delete:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
