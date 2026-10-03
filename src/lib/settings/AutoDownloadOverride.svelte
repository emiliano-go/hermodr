<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { MEDIA_TYPES } from "$lib/utils/auto-download";
  import { MediaPolicyState } from "$lib/state/media-policy.svelte";
  import type { MediaAutoDownload } from "$lib/utils/wire";
  let { accountId, chat }: { accountId: string; chat: string } = $props();
  const policy = new MediaPolicyState();
  $effect(() => { void policy.load(accountId, chat); });
  function change(kind: keyof MediaAutoDownload, select: HTMLSelectElement) {
    const next = select.value;
    select.value = policy.value[kind] === null ? "" : String(policy.value[kind]);
    void policy.change(kind, next === "" ? null : next === "true");
  }
</script>

<div class="downloads">
  <p>{t("settings.download_chat")}</p>
  {#each MEDIA_TYPES as [kind]}
    <label>
      <span>{t(`settings.media_${kind}`)}</span>
      <select value={policy.value[kind] === null ? "" : String(policy.value[kind])}
        disabled={policy.busy || !!policy.error}
        onchange={(event) => change(kind, event.currentTarget)}>
        <option value="">{t("settings.follow_global")}</option>
        <option value="true">{t("ui.on")}</option>
        <option value="false">{t("ui.off")}</option>
      </select>
    </label>
  {/each}
  {#if policy.error}
    <p role="alert">{policy.error}</p>
    <button type="button" disabled={policy.busy} onclick={() => policy.load(accountId, chat)}>{t("ui.retry")}</button>
  {/if}
</div>

<style>
  .downloads { display: flex; flex-direction: column; gap: .75rem; }
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .8rem; margin: 0; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
