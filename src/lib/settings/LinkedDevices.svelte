<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import type { LinkedDevice } from "$lib/utils/wire";

  let { account, connected, onload = (id) => invoke<LinkedDevice[]>("linked_devices", { account: id }),
    onunlink = (id, jid) => invoke<void>("unlink_device", { account: id, jid }) }: {
    account: string | null;
    connected: boolean;
    onload?: (account: string) => Promise<LinkedDevice[]>;
    onunlink?: (account: string, jid: string) => Promise<void>;
  } = $props();

  let devices = $state<LinkedDevice[] | null>(null);
  let pending = $state<LinkedDevice | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let error = $state<LocalizedError | string>("");
  let result = $state<number | null>(null);
  let generation = 0;

  function current(id: string, revision: number) {
    return connected && id === account && revision === generation;
  }

  async function refresh() {
    if (!account || !connected || busy || loading) return;
    const id = account, revision = generation;
    loading = true;
    pending = null;
    error = "";
    try {
      const rows = await onload(id);
      if (current(id, revision)) devices = rows;
    } catch (failure) {
      if (current(id, revision)) error = normalizeError(failure);
    } finally {
      if (current(id, revision)) loading = false;
    }
  }

  $effect(() => {
    account;
    connected;
    ++generation;
    devices = pending = null;
    loading = busy = false;
    error = ""; result = null;
    void untrack(refresh);
    return () => { ++generation; };
  });

  async function unlink() {
    if (!account || !connected || !pending?.can_unlink || busy || loading) return;
    const id = account, device = pending, revision = generation;
    busy = true;
    error = ""; result = null;
    try {
      await onunlink(id, device.jid);
      if (!current(id, revision)) return;
      pending = null;
      result = device.device_id;
      try {
        const rows = await onload(id);
        if (current(id, revision)) devices = rows;
      } catch (failure) {
        if (current(id, revision)) {
          devices = null;
          error = normalizeError({ kind: "postal_error", code: "error.device_logout_refresh", params: {}, diagnostic: normalizeError(failure).diagnostic });
        }
      }
    } catch (failure) {
      if (current(id, revision)) error = normalizeError(failure);
    } finally {
      if (current(id, revision)) busy = false;
    }
  }
</script>

<h2>{t("settings.linked_devices")}</h2>
<p>{t("settings.devices_hint")}</p>
{#if !account || !connected}
  <p role="status">{t("settings.devices_connect")}</p>
{:else}
  <button class="button" disabled={busy || loading} onclick={refresh}>{loading ? t("ui.refreshing") : t("ui.refresh")}</button>
  {#if devices}
    <ul>
      {#each devices as device (device.jid)}
        <li>
          <span>{t("settings.device_name", { id: String(device.device_id) })}{#if device.is_current}<small> {t("settings.this_device")}</small>{/if}</span>
          {#if device.can_unlink}
            <button class="button" disabled={busy || loading} onclick={() => { pending = device; error = ""; result = null; }}>
              {t("settings.device_logout_name", { id: String(device.device_id) })}
            </button>
          {:else if device.is_current}
            <small>{t("settings.device_logout_account")}</small>
          {:else}
            <small>{t("settings.device_logout_unavailable")}</small>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if !error}
    <p role="status">{t("settings.devices_loading")}</p>
  {/if}
  {#if pending}
    <section role="group" aria-label={t("settings.device_logout_confirm")}>
      <h3>{t("settings.device_logout_question", { id: String(pending.device_id) })}</h3>
      <p>{t("settings.device_logout_hint")}</p>
      <div class="actions">
        <button class="button" disabled={busy} onclick={unlink}>{busy ? t("settings.logging_out") : t("settings.logout_confirm")}</button>
        <button class="button" disabled={busy} onclick={() => { pending = null; }}>{t("ui.cancel")}</button>
      </div>
    </section>
  {/if}
{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result !== null}<p role="status">{t("settings.device_logged_out", { id: String(result) })}</p>{/if}

<style>
  h2 { margin-top: 0; }
  p, small { color: var(--muted); font-size: .85rem; }
  ul { list-style: none; padding: 0; }
  li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .75rem; padding: .8rem 0; border-bottom: 1px solid var(--line); }
  li > span small { margin-inline-start: .5rem; }
  section { border: 1px solid var(--line-strong); padding: 1rem; margin-top: 1rem; border-radius: 6px; }
  h3 { margin: 0; font-size: 1rem; }
  .actions { display: flex; flex-wrap: wrap; gap: .5rem; }
  [role="alert"] { color: var(--danger); overflow-wrap: anywhere; }
</style>
