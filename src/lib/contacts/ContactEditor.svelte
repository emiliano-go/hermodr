<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
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
  let error = $state<LocalizedError | string>("");
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
      result = remove ? "contact.removed" : "contact.saved";
      onsaved(target);
    } catch (failure) {
      if (id === account && revision === generation) error = normalizeError(failure);
    } finally {
      if (id === account && revision === generation) busy = null;
    }
  }
</script>

<form onsubmit={(event) => { event.preventDefault(); void submit(); }}>
  <fieldset disabled={!!busy || !account || !connected}>
    {#if !jid}
      <label>{t("contact.phone_number")}<input type="tel" dir="ltr" autocomplete="tel" bind:value={phone} placeholder={t("contact.phone_example")} required /></label>
    {/if}
    <label>{t("contact.full_name")}<input dir="auto" autocomplete="name" bind:value={fullName} oninput={() => { dirty = true; result = ""; }} required /></label>
    <label>{t("contact.first_name")} <span>{t("ui.optional")}</span><input dir="auto" autocomplete="given-name" bind:value={firstName} /></label>
    <label class="check"><input type="checkbox" bind:checked={saveOnPhone} />{t("contact.save_on_phone")}</label>
    <div class="actions">
      <Button variant="primary" type="submit">{busy === "save" ? t("ui.saving") : saved ? t("ui.save_changes") : t("contact.save")}</Button>
      {#if saved && jid}
        <Button variant="ghost" danger type="button" onclick={() => submit(true)}>{busy === "remove" ? t("ui.removing") : t("contact.remove")}</Button>
      {/if}
    </div>
  </fieldset>
</form>
{#if !account || !connected}<p role="status">{t("contact.connect_to_edit")}</p>{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result}<p role="status">{t(result)}</p>{/if}

<style>
  fieldset { border: 0; padding: 0; margin: 0; display: grid; gap: .75rem; }
  label { display: grid; gap: .3rem; font-size: .85rem; }
  label span, p { color: var(--muted); }
  input:not([type="checkbox"]) { width: 100%; box-sizing: border-box; padding: .65rem .75rem; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); color: var(--text); font: inherit; }
  .check, .actions { display: flex; align-items: center; gap: .5rem; }
  .actions { flex-wrap: wrap; }
  p { font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
</style>
