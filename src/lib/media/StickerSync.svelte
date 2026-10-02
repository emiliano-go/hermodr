<script lang="ts">
  import { untrack } from "svelte";
  import type { StickerLibrary, StickerResyncReport } from "$lib/utils/wire";
  import { stickerResyncText, stickerScopeMatches, type StickerScope } from "$lib/utils/sticker-sync";

  let { account, chat = null, generation, connected, version = 0, showPacks = false, onload, onresync, onlibrary, onsynced }: {
    account: string | null; chat?: string | null; generation: number; connected: boolean; version?: number; showPacks?: boolean;
    onload: (scope: StickerScope) => Promise<StickerLibrary>;
    onresync: (scope: StickerScope) => Promise<StickerResyncReport>;
    onlibrary?: (scope: StickerScope, value: StickerLibrary) => void;
    onsynced?: (scope: StickerScope) => void;
  } = $props();

  let library = $state<StickerLibrary | null>(null);
  let loading = $state(false), busy = $state(false), error = $state("");
  let report = $state<StickerResyncReport | null>(null);
  let epoch = 0, request = 0, ownerKey = "";
  let lastVersion = untrack(() => version);

  function scope(): StickerScope | null { return account ? { account, chat, generation } : null; }
  function current(owner: StickerScope, revision: number): boolean {
    return epoch === revision && stickerScopeMatches(owner, account, chat, generation);
  }

  async function refresh() {
    const owner = scope();
    if (!owner || busy) return;
    const revision = epoch, read = ++request;
    loading = true;
    error = "";
    try {
      const value = await onload(owner);
      if (!current(owner, revision) || read !== request) return;
      library = value;
      onlibrary?.(owner, value);
    } catch (failure) {
      if (current(owner, revision) && read === request) error = `Could not refresh cached sticker library: ${failure}`;
    } finally {
      if (current(owner, revision) && read === request) loading = false;
    }
  }

  async function resync() {
    const owner = scope();
    if (!owner || !connected || busy || loading) return;
    const revision = epoch;
    busy = true;
    error = "";
    report = null;
    try {
      const value = await onresync(owner);
      if (!current(owner, revision)) return;
      report = value;
      try {
        const next = await onload(owner);
        if (!current(owner, revision)) return;
        library = next;
        onlibrary?.(owner, next);
      } catch (failure) {
        if (current(owner, revision)) error = `Could not refresh cached sticker library: ${failure}`;
      }
      if (current(owner, revision)) onsynced?.(owner);
    } catch (failure) {
      if (current(owner, revision)) error = `Sticker resync failed: ${failure}`;
    } finally {
      if (current(owner, revision)) busy = false;
    }
  }

  $effect(() => {
    const key = JSON.stringify([account, chat, generation]);
    void connected;
    ++epoch;
    if (key !== ownerKey) library = null;
    ownerKey = key;
    loading = busy = false;
    error = "";
    report = null;
    void untrack(refresh);
    return () => { ++epoch; };
  });
  $effect(() => {
    const next = version;
    if (next === lastVersion) return;
    lastVersion = next;
    void untrack(refresh);
  });
</script>

<section class="sticker-sync" aria-label="Sticker sync">
  <button disabled={!account || !connected || busy || loading} onclick={resync}>{busy ? "Resyncing stickers…" : "Resync known stickers"}</button>
  <p class="muted">Refreshes known shared packs, favorites and recents.</p>
  {#if !account || !connected}<p class="muted" role="status">Connect this account to resync. Cached data stays available.</p>{/if}
  {#if loading}<p role="status">Loading cached sticker library…</p>{/if}
  {#if busy}<p role="status">Sticker resync is pending.</p>{/if}
  {#if report}
    <p role="status">{stickerResyncText(report)}</p>
    <p class="muted">{report.mirror_verified === true ? "Phone mirror verified." : "Phone mirror unverified."} {report.catalog_complete === true ? "Full catalog reported." : "Known shared packs only; full installed catalog unverified."}</p>
    {#if report.app_state_error}<p class="error" role="alert">{report.app_state_error}</p>{/if}
    {#if report.pack_failures?.length}<ul class="error" aria-label="Pack refresh failures">{#each report.pack_failures as failed (failed.pack_id)}<li>{failed.pack_id}: {failed.error}</li>{/each}</ul>{/if}
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if showPacks && library}
    {#if library.catalog_complete !== true}<p class="muted">This cache contains known shared packs; it does not verify the phone’s full installed catalog.</p>{/if}
    {#if library.packs.length === 0}<p class="muted">No known shared packs in this account’s cache.</p>
    {:else}<ul>{#each library.packs as pack (pack.pack_id)}<li>{pack.name ?? pack.publisher ?? pack.pack_id}</li>{/each}</ul>{/if}
  {/if}
</section>

<style>
  .sticker-sync { padding: .5rem 0; overflow-wrap: anywhere; }
  p { margin: .4rem 0; font-size: .85rem; }
  .muted { color: var(--muted); }
  .error { color: var(--danger, #ef7777); white-space: pre-wrap; }
  button { color: var(--text); background: var(--raised); border: 1px solid var(--line); border-radius: 5px; padding: .4rem .6rem; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
</style>
