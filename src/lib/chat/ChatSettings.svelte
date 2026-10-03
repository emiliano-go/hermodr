<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t, formatNumber } from "$lib/i18n/localizer";
  import type { ChatRetention, RetentionLimit } from "$lib/utils/models";
  import { limitKey, parseLimit } from "$lib/utils/retention";
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import ChatWallpaper from "$lib/chat/ChatWallpaper.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Button from "$lib/ui/Button.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import TranscriptionOverride from "$lib/settings/TranscriptionOverride.svelte";
  import AutoDownloadOverride from "$lib/settings/AutoDownloadOverride.svelte";
  import NotificationSoundOverride from "$lib/settings/NotificationSoundOverride.svelte";
  import { session } from "$lib/state/session.svelte";

  let {
    chat,
    title,
    picture = null,
    onchange,
    onclearchat,
    ondeletechat,
    onclose,
  }: {
    chat: string;
    title: string;
    /** The chat's cached picture, when there is one. */
    picture?: string | null;
    /** The chat's retention after a save, so the page can follow it. */
    onchange: (retention: ChatRetention) => void;
    onclearchat: () => void;
    ondeletechat: () => void;
    onclose: () => void;
  } = $props();

  const WINDOWS: [string, string][] = [
    ["inherit", "ui.default"],
    ["24", "chat.retention_day"],
    ["168", "chat.mute_week"],
    ["720", "chat.retention_month"],
    ["8760", "chat.retention_year"],
    ["unlimited", "chat.retention_forever"],
  ];
  const CAPS: [string, string][] = [
    ["inherit", "ui.default"],
    ["200", "200"],
    ["1000", "1,000"],
    ["5000", "5,000"],
    ["unlimited", "chat.retention_unlimited"],
  ];

  let loaded = $state(false);
  let failed = $state<LocalizedError | string | null>(null);
  let busy = $state(false);
  let retention = $state<ChatRetention>({ max_age_hours: { kind: "inherit" }, max_messages: { kind: "inherit" }, on_demand: true });
  let unarchive = $state<boolean | null>(null);
  let muteAtAll = $state(false);
  let muteBusy = $state(false);
  let initial = "";

  const snapshot = $derived(JSON.stringify([retention, unarchive]));
  const dirty = $derived(loaded && snapshot !== initial);

  onMount(async () => {
    try {
      const got = await invoke<import("$lib/utils/wire").ChatSettings>("chat_settings", { chat });
      retention = got.retention;
      unarchive = got.unarchive;
      muteAtAll = got.mute_at_all ?? false;
      initial = JSON.stringify([got.retention, got.unarchive]);
      loaded = true;
    } catch (e) {
      failed = normalizeError(e);
    }
  });

  async function toggleMuteAtAll() {
    const target = !muteAtAll;
    muteBusy = true;
    try {
      await invoke("set_chat_mute_at_all", { chat, muted: target });
      muteAtAll = target;
    } catch (e) {
      failed = normalizeError(e);
    } finally {
      muteBusy = false;
    }
  }

  async function save() {
    const accountId = session.activeAccount;
    if (!accountId) return;
    busy = true;
    failed = null;
    try {
      await invoke("set_chat_retention", { chat, retention });
      if (accountId !== session.activeAccount) throw normalizeError({ kind: "postal_error", code: "error.chat_settings_account", params: {} });
      await invoke("set_chat_unarchive", { accountId, chat, enabled: unarchive });
      onchange(retention);
      onclose();
    } catch (e) {
      failed = normalizeError(e);
    } finally {
      busy = false;
    }
  }

  function initials(label: string) {
    const words = label.replace(/[^\p{L}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
    return words.length === 0 ? "#" : (words.length === 1 ? words[0].slice(0, 2) : words[0][0] + words[1][0]).toUpperCase();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

{#snippet choices(options: [string, string][], value: RetentionLimit, set: (v: RetentionLimit) => void, label: string)}
  <div class="choices" role="radiogroup" aria-label={label}>
    {#each options as [option, text] (text)}
      <button
        class="choice"
        class:on={limitKey(value) === option}
        role="radio"
        aria-checked={limitKey(value) === option}
        onclick={() => set(parseLimit(option))}>{Number.isFinite(Number(option)) && options === CAPS ? formatNumber(Number(option)) : t(text)}</button>
    {/each}
  </div>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={t("chat.settings")}
    transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      {#if picture}
        <img class="avatar" src={convertFileSrc(picture)} alt="" />
      {:else}
        <span class="avatar">{initials(title)}</span>
      {/if}
      <div class="heading">
        <h2><bdi>{title}</bdi></h2>
        <span class="sub">{t("chat.settings_local")}</span>
      </div>
      <button class="close" aria-label={t("ui.close")} onclick={onclose}><Icon name="x" size={18} /></button>
    </header>

    <div class="body">
      {#if !loaded && !failed}
        <p class="muted">{t("ui.loading")}</p>
      {:else if loaded}
        <section>
          <h3><Icon name="clock" size={14} /> {t("chat.message_history")}</h3>
          <div class="field">
            <span class="name">{t("chat.retention_age")}</span>
            <span class="desc">{t("chat.retention_age_hint")}</span>
            {@render choices(WINDOWS, retention.max_age_hours, (v) => (retention.max_age_hours = v), t("chat.retention_age"))}
          </div>
          <div class="field">
            <span class="name">{t("chat.retention_count")}</span>
              <span class="desc">{t("chat.retention_count_hint")}</span>
            {@render choices(CAPS, retention.max_messages, (v) => (retention.max_messages = v), t("chat.retention_count"))}
          </div>
          <label class="toggle-row">
            <span>
              <span class="name">{t("chat.history_on_demand")}</span>
              <span class="desc">{t("chat.history_on_demand_hint")}</span>
            </span>
            <input class="toggle" type="checkbox" bind:checked={retention.on_demand} />
          </label>
        </section>

        <section>
          <h3><Icon name="settings" size={14} /> {t("chat.behavior")}</h3>
          <label class="toggle-row">
            <span class="name">{t("chat.unarchive_incoming")}</span>
            <select value={unarchive === null ? "" : String(unarchive)}
              onchange={(event) => unarchive = event.currentTarget.value === "" ? null : event.currentTarget.value === "true"}>
              <option value="">{t("chat.unarchive_global", { state: t(session.settings.keep_archived ? "ui.off" : "ui.on") })}</option>
              <option value="true">{t("ui.on")}</option>
              <option value="false">{t("ui.off")}</option>
            </select>
          </label>
          <label class="toggle-row">
            <span>
              <span class="name">{t("chat.all_mute")}</span>
              <span class="desc">{t("chat.all_mute_hint")}</span>
            </span>
            <input class="toggle" type="checkbox" checked={muteAtAll} disabled={muteBusy} onchange={toggleMuteAtAll} />
          </label>
          {#if session.activeAccount}
            <NotificationSoundOverride accountId={session.activeAccount} {chat} />
          {/if}
        </section>

        <section>
          <h3><Icon name="download" size={14} /> {t("chat.media")}</h3>
          {#if session.activeAccount}
            <div class="override"><AutoDownloadOverride accountId={session.activeAccount} {chat} /></div>
            <div class="override"><TranscriptionOverride accountId={session.activeAccount} {chat} /></div>
          {/if}
        </section>

        <section>
          <h3><Icon name="image" size={14} /> {t("chat.background")}</h3>
          <ChatWallpaper account={session.activeAccount ?? ""} {chat} />
        </section>

        <section>
          <h3><Icon name="trash" size={14} /> {t("chat.danger_zone")}</h3>
          <div class="danger-row">
            <span class="grow">
              <span class="name">{t("chat.clear")}</span>
              <span class="desc">{t("chat.clear_hint")}</span>
            </span>
            <button class="choice danger" onclick={onclearchat}>{t("chat.clear_more")}</button>
          </div>
          <div class="danger-row">
            <span class="grow">
              <span class="name">{t("chat.delete")}</span>
              <span class="desc">{t("chat.delete_hint")}</span>
            </span>
            <button class="choice danger" onclick={ondeletechat}>{t("ui.delete_more")}</button>
          </div>
        </section>
      {/if}
      {#if failed}<p class="error">{failed}</p>{/if}
    </div>

    <footer>
      <Button variant="ghost" onclick={onclose}>{t("ui.cancel")}</Button>
      <Button variant="primary" disabled={!dirty || busy} onclick={save}>{busy ? t("ui.saving") : t("ui.save")}</Button>
    </footer>
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
    width: min(520px, 92vw);
    max-height: 88vh;
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
    gap: 12px;
    padding: 16px 14px 16px 20px;
    border-bottom: 1px solid var(--line);
  }
  .avatar {
    display: grid;
    place-items: center;
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--raised-2);
    font-size: 14px;
    font-weight: 600;
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  h2 {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
    font-weight: 600;
  }
  .sub,
  .muted,
  .desc {
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.4;
  }
  .close {
    display: grid;
    place-items: center;
    flex: none;
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
  .body {
    overflow-y: auto;
    padding: 6px 20px 8px;
  }
  section + section {
    border-top: 1px solid var(--line);
  }
  section {
    padding: 12px 0 6px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 10px;
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 14px;
  }
  .name {
    font-size: 14px;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
  .choice {
    padding: 5px 12px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    transition:
      background calc(0.15s * var(--motion-scale)) var(--ease),
      border-color calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .choice:hover {
    background: var(--raised);
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 4px 0 12px;
    cursor: pointer;
  }
  .toggle-row > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .toggle {
    appearance: none;
    position: relative;
    flex: none;
    width: 38px;
    height: 22px;
    margin: 0;
    border-radius: 999px;
    background: var(--line-strong);
    cursor: pointer;
    transition: background calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .toggle::after {
    content: "";
    position: absolute;
    top: 3px;
    inset-inline-start: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text);
    transition: transform calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .toggle:checked {
    background: var(--accent);
  }
  .toggle:checked::after {
    transform: translateX(16px);
    background: var(--accent-ink);
  }
  .toggle:focus-visible,
  .choice:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  /* Space between the stacked per-chat overrides (auto-download, transcription). */
  .override + .override {
    margin-top: 12px;
  }
  .danger-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-bottom: 12px;
  }
  .choice.danger {
    border-color: color-mix(in srgb, var(--danger) 50%, transparent);
    color: var(--danger);
  }
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .error {
    margin: 4px 0;
    color: var(--danger);
    font-size: 13px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
    background: var(--surface);
  }
  /* Footer actions live in $lib/ui/Button.svelte (ghost/primary variants). */
  :global([dir="rtl"]) .toggle:checked::after { transform: translateX(-16px); }
</style>
