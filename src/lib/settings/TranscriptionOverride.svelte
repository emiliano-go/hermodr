<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { invoke } from "$lib/utils/ipc";
  let { accountId, chat }: { accountId: string; chat: string } = $props();
  let value = $state("");
  let busy = $state(false);
  let error = $state<LocalizedError | string>("");
  let generation = 0;
  $effect(() => {
    const token = ++generation;
    value = ""; busy = true; error = "";
    void invoke<boolean | null>("chat_auto_transcribe", { accountId, chat }).then(
      (enabled) => { if (token === generation) value = enabled === null ? "" : String(enabled); },
      (failure) => { if (token === generation) error = normalizeError(failure); },
    ).finally(() => { if (token === generation) busy = false; });
  });
  async function change(next: string) {
    const token = generation;
    busy = true; error = "";
    try {
      await invoke("set_chat_auto_transcribe", { accountId, chat, enabled: next === "" ? null : next === "true" });
      if (token === generation) value = next;
    } catch (failure) { if (token === generation) error = normalizeError(failure); }
    finally { if (token === generation) busy = false; }
  }
</script>

<label>{t("settings.transcribe_auto")}
  <select {value} disabled={busy} onchange={(event) => change(event.currentTarget.value)}>
    <option value="">{t("settings.follow_global")}</option>
    <option value="true">{t("ui.on")}</option>
    <option value="false">{t("ui.off")}</option>
  </select>
</label>
<p>{t("settings.transcribe_override_hint")}</p>
{#if error}<p role="alert">{error}</p>{/if}

<style>
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .8rem; margin: .35rem 0 0; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
