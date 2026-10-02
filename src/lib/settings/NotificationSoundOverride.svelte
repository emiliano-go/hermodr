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
  <input class="toggle" type="checkbox" checked={muted} disabled={busy || !loaded}
    onchange={(event) => change(event.currentTarget)} />
</label>
<p>Message notifications still appear for this chat.</p>
{#if error}
  <p role="alert">{error}</p>
  <button type="button" disabled={busy} onclick={() => load(accountId, chat)}>Retry</button>
{/if}

<style>
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .8rem; margin: .35rem 0 0; }
  [role="alert"] { color: var(--danger, #b91c1c); }
  /* Same switch as the chat settings dialog (scoped CSS cannot be shared). */
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
    left: 3px;
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
  .toggle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .toggle:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
