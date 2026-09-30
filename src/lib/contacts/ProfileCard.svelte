<script lang="ts" module>
  import type { UserProfile } from "$lib/utils/wire";
  export type { UserProfile };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import { phoneLabel } from "$lib/utils/phone";
  import { members } from "$lib/state/members.svelte";

  let {
    jid,
    x,
    y,
    name,
    picture,
    self = false,
    tag = null,
    aliases = [],
    onaddalias,
    onremovealias,
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
    /** The local aliases they answer to. */
    aliases?: string[];
    /** Stores the alias, resolving to the reason it was refused, or null. */
    onaddalias?: (alias: string) => Promise<string | null>;
    onremovealias?: (alias: string) => void;
    onmessage: (jid: string) => void;
    onclose: () => void;
  } = $props();

  let profile = $state<UserProfile | null>(null);
  let failed = $state(false);
  let width = $state(320);
  let height = $state(360);
  let enlarged = $state(false);
  let aliasDraft = $state("");
  let aliasBusy = $state<string | null>(null);
  let aliasError = $state<string | null>(null);
  const aliasValue = $derived(aliasDraft.trim());
  // Our own aliases would name ourselves, which `@all` already does.
  const canAlias = $derived(!self && onaddalias !== undefined);
  let innerWidth = $state(window.innerWidth);
  let innerHeight = $state(window.innerHeight);
  // Opens beside the click and stays on screen as the loaded profile grows it.
  const position = $derived({
    left: Math.max(12, Math.min(x + 8, innerWidth - width - 12)),
    top: Math.max(12, Math.min(y - 40, innerHeight - height - 12)),
  });

  onMount(() => {
    invoke<UserProfile>("user_profile", { jid })
      .then((p) => (profile = p))
      .catch(() => (failed = true));
  });

  const shown = $derived(members.displayName(profile?.name ?? profile?.business ?? name, jid));
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

  async function addAlias() {
    if (!onaddalias || !aliasValue || aliasBusy) return;
    aliasBusy = aliasValue;
    aliasError = null;
    // The reason is shown beside the field: a banner over the whole app for a
    // mistyped alias would be out of place, and a rejected alias is a normal
    // thing to hit while typing.
    const refused = await onaddalias(aliasValue);
    aliasBusy = null;
    if (refused) aliasError = refused;
    else aliasDraft = "";
  }

  function removeAlias(alias: string) {
    if (!onremovealias || aliasBusy) return;
    onremovealias(alias);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} bind:innerWidth bind:innerHeight />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="catcher" role="presentation" onclick={onclose}></div>
<div
  class="card"
  bind:offsetWidth={width}
  bind:offsetHeight={height}
  role="dialog"
  aria-label="Profile of {shown}"
  style="left: {position.left}px; top: {position.top}px; --hue: {hue}"
  transition:fly={{ y: 6, duration: motion(140) }}>
  <div class="banner"></div>
  <div class="avatar-wrap">
    {#if picture}
      <button class="zoom" title="View picture" aria-label="View picture" onclick={() => (enlarged = true)}
        ><img class="avatar" src={convertFileSrc(picture)} alt="" /></button
      >
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

    {#if canAlias}
      <div class="section">
        <h3>Aliases</h3>
        <p class="alias-note">
          Type <span class="alias-hint">@alias</span> in a group to mention them. These stay on this device and never change how anyone is shown.
        </p>
        {#if aliases.length > 0}
          <ul class="alias-list">
            {#each aliases as alias (alias)}
              <li class="alias">
                <span>@{alias}</span>
                <button
                  class="alias-drop"
                  title="Remove @{alias}"
                  aria-label="Remove @{alias}"
                  disabled={aliasBusy !== null}
                  onclick={() => removeAlias(alias)}><Icon name="x" size={12} /></button>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="alias-row">
          <input
            class="field"
            maxlength="32"
            placeholder="Add an alias"
            value={aliasDraft}
            oninput={(e) => (aliasDraft = e.currentTarget.value)}
            onkeydown={(e) => e.key === "Enter" && addAlias()} />
          <button
            class="alias-add"
            title="Add alias"
            aria-label="Add alias"
            disabled={aliasBusy !== null || !aliasValue}
            onclick={addAlias}><Icon name="plus" size={16} /></button>
        </div>
        {#if aliasError}<p class="error-text">{aliasError}</p>{/if}
      </div>
    {/if}

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

{#if enlarged && picture}
  <Lightbox {jid} preview={picture} alt={shown} onclose={() => (enlarged = false)} />
{/if}

<style>
  .zoom {
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: zoom-in;
  }
  .zoom:hover {
    filter: brightness(1.12);
  }
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
    max-height: calc(100vh - 24px);
    overflow-x: hidden;
    overflow-y: auto;
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
  .alias-note {
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.4;
  }
  /* The token as it will be typed in the composer. */
  .alias-hint {
    color: var(--accent-text);
  }
  .alias-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .alias {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 2px 2px 9px;
    border-radius: 999px;
    background: var(--raised);
    color: var(--text);
    font-size: 12px;
  }
  .alias-drop {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .alias-drop:hover:not(:disabled) {
    background: var(--raised-2);
    color: var(--danger);
  }
  .alias-drop:disabled {
    color: var(--faint);
    cursor: default;
  }
  /* One field with its add button inside, on the right. */
  .alias-row {
    position: relative;
    display: flex;
    margin-top: 8px;
  }
  .field {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    padding-right: 44px;
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    background: var(--surface);
    color: inherit;
    font: inherit;
    font-size: 14px;
  }
  .field:focus {
    outline: none;
    border-color: var(--accent);
  }
  .alias-add {
    position: absolute;
    top: 50%;
    right: 6px;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-ink);
    cursor: pointer;
  }
  .alias-add:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .alias-add:disabled {
    background: transparent;
    color: var(--faint);
    cursor: default;
  }
  .error-text {
    margin: 8px 0 0;
    color: var(--danger);
    font-size: 13px;
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
