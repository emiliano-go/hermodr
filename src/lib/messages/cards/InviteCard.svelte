<script lang="ts" module>
  import { formatDate as localeDate } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import type { InviteInfo } from "$lib/utils/wire";
  export type { InviteInfo };

  /** Lookups by link, shared by every card so a scroll does not ask again. */
  const cache = new Map<string, Promise<InviteInfo>>();

  /** The first WhatsApp group invite link in a text, if any. */
  export function inviteLink(text: string) {
    return /https?:\/\/chat\.whatsapp\.com\/(?:invite\/)?[A-Za-z0-9]{10,}/.exec(text)?.[0] ?? null;
  }
</script>

<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { session } from "$lib/state/session.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import type { Joined, StoredMessage } from "$lib/utils/wire";
  import Icon from "$lib/ui/Icon.svelte";

  let {
    link = null,
    message = null,
    onjoin,
    onopen,
  }: {
    link?: string | null;
    message?: StoredMessage | null;
    onjoin: () => Promise<Joined>;
    /** Opens the group's chat once joined. */
    onopen: (jid: string) => void;
  } = $props();

  let info = $state<InviteInfo | null>(null);
  let failed = $state<LocalizedError | string | null>(null);
  let busy = $state(false);
  let requested = $state(false);
  let attempt = $state(0);
  let generation = 0;
  onDestroy(() => { generation++; });

  $effect(() => {
    attempt;
    const account = session.activeAccount, epoch = messages.accountGeneration, target = link, row = message;
    const revision = ++generation;
    info = null; failed = null; busy = false; requested = false;
    if (row || !target || !account) return;
    const key = JSON.stringify([account, epoch, target]);
    if (!cache.has(key)) {
      const lookup = invoke<InviteInfo>("invite_info", { account, link: target });
      // A failed lookup is not remembered, so it can be tried again later.
      lookup.catch(() => { if (cache.get(key) === lookup) cache.delete(key); });
      cache.set(key, lookup);
    }
    cache
      .get(key)!
      .then((value) => { if (revision === generation && account === session.activeAccount && epoch === messages.accountGeneration) info = value; })
      .catch((error) => { if (revision === generation && account === session.activeAccount && epoch === messages.accountGeneration) failed = normalizeError(error); });
  });

  async function join() {
    if (busy || requested || (!info && !message) || message?.from_me) return;
    const account = session.activeAccount, epoch = messages.accountGeneration, revision = generation;
    if (!account) return;
    const current = () => revision === generation && account === session.activeAccount && epoch === messages.accountGeneration;
    busy = true;
    failed = null;
    try {
      if (!message && link) {
        const wasJoined = info?.joined;
        const fresh = await invoke<InviteInfo>("invite_info", { account, link });
        if (!current()) return;
        info = fresh;
        if (fresh.joined) return onopen(fresh.jid);
        if (wasJoined) return;
      }
      const joined = await onjoin();
      if (!current()) return;
      if (joined.pending) requested = true;
      else {
        cache.delete(JSON.stringify([account, epoch, link]));
        onopen(joined.jid);
      }
    } catch (e) {
      if (current()) failed = normalizeError(e);
    } finally {
      if (current()) busy = false;
    }
  }
</script>

<div class="invite">
  <span class="kind"><Icon name="users" size={13} /> {info?.community ? t("content.community_invite") : t("content.group_invite")}</span>
  {#if info || message}
    {@const picture = message?.media_thumb ?? (info?.picture ? convertFileSrc(info.picture) : null)}
    <div class="head">
      {#if picture}
        <img class="picture" src={picture} alt="" />
      {:else}
        <span class="picture blank"><Icon name="users" size={22} /></span>
      {/if}
      <div class="text">
        <span class="subject">{info?.subject ?? message?.text ?? t("content.whatsapp_group")}</span>
        <span class="meta">
          {#if info}
          {t("content.member_count", { count: info.size })}{#if info.created_at}{t("content.created")} {localeDate((new Date(info.created_at * 1000)).getTime()/1000, { dateStyle: "short" })}{/if}
          {:else}{t("content.group_invitation")}{/if}
        </span>
      </div>
    </div>
    {#if info?.description}<p class="description">{info.description}</p>{/if}
    <button class="join" disabled={busy || requested || !session.activeAccount || !!message?.from_me} onclick={join}>
      {info?.joined
        ? t("content.open_chat")
        : requested
          ? t("content.request_sent")
          : busy
            ? t("content.joining")
            : info?.approval
              ? t("content.request_to_join")
              : t("content.join_group")}
    </button>
    {#if requested}<span class="meta">{t("content.waiting_for_an_admin_to_approve")}</span>{/if}
    {#if failed}<span class="meta" role="alert">{failed}</span>{/if}
  {:else if failed}
    <span class="meta" role="alert">{failed}</span>
    <button class="join" onclick={() => attempt++}>{t("content.retry")}</button>
  {:else}
    <div class="head">
      <span class="picture blank loading"></span>
      <div class="text"><span class="bar"></span><span class="bar short"></span></div>
    </div>
  {/if}
</div>

<style>
  .invite {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
    padding: 10px 12px;
    min-width: 260px;
    max-width: 360px;
    border-radius: var(--radius);
    border-inline-start: 3px solid var(--accent);
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }
  .kind {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .picture {
    width: 48px;
    height: 48px;
    flex: none;
    border-radius: 14px;
    object-fit: cover;
  }
  .picture.blank {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--muted);
  }
  .loading {
    animation: pulse 1.2s ease-in-out infinite;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .subject {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 12.5px;
    color: var(--muted);
  }
  .description {
    margin: 0;
    font-size: 13px;
    color: var(--muted);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-wrap;
  }
  .bar {
    width: 140px;
    height: 10px;
    border-radius: 4px;
    background: var(--raised-2);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .bar.short {
    width: 80px;
  }
  @keyframes pulse {
    50% {
      opacity: 0.5;
    }
  }
  .join {
    padding: 8px;
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-ink);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .join:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .join:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
