<script lang="ts" module>
  import type { UiSettings } from "$lib/utils/models";
  export type { DiskRetention, UiSettings } from "$lib/utils/models";
  import type { Account } from "$lib/utils/wire";
  export type { Account };
  export type Section =
    | "profile"
    | "linked"
    | "blocked"
    | "transcription"
    | "accounts"
    | "whatsapp"
    | "privacy"
    | "chats"
    | "notifications"
    | "device"
    | "media"
    | "startup"
    | "plugins"
    | "keybinds"
    | "appearance"
    | "advanced"
    | "about";
  import type { Profile } from "$lib/utils/wire";
</script>

<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import { base64Of as toBase64 } from "$lib/utils/files";
  import { once } from "$lib/state/once.svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import Icon from "$lib/ui/Icon.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Customization from "$lib/settings/Customization.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import BooleanProps from "$lib/ui/BooleanProps.svelte";
  import StorageManager from "$lib/settings/StorageManager.svelte";
  import ArchiveManager from "$lib/settings/ArchiveManager.svelte";
  import PluginManager from "$lib/settings/PluginManager.svelte";
  import LinkedDevices from "$lib/settings/LinkedDevices.svelte";
  import TranscriptionSettings from "$lib/settings/TranscriptionSettings.svelte";
  import AutoDownloadSettings from "$lib/settings/AutoDownloadSettings.svelte";
  import KeywordSettings from "$lib/settings/KeywordSettings.svelte";
  import NotificationHistory from "$lib/notifications/NotificationHistory.svelte";
  import { notificationHistory } from "$lib/notifications/history-store";
  import BlockedContacts from "$lib/settings/BlockedContacts.svelte";
  import { session } from "$lib/state/session.svelte";
  import { limitValue, parseLimit } from "$lib/utils/retention";
  import type { NotifPermission } from "$lib/utils/notifications";
  import {
    notificationPermission,
    requestNotificationPermission,
    sendTestNotification,
  } from "$lib/utils/notifications";
  import {
    ACTIONS,
    keybinds,
    bindingFromEvent,
    setBinding,
    resetBinding,
    resetBindings,
    isDefault,
    label as keyLabel,
    conflicting,
    type Action,
  } from "$lib/utils/keybinds.svelte";

  let {
    settings,
    accounts,
    active,
    me,
    meAvatar,
    accountAvatars,
    section = $bindable("accounts"),
    onclose,
    onsave,
    onflush,
    onclearhistory,
    onrename,
    onremove,
    onadd,
    onswitch,
    onprivacy,
    onpicture,
    onblockedload,
    onunblockcontact,
    onnotificationjump,
  }: {
    settings: UiSettings;
    accounts: Account[];
    active: string | null;
    me: string | null;
    meAvatar: string | null;
    accountAvatars: Record<string, string | null>;
    section?: Section;
    onclose: () => void;
    onsave: (settings: UiSettings) => Promise<void>;
    onflush: () => void;
    onclearhistory: () => void;
    onrename: (id: string, label: string) => void;
    onremove: (id: string) => void;
    onadd: () => void;
    onswitch: (id: string) => void;
    onprivacy: (privacy: Record<string, string>) => void;
    /** Our picture changed, so the cached one is stale. */
    onpicture: () => void;
    onblockedload: (account: string) => Promise<import("$lib/utils/wire").BlockedContact[]>;
    onunblockcontact: (account: string, jid: string) => Promise<void>;
    onnotificationjump?: (account: string, chat: string, id: string) => Promise<void>;
  } = $props();

  let picker: HTMLInputElement | undefined = $state();
  /** The account whose removal is waiting for confirmation. */
  let removing = $state<string | null>(null);
  let pictureBusy = $state(false);
  /** Bumped per upload: the new picture reuses the old file name. */
  let pictureVersion = $state(0);
  /** The keybind action waiting for the user to press a combination. */
  let capturing: Action | null = $state(null);
  const keyConflicts = $derived(conflicting());

  // While capturing, the next non-modifier key becomes the binding.
  $effect(() => {
    if (!capturing) return;
    const action = capturing;
    const onKey = (event: KeyboardEvent) => {
      const binding = bindingFromEvent(event);
      if (!binding) return;
      // A bare printable key would fire while typing; require a modifier for those.
      if (binding.key.length === 1 && !binding.ctrl && !binding.alt && !binding.meta) return;
      event.preventDefault();
      event.stopPropagation();
      setBinding(action, binding);
      capturing = null;
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  /** Uploads a new picture, or removes it when `file` is null. */
  async function setPicture(file: File | null) {
    pictureBusy = true;
    invalidateProfileFetch();
    profileError = null;
    try {
      const data = file ? await toBase64(file) : "";
      await invoke("set_profile_picture", { data });
      pictureVersion += 1;
      onpicture();
    } catch (e) {
      profileError = String(e);
    } finally {
      pictureBusy = false;
      if (picker) picker.value = "";
    }
  }

  // Profile and WhatsApp privacy live on the account, so they wait for pairing.
  const NAV = $derived<{ id: Section; label: string; group: string }[]>([
    ...(me ? [{ id: "profile" as Section, label: "My profile", group: "User settings" }] : []),
    ...(me ? [{ id: "linked" as Section, label: "Linked devices", group: "User settings" }] : []),
    ...(me ? [{ id: "blocked" as Section, label: "Blocked contacts", group: "User settings" }] : []),
    { id: "accounts", label: "My accounts", group: "User settings" },
    ...(me ? [{ id: "whatsapp" as Section, label: "WhatsApp privacy", group: "User settings" }] : []),
    { id: "privacy", label: "Storage & history", group: "Data & device" },
    { id: "chats", label: "Chats", group: "Messaging" },
    { id: "notifications", label: "Notifications", group: "Messaging" },
    { id: "device", label: "Device", group: "Data & device" },
    { id: "media", label: "Media", group: "Messaging" },
    { id: "transcription", label: "Transcription", group: "Messaging" },
    { id: "startup", label: "Startup", group: "App settings" },
    { id: "plugins", label: "Plugins", group: "Postal" },
    { id: "keybinds", label: "Keybinds", group: "App settings" },
    { id: "appearance", label: "Customization", group: "App settings" },
    { id: "advanced", label: "Advanced", group: "Postal" },
    { id: "about", label: "About", group: "Postal" },
  ]);
  $effect(() => {
    if (!NAV.some((n) => n.id === section)) section = "accounts";
  });

  // Edits stay local until saved, so leaving with unsaved changes is visible.
  let draft = $state<UiSettings>(untrack(() => structuredClone($state.snapshot(settings))));
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(settings));
  let saving = $state(false);

  let version = $state("");
  onMount(() => {
    getVersion().then((v) => (version = v)).catch(() => {});
  });

  $effect(() => {
    // Re-read when the section opens and after a save, which may have started it.
    void settings.android_instance;
    if (section !== "device") return;
    void once.refresh();
  });

  const activeLabel = $derived(accounts.find((a) => a.id === active)?.label ?? "Not signed in");
  const number = $derived(me ? `+${me.split("@")[0]}` : null);

  async function save() {
    saving = true;
    try {
      await onsave($state.snapshot(draft));
    } finally {
      saving = false;
    }
  }

  function reset() {
    draft = structuredClone($state.snapshot(settings));
  }

  function hoursField(value: string) {
    return value ? Math.min(0xffffffff, Math.max(1, Math.floor(Number(value)))) : null;
  }

  /** Hours per unit of the "keep messages for" field; a month counts as 30 days. */
  const AGE_UNITS: [number, string][] = [
    [1, "hours"],
    [24, "days"],
    [168, "weeks"],
    [720, "months"],
  ];
  let ageUnit = $state(
    untrack(() => {
      const hours = limitValue(settings.retention.max_age_hours);
      return [720, 168, 24].find((unit) => hours && hours % unit === 0) ?? 1;
    }),
  );
  function setAgeUnit(unit: number) {
    const hours = limitValue(draft.retention.max_age_hours);
    if (hours !== null) draft.retention.max_age_hours = parseLimit(String(Math.min(0xffffffff, Math.max(1, Math.round(hours / ageUnit)) * unit)));
    ageUnit = unit;
  }
  let clearingHistory = $state(false);
  let backfillError = $state<string | null>(null);

  // Desktop notification permission lives with the OS, not in our settings,
  // so it is shown, not edited. Delivered through the native plugin: the
  // webview auto-denies Notification requests, which made an Allow button
  // built on it a dead click. A `denied` answer is final until the user
  // re-enables Postal in the system settings, so that state offers unblock
  // steps and a recheck instead of another dead prompt.
  let notifPermission = $state<NotifPermission>("prompt");
  let notifBusy = $state(false);
  async function refreshNotifPermission() {
    notifBusy = true;
    try {
      notifPermission = await notificationPermission();
    } finally {
      notifBusy = false;
    }
  }
  async function requestNotifPermission() {
    notifBusy = true;
    try {
      notifPermission = await requestNotificationPermission();
    } finally {
      notifBusy = false;
    }
  }
  $effect(() => {
    if (section === "notifications") void refreshNotifPermission();
  });

  // The account's profile lives on WhatsApp's servers, so it is fetched when a
  // section that shows it opens and written back field by field.
  let profile = $state<Profile | null>(null);
  let profileError = $state<string | null>(null);
  let nameDraft = $state("");
  let aboutDraft = $state("");
  let profileSaved = $state(false);
  let profileLoading = $state(false);
  let profileBusy = $state(false);
  let profileSeenVersion = $state(-1);
  let profileAccount: string | null = null;
  let profileEpoch = 0;
  let profileRequest = 0;
  const profileDirty = $derived(!!profile && (nameDraft !== profile.name || aboutDraft !== (profile.about ?? "")));

  function currentProfile(account: string, epoch: number) {
    return account === session.activeAccount && epoch === profileEpoch;
  }

  function invalidateProfileFetch() {
    ++profileRequest;
    profileLoading = false;
  }

  onDestroy(() => { ++profileEpoch; ++profileRequest; });

  $effect(() => {
    const account = session.activeAccount, version = session.profileVersion;
    if (account !== profileAccount) {
      profileAccount = account;
      ++profileEpoch;
      invalidateProfileFetch();
      profile = null;
      profileBusy = profileSaved = false;
      profileError = null;
      nameDraft = aboutDraft = "";
      profileSeenVersion = -1;
    }
    if (!account || (section !== "profile" && section !== "whatsapp") || profileLoading
      || profileBusy || pictureBusy || profileError || profileDirty || profileSeenVersion === version) return;
    const epoch = profileEpoch;
    const timer = setTimeout(() => {
      const request = ++profileRequest;
      profileLoading = true;
      invoke<Profile>("profile").then((p) => {
        if (!currentProfile(account, epoch) || request !== profileRequest || version !== session.profileVersion
          || profileDirty || profileBusy || pictureBusy || profileError) return;
        profile = p;
        nameDraft = p.name;
        aboutDraft = p.about ?? "";
        profileSeenVersion = version;
      }).catch((e) => {
        if (currentProfile(account, epoch) && request === profileRequest) profileError = String(e);
      }).finally(() => {
        if (currentProfile(account, epoch) && request === profileRequest) profileLoading = false;
      });
    }, profile ? 200 : 0);
    return () => clearTimeout(timer);
  });

  async function saveProfile() {
    if (!profile || profileBusy || !session.activeAccount) return;
    const account = session.activeAccount, epoch = profileEpoch, cached = profile;
    const name = nameDraft.trim(), about = aboutDraft;
    invalidateProfileFetch();
    profileBusy = true;
    profileError = null;
    try {
      if (name && name !== cached.name) {
        await invoke("set_push_name", { name });
        if (!currentProfile(account, epoch)) return;
        cached.name = name;
        nameDraft = name;
      }
      if (about !== (cached.about ?? "")) {
        await invoke("set_about", { text: about });
        if (!currentProfile(account, epoch)) return;
        cached.about = about;
      }
      if (!currentProfile(account, epoch)) return;
      profileSaved = true;
      setTimeout(() => { if (currentProfile(account, epoch)) profileSaved = false; }, 1500);
    } catch (e) {
      if (currentProfile(account, epoch)) profileError = String(e);
    } finally {
      if (currentProfile(account, epoch)) profileBusy = false;
    }
  }

  // "My contacts except…" needs a contact picker; set it on the phone until then.
  const AUDIENCE: [string, string][] = [
    ["all", "Everyone"],
    ["contacts", "My contacts"],
    ["none", "Nobody"],
  ];
  const PRIVACY: { category: string; label: string; options: [string, string][] }[] = [
    { category: "last", label: "Last seen", options: AUDIENCE },
    { category: "online", label: "Online", options: [["all", "Everyone"], ["match_last_seen", "Same as last seen"]] },
    { category: "profile", label: "Profile photo", options: AUDIENCE },
    { category: "status", label: "About", options: AUDIENCE },
    { category: "groupadd", label: "Who can add me to groups", options: AUDIENCE.slice(0, 2) },
    { category: "calladd", label: "Who can call me", options: [["all", "Everyone"], ["known", "People I know"]] },
    { category: "readreceipts", label: "Read receipts", options: [["all", "On"], ["none", "Off"]] },
  ];

  async function setPrivacy(category: string, value: string) {
    if (!profile || profileBusy || !session.activeAccount) return;
    const account = session.activeAccount, epoch = profileEpoch, cached = profile;
    const previous = cached.privacy[category];
    invalidateProfileFetch();
    profileBusy = true;
    cached.privacy[category] = value;
    try {
      await invoke("set_privacy", { category, value });
      if (currentProfile(account, epoch)) onprivacy($state.snapshot(cached.privacy));
    } catch (e) {
      if (currentProfile(account, epoch)) {
        cached.privacy[category] = previous;
        profileError = String(e);
      }
    } finally {
      if (currentProfile(account, epoch)) profileBusy = false;
    }
  }
</script>

<Panel label="Settings" nav={NAV} bind:section {onclose}>
  {#snippet header()}
      <div class="me">
        {#if meAvatar}
          <img class="me-avatar" src="{convertFileSrc(meAvatar)}?v={pictureVersion}" alt="" />
        {:else}
          <span class="me-avatar placeholder">{activeLabel.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="me-text">
          <span class="me-name">{activeLabel}</span>
          <span class="me-sub">{number ?? "Pairing"}</span>
        </span>
      </div>
  {/snippet}

  {#snippet pageHead()}
    {#if section === "linked"}
      <h2>Linked devices</h2>
    {:else if section === "blocked"}
      <h2>Blocked contacts</h2>
    {:else if section === "transcription"}
      <h2>Transcription</h2>
    {:else if section === "profile"}
      <h2>My profile</h2>
      <p class="lede">How you appear to others on WhatsApp.</p>
    {:else if section === "whatsapp"}
      <h2>WhatsApp privacy</h2>
      <p class="lede">Account settings stored by WhatsApp, the same ones your phone shows.</p>
    {:else if section === "accounts"}
      <h2>My accounts</h2>
      <p class="lede">Every account keeps its own session, history and settings.</p>
    {:else if section === "privacy"}
      <h2>Storage & history</h2>
      <p class="lede">
        The RAM window and disk archive have separate limits. Evicting a message from RAM
        leaves it available on disk. Disk limits below delete stored messages.
      </p>
    {:else if section === "chats"}
      <h2>Chats</h2>
    {:else if section === "notifications"}
      <h2>Notifications</h2>
      <p class="lede">Desktop notifications for new direct messages and group messages.</p>
    {:else if section === "device"}
      <h2>Android companion</h2>
      <p class="lede">
        View-once photos, videos and voice notes only reach this device through a second,
        Android-style link. It is not a second inbox: it wakes when a one-time message arrives,
        fetches it into this chat, then goes dormant. It never replaces your main link and
        never unlinks anything from your phone.
      </p>
    {:else if section === "media"}
      <h2>Media</h2>
    {:else if section === "plugins"}
      <h2>Plugins</h2>
      <p class="lede">Installed plugins are native programs. Enable only plugins you trust. Restart Postal to discover newly installed plugins.</p>
    {:else if section === "startup"}
      <h2>Startup</h2>
    {:else if section === "keybinds"}
      <h2>Keybinds</h2>
      <p class="lede">Composer shortcuts. Click a shortcut, then press the keys you want.</p>
    {:else if section === "appearance"}
      <h2>Customization</h2>
      <p class="lede">Themes and CSS extensions apply instantly and are saved on this device.</p>
    {:else if section === "advanced"}
      <h2>Advanced</h2>
      <p class="lede">Developer-oriented options. These only change logging.</p>
    {:else}
      <h2>About</h2>
      <p class="lede">A native WhatsApp client that speaks the protocol directly.</p>
    {/if}
  {/snippet}

        {#if section === "blocked"}
          <BlockedContacts account={active} connected={session.connected} onload={onblockedload} onunblock={onunblockcontact} />
        {:else if section === "profile"}
          {#if profile}
            <div class="profile-card">
              <div class="picture">
                <button
                  class="picture-edit"
                  title="Change profile photo"
                  disabled={pictureBusy}
                  onclick={() => picker?.click()}>
                  {#if meAvatar}
                    <img
                      class="profile-avatar"
                      src="{convertFileSrc(meAvatar)}?v={pictureVersion}"
                      alt="" />
                  {:else}
                    <span class="profile-avatar placeholder">{activeLabel.slice(0, 1).toUpperCase()}</span>
                  {/if}
                  <span class="picture-overlay">
                    <Icon name="image" size={20} />
                    {pictureBusy ? "Uploading…" : "Change photo"}
                  </span>
                </button>
                {#if meAvatar}
                  <button class="link-button small" disabled={pictureBusy} onclick={() => setPicture(null)}>
                    Remove photo
                  </button>
                {/if}
                <input
                  class="file-input"
                  type="file"
                  accept="image/*"
                  bind:this={picker}
                  onchange={(e) => {
                    const file = e.currentTarget.files?.[0];
                    if (file) setPicture(file);
                  }} />
              </div>
              <div class="profile-fields">
                <label class="field-label">
                  Name
                  <input class="field" maxlength="25" bind:value={nameDraft} disabled={profileBusy} />
                </label>
                <label class="field-label">
                  About
                  <textarea class="field" rows="3" maxlength="139" bind:value={aboutDraft} disabled={profileBusy}></textarea>
                </label>
                {#if profile.username}
                  <div class="field-label">
                    Username
                    <span class="readonly">
                      @{profile.username}{#if profile.username_reserved}<span class="tag">Reserved</span>{/if}
                    </span>
                    <span class="setting-desc">Usernames are managed on your phone.</span>
                  </div>
                {/if}
              </div>
            </div>
            <div class="actions-row">
              <button
                class="button primary"
                disabled={profileBusy || pictureBusy || (nameDraft === profile.name && aboutDraft === (profile.about ?? ""))}
                onclick={saveProfile}>{profileBusy ? "Saving…" : profileSaved ? "Saved" : "Save profile"}</button>
            </div>
          {:else if !profileError}
            <p class="muted">Loading your profile…</p>
          {/if}
          {#if profileError}<p class="error-text">{profileError}</p>{/if}
        {:else if section === "whatsapp"}
          <label class="setting">
            <div>
              <span class="setting-title">Send typing indicator</span>
              <span class="setting-desc">
                Others see "typing…" while you write. Off, you still see theirs. The default for every
                chat; the eye beside a chat's message box overrides it there.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.send_typing} />
          </label>
          <label class="setting">
            <div>
              <span class="setting-title">Send read and played receipts</span>
              <span class="setting-desc">
                Off, nobody learns you read a message, heard a voice note or opened view-once media,
                in groups too. Unlike WhatsApp's own read receipts setting below, you keep seeing
                other people's. The default for every chat; the eye beside a chat's message box
                overrides it there.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.send_receipts} />
          </label>
          {#if profile}
            {#each PRIVACY as item (item.category)}
              {@const value = profile.privacy[item.category] ?? ""}
              <div class="setting">
                <span class="setting-title">{item.label}</span>
                <select
                  class="field"
                  {value}
                  disabled={profileBusy}
                  onchange={(e) => setPrivacy(item.category, e.currentTarget.value)}>
                  {#each item.options as [option, label] (option)}
                    <option value={option}>{label}</option>
                  {/each}
                  {#if value && !item.options.some(([o]) => o === value)}
                    <option value={value}>Custom (set on your phone)</option>
                  {/if}
                </select>
              </div>
            {/each}
          {:else if !profileError}
            <p class="muted">Loading your settings…</p>
          {/if}
          {#if profileError}<p class="error-text">{profileError}</p>{/if}
        {:else if section === "accounts"}
          <div class="card">
            {#each accounts as account (account.id)}
              <div class="account">
                {#if accountAvatars[account.id]}
                  <img class="account-avatar" src={convertFileSrc(accountAvatars[account.id]!)} alt="" />
                {:else}
                  <span class="account-avatar">{account.label.slice(0, 1).toUpperCase()}</span>
                {/if}
                <input
                  class="field"
                  value={account.label}
                  aria-label="Account name"
                  onchange={(e) => onrename(account.id, e.currentTarget.value)} />
                {#if account.id === active}
                  <span class="tag">Active</span>
                {:else}
                  <button class="button" onclick={() => onswitch(account.id)}>Switch</button>
                {/if}
                <button
                  class="remove-account"
                  title="Remove account"
                  aria-label="Remove {account.label}"
                  onclick={() => (removing = account.id)}><Icon name="trash" size={16} /></button>
              </div>
              {#if removing === account.id}
                <div class="remove-confirm" role="alert">
                  <span>
                    Remove <strong>{account.label}</strong>? Its session and history on this computer are
                    deleted; the phone keeps everything.
                  </span>
                  <button class="button" onclick={() => (removing = null)}>Cancel</button>
                  <button
                    class="button danger"
                    onclick={() => {
                      removing = null;
                      onremove(account.id);
                    }}>Remove</button>
                </div>
              {/if}
            {/each}
          </div>
          <div class="actions-row">
            <button class="button primary" onclick={onadd}><Icon name="plus" size={15} /> Add account</button>
          </div>
        {:else if section === "privacy"}
          <div class="setting">
            <div>
              <span class="setting-title">Messages in RAM</span>
              <span class="setting-desc">Maximum loaded in the open conversation: 50–2,000. Default: 500. Older pages load from disk before asking your phone.</span>
            </div>
            <input class="field number" type="number" min="50" max="2000" step="1" aria-label="Messages in RAM"
              value={draft.message_window_size} oninput={(e) => {
                if (e.currentTarget.validity.valid && e.currentTarget.value) draft.message_window_size = Number(e.currentTarget.value);
              }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">Keep history on this computer</span>
              <span class="setting-desc">
                Off keeps messages in memory only: they show while Postal runs and are gone when it
                quits. Applies the next time Postal starts; clear history below to remove what is
                already saved.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.keep_history} />
          </label>
          <div class="setting stack">
            <div>
              <span class="setting-title">History folder (cold storage)</span>
              <span class="setting-desc">
                Where the full message archive is kept. Empty uses the app data folder. Applies the
                next time Postal starts: the existing archive is moved there. Leave both disk limits
                below blank to keep everything.
              </span>
            </div>
            <input
              class="field wide"
              placeholder="App data folder"
              value={draft.history_dir ?? ""}
              oninput={(e) => (draft.history_dir = e.currentTarget.value || null)} />
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Keep messages on disk for</span>
              <span class="setting-desc">
                Older messages are deleted after a live message batch, and the space is freed.
                New installations have no disk limit. Leave blank for no limit.
              </span>
            </div>
            <span class="unit-field">
              <input
                class="field number"
                type="number"
                min="1"
                value={limitValue(draft.retention.max_age_hours) === null ? "" : limitValue(draft.retention.max_age_hours)! / ageUnit}
                aria-label="Retention duration"
                oninput={(e) => {
                  if (e.currentTarget.validity.badInput) return;
                  const amount = hoursField(e.currentTarget.value);
                  draft.retention.max_age_hours = parseLimit(amount === null ? "" : String(Math.min(0xffffffff, amount * ageUnit)));
                }} />
              <select class="field" value={ageUnit} onchange={(e) => setAgeUnit(Number(e.currentTarget.value))}>
                {#each AGE_UNITS as [unit, label] (unit)}
                  <option value={unit}>{label}</option>
                {/each}
              </select>
            </span>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Messages per chat on disk</span>
              <span class="setting-desc">
                Only the newest are kept when a disk cap is set. Leave blank to keep the archive unlimited.
              </span>
            </div>
            <input
              class="field number"
              type="number"
              min="1"
              value={limitValue(draft.retention.max_messages_per_chat) ?? ""}
              aria-label="Messages per chat"
              oninput={(e) => {
                if (!e.currentTarget.validity.badInput) draft.retention.max_messages_per_chat = parseLimit(String(hoursField(e.currentTarget.value) ?? ""));
              }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">Request full history when pairing</span>
              <span class="setting-desc">
                The next link requests up to 10,000 days of history; your phone may supply less.
                This can take time and disk space. Disk retention above still applies independently.
                Off accepts recent history only; older messages remain available on demand.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.request_full_history} />
          </label>
          <div class="setting">
            <div>
              <span class="setting-title">Download all history now</span>
              <span class="setting-desc">
                Asks your phone for every chat's older messages, page by page, for an account that is
                already linked. Keep the phone online; progress shows at the bottom.
                Disk retention still applies. Set both disk limits to unlimited to keep all fetched history.
                {backfillError ?? ""}
              </span>
            </div>
            <button
              class="button"
              onclick={() => {
                backfillError = null;
                invoke("backfill_history").catch((e) => (backfillError = String(e)));
              }}>Download</button>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Clear message history</span>
              <span class="setting-desc">
                Deletes every message stored on this computer. Names and chat settings stay; the phone
                keeps everything.
              </span>
            </div>
            {#if clearingHistory}
              <span class="unit-field">
                <button class="button" onclick={() => (clearingHistory = false)}>Cancel</button>
                <button
                  class="button danger"
                  onclick={() => {
                    clearingHistory = false;
                    onclearhistory();
                  }}>Delete all</button>
              </span>
            {:else}
              <button class="button danger" onclick={() => (clearingHistory = true)}>Clear history</button>
            {/if}
          </div>
          <ArchiveManager />
        {:else if section === "chats"}
          <h3>Keyword rules</h3>
          <KeywordSettings account={active} />
          <label class="setting">
            <div>
              <span class="setting-title">Keep chats archived</span>
              <span class="setting-desc">
                On: a new message leaves the chat in the Archived list. Off: the chat moves back to
                your main list as soon as a message arrives.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.keep_archived} />
          </label>
          <label class="setting">
            <div>
              <span class="setting-title">Pause chat reordering while hovering</span>
              <span class="setting-desc">
                On: the chat list keeps its order while the pointer is over it, so a new
                message cannot move the chat under your cursor. Previews and unread counts
                still update; the new order applies when the pointer leaves or you open a
                chat. Off: the list reorders immediately.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.freeze_chat_list_on_hover} />
          </label>
        {:else if section === "notifications"}
          <label class="setting">
            <div>
              <span class="setting-title">Enable notifications</span>
              <span class="setting-desc">
                Off silences every chat, immediately. Muted chats never notify, whether this is
                on or off; unmute one from its menu in the chat list.
              </span>
            </div>
            <input
              class="switch"
              type="checkbox"
              checked={draft.notifications_enabled}
              onchange={(e) => {
                draft.notifications_enabled = e.currentTarget.checked;
                // A kill switch must take effect at once, not sit behind Save.
                void save();
              }} />
          </label>
          <p class="lede">
            {#if settings.notifications_enabled === false}
              Notifications are off — new messages will not ping you.
            {:else if notifPermission === "granted"}
              Notifications are on for every chat except muted ones.
            {:else}
              Notifications are on here, but the system has not allowed them yet — see below.
            {/if}
          </p>
          <div class="setting">
            <div>
              <span class="setting-title">System permission</span>
              <span class="setting-desc">
                {#if notifPermission === "unsupported"}
                  This system does not support desktop notifications.
                {:else if notifPermission === "granted"}
                  Allowed. Notifications appear when a new message arrives in another chat.
                {:else if notifPermission === "denied"}
                  Blocked. Postal cannot ask again from here: re-enable it in the system
                  settings (GNOME Settings → Notifications → Postal; macOS System Settings →
                  Notifications; Windows Settings → Notifications), then press Recheck.
                {:else}
                  Not decided yet. Press Allow and the system asks once.
                {/if}
              </span>
            </div>
            {#if notifPermission === "prompt"}
              <button class="button" disabled={notifBusy} onclick={requestNotifPermission}>
                {notifBusy ? "Asking…" : "Allow"}
              </button>
            {:else if notifPermission === "denied"}
              <button class="button" disabled={notifBusy} onclick={refreshNotifPermission}>
                {notifBusy ? "Checking…" : "Recheck"}
              </button>
            {:else if notifPermission === "granted"}
              <button class="button" onclick={() => void sendTestNotification()}>Test</button>
            {/if}
          </div>
          <div class="setting">
            <div><span class="setting-title">Notification history</span><span class="setting-desc">Up to 100 recent events, stored locally for this account.</span></div>
            <button class="button" disabled={!active || $notificationHistory.account !== active
              || (!$notificationHistory.entries.length && !$notificationHistory.error && $notificationHistory.writable)}
              onclick={() => {
                const account = active;
                if (account) notificationHistory.clear(account, () => account === active && account === session.activeAccount);
              }}>Clear notification history</button>
          </div>
          <NotificationHistory account={active} connected={session.connected} onjump={async (account, chat, id) => {
            if (account !== active || account !== session.activeAccount) throw new Error("account changed");
            if (!onnotificationjump) throw new Error("Message navigation is unavailable.");
            await onnotificationjump(account, chat, id);
          }} />
        {:else if section === "device"}
          {#if !once.paired}
            <p class="lede">
              Pair the companion once before it can be enabled. The pairing link is only kept open
              while the QR is on screen.
            </p>
            {#if once.pairing}
              <div class="setting stack">
                <div>
                  <span class="setting-title">Waiting for pairing</span>
                  <span class="setting-desc">
                    In WhatsApp, open Settings → Linked devices → Link a device, then scan.
                  </span>
                </div>
                {#if once.qrSvg}
                  <div class="qr">
                    <div class="qr-code">
                      {@html once.qrSvg}
                    </div>
                  </div>
                {/if}
                <button class="button" onclick={() => once.cancelPair()}>Cancel</button>
              </div>
            {:else}
              <button class="button" onclick={() => once.pair()} disabled={!draft.keep_history}>
                Pair Android companion
              </button>
              {#if !draft.keep_history}
                <p class="muted setting-desc">
                  Turn on "Download and keep history" first: both links share one message store.
                </p>
              {/if}
            {/if}
          {:else}
            <label class="setting">
              <div>
                <span class="setting-title">Run Android companion</span>
                <span class="setting-desc">
                  Downloads and keeps one-time media the main link cannot fetch, waking up only when
                  one arrives. Needs "Download and keep history" on, since both links share one
                  message store.
                </span>
              </div>
              <input
                class="switch"
                type="checkbox"
                bind:checked={draft.android_instance}
                disabled={!draft.keep_history} />
            </label>
            <p class="muted setting-desc">
              Linked. {settings.android_instance ? "Unchecking stops it without unlinking; the link is kept for next time." : "Checking wakes it only when a one-time message arrives."}
            </p>
          {/if}
          {#if settings.android_instance || once.pairing}
            <div class="setting stack">
              <div>
                <span class="setting-title">Status</span>
                <span class="setting-desc">
                  {once.pairing && !once.paired
                    ? once.connected
                      ? "Linked, finishing up…"
                      : "Waiting for pairing."
                    : once.connected
                      ? "Fetching one-time media."
                      : once.running
                        ? "Waking up…"
                        : "Dormant — wakes when a one-time message arrives."}
                </span>
              </div>
            </div>
          {/if}
        {:else if section === "media"}
          <div class="setting stack">
            <AutoDownloadSettings value={draft.auto_download_types} onchange={(next) => {
              draft.auto_download_types = next;
              draft.auto_download_media = Object.values(next).every(Boolean);
            }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">Warn when a video goes out without a preview</span>
              <span class="setting-desc">Happens when the video cannot be decoded locally.</span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.warn_missing_video_preview} />
          </label>
          <div class="setting stack">
            <div>
              <span class="setting-title">Download folder</span>
              <span class="setting-desc">Empty uses the app cache. Applies the next time Postal starts.</span>
            </div>
            <input
              class="field wide"
              placeholder="App cache folder"
              value={draft.media_dir ?? ""}
              oninput={(e) => (draft.media_dir = e.currentTarget.value || null)} />
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Clear downloaded media</span>
              <span class="setting-desc">Deletes the files and profile pictures; messages are kept.</span>
            </div>
            <button class="button danger" onclick={onflush}>Clear media</button>
          </div>
          <StorageManager />
        {:else if section === "linked"}
          <LinkedDevices account={active} connected={session.connected} />
        {:else if section === "transcription"}
          <TranscriptionSettings autoTranscribe={settings.auto_transcribe} onAutoTranscribe={async (enabled) => {
            const next = { ...settings, auto_transcribe: enabled };
            await onsave(next);
            settings = next;
          }} />
        {:else if section === "plugins"}
          <PluginManager />
        {:else if section === "startup"}
          <label class="setting">
            <div>
              <span class="setting-title">Skip the loading screen</span>
              <span class="setting-desc">
                On shows the chat UI immediately while messages sync in the background.
                Off waits until the initial catch-up is applied.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.skip_loading_screen} />
          </label>
        {:else if section === "keybinds"}
          {#each ACTIONS as action (action.id)}
            <div class="setting">
              <div>
                <span class="setting-title">{action.label}</span>
                <span class="setting-desc">
                  {action.description}
                  {#if keyConflicts.has(action.id)}
                    <strong class="conflict">Conflicts with another shortcut</strong>
                  {/if}
                </span>
              </div>
              <div class="keybind">
                <button
                  class="button"
                  class:capturing={capturing === action.id}
                  onclick={() => (capturing = capturing === action.id ? null : action.id)}>
                  {capturing === action.id ? "Press keys…" : keyLabel(keybinds[action.id])}
                </button>
                {#if !isDefault(action.id)}
                  <Button
                    variant="ghost"
                    title="Reset to default"
                    aria-label="Reset {action.label} to default"
                    onclick={() => resetBinding(action.id)}>Reset</Button>
                {/if}
              </div>
            </div>
          {/each}
          <div class="setting">
            <div>
              <span class="setting-title">Reset all keybinds</span>
              <span class="setting-desc">Restores every shortcut to its default.</span>
            </div>
            <button class="button danger" onclick={resetBindings}>Reset all</button>
          </div>
        {:else if section === "appearance"}
          <div class="customization"><Customization /></div>
        {:else if section === "advanced"}
          <label class="setting">
            <div>
              <span class="setting-title">Verbose WhatsApp logs</span>
              <span class="setting-desc">
                Log the library's keepalive pings, transport frames and link probes, which is what
                makes a stalled connection diagnosable. Applies the next time Postal starts.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.verbose_whatsapp_logs} />
          </label>
        {:else}
          <div class="setting">
            <span class="setting-title">Postal</span>
            <span class="muted">{version ? `Version ${version}` : ""}</span>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Log file</span>
              <span class="setting-desc">
                What the app did, failed commands and crashes. Attach it when reporting a bug.
              </span>
            </div>
            <button class="button" onclick={() => invoke("open_log").catch(() => {})}>Open log</button>
          </div>
          <BooleanProps />
        {/if}

  {#snippet footer()}
      {#if dirty}
        <div class="unsaved" role="status">
          <span>You have unsaved changes.</span>
          <button class="link-button" onclick={reset}>Reset</button>
          <button class="button primary" disabled={saving} onclick={save}>Save changes</button>
        </div>
      {/if}
  {/snippet}
</Panel>

<style>
  .me {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px 12px;
  }
  .me-avatar {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .placeholder,
  .account-avatar {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--text);
    font-weight: 600;
  }
  .me-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .me-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .me-sub {
    font-size: 12.5px;
    color: var(--muted);
  }
  .account {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
  }
  .account + .account,
  .remove-confirm + .account {
    border-top: 1px solid var(--line);
  }
  .remove-account {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    transition:
      color calc(0.15s * var(--motion-scale)) var(--ease),
      background-color calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .remove-account:hover {
    color: var(--danger);
    background: var(--danger-soft);
  }
  .remove-confirm {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px 12px 60px;
    font-size: 13px;
    color: var(--muted);
  }
  .remove-confirm > span {
    flex: 1;
  }
  .account-avatar {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .profile-card {
    display: flex;
    gap: 24px;
    padding: 20px;
    background: var(--surface);
    border-radius: var(--radius);
  }
  .profile-avatar {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
    font-size: 32px;
  }
  .profile-fields {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .field-label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .picture {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .picture-edit {
    position: relative;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .picture-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    font-size: 11px;
    font-weight: 600;
    opacity: 0;
    transition: opacity calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .picture-edit:hover .picture-overlay,
  .picture-edit:focus-visible .picture-overlay,
  .picture-edit:disabled .picture-overlay {
    opacity: 1;
  }
  .link-button.small {
    font-size: 12.5px;
    color: var(--muted);
  }
  .file-input {
    display: none;
  }
  .readonly {
    font-size: 14px;
    font-weight: 400;
    color: var(--text);
  }
  .field-label .setting-desc {
    font-weight: 400;
  }
  .field-label .field {
    background: var(--bg);
    resize: vertical;
  }
  select.field {
    min-width: 200px;
    cursor: pointer;
  }
  .account .field {
    flex: 1;
  }
  .field.number {
    width: 90px;
  }
  .unit-field {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 13px;
  }
  .unit-field select.field {
    min-width: 0;
  }
  .customization {
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  .unsaved {
    position: absolute;
    left: 24px;
    right: 24px;
    bottom: 20px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px 10px 16px;
    background: var(--chat-bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .unsaved span {
    flex: 1;
  }
  .link-button {
    background: transparent;
    border: 0;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }
  .link-button:hover {
    text-decoration: underline;
  }
  .qr {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .qr-code {
    background: var(--bg);
    padding: 12px;
    border-radius: 12px;
    line-height: 0;
  }
  .qr-code :global(svg) {
    width: 220px;
    height: 220px;
  }
  .keybind {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .keybind .capturing {
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .conflict {
    display: block;
    color: var(--danger);
    font-weight: 600;
  }
</style>
