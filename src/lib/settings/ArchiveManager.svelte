<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import { members } from "$lib/state/members.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { ChatSummary } from "$lib/utils/models";
  import type { ArchiveReport as Report } from "$lib/utils/wire";
  let chats = $state<ChatSummary[]>([]);
  let chat = $state("");
  let busy = $state(false);
  let error = $state<LocalizedError | string>("");
  let result = $state<{ report: Report; restored: boolean } | null>(null);
  async function loadChats(event: Event) {
    if (!(event.currentTarget as HTMLDetailsElement).open) return;
    try { chats = await invoke<ChatSummary[]>("chats"); } catch { chats = []; }
  }
  async function run(restore: boolean) {
    if (busy) return;
    busy = true;
    error = ""; result = null;
    try {
      const report = restore ? await invoke<Report | null>("restore_local_backup")
        : await invoke<Report | null>("export_archive", { chat: chat || null });
      if (!report) return;
      result = { report, restored: restore };
      if (restore) await session.loadAccounts();
    } catch (failure) { error = normalizeError(failure); }
    finally { busy = false; }
  }
</script>

<details class="archives" ontoggle={loadChats}>
  <summary><span>{t("settings.archive_title")}</span><span class="chev"><Icon name="chevronDown" size={16} /></span></summary>
  <p>{t("settings.archive_hint")}</p>
  <label>{t("settings.archive_scope")}
    <select bind:value={chat} disabled={busy}>
      <option value="">{t("settings.archive_whole")}</option>
      {#each chats as item (item.chat)}<option value={item.chat}>{members.displayName(item.display_name, item.chat)}</option>{/each}
    </select>
  </label>
  <div class="actions">
    <button class="button" disabled={busy} onclick={() => run(false)}>{chat ? t("settings.archive_export") : t("settings.archive_backup")}</button>
    <button class="button" disabled={busy} onclick={() => run(true)}>{t("settings.archive_restore")}</button>
  </div>
  <p>{t("settings.archive_private_hint")}</p>
  {#if busy}<p role="status">{t("ui.working")}</p>{/if}
  {#if result}<p role="status">{t("settings.archive_report", { action: t(result.restored ? "settings.archive_restored" : "settings.archive_exported"), messages: t("settings.archive_messages", { count: result.report.messages }), attachments: t("settings.archive_attachments", { count: result.report.attachments }) })} <bdi>{result.report.directory}</bdi>{#if result.report.missing_attachments}{t("settings.archive_missing", { count: result.report.missing_attachments })}{/if}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</details>

<style>
  .archives { margin-top: 1rem; padding-top: 1rem; }
  summary { display: flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 600; list-style: none; user-select: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: grid; margin-inline-start: auto; color: var(--muted); transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease); }
  details[open] .chev { transform: rotate(180deg); }
  p { font-size: .85rem; color: var(--muted); overflow-wrap: anywhere; }
  label { display: grid; gap: .4rem; }
  select { padding: .5rem; background: var(--raised); color: var(--text); border: 1px solid var(--line-strong); }
  .actions { display: flex; flex-wrap: wrap; gap: .5rem; margin-top: .75rem; }
  [role="alert"] { color: var(--danger); }
</style>
