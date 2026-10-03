<script lang="ts" module>
  import type { LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
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
    onaddalias?: (alias: string) => Promise<LocalizedError | string | null>;
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
  let aliasError = $state<LocalizedError | string | null>(null);
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
  aria-label={t("contact.profile_of", { name: shown })}
  style="left: {position.left}px; top: {position.top}px; --hue: {hue}"
  transition:fly={{ y: 6, duration: motion(140) }}>
  <div class="banner"></div>
  <div class="avatar-wrap">
    {#if picture}
      <button class="zoom" title={t("contact.view_picture")} aria-label={t("contact.view_picture")} onclick={() => (enlarged = true)}
        ><img class="avatar" src={convertFileSrc(picture)} alt="" /></button
      >
    {:else}
      <span class="avatar blank">{#if letters}{letters}{:else}<Icon name="user" size={34} />{/if}</span>
    {/if}
  </div>
  <div class="body">
    <h2>{shown}{#if self}<span class="you">{t("chat.you")}</span>{/if}</h2>
    <div class="handles">
      {#if profile?.username}<span>@{profile.username}</span>{/if}
      {#if number}<span><bdi>{number}</bdi></span>{/if}
    </div>
    {#if profile?.business}
      <span class="badge"><Icon name="check" size={12} /> {profile.business}</span>
    {/if}
    {#if tag}<span class="tag">{tag}</span>{/if}

    <div class="section">
      <h3>{t("contact.about")}</h3>
      {#if profile}
        <p dir="auto">{profile.about ?? t("contact.profile_hidden")}</p>
      {:else if failed}
        <p class="muted">{t("contact.profile_failed")}</p>
      {:else}
        <p class="muted">{t("ui.loading")}</p>
      {/if}
    </div>

    {#if canAlias}
      <div class="section">
        <h3>{t("contact.aliases")}</h3>
        <p class="alias-note">
          {t("contact.alias_hint")}
        </p>
        {#if aliases.length > 0}
          <ul class="alias-list">
            {#each aliases as alias (alias)}
              <li class="alias">
                <span>@{alias}</span>
                <button
                  class="alias-drop"
                  title={t("contact.alias_remove", { alias })}
                  aria-label={t("contact.alias_remove", { alias })}
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
            placeholder={t("contact.alias_add_placeholder")}
            value={aliasDraft}
            oninput={(e) => (aliasDraft = e.currentTarget.value)}
            onkeydown={(e) => e.key === "Enter" && addAlias()} />
          <button
            class="alias-add"
            title={t("contact.alias_add")}
            aria-label={t("contact.alias_add")}
            disabled={aliasBusy !== null || !aliasValue}
            onclick={addAlias}><Icon name="plus" size={16} /></button>
        </div>
        {#if aliasError}<p class="error-text">{aliasError}</p>{#if typeof aliasError !== "string" && aliasError.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{aliasError.diagnostic}</pre></details>{/if}{/if}
      </div>
    {/if}

    {#if !self}
      <!-- Direct chats are keyed by phone number, so prefer it over a LID. -->
      <button
        class="message"
        onclick={() => onmessage(profile?.number ? `${profile.number}@s.whatsapp.net` : jid)}>
        <Icon name="message" size={16} /> {t("chat.message")}
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
    font-size: 1.75rem;
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
    font-size: 1.25rem;
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
    font-size: 0.6875rem;
    font-weight: 600;
  }
  .handles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .badge,
  .tag {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 0.75rem;
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
    font-size: 0.6875rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  p {
    margin: 0;
    font-size: 0.8438rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
  }
  .alias-note {
    color: var(--muted);
    font-size: 0.7812rem;
    line-height: 1.4;
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
    font-size: 0.75rem;
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
    padding-inline-end: 44px;
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    background: var(--surface);
    color: inherit;
    font: inherit;
    font-size: 0.875rem;
  }
  .field:focus {
    outline: none;
    border-color: var(--accent);
  }
  .alias-add {
    position: absolute;
    top: 50%;
    inset-inline-end: 6px;
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
    font-size: 0.8125rem;
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
  details pre { max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
