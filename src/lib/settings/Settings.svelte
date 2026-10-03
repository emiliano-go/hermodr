<script lang="ts" module>
  import type { UiSettings } from "$lib/utils/models";
  export type { DiskRetention, UiSettings } from "$lib/utils/models";
  import type { Account } from "$lib/utils/wire";
  export type { Account };
  export type Section =
    | "profile"
    | "linked"
    | "blocked"
    | "contacts"
    | "transcription"
    | "accounts"
    | "whatsapp"
    | "privacy"
    | "chats"
    | "spaces"
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
  import StickerSync from "$lib/media/StickerSync.svelte";
  import { stickers as stickerEvents } from "$lib/state/stickers.svelte";
  import type { StickerLibrary, StickerResyncReport } from "$lib/utils/wire";
  import KeywordSettings from "$lib/settings/KeywordSettings.svelte";
  import NotificationHistory from "$lib/notifications/NotificationHistory.svelte";
  import ContactSharing from "$lib/contacts/ContactSharing.svelte";
  import PhoneLink from "$lib/settings/PhoneLink.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { notificationHistory } from "$lib/notifications/history-store";
  import BlockedContacts from "$lib/settings/BlockedContacts.svelte";
  import AtAllMuteList from "$lib/settings/AtAllMuteList.svelte";
  import { session } from "$lib/state/session.svelte";
  import { locale } from "$lib/i18n/locale.svelte";
  import { t, localeNames, type LocalePreference } from "$lib/i18n/localizer";
  import { messageText, normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import type { DatabaseEncryptionStatus, MessageRef } from "$lib/utils/wire";
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
    onopencontact,
    onspaceexport,
    onspaceimport,
    spacesReady = false,
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
    onopencontact?: (jid: string) => Promise<void>;
    /** Spaces metadata backup; absent when Spaces are unavailable (e.g. test harnesses). */
    onspaceexport?: () => Promise<string>;
    onspaceimport?: (json: string) => Promise<void>;
    spacesReady?: boolean;
  } = $props();

  let picker: HTMLInputElement | undefined = $state();
  let encryption = $state<DatabaseEncryptionStatus | null>(null);
  let encryptionLoadError = $state<unknown>(null);
  $effect(() => {
    if (section !== "privacy") return;
    const account = active;
    let current = true;
    encryption = null;
    encryptionLoadError = null;
    void invoke<DatabaseEncryptionStatus>("database_encryption_status").then((status) => {
      if (current && account === active && status.active_account === account) encryption = status;
    }).catch((error) => { if (current && account === active) encryptionLoadError = error; });
    return () => { current = false; };
  });
  /** The account whose removal is waiting for confirmation. */
  let removing = $state<string | null>(null);
  let pictureBusy = $state(false);
  /** Bumped per upload: the new picture reuses the old file name. */
  let pictureVersion = $state(0);
  /** The keybind action waiting for the user to press a combination. */
  let capturing: Action | null = $state(null);
  const keyConflicts = $derived(conflicting());
  /** Spaces metadata backup state. */
  let spaceJson = $state("");
  let spaceImport = $state("");
  let spaceBusy = $state(false);
  let spaceError = $state<LocalizedError | null>(null);

  async function exportSpaces() {
    if (!onspaceexport || spaceBusy) return;
    spaceBusy = true;
    spaceError = null;
    try { spaceJson = await onspaceexport(); }
    catch (cause) { spaceError = normalizeError(cause); }
    finally { spaceBusy = false; }
  }

  async function importSpaces() {
    if (!onspaceimport || spaceBusy || !spaceImport.trim()) return;
    spaceBusy = true;
    spaceError = null;
    try { await onspaceimport(spaceImport); spaceImport = ""; }
    catch (cause) { spaceError = normalizeError(cause); }
    finally { spaceBusy = false; }
  }

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
      profileError = normalizeError(e);
    } finally {
      pictureBusy = false;
      if (picker) picker.value = "";
    }
  }

  // Profile and WhatsApp privacy live on the account, so they wait for pairing.
  const NAV = $derived<{ id: Section; label: string; group: string }[]>([
    ...(me ? [{ id: "profile" as Section, label: t("settings.main.profile"), group: t("settings.main.user_group") }] : []),
    ...(me ? [{ id: "linked" as Section, label: t("settings.main.linked"), group: t("settings.main.user_group") }] : []),
    ...(me ? [{ id: "blocked" as Section, label: t("settings.main.blocked"), group: t("settings.main.user_group") }] : []),
    ...(me ? [{ id: "contacts" as Section, label: t("settings.main.contacts"), group: t("settings.main.user_group") }] : []),
    { id: "accounts", label: t("settings.main.accounts"), group: t("settings.main.user_group") },
    ...(me ? [{ id: "whatsapp" as Section, label: t("settings.main.whatsapp"), group: t("settings.main.user_group") }] : []),
    { id: "privacy", label: t("settings.main.privacy"), group: t("settings.main.data_group") },
    { id: "chats", label: t("settings.main.chats"), group: t("settings.main.messaging_group") },
    { id: "spaces", label: t("settings.main.spaces"), group: t("settings.main.messaging_group") },
    { id: "notifications", label: t("settings.main.notifications"), group: t("settings.main.messaging_group") },
    { id: "device", label: t("settings.main.device"), group: t("settings.main.data_group") },
    { id: "media", label: t("settings.main.media"), group: t("settings.main.messaging_group") },
    { id: "transcription", label: t("settings.main.transcription"), group: t("settings.main.messaging_group") },
    { id: "startup", label: t("settings.main.startup"), group: t("settings.main.app_group") },
    { id: "plugins", label: t("settings.main.plugins"), group: t("settings.main.postal") },
    { id: "keybinds", label: t("settings.main.keybinds"), group: t("settings.main.app_group") },
    { id: "appearance", label: t("settings.main.appearance"), group: t("settings.main.app_group") },
    { id: "advanced", label: t("settings.main.advanced"), group: t("settings.main.postal") },
    { id: "about", label: t("settings.main.about"), group: t("settings.main.postal") },
  ]);
  $effect(() => {
    if (!NAV.some((n) => n.id === section)) section = "accounts";
  });

  // Edits stay local until saved, so leaving with unsaved changes is visible.
  let draft = $state<UiSettings>(untrack(() => structuredClone($state.snapshot(settings))));
  const dirty = $derived(JSON.stringify(draft) !== JSON.stringify(settings));
  let saving = $state(false);
  let desktopStatus = $state<import("$lib/utils/wire").DesktopStatus | null>(null);
  let desktopError = $state<LocalizedError | null>(null);
  $effect(() => {
    if (section !== "startup") return;
    void settings.start_on_login;
    let current = true;
    desktopStatus = null;
    desktopError = null;
    invoke<import("$lib/utils/wire").DesktopStatus>("get_desktop_status")
      .then((status) => { if (current) desktopStatus = status; })
      .catch((error) => { if (current) desktopError = normalizeError(error); });
    return () => { current = false; };
  });

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

  /** The companion pairing card shows the QR unless phone-number linking is chosen. */
  let oncePhoneMode = $state(false);
  $effect(() => {
    if (!once.pairing) oncePhoneMode = false;
  });

  const activeLabel = $derived(accounts.find((a) => a.id === active)?.label ?? t("settings.main.not_signed_in"));
  const number = $derived(me ? `+${me.split("@")[0]}` : null);
  function localizedMessage(message: MessageRef): string {
    return messageText(message);
  }
  function bindingLabel(action: Action): string {
    const names: Record<string, string> = { Space: "space", Enter: "enter", Tab: "tab", Backspace: "backspace", Del: "delete", Esc: "escape" };
    return keyLabel(keybinds[action]).split("+").map((key) => names[key] ? t(`settings.main.key_${names[key]}`) : key).join("+");
  }

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
    [1, "settings.main.hours"],
    [24, "settings.main.days"],
    [168, "settings.main.weeks"],
    [720, "settings.main.months"],
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
  let backfillError = $state<LocalizedError | null>(null);

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
  let profileError = $state<LocalizedError | null>(null);
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
        if (currentProfile(account, epoch) && request === profileRequest) profileError = normalizeError(e);
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
      if (currentProfile(account, epoch)) profileError = normalizeError(e);
    } finally {
      if (currentProfile(account, epoch)) profileBusy = false;
    }
  }

  // "My contacts except…" needs a contact picker; set it on the phone until then.
  const AUDIENCE: [string, string][] = [
    ["all", "settings.main.everyone"],
    ["contacts", "settings.main.my_contacts"],
    ["none", "settings.main.nobody"],
  ];
  const PRIVACY: { category: string; label: string; options: [string, string][] }[] = [
    { category: "last", label: "settings.main.last_seen", options: AUDIENCE },
    { category: "online", label: "settings.main.online", options: [["all", "settings.main.everyone"], ["match_last_seen", "settings.main.same_last_seen"]] },
    { category: "profile", label: "settings.main.profile_photo", options: AUDIENCE },
    { category: "status", label: "settings.main.profile_about", options: AUDIENCE },
    { category: "groupadd", label: "settings.main.group_add", options: AUDIENCE.slice(0, 2) },
    { category: "calladd", label: "settings.main.call_add", options: [["all", "settings.main.everyone"], ["known", "settings.main.people_known"]] },
    { category: "readreceipts", label: "settings.main.read_receipts", options: [["all", "settings.main.on"], ["none", "settings.main.off"]] },
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
        profileError = normalizeError(e);
      }
    } finally {
      if (currentProfile(account, epoch)) profileBusy = false;
    }
  }
</script>

<Panel label={t("settings.main.settings")} nav={NAV} bind:section {onclose}>
  {#snippet header()}
      <div class="me">
        {#if meAvatar}
          <img class="me-avatar" src="{convertFileSrc(meAvatar)}?v={pictureVersion}" alt="" />
        {:else}
          <span class="me-avatar placeholder">{activeLabel.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="me-text">
          <span class="me-name"><bdi dir="auto">{activeLabel}</bdi></span>
          <span class="me-sub">{#if number}<bdi dir="ltr">{number}</bdi>{:else}{t("settings.main.pairing")}{/if}</span>
        </span>
      </div>
  {/snippet}

  {#snippet pageHead()}
    {#if section === "linked"}
      <h2>{t("settings.main.linked")}</h2>
    {:else if section === "blocked"}
      <h2>{t("settings.main.blocked")}</h2>
    {:else if section === "contacts"}
      <h2>{t("settings.main.contacts")}</h2>
    {:else if section === "transcription"}
      <h2>{t("settings.main.transcription")}</h2>
    {:else if section === "profile"}
      <h2>{t("settings.main.profile")}</h2>
      <p class="lede">{t("settings.main.profile_hint")}</p>
    {:else if section === "whatsapp"}
      <h2>{t("settings.main.whatsapp")}</h2>
      <p class="lede">{t("settings.main.whatsapp_hint")}</p>
    {:else if section === "accounts"}
      <h2>{t("settings.main.accounts")}</h2>
      <p class="lede">{t("settings.main.accounts_hint")}</p>
    {:else if section === "privacy"}
      <h2>{t("settings.main.privacy")}</h2>
      <p class="lede">
        {t("settings.main.privacy_hint")}
      </p>
    {:else if section === "chats"}
      <h2>{t("settings.main.chats")}</h2>
    {:else if section === "spaces"}
      <h2>{t("settings.main.spaces")}</h2>
      <p class="lede">{t("settings.main.spaces_hint")}</p>
    {:else if section === "notifications"}
      <h2>{t("settings.main.notifications")}</h2>
      <p class="lede">{t("settings.main.notifications_hint")}</p>
    {:else if section === "device"}
      <h2>{t("settings.main.companion")}</h2>
      <p class="lede">
        {t("settings.main.companion_hint")}
      </p>
    {:else if section === "media"}
      <h2>{t("settings.main.media")}</h2>
    {:else if section === "plugins"}
      <h2>{t("settings.main.plugins")}</h2>
      <p class="lede">{t("settings.main.plugins_hint")}</p>
    {:else if section === "startup"}
      <h2>{t("settings.main.startup")}</h2>
    {:else if section === "keybinds"}
      <h2>{t("settings.main.keybinds")}</h2>
      <p class="lede">{t("settings.main.keybinds_hint")}</p>
    {:else if section === "appearance"}
      <h2>{t("settings.main.appearance")}</h2>
      <p class="lede">{t("settings.main.appearance_hint")}</p>
    {:else if section === "advanced"}
      <h2>{t("settings.main.advanced")}</h2>
      <p class="lede">{t("settings.main.advanced_hint")}</p>
    {:else}
      <h2>{t("settings.main.about")}</h2>
      <p class="lede">{t("settings.main.about_hint")}</p>
    {/if}
  {/snippet}

        {#if section === "blocked"}
          <BlockedContacts account={active} connected={session.connected} onload={onblockedload} onunblock={onunblockcontact} />
        {:else if section === "contacts"}
          <ContactSharing account={active} connected={session.connected} generation={messages.accountGeneration}
            onopenchat={async (jid) => { if (!onopencontact) throw normalizeError({ kind: "postal_error", code: "error.settings_chat_navigation_unavailable", params: {} }); await onopencontact(jid); }} />
        {:else if section === "profile"}
          {#if profile}
            <div class="profile-card">
              <div class="picture">
                <button
                  class="picture-edit"
                  title={t("settings.main.change_photo")}
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
                    {pictureBusy ? t("settings.main.uploading") : t("settings.main.change_photo_short")}
                  </span>
                </button>
                {#if meAvatar}
                  <button class="link-button small" disabled={pictureBusy} onclick={() => setPicture(null)}>
                    {t("settings.main.remove_photo")}
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
                  {t("settings.main.name")}
                  <input class="field" dir="auto" maxlength="25" bind:value={nameDraft} disabled={profileBusy} />
                </label>
                <label class="field-label">
                  {t("settings.main.profile_about")}
                  <textarea class="field" dir="auto" rows="3" maxlength="139" bind:value={aboutDraft} disabled={profileBusy}></textarea>
                </label>
                {#if profile.username}
                  <div class="field-label">
                    {t("settings.main.username")}
                    <span class="readonly">
                      <bdi dir="ltr">@{profile.username}</bdi>{#if profile.username_reserved}<span class="tag">{t("settings.main.reserved")}</span>{/if}
                    </span>
                    <span class="setting-desc">{t("settings.main.username_hint")}</span>
                  </div>
                {/if}
              </div>
            </div>
            <div class="actions-row">
              <button
                class="button primary"
                disabled={profileBusy || pictureBusy || (nameDraft === profile.name && aboutDraft === (profile.about ?? ""))}
                onclick={saveProfile}>{profileBusy ? t("settings.main.saving") : profileSaved ? t("settings.main.saved") : t("settings.main.save_profile")}</button>
            </div>
          {:else if !profileError}
            <p class="muted">{t("settings.main.profile_loading")}</p>
          {/if}
          {#if profileError}<p class="error-text"><bdi>{localizedMessage(profileError.descriptor)}</bdi></p>{/if}
        {:else if section === "whatsapp"}
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.send_typing")}</span>
              <span class="setting-desc">
                {t("settings.main.send_typing_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.send_typing} />
          </label>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.send_receipts")}</span>
              <span class="setting-desc">
                {t("settings.main.send_receipts_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.send_receipts} />
          </label>
          {#if profile}
            {#each PRIVACY as item (item.category)}
              {@const value = profile.privacy[item.category] ?? ""}
              <div class="setting">
                <span class="setting-title">{t(item.label)}</span>
                <select
                  class="field"
                  {value}
                  disabled={profileBusy}
                  onchange={(e) => setPrivacy(item.category, e.currentTarget.value)}>
                  {#each item.options as [option, label] (option)}
                    <option value={option}>{t(label)}</option>
                  {/each}
                  {#if value && !item.options.some(([o]) => o === value)}
                    <option value={value}>{t("settings.main.custom_phone")}</option>
                  {/if}
                </select>
              </div>
            {/each}
          {:else if !profileError}
            <p class="muted">{t("settings.main.settings_loading")}</p>
          {/if}
          {#if profileError}<p class="error-text"><bdi>{localizedMessage(profileError.descriptor)}</bdi></p>{/if}
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
                  dir="auto"
                  value={account.label}
                  aria-label={t("settings.main.account_name")}
                  onchange={(e) => onrename(account.id, e.currentTarget.value)} />
                {#if account.id === active}
                  <span class="tag">{t("settings.main.active")}</span>
                {:else}
                  <button class="button" onclick={() => onswitch(account.id)}>{t("settings.main.switch")}</button>
                {/if}
                <button
                  class="remove-account"
                  title={t("settings.main.remove_account")}
                  aria-label={t("settings.main.remove_named_account", { name: account.label })}
                  onclick={() => (removing = account.id)}><Icon name="trash" size={16} /></button>
              </div>
              {#if removing === account.id}
                <div class="remove-confirm" role="alert">
                  <span>
                    {t("settings.main.remove_account_prefix")}<strong><bdi dir="auto">{account.label}</bdi></strong>{t("settings.main.remove_account_suffix")}
                  </span>
                  <button class="button" onclick={() => (removing = null)}>{t("settings.main.cancel")}</button>
                  <button
                    class="button danger"
                    onclick={() => {
                      removing = null;
                      onremove(account.id);
                    }}>{t("settings.main.remove")}</button>
                </div>
              {/if}
            {/each}
          </div>
          <div class="actions-row">
            <button class="button primary" onclick={onadd}><Icon name="plus" size={15} /> {t("settings.main.add_account")}</button>
          </div>
        {:else if section === "privacy"}
          <label class="setting">
            <div><span class="setting-title">{t("settings.encrypt_databases")}</span>
              <span class="setting-desc">{t("settings.encrypt_databases_description")}</span></div>
            <input class="switch" type="checkbox" bind:checked={draft.encrypt_databases} />
          </label>
          {#if encryption?.error}<p class="error" role="alert"><bdi>{localizedMessage(encryption.error)}</bdi></p>{/if}
          {#if encryption?.diagnostic}<details><summary>{t("settings.encryption_diagnostics")}</summary><pre dir="ltr">{encryption.diagnostic}</pre></details>{/if}
          {#if encryptionLoadError}<p class="error" role="alert">{t("settings.encryption_status_failed")} <bdi>{localizedMessage(normalizeError(encryptionLoadError).descriptor)}</bdi></p>{/if}
          {#if encryption}<p class="setting-desc" role="status">{t(encryption.active_account_encrypted === null ? "settings.encryption_unknown" : encryption.active_account_encrypted ? "settings.encryption_active" : "settings.encryption_inactive")}</p>{/if}
          {#if encryption && draft.encrypt_databases !== encryption.enabled_at_start}<p class="setting-desc">{t("settings.encryption_restart")}</p>{/if}
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.ram_messages")}</span>
              <span class="setting-desc">{t("settings.main.ram_messages_hint", { min: 50, max: 2000, count: 150 })}</span>
            </div>
            <input class="field number" type="number" min="50" max="2000" step="1" aria-label={t("settings.main.ram_messages")}
              value={draft.message_window_size} oninput={(e) => {
                if (e.currentTarget.validity.valid && e.currentTarget.value) draft.message_window_size = Number(e.currentTarget.value);
              }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.keep_history")}</span>
              <span class="setting-desc">
                {t("settings.main.keep_history_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.keep_history} />
          </label>
          <div class="setting stack">
            <div>
              <span class="setting-title">{t("settings.main.history_folder")}</span>
              <span class="setting-desc">
                {t("settings.main.history_folder_hint")}
              </span>
            </div>
            <input
              class="field wide"
              dir="ltr"
              placeholder={t("settings.main.app_data_folder")}
              value={draft.history_dir ?? ""}
              oninput={(e) => (draft.history_dir = e.currentTarget.value || null)} />
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.disk_duration")}</span>
              <span class="setting-desc">
                {t("settings.main.disk_duration_hint")}
              </span>
            </div>
            <span class="unit-field">
              <input
                class="field number"
                type="number"
                min="1"
                value={limitValue(draft.retention.max_age_hours) === null ? "" : limitValue(draft.retention.max_age_hours)! / ageUnit}
                aria-label={t("settings.main.retention_duration")}
                oninput={(e) => {
                  if (e.currentTarget.validity.badInput) return;
                  const amount = hoursField(e.currentTarget.value);
                  draft.retention.max_age_hours = parseLimit(amount === null ? "" : String(Math.min(0xffffffff, amount * ageUnit)));
                }} />
              <select class="field" value={ageUnit} onchange={(e) => setAgeUnit(Number(e.currentTarget.value))}>
                {#each AGE_UNITS as [unit, label] (unit)}
                  <option value={unit}>{t(label)}</option>
                {/each}
              </select>
            </span>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.disk_messages")}</span>
              <span class="setting-desc">
                {t("settings.main.disk_messages_hint")}
              </span>
            </div>
            <input
              class="field number"
              type="number"
              min="1"
              value={limitValue(draft.retention.max_messages_per_chat) ?? ""}
              aria-label={t("settings.main.messages_per_chat")}
              oninput={(e) => {
                if (!e.currentTarget.validity.badInput) draft.retention.max_messages_per_chat = parseLimit(String(hoursField(e.currentTarget.value) ?? ""));
              }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.request_full_history")}</span>
              <span class="setting-desc">
                {t("settings.main.request_full_history_hint", { count: 10000 })}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.request_full_history} />
          </label>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.download_history")}</span>
              <span class="setting-desc">
                {t("settings.main.download_history_hint")}
                {#if backfillError}<bdi>{localizedMessage(backfillError.descriptor)}</bdi>{/if}
              </span>
            </div>
            <button
              class="button"
              onclick={() => {
                backfillError = null;
                invoke("backfill_history").catch((e) => (backfillError = normalizeError(e)));
              }}>{t("settings.main.download")}</button>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.clear_message_history")}</span>
              <span class="setting-desc">
                {t("settings.main.clear_message_history_hint")}
              </span>
            </div>
            {#if clearingHistory}
              <span class="unit-field">
                <button class="button" onclick={() => (clearingHistory = false)}>{t("settings.main.cancel")}</button>
                <button
                  class="button danger"
                  onclick={() => {
                    clearingHistory = false;
                    onclearhistory();
                  }}>{t("settings.main.delete_all")}</button>
              </span>
            {:else}
              <button class="button danger" onclick={() => (clearingHistory = true)}>{t("settings.main.clear_history")}</button>
            {/if}
          </div>
          <ArchiveManager />
        {:else if section === "chats"}
          <h3>{t("settings.main.keyword_rules")}</h3>
          <KeywordSettings account={active} />
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.keep_archived")}</span>
              <span class="setting-desc">
                {t("settings.main.keep_archived_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.keep_archived} />
          </label>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.freeze_hover")}</span>
              <span class="setting-desc">
                {t("settings.main.freeze_hover_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.freeze_chat_list_on_hover} />
          </label>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.chat_preview")}</span>
              <span class="setting-desc">
                {t("settings.main.chat_preview_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.chat_preview} />
          </label>
        {:else if section === "spaces"}
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.export_metadata")}</span>
              <span class="setting-desc">
                {t("settings.main.export_metadata_hint")}
              </span>
            </div>
            <button class="button" disabled={!spacesReady || spaceBusy} onclick={exportSpaces}>
              {spaceBusy ? t("settings.main.working") : t("settings.main.export")}
            </button>
          </div>
          {#if spaceJson}
            <label class="setting stack">
              <div><span class="setting-title">{t("settings.main.exported_metadata")}</span></div>
              <textarea class="field" dir="ltr" rows="8" readonly value={spaceJson}></textarea>
            </label>
          {/if}
          <div class="setting stack">
            <div>
              <span class="setting-title">{t("settings.main.import_metadata")}</span>
              <span class="setting-desc">
                {t("settings.main.import_metadata_hint")}
              </span>
            </div>
            <textarea class="field" dir="ltr" rows="8" placeholder={t("settings.main.metadata_placeholder")} bind:value={spaceImport} disabled={spaceBusy}></textarea>
            <div class="actions-row">
              <button class="button" disabled={!spacesReady || spaceBusy || !spaceImport.trim()} onclick={importSpaces}>
                {spaceBusy ? t("settings.main.importing") : t("settings.main.import")}
              </button>
            </div>
          </div>
          {#if spaceError}<p class="error-text" role="alert"><bdi>{localizedMessage(spaceError.descriptor)}</bdi></p>{/if}
        {:else if section === "notifications"}
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.enable_notifications")}</span>
              <span class="setting-desc">
                {t("settings.main.enable_notifications_hint")}
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
              {t("settings.main.notifications_off")}
            {:else if notifPermission === "granted"}
              {t("settings.main.notifications_on")}
            {:else}
              {t("settings.main.notifications_pending")}
            {/if}
          </p>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.mute_all")}</span>
              <span class="setting-desc">
                {t("settings.main.mute_all_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.mute_all_at_all} />
          </label>
          {#if !draft.mute_all_at_all}
            <div class="setting stack">
              <div>
                <span class="setting-title">{t("settings.main.chats_muting_all")}</span>
                <span class="setting-desc">{t("settings.main.chats_muting_all_hint")}</span>
              </div>
              <AtAllMuteList />
            </div>
          {/if}
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.system_permission")}</span>
              <span class="setting-desc">
                {#if notifPermission === "unsupported"}
                  {t("settings.main.notifications_unsupported")}
                {:else if notifPermission === "granted"}
                  {t("settings.main.notifications_allowed")}
                {:else if notifPermission === "denied"}
                  {t("settings.main.notifications_denied")}
                {:else}
                  {t("settings.main.notifications_prompt")}
                {/if}
              </span>
            </div>
            {#if notifPermission === "prompt"}
              <button class="button" disabled={notifBusy} onclick={requestNotifPermission}>
                {notifBusy ? t("settings.main.asking") : t("settings.main.allow")}
              </button>
            {:else if notifPermission === "denied"}
              <button class="button" disabled={notifBusy} onclick={refreshNotifPermission}>
                {notifBusy ? t("settings.main.checking") : t("settings.main.recheck")}
              </button>
            {:else if notifPermission === "granted"}
              <button class="button" onclick={() => void sendTestNotification()}>{t("settings.main.test")}</button>
            {/if}
          </div>
          <div class="setting">
            <div><span class="setting-title">{t("settings.main.notification_history")}</span><span class="setting-desc">{t("settings.main.notification_history_hint", { count: 100 })}</span></div>
            <button class="button" disabled={!active || $notificationHistory.account !== active
              || (!$notificationHistory.entries.length && !$notificationHistory.error && $notificationHistory.writable)}
              onclick={() => {
                const account = active;
                if (account) notificationHistory.clear(account, () => account === active && account === session.activeAccount);
              }}>{t("settings.main.clear_notification_history")}</button>
          </div>
          <NotificationHistory account={active} connected={session.connected} onjump={async (account, chat, id) => {
            if (account !== active || account !== session.activeAccount) throw normalizeError({ kind: "postal_error", code: "error.settings_account_changed", params: {} });
            if (!onnotificationjump) throw normalizeError({ kind: "postal_error", code: "error.settings_message_navigation_unavailable", params: {} });
            await onnotificationjump(account, chat, id);
          }} />
        {:else if section === "device"}
          {#if !once.paired}
            <p class="lede">
              {t("settings.main.companion_pair_hint")}
            </p>
            {#if once.pairing}
              <div class="setting stack">
                <div>
                  <span class="setting-title">{t("settings.main.waiting_pairing")}</span>
                  <span class="setting-desc">
                    {t("settings.main.pairing_steps")}
                  </span>
                </div>
                {#if oncePhoneMode}
                  <PhoneLink
                    active
                    code={once.pairCode}
                    expiresAt={once.pairCodeExpiresAt}
                    error={once.pairCodeError}
                    busy={once.pairCodeBusy}
                    manual={once.pairCodeManual}
                    onrequest={(phone) => void once.requestPairCode(phone)}
                    onrefresh={() => void once.refreshPairCode()}
                    ondeactivate={() => {
                      oncePhoneMode = false;
                      void once.cancelPairCode();
                    }} />
                {:else}
                  {#if once.qrSvg}
                    <div class="qr">
                      <div class="qr-code">
                        {@html once.qrSvg}
                      </div>
                    </div>
                  {/if}
                  <PhoneLink onactivate={() => (oncePhoneMode = true)} />
                {/if}
                <button class="button" onclick={() => once.cancelPair()}>{t("settings.main.cancel")}</button>
              </div>
            {:else}
              <button class="button" onclick={() => once.pair()} disabled={!draft.keep_history}>
                {t("settings.main.pair_companion")}
              </button>
              {#if !draft.keep_history}
                <p class="muted setting-desc">
                  {t("settings.main.companion_history_required")}
                </p>
              {/if}
            {/if}
          {:else}
            <label class="setting">
              <div>
                <span class="setting-title">{t("settings.main.run_companion")}</span>
                <span class="setting-desc">
                  {t("settings.main.run_companion_hint")}
                </span>
              </div>
              <input
                class="switch"
                type="checkbox"
                bind:checked={draft.android_instance}
                disabled={!draft.keep_history} />
            </label>
            <p class="muted setting-desc">
              {t(settings.android_instance ? "settings.main.companion_linked_enabled" : "settings.main.companion_linked_disabled")}
            </p>
          {/if}
          {#if settings.android_instance || once.pairing}
            <div class="setting stack">
              <div>
                <span class="setting-title">{t("settings.main.status")}</span>
                <span class="setting-desc">
                  {once.pairing && !once.paired
                    ? once.connected
                      ? t("settings.main.companion_finishing")
                      : t("settings.main.companion_waiting")
                    : once.connected
                      ? t("settings.main.companion_fetching")
                      : once.running
                        ? t("settings.main.companion_waking")
                        : t("settings.main.companion_dormant")}
                </span>
              </div>
            </div>
          {/if}
        {:else if section === "media"}
          <h2>{t("settings.main.stickers")}</h2>
          <StickerSync account={active} generation={messages.accountGeneration} connected={session.connected} version={stickerEvents.version} showPacks
            onload={(owner) => invoke<StickerLibrary>("sticker_library", { accountId: owner.account })}
            onresync={(owner) => invoke<StickerResyncReport>("resync_stickers", { accountId: owner.account })}
            onsynced={() => stickerEvents.touch()} />
          <label class="setting">
            <div><span class="setting-title">{t("settings.main.upload_quality")}</span>
              <span class="setting-desc">{t("settings.main.upload_quality_hint", { pixels: 1600, resolution: 480 })}</span></div>
            <select bind:value={draft.media_quality} aria-label={t("settings.main.upload_quality")}>
              <option value="standard">{t("settings.main.standard")}</option><option value="hd">{t("settings.main.hd")}</option>
            </select>
          </label>
          <div class="setting stack">
            <AutoDownloadSettings value={draft.auto_download_types} onchange={(next) => {
              draft.auto_download_types = next;
              draft.auto_download_media = Object.values(next).every(Boolean);
            }} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.video_preview_warning")}</span>
              <span class="setting-desc">{t("settings.main.video_preview_warning_hint")}</span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.warn_missing_video_preview} />
          </label>
          <div class="setting stack">
            <div>
              <span class="setting-title">{t("settings.main.download_folder")}</span>
              <span class="setting-desc">{t("settings.main.download_folder_hint")}</span>
            </div>
            <input
              class="field wide"
              dir="ltr"
              placeholder={t("settings.main.app_cache_folder")}
              value={draft.media_dir ?? ""}
              oninput={(e) => (draft.media_dir = e.currentTarget.value || null)} />
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.clear_downloaded_media")}</span>
              <span class="setting-desc">{t("settings.main.clear_downloaded_media_hint")}</span>
            </div>
            <button class="button danger" onclick={onflush}>{t("settings.main.clear_media")}</button>
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
            <div><span class="setting-title">{t("settings.main.start_on_login")}</span><span class="setting-desc">{t("settings.main.start_on_login_hint")}</span></div>
            <input class="switch" type="checkbox" bind:checked={draft.start_on_login} />
          </label>
          {#if desktopStatus}
            <p class="setting-desc">{t(desktopStatus.start_on_login ? "settings.main.start_on_login_enabled" : "settings.main.start_on_login_disabled")}</p>
            <p class="setting-desc"><bdi>{t(desktopStatus.shortcut_registered ? "settings.main.shortcut_registered" : "settings.main.shortcut_unavailable", { keys: "Ctrl+Alt+P" })}</bdi></p>
          {:else if desktopError}<p role="alert"><bdi>{localizedMessage(desktopError.descriptor)}</bdi></p>{/if}
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.skip_loading")}</span>
              <span class="setting-desc">
                {t("settings.main.skip_loading_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.skip_loading_screen} />
          </label>
        {:else if section === "keybinds"}
          {#each ACTIONS as action (action.id)}
            <div class="setting">
              <div>
                <span class="setting-title">{t(`settings.main.keybind.${action.id}.label`)}</span>
                <span class="setting-desc">
                  {t(`settings.main.keybind.${action.id}.description`)}
                  {#if keyConflicts.has(action.id)}
                    <strong class="conflict">{t("settings.main.key_conflict")}</strong>
                  {/if}
                </span>
              </div>
              <div class="keybind">
                <button
                  class="button"
                  class:capturing={capturing === action.id}
                  onclick={() => (capturing = capturing === action.id ? null : action.id)}>
                  {#if capturing === action.id}{t("settings.main.press_keys")}{:else}<bdi dir="ltr">{bindingLabel(action.id)}</bdi>{/if}
                </button>
                {#if !isDefault(action.id)}
                  <Button
                    variant="ghost"
                    title={t("settings.main.reset_default")}
                    aria-label={t("settings.main.reset_named_default", { action: t(`settings.main.keybind.${action.id}.label`) })}
                    onclick={() => resetBinding(action.id)}>{t("settings.main.reset")}</Button>
                {/if}
              </div>
            </div>
          {/each}
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.show_hide")}</span>
              <span class="setting-desc">
                {t("settings.main.show_hide_hint")}
                {#if desktopStatus && !desktopStatus.shortcut_registered}
                  {t("settings.main.shortcut_conflict")}
                {/if}
              </span>
            </div>
            <div class="keybind">
              <button class="button" disabled title={t("settings.main.system_shortcut_fixed")}><bdi dir="ltr">Ctrl+Alt+P</bdi></button>
            </div>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.reset_keybinds")}</span>
              <span class="setting-desc">{t("settings.main.reset_keybinds_hint")}</span>
            </div>
            <button class="button danger" onclick={resetBindings}>{t("settings.main.reset_all")}</button>
          </div>
        {:else if section === "appearance"}
          <label class="setting">
            <div><span class="setting-title">{t("settings.language")}</span>
              <span class="setting-desc">{t("settings.language_description")}</span></div>
            <select class="field" aria-label={t("settings.language")} value={locale.preference}
              oninput={(event) => void locale.setPreference(event.currentTarget.value as LocalePreference)}>
              <option value="system">{t("settings.language_system")}</option>
              <option value="en">{localeNames.en}</option><option value="ar">{localeNames.ar}</option>
            </select>
          </label>
          {#if locale.error}<p class="error" role="alert">{locale.errorText}</p>{/if}
          <div class="customization"><Customization /></div>
        {:else if section === "advanced"}
          <label class="setting">
            <div>
              <span class="setting-title">{t("settings.main.verbose_logs")}</span>
              <span class="setting-desc">
                {t("settings.main.verbose_logs_hint")}
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.verbose_whatsapp_logs} />
          </label>
        {:else}
          <div class="setting">
            <span class="setting-title">{t("settings.main.postal")}</span>
            <span class="muted"><bdi>{version ? t("settings.main.version", { version }) : ""}</bdi></span>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">{t("settings.main.log_file")}</span>
              <span class="setting-desc">
                {t("settings.main.log_file_hint")}
              </span>
            </div>
            <button class="button" onclick={() => invoke("open_log").catch(() => {})}>{t("settings.main.open_log")}</button>
          </div>
          <BooleanProps />
        {/if}

  {#snippet footer()}
      {#if dirty}
        <div class="unsaved" role="status">
          <span>{t("settings.main.unsaved")}</span>
          <button class="link-button" onclick={reset}>{t("settings.main.reset")}</button>
          <button class="button primary" disabled={saving} onclick={save}>{t("settings.main.save_changes")}</button>
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
    border-block-start: 1px solid var(--line);
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
    padding-block: 10px 12px;
    padding-inline: 60px 14px;
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
    inset-inline: 24px;
    inset-block-end: 20px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding-block: 10px;
    padding-inline: 16px 12px;
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
