<script lang="ts">
  import { onMount, tick } from "svelte";
  import ContactEditor from "$lib/contacts/ContactEditor.svelte";
  import Settings from "$lib/settings/Settings.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { ContactIdentity, Profile } from "$lib/utils/wire";

  let account = $state("synthetic-contact-account-a");
  let jid = $state<string | null>(null);
  let identity = $state<ContactIdentity | null>(null);
  let callbacks = $state(0);
  let completed = $state(0);
  let settingsOpen = $state(false);
  let profileCalls = 0, profileCompleted = 0;
  let delayProfile = false;
  let releaseProfile: (() => void) | undefined;
  const profiles = new Map<string, Profile>();
  const ownProfile = (name: string, about: string): Profile => ({ name, about, privacy: {}, username: null, username_reserved: false });
  profiles.set("synthetic-contact-account-a", ownProfile("Initial own name", "Initial own about"));
  profiles.set("synthetic-contact-account-b", ownProfile("Account B own name", "Account B own about"));
  const noop = () => {};
  let mode = "success";
  let release: (() => void) | undefined;
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  const contacts = new Map<string, ContactIdentity>();
  const key = (id: string, address: string) => `${id}:${address}`;
  const unsaved = (): ContactIdentity => ({ contact_saved: false, saved_name: null, legacy_name: null,
    push_name: "Synthetic push name", username: null, number: "59891954564", own: false });

  async function invoke(command: string, args: Record<string, unknown> = {}) {
    if (command === "profile") {
      ++profileCalls;
      const profile = profiles.get(session.activeAccount ?? "");
      if (!profile) throw new Error("Unknown synthetic profile account");
      const snapshot = { ...profile, privacy: { ...profile.privacy } };
      if (delayProfile) {
        delayProfile = false;
        await new Promise<void>((resolve) => { releaseProfile = resolve; });
      }
      ++profileCompleted;
      return snapshot;
    }
    if (command !== "save_contact" && command !== "remove_contact") throw new Error(`Unexpected synthetic command: ${command}`);
    if (typeof args.account !== "string" || typeof args.jid !== "string") throw new Error("Missing account/contact");
    calls.push({ command, args: { ...args } });
    const behavior = mode, id = args.account, address = args.jid;
    if (behavior === "delay") await new Promise<void>((resolve) => { release = resolve; });
    if (behavior === "reject") throw new Error("Synthetic provider rejected contact change");
    const contact = contacts.get(key(id, address)) ?? unsaved();
    contacts.set(key(id, address), command === "remove_contact" ? { ...contact, contact_saved: false, saved_name: null }
      : { ...contact, contact_saved: true, saved_name: String(args.fullName) });
    ++completed;
  }

  function saved(address: string) {
    ++callbacks;
    jid = address;
    identity = contacts.get(key(account, address)) ?? unsaved();
  }

  onMount(() => {
    session.activeAccount = account;
    session.profileVersion = 0;
    Object.assign(window, { __contactsFixture: {
      invoke,
      mode: (next: string) => { mode = next; },
      release: () => { release?.(); release = undefined; },
      switchAccount: (next: string) => {
        account = next;
        session.activeAccount = next;
        session.profileVersion = 0;
        identity = jid ? contacts.get(key(account, jid)) ?? unsaved() : null;
      },
      openProfile: () => { settingsOpen = true; },
      updateProfile: (name: string, about?: string) => {
        const profile = profiles.get(account);
        if (!profile) throw new Error("Unknown synthetic profile");
        profiles.set(account, { ...profile, name, about: about ?? profile.about });
        ++session.profileVersion;
      },
      delayProfile: () => { delayProfile = true; },
      releaseProfile: () => { releaseProfile?.(); releaseProfile = undefined; },
      settle: tick,
      state: () => ({ account, jid, identity, callbacks, completed, profileCalls, profileCompleted,
        calls: calls.map((call) => ({ ...call, args: { ...call.args } })) }),
    } });
  });
</script>

<main>
  <h1>Contact editor synthetic checks</h1>
  <p>Account: <output>{account}</output> · Accepted callbacks: <output>{callbacks}</output></p>
  <ContactEditor {account} connected={true} {jid} {identity} onsaved={saved} />
</main>

{#if settingsOpen}
  <Settings settings={session.settings} section="profile" accounts={[]} active={account} me="59891954564@s.whatsapp.net"
    meAvatar={null} accountAvatars={{}} onclose={() => { settingsOpen = false; }} onsave={async () => {}}
    onflush={noop} onclearhistory={noop} onrename={noop} onremove={noop} onadd={noop} onswitch={noop}
    onprivacy={noop} onpicture={noop} onblockedload={async () => []} onunblockcontact={async () => {}} />
{/if}

<style>
  :global(body) { margin: 0; padding: 2rem; font: 15px "Segoe UI", sans-serif; background: #121418; color: #e8eaed;
    --surface: #1b1e25; --text: #e8eaed; --muted: #a5acb8; --border: #373d49; --radius: 8px;
    --accent: #67d5b5; --accent-text: #10271f; --accent-soft: #254c41; --danger: #f87171; }
  main { max-width: 450px; margin: auto; }
  h1 { font-size: 1.3rem; }
  p { color: var(--muted); }
</style>
