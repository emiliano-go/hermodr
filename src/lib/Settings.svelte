<script lang="ts" module>
  export type Retention = { max_age_hours: number | null; max_messages_per_chat: number | null };
  export type UiSettings = {
    retention: Retention;
    accept_full_history: boolean;
    auto_download_media: boolean;
    warn_missing_video_preview: boolean;
    media_dir: string | null;
    send_typing: boolean;
    send_receipts: boolean;
  };
  export type Account = { id: string; label: string; jid: string | null };
  export type Section =
    | "profile"
    | "accounts"
    | "whatsapp"
    | "privacy"
    | "media"
    | "appearance"
    | "about";
  type Profile = {
    name: string;
    about: string | null;
    username: string | null;
    username_reserved: boolean;
    privacy: Record<string, string>;
  };
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import Icon from "$lib/Icon.svelte";
  import Customization from "$lib/Customization.svelte";
  import Panel from "$lib/Panel.svelte";

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
    onrename,
    onremove,
    onadd,
    onswitch,
    onprivacy,
    onpicture,
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
    onrename: (id: string, label: string) => void;
    onremove: (id: string) => void;
    onadd: () => void;
    onswitch: (id: string) => void;
    onprivacy: (privacy: Record<string, string>) => void;
    /** Our picture changed, so the cached one is stale. */
    onpicture: () => void;
  } = $props();

  let picker: HTMLInputElement | undefined = $state();
  let pictureBusy = $state(false);
  /** Bumped per upload: the new picture reuses the old file name. */
  let pictureVersion = $state(0);

  /** Uploads a new picture, or removes it when `file` is null. */
  async function setPicture(file: File | null) {
    pictureBusy = true;
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

  function toBase64(file: File) {
    return new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  // Profile and WhatsApp privacy live on the account, so they wait for pairing.
  const NAV = $derived<{ id: Section; label: string; group: string }[]>([
    ...(me ? [{ id: "profile" as Section, label: "My profile", group: "User settings" }] : []),
    { id: "accounts", label: "My accounts", group: "User settings" },
    ...(me ? [{ id: "whatsapp" as Section, label: "WhatsApp privacy", group: "User settings" }] : []),
    { id: "privacy", label: "Storage & history", group: "App settings" },
    { id: "media", label: "Media", group: "App settings" },
    { id: "appearance", label: "Customization", group: "App settings" },
    { id: "about", label: "About", group: "Hermóðr" },
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
    return value ? Math.max(1, Number(value)) : null;
  }

  // The account's profile lives on WhatsApp's servers, so it is fetched when a
  // section that shows it opens and written back field by field.
  let profile = $state<Profile | null>(null);
  let profileError = $state<string | null>(null);
  let nameDraft = $state("");
  let aboutDraft = $state("");
  let profileSaved = $state(false);

  $effect(() => {
    if ((section === "profile" || section === "whatsapp") && !profile && !profileError) {
      invoke<Profile>("profile")
        .then((p) => {
          profile = p;
          nameDraft = p.name;
          aboutDraft = p.about ?? "";
        })
        .catch((e) => (profileError = String(e)));
    }
  });

  async function saveProfile() {
    if (!profile) return;
    profileError = null;
    try {
      if (nameDraft.trim() && nameDraft !== profile.name) {
        await invoke("set_push_name", { name: nameDraft.trim() });
        profile.name = nameDraft.trim();
      }
      if (aboutDraft !== (profile.about ?? "")) {
        await invoke("set_about", { text: aboutDraft });
        profile.about = aboutDraft;
      }
      profileSaved = true;
      setTimeout(() => (profileSaved = false), 1500);
    } catch (e) {
      profileError = String(e);
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
    if (!profile) return;
    const previous = profile.privacy[category];
    profile.privacy[category] = value;
    try {
      await invoke("set_privacy", { category, value });
      onprivacy($state.snapshot(profile.privacy));
    } catch (e) {
      profile.privacy[category] = previous;
      profileError = String(e);
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

        {#if section === "profile"}
          <h2>My profile</h2>
          <p class="lede">How you appear to others on WhatsApp.</p>
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
                  <input class="field" maxlength="25" bind:value={nameDraft} />
                </label>
                <label class="field-label">
                  About
                  <textarea class="field" rows="3" maxlength="139" bind:value={aboutDraft}></textarea>
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
                disabled={nameDraft === profile.name && aboutDraft === (profile.about ?? "")}
                onclick={saveProfile}>{profileSaved ? "Saved" : "Save profile"}</button>
            </div>
          {:else if !profileError}
            <p class="muted">Loading your profile…</p>
          {/if}
          {#if profileError}<p class="error-text">{profileError}</p>{/if}
        {:else if section === "whatsapp"}
          <h2>WhatsApp privacy</h2>
          <p class="lede">Account settings stored by WhatsApp, the same ones your phone shows.</p>
          <label class="setting">
            <div>
              <span class="setting-title">Send typing indicator</span>
              <span class="setting-desc">
                Others see "typing…" while you write. Off, you still see theirs.
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
                other people's.
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
          <h2>My accounts</h2>
          <p class="lede">Every account keeps its own session, history and settings.</p>
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
                <button class="button danger" onclick={() => onremove(account.id)}>Remove</button>
              </div>
            {/each}
          </div>
          <div class="actions-row">
            <button class="button primary" onclick={onadd}><Icon name="plus" size={15} /> Add account</button>
          </div>
        {:else if section === "privacy"}
          <h2>Privacy & storage</h2>
          <p class="lede">
            History is kept on this device only, and only within these limits. Leave a field
            empty for no limit.
          </p>
          <div class="setting">
            <div>
              <span class="setting-title">Keep messages for</span>
              <span class="setting-desc">Older messages are deleted from this device.</span>
            </div>
            <span class="unit-field">
              <input
                class="field number"
                type="number"
                min="1"
                value={draft.retention.max_age_hours ?? ""}
                oninput={(e) => (draft.retention.max_age_hours = hoursField(e.currentTarget.value))} />
              hours
            </span>
          </div>
          <div class="setting">
            <div>
              <span class="setting-title">Messages per chat</span>
              <span class="setting-desc">Only the newest are kept in each conversation.</span>
            </div>
            <input
              class="field number"
              type="number"
              min="1"
              value={draft.retention.max_messages_per_chat ?? ""}
              oninput={(e) =>
                (draft.retention.max_messages_per_chat = hoursField(e.currentTarget.value))} />
          </div>
          <label class="setting">
            <div>
              <span class="setting-title">Download full history when pairing</span>
              <span class="setting-desc">
                Pulls every past message the next time an account is linked. Off keeps only the
                recent window.
              </span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.accept_full_history} />
          </label>
        {:else if section === "media"}
          <h2>Media</h2>
          <label class="setting">
            <div>
              <span class="setting-title">Download media automatically</span>
              <span class="setting-desc">Off shows a download button instead. Each chat can override this.</span>
            </div>
            <input class="switch" type="checkbox" bind:checked={draft.auto_download_media} />
          </label>
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
              <span class="setting-desc">Empty uses the app cache. Applies the next time Hermóðr starts.</span>
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
        {:else if section === "appearance"}
          <h2>Customization</h2>
          <p class="lede">Themes and CSS extensions apply instantly and are saved on this device.</p>
          <div class="customization"><Customization /></div>
        {:else}
          <h2>About</h2>
          <div class="setting">
            <span class="setting-title">Hermóðr</span>
            <span class="muted">{version ? `Version ${version}` : ""}</span>
          </div>
          <p class="lede">A native WhatsApp client that speaks the protocol directly.</p>
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
  .account + .account {
    border-top: 1px solid var(--line);
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
    transition: opacity 0.15s ease;
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
</style>
