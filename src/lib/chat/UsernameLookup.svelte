<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onMount, untrack } from "svelte";
  import type { UsernameLookupResult } from "$lib/utils/wire";

  let { account, generation, onlookup, onfound, onclose }: {
    account: string;
    generation: number;
    onlookup: (username: string, usernameKey?: string) => Promise<UsernameLookupResult>;
    onfound: (jid: string, username: string | null) => void | Promise<void>;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let input: HTMLInputElement;
  const openedAccount = untrack(() => account);
  const openedGeneration = untrack(() => generation);
  let closed = false;
  let request = 0;
  let query = $state("");
  let usernameKey = $state("");
  let result = $state<UsernameLookupResult | null>(null);
  let busy = $state(false);
  let error = $state<LocalizedError | string | null>(null);

  function current() {
    return !closed && account === openedAccount && generation === openedGeneration;
  }

  function changeQuery(value: string) {
    request++;
    query = value;
    usernameKey = "";
    result = null;
    error = null;
    busy = false;
  }

  function changeKey(value: string) {
    request++;
    usernameKey = value;
    error = null;
    busy = false;
  }

  async function lookup() {
    const username = query.trim();
    if (busy || !username || !current()) return;
    const revision = ++request;
    const key = usernameKey;
    const active = () => current() && request === revision && query.trim() === username && usernameKey === key;
    busy = true;
    error = null;
    try {
      const found = await onlookup(username, result?.kind === "keyRequired" && key ? key : undefined);
      if (!active()) return;
      if (found.kind === "found") {
        await onfound(found.jid, found.username);
        if (active()) close();
      } else {
        result = found;
        if (found.kind === "notFound") usernameKey = "";
      }
    } catch (failure) {
      if (active()) error = normalizeError(failure);
    } finally {
      if (current() && request === revision) busy = false;
    }
  }

  function close() {
    if (closed) return;
    closed = true;
    changeQuery("");
    dialog?.close();
    onclose();
  }

  onMount(() => {
    const previous = document.activeElement;
    dialog.showModal();
    input.focus();
    return () => {
      closed = true;
      request++;
      dialog.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  });

  $effect(() => { if (account !== openedAccount || generation !== openedGeneration) close(); });
</script>

<dialog bind:this={dialog} aria-labelledby="username-lookup-title"
  oncancel={(event) => { event.preventDefault(); close(); }}>
  <header>
    <h2 id="username-lookup-title">{t("contact.username_find")}</h2>
    <button type="button" class="close" aria-label={t("contact.username_close")} onclick={close}>×</button>
  </header>
  <form onsubmit={(event) => { event.preventDefault(); void lookup(); }} aria-busy={busy}>
    <label for="username-lookup-query">{t("contact.username")}</label>
    <input bind:this={input} id="username-lookup-query" value={query}
      oninput={(event) => changeQuery(event.currentTarget.value)} autocomplete="off" autocapitalize="none"
      spellcheck="false" placeholder={t("contact.username_example")} required />
    {#if result?.kind === "keyRequired"}
      <p class="status" role="status">{t("contact.username_key_hint")}</p>
      <label for="username-lookup-key">{t("contact.username_key")}</label>
      <input id="username-lookup-key" type="password" value={usernameKey}
        oninput={(event) => changeKey(event.currentTarget.value)} autocomplete="off" autocapitalize="none" spellcheck="false" />
    {:else if result?.kind === "notFound"}
      <p class="status" role="status">{t("contact.username_empty")}</p>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <footer><button type="submit" disabled={busy || !query.trim()}>{busy ? t("contact.username_finding") : t("contact.username_find")}</button></footer>
  </form>
</dialog>

<style>
  dialog { width: min(400px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto; box-sizing: border-box; padding: 20px 22px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 18px; }
  h2 { margin: 0; font-size: 1.0625rem; }
  .close { border: 0; background: transparent; color: var(--text); font: inherit; font-size: 1.5rem; cursor: pointer; }
  label { display: block; margin-bottom: 6px; font-size: 0.8125rem; }
  input { box-sizing: border-box; width: 100%; padding: 10px 12px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; }
  .status, .error { margin: 12px 0; font-size: 0.8125rem; }
  .status { color: var(--muted); }
  .error { color: var(--danger); }
  footer { display: flex; justify-content: flex-end; margin-top: 16px; }
  footer button { padding: 8px 14px; border: 0; border-radius: 6px; background: var(--accent); color: var(--accent-ink); font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
</style>
