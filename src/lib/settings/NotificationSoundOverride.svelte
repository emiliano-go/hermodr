<script lang="ts">
  import { invoke } from "$lib/utils/ipc";
  let { accountId, chat }: { accountId: string; chat: string } = $props();
  let muted = $state(false);
  let loaded = $state(false);
  let busy = $state(true);
  let error = $state("");
  let generation = 0;
  function current(token: number, account: string, target: string) {
    return token === generation && account === accountId && target === chat;
  }
  async function load(account: string, target: string) {
    const token = ++generation;
    muted = false; loaded = false; busy = true; error = "";
    try {
      const value = await invoke<boolean | null>("chat_sound_muted", { accountId: account, chat: target });
      if (current(token, account, target)) { muted = value ?? false; loaded = true; }
    } catch (failure) { if (current(token, account, target)) error = String(failure); }
    finally { if (current(token, account, target)) busy = false; }
  }
  $effect(() => {
    void load(accountId, chat);
    return () => { generation++; };
  });
  async function change(input: HTMLInputElement) {
    const next = input.checked;
    input.checked = muted;
    if (busy || !loaded) return;
    const token = generation, account = accountId, target = chat;
    busy = true; error = "";
    try {
      await invoke("set_chat_sound_muted", { accountId: account, chat: target, muted: next });
      if (current(token, account, target)) muted = next;
    } catch (failure) { if (current(token, account, target)) error = String(failure); }
    finally { if (current(token, account, target)) busy = false; }
  }
</script>

<label><span>Mute notification sound</span>
  <input type="checkbox" checked={muted} disabled={busy || !loaded}
    onchange={(event) => change(event.currentTarget)} />
</label>
<p>Message notifications still appear for this chat.</p>
{#if error}
  <p role="alert">{error}</p>
  <button type="button" disabled={busy} onclick={() => load(accountId, chat)}>Retry</button>
{/if}

<style>
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .8rem; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
