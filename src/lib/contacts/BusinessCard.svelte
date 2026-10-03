<script lang="ts">
  import type { LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import type { MemberProfileLive } from "$lib/utils/wire";
  import { memberBusinessHours, memberFieldText } from "$lib/utils/member-sheet";
  import { quickReplyScopeMatches, type QuickReplyScope } from "$lib/utils/quick-replies";

  let { account, chat, generation, requestKey, dataScope, field: incomingField = null, loading = false, error = null,
    connected = false, onrefresh }: {
    account: string | null; chat: string; generation: number; requestKey: string | number; dataScope: QuickReplyScope | null;
    field?: MemberProfileLive["business"] | null; loading?: boolean; error?: LocalizedError | string | null; connected?: boolean;
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

<section aria-label={t("contact.business_information")}>
  <header><h3>{t("contact.business_information")}</h3>
    {#if onrefresh}<button type="button" disabled={!account || !chat || !connected || loading} onclick={refresh}>{t("ui.refresh")}</button>{/if}
  </header>
  {#if ready && loading}<p class="muted" role="status">{t("contact.business_loading")}</p>{/if}
  {#if ready && error}<p class="error" role="alert">{error}</p>{/if}
  {#if !connected && ready}<p class="muted">{t("contact.business_offline")}</p>{/if}
  <p class:error={field?.state === "error"} class="summary" role={field?.state === "error" ? "alert" : "status"}>
    {memberFieldText(field, () => field?.value?.name || t("ui.available"))}
  </p>
  {#if business}
    <dl>
      {#if business.description}<dt>{t("contact.description")}</dt><dd dir="auto">{business.description}</dd>{/if}
      <dt>{t("contact.address")}</dt><dd dir="auto">{business.address || t("ui.not_provided")}</dd>
      <dt>{t("contact.categories")}</dt><dd dir="auto">{business.categories.join(", ") || t("ui.not_provided")}</dd>
      <dt>{t("contact.hours")}</dt><dd>
        {#if business.hours === null}{t("ui.not_provided")}
        {:else if business.hours.length === 0}{t("contact.no_hours")}
        {:else}<ul>{#each business.hours as hours}<li>{memberBusinessHours(hours)}</li>{/each}</ul>{/if}
      </dd>
      {#if business.timezone}<dt>{t("contact.time_zone")}</dt><dd dir="auto">{business.timezone}</dd>{/if}
      {#if business.email}<dt>{t("contact.email")}</dt><dd dir="auto">{business.email}</dd>{/if}
      {#if business.websites.length}<dt>{t("contact.websites")}</dt><dd dir="auto">{business.websites.join(", ")}</dd>{/if}
    </dl>
  {/if}
</section>

<style>
  section { min-width: 0; color: var(--text); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  h3 { margin: 0; font-size: 0.875rem; }
  button { padding: 5px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); color: var(--text); font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  p { margin: 10px 0; overflow-wrap: anywhere; font-size: 0.8125rem; }
  .muted { color: var(--muted); }
  .error { color: var(--danger); }
  dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 9px 16px; margin: 12px 0 0; font-size: 0.8125rem; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  ul { list-style: none; margin: 0; padding: 0; }
  li + li { margin-top: 5px; }
  @media (max-width: 360px) { dl { grid-template-columns: minmax(0, 1fr); gap: 4px; } dd { margin-bottom: 8px; } }
</style>
