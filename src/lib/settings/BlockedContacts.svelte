<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { untrack } from "svelte";
  import { displayName, phoneLabel } from "$lib/utils/phone";
  import type { BlockedContact } from "$lib/utils/wire";

  let { account, connected, onload, onunblock }: {
    account: string | null;
    connected: boolean;
    onload: (account: string) => Promise<BlockedContact[]>;
    onunblock: (account: string, jid: string) => Promise<void>;
  } = $props();

  let contacts = $state<BlockedContact[] | null>(null);
  let loading = $state(false);
  let busy = $state<string | null>(null);
  let error = $state<LocalizedError | string>("");
  let result = $state("");
  let generation = 0;

  function current(id: string, revision: number) {
    return connected && id === account && revision === generation;
  }

  async function refresh() {
    if (!account || !connected || busy || loading) return;
    const id = account, revision = generation;
    loading = true;
    error = result = "";
    try {
      const rows = await onload(id);
      if (current(id, revision)) contacts = rows;
    } catch (failure) {
      if (current(id, revision)) { contacts = null; error = normalizeError(failure); }
    } finally {
      if (current(id, revision)) loading = false;
    }
  }

  $effect(() => {
    account;
    connected;
    ++generation;
    contacts = null;
    loading = false;
    busy = null;
    error = result = "";
    void untrack(refresh);
    return () => { ++generation; };
  });

  async function unblock(contact: BlockedContact) {
    if (!account || !connected || busy || loading) return;
    const id = account, revision = generation;
    busy = contact.jid;
    error = result = "";
    try {
      await onunblock(id, contact.jid);
      if (!current(id, revision)) return;
      result = displayName(undefined, contact.jid, contact.identity);
      try {
        const rows = await onload(id);
        if (current(id, revision)) contacts = rows;
      } catch (failure) {
        if (current(id, revision)) {
          contacts = null;
          error = normalizeError({ kind: "postal_error", code: "error.unblock_refresh", params: {}, diagnostic: normalizeError(failure).diagnostic });
        }
      }
    } catch (failure) {
      if (current(id, revision)) error = normalizeError(failure);
    } finally {
      if (current(id, revision)) busy = null;
    }
  }
</script>

<p>{t("settings.blocked_hint")}</p>
{#if !account || !connected}
  <p role="status">{t("settings.blocked_connect")}</p>
{:else}
  <button class="button" disabled={!!busy || loading} onclick={refresh}>{loading ? t("ui.refreshing") : t("ui.refresh")}</button>
  {#if contacts}
    {#if contacts.length === 0}<p role="status">{t("settings.blocked_empty")}</p>{/if}
    <ul>
      {#each contacts as contact (contact.jid)}
        <li>
          <span>
            <bdi>{displayName(undefined, contact.jid, contact.identity)}</bdi>
            {#if contact.identity.number && (contact.identity.saved_name || contact.identity.legacy_name)}
              <small>{phoneLabel(contact.identity.number) ?? `+${contact.identity.number}`}</small>
            {/if}
          </span>
          <button class="button" disabled={!!busy || loading} onclick={() => unblock(contact)}
            aria-label={t("contact.unblock_name", { name: displayName(undefined, contact.jid, contact.identity) })}>
            {busy === contact.jid ? t("contact.unblocking") : t("contact.unblock")}
          </button>
        </li>
      {/each}
    </ul>
  {:else if !error}
    <p role="status">{t("settings.blocked_loading")}</p>
  {/if}
{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result}<p role="status">{t("contact.unblocked", { name: result })}</p>{/if}

<style>
  p, small { color: var(--muted); font-size: .85rem; }
  ul { list-style: none; padding: 0; }
  li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .75rem; padding: .8rem 0; border-bottom: 1px solid var(--line); }
  li > span { overflow-wrap: anywhere; }
  small { display: block; margin-top: .25rem; }
  [role="alert"] { color: var(--danger); overflow-wrap: anywhere; }
</style>
