<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onDestroy, untrack } from "svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { session } from "$lib/state/session.svelte";

  let { chat, canReset = false, onload, onreset }: { chat: string; canReset?: boolean; onload: () => Promise<string>; onreset: () => Promise<string> } = $props();
  let link = $state<string | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let confirming = $state(false);
  let error = $state<LocalizedError | string | null>(null);
  let outcome = $state("");
  let generation = 0;

  function current(target: string, account: string | null, revision: number) {
    return target === chat && account === session.activeAccount && revision === generation;
  }

  async function refresh() {
    if (loading || busy) return;
    const target = chat, account = session.activeAccount, revision = generation;
    loading = true; error = null;
    try {
      const result = await onload();
      if (current(target, account, revision)) link = result;
    } catch (failure) { if (current(target, account, revision)) error = normalizeError(failure); }
    finally { if (current(target, account, revision)) loading = false; }
  }

  $effect(() => {
    chat; session.activeAccount; ++generation;
    link = null; loading = false; busy = false; confirming = false; error = null; outcome = "";
    void untrack(refresh);
  });
  onDestroy(() => { generation++; });
  $effect(() => { if (!canReset) confirming = false; });

  async function reset() {
    if (!canReset || !confirming || busy || loading) return;
    const target = chat, account = session.activeAccount, revision = generation;
    confirming = false; busy = true; error = null; outcome = "";
    try {
      const result = await onreset();
      if (current(target, account, revision)) { link = result; outcome = "group.invite_reset_done"; }
    } catch (failure) { if (current(target, account, revision)) { link = null; error = normalizeError(failure); } }
    finally { if (current(target, account, revision)) busy = false; }
  }

  async function copy() {
    if (!link || busy || loading) return;
    const target = chat, account = session.activeAccount, revision = generation;
    try {
      await navigator.clipboard.writeText(link);
      if (current(target, account, revision)) outcome = "group.invite_copied";
    } catch (failure) { if (current(target, account, revision)) error = normalizeError(failure); }
  }
</script>

<div class="group-invite-links">
  <h3>{t("group.invite_link")}</h3>
  {#if link}<input aria-label={t("group.invite_label")} readonly value={link} />{/if}
  {#if loading || (!link && !error)}<p>{t("group.invite_loading")}</p>{:else if busy}<p>{t("group.invite_resetting")}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if outcome}<p role="status">{t(outcome)}</p>{/if}
  <div class="actions">
    <button disabled={!link || loading || busy} onclick={copy}>{t("contact.copy_link")}</button>
    {#if canReset}<button disabled={!link || loading || busy} onclick={() => { confirming = true; }}>{t("group.invite_reset_more")}</button>{/if}
    {#if error}<button disabled={loading || busy} onclick={refresh}>{t("ui.retry")}</button>{/if}
  </div>
</div>

{#if confirming}
  <ConfirmDialog label={t("group.invite_reset_title")} title={t("group.invite_reset_question")} hint={t("group.invite_reset_hint")}
    onclose={() => { if (!busy) confirming = false; }}>
    {#snippet actions()}
      <button onclick={() => { confirming = false; }}>{t("ui.cancel")}</button>
      <button class="danger" onclick={reset}>{t("group.invite_reset")}</button>
    {/snippet}
  </ConfirmDialog>
{/if}

<style>
  .group-invite-links { display: flex; flex-direction: column; gap: 10px; }
  h3, p { margin: 0; }
  input { width: 100%; box-sizing: border-box; padding: 8px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; font: inherit; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  button { padding: 7px 10px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  .error, .danger { color: var(--danger); }
</style>
