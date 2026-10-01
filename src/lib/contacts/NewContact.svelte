<script lang="ts">
  import { onMount } from "svelte";
  import ContactEditor from "./ContactEditor.svelte";
  let { account, connected, onsaved, onclose }: {
    account: string; connected: boolean; onsaved: (jid: string) => void; onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  function close() { dialog.close(); onclose(); }
  function saved(jid: string) { dialog.close(); onsaved(jid); }
  onMount(() => {
    const previous = document.activeElement;
    dialog.showModal();
    dialog.querySelector("input")?.focus();
    return () => { dialog.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  });
</script>

<dialog bind:this={dialog} aria-label="New contact" oncancel={close}>
  <header><h2>Add a contact</h2><button type="button" aria-label="Close" onclick={close}>×</button></header>
  <ContactEditor {account} {connected} onsaved={saved} />
</dialog>

<style>
  dialog { width: min(400px, 90vw); max-height: 86vh; overflow: auto; box-sizing: border-box;
    padding: 20px 22px; background: var(--surface); color: var(--text); border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); backdrop-filter: blur(2px); }
  header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 18px; }
  h2 { margin: 0; font-size: 17px; }
  header button { color: var(--text); background: transparent; border: 0; font-size: 24px; cursor: pointer; }
</style>
