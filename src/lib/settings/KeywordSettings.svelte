<script lang="ts">
  import { keywords } from "$lib/state/keywords.svelte";
  import Button from "$lib/ui/Button.svelte";
  let { account, onchange = () => {} }: { account: string | null; onchange?: () => void } = $props();
  let highlight = $state("");
  let hide = $state("");
  let saved = $state(false);
  let draftAccount: string | null | undefined;

  $effect(() => {
    if (account !== keywords.account) keywords.load(account);
    highlight = keywords.rules.highlight.join("\n");
    hide = keywords.rules.hide.join("\n");
    if (draftAccount !== account) { saved = false; draftAccount = account; }
  });

  const dirty = $derived(highlight !== keywords.rules.highlight.join("\n") || hide !== keywords.rules.hide.join("\n"));

  function save() {
    if (!account || !keywords.save(account, { highlight: highlight.split("\n").filter((term) => term.trim().length > 0),
      hide: hide.split("\n").filter((term) => term.trim().length > 0) })) return;
    highlight = keywords.rules.highlight.join("\n");
    hide = keywords.rules.hide.join("\n");
    saved = true;
    onchange();
  }
</script>

<p>Rules are saved locally for this account. Match literal text anywhere in incoming messages, ignoring case. One word or phrase per line; up to 50 entries per list and 100 characters per entry.</p>
<p>Hidden matches take priority. Spoilers, view-once content and unavailable messages are excluded.</p>
<fieldset disabled={!account}>
  <label>Highlight keywords<textarea rows="5" bind:value={highlight} oninput={() => { saved = false; }}></textarea></label>
  <label>Hide keywords<textarea rows="5" bind:value={hide} oninput={() => { saved = false; }}></textarea></label>
  <Button variant="primary" disabled={!dirty && !keywords.error} onclick={save}>{saved ? "Saved" : "Save keyword rules"}</Button>
</fieldset>
{#if !account}<p role="status">Select an account to edit its keyword rules.</p>{/if}
{#if keywords.error}<p role="alert">{keywords.error}</p>{/if}
{#if keywords.countError}<p role="alert">{keywords.countError}</p>{/if}

<style>
  fieldset { display: grid; gap: .8rem; border: 0; padding: 0; margin: 0; }
  label { display: grid; gap: .35rem; }
  textarea { resize: vertical; box-sizing: border-box; width: 100%; padding: .7rem; color: var(--text); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); }
  p { color: var(--muted); font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
</style>
