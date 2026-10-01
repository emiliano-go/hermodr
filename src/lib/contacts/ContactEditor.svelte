<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import Button from "$lib/ui/Button.svelte";
  import type { ContactIdentity } from "$lib/utils/wire";
  import { editContact } from "./contact-edit";

  let { account, connected, jid = null, identity = null, onsaved }: {
    account: string | null;
    connected: boolean;
    jid?: string | null;
    identity?: ContactIdentity | null;
    onsaved: (jid: string) => void;
  } = $props();

  let phone = $state("");
  let fullName = $state("");
  let firstName = $state("");
  let saveOnPhone = $state(true);
  let busy = $state<"save" | "remove" | null>(null);
  let error = $state("");
  let result = $state("");
  let dirty = $state(false);
  let generation = 0;
  const saved = $derived(identity?.contact_saved === true || (identity?.contact_saved == null && !!identity?.legacy_name));

  $effect(() => {
    account; jid;
    ++generation;
    phone = firstName = error = result = "";
    fullName = untrack(() => identity?.saved_name ?? identity?.legacy_name ?? "");
    saveOnPhone = true;
    busy = null;
    dirty = false;
    return () => { ++generation; };
  });

  $effect(() => {
    if (!dirty && !busy) fullName = identity?.saved_name ?? identity?.legacy_name ?? "";
  });

  async function submit(remove = false) {
    if (!account || !connected || busy) return;
    const id = account, revision = generation;
    busy = remove ? "remove" : "save";
    error = result = "";
    try {
      const target = await editContact(invoke, id, () => account, { jid, phone, fullName, firstName, saveOnPhone }, remove);
      if (id !== account || revision !== generation) return;
      dirty = true;
      if (remove) fullName = firstName = "";
      result = remove ? "Contact removed." : "Contact saved.";
      onsaved(target);
    } catch (failure) {
      if (id === account && revision === generation) error = String(failure);
    } finally {
      if (id === account && revision === generation) busy = null;
    }
  }
</script>

<form onsubmit={(event) => { event.preventDefault(); void submit(); }}>
  <fieldset disabled={!!busy || !account || !connected}>
    {#if !jid}
      <label>Phone number<input type="tel" autocomplete="tel" bind:value={phone} placeholder="+598 91954564" required /></label>
    {/if}
    <label>Full name<input autocomplete="name" bind:value={fullName} oninput={() => { dirty = true; result = ""; }} required /></label>
    <label>First name <span>(optional)</span><input autocomplete="given-name" bind:value={firstName} /></label>
    <label class="check"><input type="checkbox" bind:checked={saveOnPhone} />Save to phone's address book</label>
    <div class="actions">
      <Button variant="primary" type="submit">{busy === "save" ? "Saving…" : saved ? "Save changes" : "Save contact"}</Button>
      {#if saved && jid}
        <Button variant="ghost" danger type="button" onclick={() => submit(true)}>{busy === "remove" ? "Removing…" : "Remove contact"}</Button>
      {/if}
    </div>
  </fieldset>
</form>
{#if !account || !connected}<p role="status">Connect this account to edit contacts.</p>{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result}<p role="status">{result}</p>{/if}

<style>
  fieldset { border: 0; padding: 0; margin: 0; display: grid; gap: .75rem; }
  label { display: grid; gap: .3rem; font-size: .85rem; }
  label span, p { color: var(--muted); }
  input:not([type="checkbox"]) { width: 100%; box-sizing: border-box; padding: .65rem .75rem; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text); }
  .check, .actions { display: flex; align-items: center; gap: .5rem; }
  .actions { flex-wrap: wrap; }
  p { font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
</style>
