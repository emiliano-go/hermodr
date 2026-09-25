<script lang="ts" module>
  export type UserProfile = {
    jid: string;
    name: string | null;
    number: string | null;
    username: string | null;
    about: string | null;
    business: string | null;
  };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";
  import { phoneLabel } from "$lib/phone";

  let {
    jid,
    x,
    y,
    name,
    picture,
    self = false,
    tag = null,
    onmessage,
    onclose,
  }: {
    jid: string;
    /** Where the card was opened from, in viewport pixels. */
    x: number;
    y: number;
    /** The name already shown for them, until the fresh one arrives. */
    name: string;
    picture: string | null;
    self?: boolean;
    /** Their tag in the open group. */
    tag?: string | null;
    onmessage: (jid: string) => void;
    onclose: () => void;
  } = $props();

  let profile = $state<UserProfile | null>(null);
  let failed = $state(false);
  let card: HTMLDivElement | undefined = $state();
  let position = $state({ left: 0, top: 0 });

  onMount(() => {
    invoke<UserProfile>("user_profile", { jid })
      .then((p) => (profile = p))
      .catch(() => (failed = true));
    // Opens beside the click, kept on screen.
    const width = card?.offsetWidth ?? 320;
    const height = card?.offsetHeight ?? 360;
    position = {
      left: Math.max(12, Math.min(x + 8, window.innerWidth - width - 12)),
      top: Math.max(12, Math.min(y - 40, window.innerHeight - height - 12)),
    };
  });

  const shown = $derived(profile?.name ?? profile?.business ?? name);
  const number = $derived(profile?.number ? (phoneLabel(profile.number) ?? `+${profile.number}`) : null);
  const hue = $derived([...jid].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0));
  const letters = $derived(
    shown
      .replace(/[^\p{L}\s]/gu, "")
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0].toUpperCase())
      .join(""),
  );
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="catcher" role="presentation" onclick={onclose}></div>
<div
  class="card"
  bind:this={card}
  role="dialog"
  aria-label="Profile of {shown}"
  style="left: {position.left}px; top: {position.top}px; --hue: {hue}"
  transition:fly={{ y: 6, duration: 140 }}>
  <div class="banner"></div>
  <div class="avatar-wrap">
    {#if picture}
      <img class="avatar" src={convertFileSrc(picture)} alt="" />
    {:else}
      <span class="avatar blank">{#if letters}{letters}{:else}<Icon name="user" size={34} />{/if}</span>
    {/if}
  </div>
  <div class="body">
    <h2>{shown}{#if self}<span class="you">You</span>{/if}</h2>
    <div class="handles">
      {#if profile?.username}<span>@{profile.username}</span>{/if}
      {#if number}<span>{number}</span>{/if}
    </div>
    {#if profile?.business}
      <span class="badge"><Icon name="check" size={12} /> {profile.business}</span>
    {/if}
    {#if tag}<span class="tag">{tag}</span>{/if}

    <div class="section">
      <h3>About</h3>
      {#if profile}
        <p>{profile.about ?? "Nothing shared, or hidden by their privacy settings."}</p>
      {:else if failed}
        <p class="muted">Could not reach WhatsApp for their profile.</p>
      {:else}
        <p class="muted">Loading…</p>
      {/if}
    </div>

    {#if !self}
      <!-- Direct chats are keyed by phone number, so prefer it over a LID. -->
      <button
        class="message"
        onclick={() => onmessage(profile?.number ? `${profile.number}@s.whatsapp.net` : jid)}>
        <Icon name="message" size={16} /> Message
      </button>
    {/if}
  </div>
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 290;
  }
  .card {
    position: fixed;
    z-index: 291;
    width: 320px;
    border-radius: var(--radius-lg);
    background: var(--bg);
    border: 1px solid var(--line-strong);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .banner {
    height: 64px;
    background: linear-gradient(135deg, hsl(var(--hue) 45% 32%), hsl(calc(var(--hue) + 40) 45% 22%));
  }
  .avatar-wrap {
    margin: -40px 0 0 16px;
  }
  .avatar {
    width: 80px;
    height: 80px;
    border-radius: 50%;
    object-fit: cover;
    border: 6px solid var(--bg);
    display: grid;
    place-items: center;
    box-sizing: content-box;
  }
  .avatar.blank {
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: 28px;
    font-weight: 600;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 16px 16px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    overflow-wrap: anywhere;
  }
  .you {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 600;
  }
  .handles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    color: var(--muted);
    font-size: 13px;
  }
  .badge,
  .tag {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 12px;
  }
  .badge {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .tag {
    background: var(--raised);
    color: var(--text);
  }
  .section {
    margin-top: 6px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--surface);
  }
  h3 {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  p {
    margin: 0;
    font-size: 13.5px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .message {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin-top: 8px;
    padding: 9px;
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-ink);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .message:hover {
    background: var(--accent-hover);
  }
</style>
