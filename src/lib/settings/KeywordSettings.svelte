<script lang="ts">
  import { t } from "$lib/i18n/localizer";
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

<p>{t("settings.keywords_hint")}</p>
<p>{t("settings.keywords_priority")}</p>
<fieldset disabled={!account}>
  <label>{t("settings.keywords_highlight")}<textarea dir="auto" rows="5" bind:value={highlight} oninput={() => { saved = false; }}></textarea></label>
  <label>{t("settings.keywords_hide")}<textarea dir="auto" rows="5" bind:value={hide} oninput={() => { saved = false; }}></textarea></label>
  <Button variant="primary" disabled={!dirty && !keywords.error} onclick={save}>{saved ? t("ui.saved") : t("settings.keywords_save")}</Button>
</fieldset>
{#if !account}<p role="status">{t("settings.keywords_select_account")}</p>{/if}
{#if keywords.error}<p role="alert">{keywords.error}</p>{/if}
{#if keywords.countError}<p role="alert">{keywords.countError}</p>{/if}

<style>
  fieldset { display: grid; gap: .8rem; border: 0; padding: 0; margin: 0; }
  label { display: grid; gap: .35rem; }
  textarea { resize: vertical; box-sizing: border-box; width: 100%; padding: .7rem; color: var(--text); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); }
  p { color: var(--muted); font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
</style>
