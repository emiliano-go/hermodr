<script lang="ts" module>
  import { formatNumber as localeNumber } from "$lib/i18n/localizer";
  import { formatDate as localeDate, formatTime as localeTime } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import type { MessageReceipt as Receipt } from "$lib/utils/wire";
  export type { Receipt };
</script>

<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";

  let {
    id,
    sentAt,
    preview,
    voice,
    group,
    /** Members besides us, to tell how many have not received it yet. */
    audience,
    version,
    namer,
    picture,
    onclose,
  }: {
    id: string;
    sentAt: number;
    preview: string;
    voice: boolean;
    group: boolean;
    audience: number;
    /** Bumped when a receipt for this message arrives, to reload. */
    version: number;
    namer: (name: string | null, jid: string) => string;
    picture: (jid: string) => string | null;
    onclose: () => void;
  } = $props();

  let receipts = $state<Receipt[] | null>(null);
  let failed = $state<LocalizedError | string | null>(null);

  $effect(() => {
    void version;
    invoke<Receipt[]>("message_info", { id })
      .then((r) => (receipts = r))
      .catch((e) => (failed = normalizeError(e)));
  });

  const played = $derived((receipts ?? []).filter((r) => r.played_at));
  const read = $derived((receipts ?? []).filter((r) => r.read_at && !(voice && r.played_at)));
  const delivered = $derived((receipts ?? []).filter((r) => r.delivered_at && !r.read_at));
  const waiting = $derived(Math.max(0, audience - (receipts?.length ?? 0)));
  // A direct chat has one recipient, so its info is a timeline.
  const single = $derived(receipts?.[0] ?? null);

  function when(ts: number | null) {
    if (!ts) return "—";
    const date = new Date(ts * 1000);
    const time = localeTime((date).getTime() / 1000, { hour: "2-digit", minute: "2-digit" });
    return new Date().toDateString() === date.toDateString()
      ? t("content.today_at_value", { param0: (time) })
      : t("content.value_at_value", { param0: (localeDate((date).getTime() / 1000, { dateStyle: "short" })), param1: (time) });
  }
</script>

{#snippet person(r: Receipt, at: number | null)}
  {@const path = picture(r.recipient)}
  {@const name = namer(r.name, r.recipient)}
  <li class="person">
    {#if path}
      <img class="avatar" src={convertFileSrc(path)} alt="" />
    {:else}
      <span class="avatar blank"><Icon name="user" size={16} /></span>
    {/if}
    <span class="who"><bdi dir="auto">{name}</bdi></span>
    <span class="at">{when(at)}</span>
  </li>
{/snippet}

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t("content.message_info")} transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      <h2>{t("content.message_info")}</h2>
      <button class="close" aria-label={t("content.close")} onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    <div class="quoted">
      <span class="quoted-text">{preview}</span>
      <span class="at">{t("content.sent")} {when(sentAt).replace(/^T/, "t")}</span>
    </div>

    {#if failed}
      <p class="muted">{failed}</p>
    {:else if receipts === null}
      <p class="muted">{t("content.loading")}</p>
    {:else if !group}
      <ul class="timeline">
        <li><span class="mark sent"><Icon name="check" size={16} /></span>{t("content.sent")}<span class="at">{when(sentAt)}</span></li>
        <li><span class="mark"><Icon name="checks" size={16} /></span>{t("content.delivered")}<span class="at">{when(single?.delivered_at ?? null)}</span></li>
        <li><span class="mark read"><Icon name="checks" size={16} /></span>{t("content.read")}<span class="at">{when(single?.read_at ?? null)}</span></li>
        {#if voice}
          <li><span class="mark read"><Icon name="mic" size={13} /></span>{t("content.played")}<span class="at">{when(single?.played_at ?? null)}</span></li>
        {/if}
      </ul>
    {:else}
      <div class="lists">
        {#if voice}
          <section>
            <h3><Icon name="mic" size={14} /> {t("content.played_by")} <span class="count">{localeNumber(played.length)}</span></h3>
            <ul>{#each played as r (r.recipient)}{@render person(r, r.played_at)}{/each}</ul>
          </section>
        {/if}
        <section>
          <h3><span class="mark read"><Icon name="checks" size={16} /></span> {t("content.read_by")} <span class="count">{localeNumber(read.length)}</span></h3>
          <ul>
            {#each read as r (r.recipient)}{@render person(r, r.read_at)}{/each}
            {#if read.length === 0}<li class="empty">{t("content.no_one_yet")}</li>{/if}
          </ul>
        </section>
        <section>
          <h3><span class="mark"><Icon name="checks" size={16} /></span> {t("content.delivered_to")} <span class="count">{localeNumber(delivered.length)}</span></h3>
          <ul>
            {#each delivered as r (r.recipient)}{@render person(r, r.delivered_at)}{/each}
            {#if delivered.length === 0}<li class="empty">{t("content.no_one_else")}</li>{/if}
          </ul>
        </section>
        {#if waiting > 0}
          <p class="muted">{t("content.missing_receipts", { count: waiting })}</p>
        {/if}
      </div>
    {/if}
    <p class="note">{t("content.receipts_that_arrived_while_this_computer_was_offline_or_before_this_ver")}</p>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 275;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }
  .dialog {
    width: min(460px, 92vw);
    max-height: min(680px, 86vh);
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 16px 8px 20px;
  }
  h2 {
    margin: 0;
    font-size: 1.0625rem;
    font-weight: 600;
  }
  .close {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--raised);
    color: var(--text);
  }
  .quoted {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 4px 20px 12px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--bubble-mine);
  }
  .quoted-text {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-wrap;
  }
  .lists {
    overflow-y: auto;
    padding: 0 20px;
  }
  section + section {
    margin-top: 14px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 6px;
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--muted);
  }
  .count {
    margin-inline-start: auto;
    font-weight: 400;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .person {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
  }
  .avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .avatar.blank {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--muted);
  }
  .who {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .at {
    font-size: 0.75rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .empty,
  .muted {
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .muted {
    margin: 12px 0 0;
  }
  .timeline li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 20px;
    border-top: 1px solid var(--line);
  }
  .timeline .at {
    margin-inline-start: auto;
  }
  .mark {
    display: inline-grid;
    place-items: center;
    min-width: 22px;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .mark.read {
    color: var(--link);
  }
  .note {
    margin: 12px 20px 16px;
    font-size: 0.7188rem;
    color: var(--faint);
  }
</style>
