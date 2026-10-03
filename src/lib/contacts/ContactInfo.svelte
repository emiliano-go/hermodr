<!-- A direct chat's contact, in the same full-window panel as group info. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import type { MemberProfile } from "$lib/utils/wire";
  import type { QuickReplyScope } from "$lib/utils/quick-replies";
  import BusinessCard from "./BusinessCard.svelte";
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
    generation = 0,
    oncontactchange,
    onclose,
  }: {
    jid: string;
    title: string;
    picture: string | null;
    aliases?: string[];
    account: string | null;
    connected: boolean;
    generation?: number;
    oncontactchange: (jid: string) => void;
    onclose: () => void;
  } = $props();

  let profile = $state<MemberProfile | null>(null);
  let dataScope = $state.raw<QuickReplyScope | null>(null);
  let refreshVersion = $state(0);
  let loading = $state(false);
  let profileError = $state<LocalizedError | string>("");
  let failed = $state(false);
  let enlarged = $state(false);
  let section = $state<"overview">("overview");
  let profileGeneration = 0;
  const nav = $derived([{ id: "overview" as const, label: t("ui.overview"), group: title }]);

  $effect(() => {
    const id = account, target = jid, online = connected, ownerGeneration = generation, version = refreshVersion, request = ++profileGeneration;
    profile = null;
    dataScope = null; profileError = ""; loading = !!id;
    failed = false;
    if (!id) return;
    dataScope = { account: id, chat: target, generation: ownerGeneration, requestKey: version };
    const current = () => id === account && target === jid && ownerGeneration === generation && request === profileGeneration;
    void (async () => {
      try {
        for (const live of online ? [false, true] : [false]) {
          const value = await invoke<MemberProfile>("user_profile", { accountId: id, jid: target, live, force: version > 0 });
          if (!current()) return;
          profile = value;
          dataScope = { account: id, chat: target, generation: ownerGeneration, requestKey: version };
        }
      } catch (error) { if (current()) { failed = true; profileError = normalizeError(error); } }
      finally { if (current()) loading = false; }
    })();
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

<Panel label={t("contact.info")} {nav} bind:section {onclose}>
  {#snippet header()}
    <div class="head">
      {@render avatar(44)}
      <span class="head-text">
        <span class="head-name"><bdi>{shown}</bdi></span>
        <span class="head-sub"><bdi>{number ?? t("contact.contact")}</bdi></span>
      </span>
    </div>
  {/snippet}

  <div class="hero">
    <button
      class="hero-picture"
      title={picture ? t("contact.view_picture") : undefined}
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

  <h3>{t("contact.about")}</h3>
  <p class="about" dir="auto">
    {#if profile}
      {profile.about ?? t("contact.profile_hidden")}
    {:else if failed}
      <span class="muted">{t("contact.profile_failed")}</span>
    {:else}
      <span class="muted">{t("ui.loading")}</span>
    {/if}
  </p>

  {#if profile?.live?.business.value || profile?.live?.business_name.value || profile?.live?.business.state === "error"}
  <BusinessCard {account} chat={jid} {generation} requestKey={refreshVersion} {dataScope}
    field={profile?.live?.business ?? null} {loading} error={profileError} {connected}
    onrefresh={(scope) => {
      if (scope.account === account && scope.chat === jid && scope.generation === generation && scope.requestKey === refreshVersion) ++refreshVersion;
    }} />
  {/if}

  {#if aliases.length > 0}
    <h3>{t("contact.aliases")}</h3>
    <p class="about">{aliases.map((a) => `@${a}`).join("  ")}</p>
  {/if}

  <h3>{t("contact.saved_contact")}</h3>
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
    font-size: 0.7812rem;
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
    font-size: 0.75rem;
  }
  h3 {
    margin: 12px 0 6px;
    font-size: 0.75rem;
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
