<script lang="ts">
  import type { MemberProfileLive } from "$lib/utils/wire";
  import { memberBusinessHours, memberFieldText } from "$lib/utils/member-sheet";
  import { quickReplyScopeMatches, type QuickReplyScope } from "$lib/utils/quick-replies";

  let { account, chat, generation, requestKey, dataScope, field: incomingField = null, loading = false, error = null,
    connected = false, onrefresh }: {
    account: string | null; chat: string; generation: number; requestKey: string | number; dataScope: QuickReplyScope | null;
    field?: MemberProfileLive["business"] | null; loading?: boolean; error?: string | null; connected?: boolean;
    onrefresh?: (scope: QuickReplyScope) => void;
  } = $props();

  const ready = $derived(quickReplyScopeMatches(dataScope, account, chat, generation, requestKey));
  const field = $derived(ready ? incomingField : null);
  const business = $derived(field?.value && (field.state === "available" || field.state === "error" && field.stale) ? field.value : null);

  function refresh() {
    if (!account || !chat || !connected || loading || !onrefresh) return;
    onrefresh({ account, chat, generation, requestKey });
  }
</script>

<section aria-label="Business information">
  <header><h3>Business information</h3>
    {#if onrefresh}<button type="button" disabled={!account || !chat || !connected || loading} onclick={refresh}>Refresh</button>{/if}
  </header>
  {#if ready && loading}<p class="muted" role="status">Loading business information…</p>{/if}
  {#if ready && error}<p class="error" role="alert">{error}</p>{/if}
  {#if !connected && ready}<p class="muted">Offline. Stored business information may be outdated.</p>{/if}
  <p class:error={field?.state === "error"} class="summary" role={field?.state === "error" ? "alert" : "status"}>
    {memberFieldText(field, () => field?.value?.name || "Available")}
  </p>
  {#if business}
    <dl>
      {#if business.description}<dt>Description</dt><dd>{business.description}</dd>{/if}
      <dt>Address</dt><dd>{business.address || "Not provided"}</dd>
      <dt>Categories</dt><dd>{business.categories.join(", ") || "Not provided"}</dd>
      <dt>Hours</dt><dd>
        {#if business.hours === null}Not provided
        {:else if business.hours.length === 0}No hours provided
        {:else}<ul>{#each business.hours as hours}<li>{memberBusinessHours(hours)}</li>{/each}</ul>{/if}
      </dd>
      {#if business.timezone}<dt>Time zone</dt><dd>{business.timezone}</dd>{/if}
      {#if business.email}<dt>Email</dt><dd>{business.email}</dd>{/if}
      {#if business.websites.length}<dt>Websites</dt><dd>{business.websites.join(", ")}</dd>{/if}
    </dl>
  {/if}
</section>

<style>
  section { min-width: 0; color: var(--text); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  h3 { margin: 0; font-size: 14px; }
  button { padding: 5px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); color: var(--text); font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  p { margin: 10px 0; overflow-wrap: anywhere; font-size: 13px; }
  .muted { color: var(--muted); }
  .error { color: var(--danger); }
  dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 9px 16px; margin: 12px 0 0; font-size: 13px; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  ul { list-style: none; margin: 0; padding: 0; }
  li + li { margin-top: 5px; }
  @media (max-width: 360px) { dl { grid-template-columns: minmax(0, 1fr); gap: 4px; } dd { margin-bottom: 8px; } }
</style>
