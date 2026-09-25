<script lang="ts" module>
  export type InviteInfo = {
    jid: string;
    subject: string | null;
    description: string | null;
    size: number;
    created_at: number | null;
    approval: boolean;
    community: boolean;
    joined: boolean;
    picture: string | null;
  };

  /** Lookups by link, shared by every card so a scroll does not ask again. */
  const cache = new Map<string, Promise<InviteInfo>>();

  /** The first WhatsApp group invite link in a text, if any. */
  export function inviteLink(text: string) {
    return /https?:\/\/chat\.whatsapp\.com\/(?:invite\/)?[A-Za-z0-9]{10,}/.exec(text)?.[0] ?? null;
  }
</script>

<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";

  let {
    link,
    onopen,
  }: {
    link: string;
    /** Opens the group's chat once joined. */
    onopen: (jid: string) => void;
  } = $props();

  let info = $state<InviteInfo | null>(null);
  let failed = $state<string | null>(null);
  let busy = $state(false);
  let requested = $state(false);

  $effect(() => {
    if (!cache.has(link)) {
      const lookup = invoke<InviteInfo>("invite_info", { link });
      // A failed lookup is not remembered, so it can be tried again later.
      lookup.catch(() => cache.delete(link));
      cache.set(link, lookup);
    }
    cache
      .get(link)!
      .then((i) => (info = i))
      .catch((e) => (failed = String(e)));
  });

  async function join() {
    if (!info) return;
    if (info.joined) return onopen(info.jid);
    busy = true;
    try {
      const joined = await invoke<{ jid: string; pending: boolean }>("join_invite", { link });
      if (joined.pending) requested = true;
      else {
        cache.delete(link);
        onopen(joined.jid);
      }
    } catch (e) {
      failed = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="invite">
  <span class="kind"><Icon name="users" size={13} /> {info?.community ? "Community invite" : "Group invite"}</span>
  {#if info}
    <div class="head">
      {#if info.picture}
        <img class="picture" src={convertFileSrc(info.picture)} alt="" />
      {:else}
        <span class="picture blank"><Icon name="users" size={22} /></span>
      {/if}
      <div class="text">
        <span class="subject">{info.subject ?? "WhatsApp group"}</span>
        <span class="meta">
          {info.size}
          {info.size === 1 ? "member" : "members"}{#if info.created_at}
            · created {new Date(info.created_at * 1000).toLocaleDateString()}{/if}
        </span>
      </div>
    </div>
    {#if info.description}<p class="description">{info.description}</p>{/if}
    <button class="join" disabled={busy || requested} onclick={join}>
      {info.joined
        ? "Open chat"
        : requested
          ? "Request sent"
          : busy
            ? "Joining…"
            : info.approval
              ? "Request to join"
              : "Join group"}
    </button>
  {:else if failed}
    <span class="meta">This invite could not be opened: it may have been reset or expired.</span>
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
    border-left: 3px solid var(--accent);
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
