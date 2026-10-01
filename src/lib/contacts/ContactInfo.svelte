<!-- A direct chat's contact, in the same full-window panel as group info. -->
<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import type { UserProfile } from "$lib/contacts/ProfileCard.svelte";
  import { phoneLabel } from "$lib/utils/phone";
  import { members } from "$lib/state/members.svelte";
  import { bare } from "$lib/utils/message";
  import ContactEditor from "./ContactEditor.svelte";

  let {
    jid,
    title,
    picture,
    aliases = [],
    account,
    connected,
    oncontactchange,
    onclose,
  }: {
    jid: string;
    title: string;
    picture: string | null;
    aliases?: string[];
    account: string | null;
    connected: boolean;
    oncontactchange: (jid: string) => void;
    onclose: () => void;
  } = $props();

  let profile = $state<UserProfile | null>(null);
  let failed = $state(false);
  let enlarged = $state(false);
  let section = $state<"overview">("overview");
  let profileGeneration = 0;
  const nav = $derived([{ id: "overview" as const, label: "Overview", group: title }]);

  $effect(() => {
    const id = account, target = jid, online = connected, generation = ++profileGeneration;
    profile = null;
    failed = false;
    if (!id || !online) return;
    invoke<UserProfile>("user_profile", { jid: target })
      .then((p) => { if (id === account && generation === profileGeneration) profile = p; })
      .catch(() => { if (id === account && generation === profileGeneration) failed = true; });
    return () => { ++profileGeneration; };
  });

  const shown = $derived(members.displayName(profile?.name ?? profile?.business ?? title, jid));
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

{#snippet avatar(size: number)}
  {#if picture}
    <img class="avatar" style="--size: {size}px" src={convertFileSrc(picture)} alt="" />
  {:else}
    <span class="avatar placeholder" style="--size: {size}px; --hue: {hue}"
      >{#if letters}{letters}{:else}<Icon name="user" size={Math.round(size * 0.5)} />{/if}</span
    >
  {/if}
{/snippet}

<Panel label="Contact info" {nav} bind:section {onclose}>
  {#snippet header()}
    <div class="head">
      {@render avatar(44)}
      <span class="head-text">
        <span class="head-name">{shown}</span>
        <span class="head-sub">{number ?? "Contact"}</span>
      </span>
    </div>
  {/snippet}

  <div class="hero">
    <button
      class="hero-picture"
      title={picture ? "View picture" : undefined}
      disabled={!picture}
      onclick={() => (enlarged = true)}>{@render avatar(96)}</button>
    <div>
      <h2>{shown}</h2>
      <span class="muted">
        {[number, profile?.username ? `@${profile.username}` : null].filter(Boolean).join(" · ")}
      </span>
      {#if profile?.business}
        <span class="badge"><Icon name="check" size={12} /> {profile.business}</span>
      {/if}
    </div>
  </div>

  <h3>About</h3>
  <p class="about">
    {#if profile}
      {profile.about ?? "Nothing shared, or hidden by their privacy settings."}
    {:else if failed}
      <span class="muted">Could not reach WhatsApp for their profile.</span>
    {:else}
      <span class="muted">Loading…</span>
    {/if}
  </p>

  {#if aliases.length > 0}
    <h3>Aliases</h3>
    <p class="about">{aliases.map((a) => `@${a}`).join("  ")}</p>
  {/if}

  <h3>Saved contact</h3>
  <ContactEditor {account} {connected} jid={bare(jid)} identity={members.identities[jid] ?? members.identities[bare(jid)] ?? null}
    onsaved={oncontactchange} />
</Panel>

{#if enlarged && picture}
  <Lightbox {jid} preview={picture} alt={shown} onclose={() => (enlarged = false)} />
{/if}

<style>
  .avatar {
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .placeholder {
    display: grid;
    place-items: center;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: calc(var(--size) * 0.36);
    font-weight: 600;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px 12px;
  }
  .head-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .head-sub {
    font-size: 12.5px;
    color: var(--muted);
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 16px;
  }
  .hero h2 {
    margin: 0 0 4px;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: 6px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 12px;
  }
  h3 {
    margin: 12px 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .about {
    margin: 0 0 8px;
    padding: 14px 16px;
    background: var(--surface);
    border-radius: var(--radius);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.5;
  }
  .hero-picture {
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: zoom-in;
    transition: filter calc(0.15s * var(--motion-scale));
  }
  .hero-picture:disabled {
    cursor: default;
  }
  .hero-picture:not(:disabled):hover {
    filter: brightness(1.12);
  }
</style>
