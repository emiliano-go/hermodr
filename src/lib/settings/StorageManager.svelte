<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t, formatDate, type MessageParams } from "$lib/i18n/localizer";
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import Spinner from "$lib/ui/Spinner.svelte";
  import { members } from "$lib/state/members.svelte";
  import { storageSize, type StorageReport, type StorageCleanup } from "$lib/utils/storage";
  const fileKind = (kind: string) => t(({ image: "settings.storage_kind_image", video: "settings.storage_kind_video", audio: "settings.storage_kind_audio", document: "settings.storage_kind_document", sticker: "settings.storage_kind_sticker", gif: "settings.storage_kind_gif" } as Record<string, string>)[kind] ?? "settings.storage_kind_unknown");
  let open = $state(false);
  let report = $state<StorageReport | null>(null);
  let busy = $state(false);
  let error = $state<LocalizedError | string>("");
  let notice = $state<import("$lib/utils/wire").CleanupResult | null>(null);
  let chat = $state("");
  let order = $state("largest");
  let offset = $state(0);
  let pending = $state<{ action: StorageCleanup; title: string; params?: MessageParams } | null>(null);
  const usages = $derived((report?.chats ?? []).filter((row) => !chat || row.chat === chat)
    .toSorted((a, b) => b.bytes - a.bytes).slice(0, 100));

  async function refresh(clearError = true) {
    busy = true;
    if (clearError) error = "";
    try {
      report = await invoke<StorageReport>("storage_report", { chat: chat || null, order, offset });
      if (offset > 0 && offset >= report.total_files) {
        offset = Math.max(0, Math.floor((report.total_files - 1) / 50) * 50);
        report = await invoke<StorageReport>("storage_report", { chat: chat || null, order, offset });
      }
    } catch (e) { error = normalizeError(e); }
    finally { busy = false; }
  }

  async function cleanup() {
    if (!pending || busy) return;
    busy = true;
    error = "";
    notice = null;
    try {
      const result = await invoke<import("$lib/utils/wire").CleanupResult>("storage_cleanup", { action: pending.action });
      notice = result;
    } catch (e) { error = normalizeError(e); }
    finally { pending = null; await refresh(false); }
  }

  $effect(() => { if (open) void untrack(refresh); });
</script>

<details bind:open>
  <summary><span>{t("settings.storage_manager")}</span><span class="chev"><Icon name="chevronDown" size={16} /></span></summary>
  <p>{t("settings.storage_hint")}</p>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{t("settings.storage_removed", { count: notice.files, size: storageSize(notice.bytes) })}</p>{/if}
  <button disabled={busy} onclick={() => refresh()}>{#if busy}<Spinner />{/if} {t("settings.storage_refresh")}</button>
  {#if report}
    <dl class="totals">
      <div><dt>{t("settings.storage_database")}</dt><dd>{storageSize(report.database_bytes)}</dd></div>
      <div><dt>{t("settings.storage_attachments")}</dt><dd>{storageSize(report.attachment_bytes)}</dd></div>
      <div><dt>{t("settings.storage_profiles")}</dt><dd>{storageSize(report.cache_bytes)}</dd></div>
      <div><dt>{t("settings.storage_other")}</dt><dd>{storageSize(report.other_bytes)}</dd></div>
    </dl>
    <p>{t("settings.storage_count_hint")}</p>
    <div class="controls">
      <label>{t("chat.chat")}
        <select bind:value={chat} disabled={busy} onchange={() => { offset = 0; void refresh(); }}>
          <option value="">{t("chat.all_chats")}</option>
          {#each report.chats as row (row.chat)}<option value={row.chat}>{members.displayName(row.name, row.chat)}</option>{/each}
        </select>
      </label>
      <button disabled={busy || !chat} onclick={() => { pending = { action: { kind: "chat_media", chat }, title: "settings.storage_chat_confirm" }; }}>{t("settings.storage_clean_chat")}</button>
      <button disabled={busy || report.cache_bytes === 0} onclick={() => { pending = { action: { kind: "cache" }, title: "settings.storage_cache_confirm" }; }}>{t("settings.storage_clear_cache")}</button>
    </div>
    <div class="table">
      <table aria-label={t("settings.storage_by_chat")}>
        <thead><tr><th>{t("chat.chat")}</th><th>{t("settings.storage_videos")}</th><th>{t("settings.storage_images")}</th><th>{t("settings.storage_documents")}</th><th>{t("settings.storage_audio")}</th><th>{t("ui.total")}</th></tr></thead>
        <tbody>{#each usages as row (row.chat)}
          <tr><td>{members.displayName(row.name, row.chat)}</td>{#each ["video", "image", "document", "audio"] as kind}<td>{storageSize(row.by_kind[kind] ?? 0)}</td>{/each}<td>{storageSize(row.bytes)}</td></tr>
        {/each}</tbody>
      </table>
    </div>
    {#if !chat && report.chats.length > 100}<p>{t("settings.storage_largest_hint")}</p>{/if}
    <label>{t("settings.storage_sort")}
      <select bind:value={order} disabled={busy} onchange={() => { offset = 0; void refresh(); }}>
        <option value="largest">{t("settings.storage_largest_first")}</option><option value="oldest">{t("settings.storage_oldest_first")}</option>
      </select>
    </label>
    <p>{t("settings.storage_references", { count: report.total_files, start: offset + (report.files.length ? 1 : 0), end: offset + report.files.length })}</p>
    <ul>{#each report.files as file (`${file.chat}/${file.id}/${file.quoted}`)}
      <li>
        <div><bdi>{file.filename}</bdi><small>{file.quoted ? t("settings.storage_quoted", { kind: fileKind(file.kind) }) : fileKind(file.kind)} · {formatDate(file.timestamp)} · {storageSize(file.bytes)}</small>
          {#if !file.available}<small>{t("settings.storage_missing")}</small>{/if}
        </div>
        <button disabled={busy || !file.available} aria-label={t("settings.storage_file_delete", { name: file.filename })}
          onclick={() => { pending = { action: { kind: "attachment", chat: file.chat, id: file.id, quoted: file.quoted }, title: "settings.storage_file_confirm", params: { name: file.filename } }; }}>{t("ui.delete_more")}</button>
      </li>
    {/each}</ul>
    <div class="controls">
      <button disabled={busy || offset === 0} onclick={() => { offset -= 50; void refresh(); }}>{t("settings.storage_previous")}</button>
      <button disabled={busy || offset + 50 >= report.total_files} onclick={() => { offset += 50; void refresh(); }}>{t("settings.storage_next")}</button>
    </div>
  {/if}
</details>

{#if pending}
  <ConfirmDialog label={t("settings.storage_confirm_title")} title={t(pending.title, pending.params)}
    hint={t("settings.storage_confirm_hint")}
    onclose={() => { if (!busy) pending = null; }}>
    {#snippet actions()}
      <button disabled={busy} onclick={() => { pending = null; }}>{t("ui.cancel")}</button>
      <button disabled={busy} onclick={cleanup}>{busy ? t("ui.removing") : t("settings.storage_remove")}</button>
    {/snippet}
  </ConfirmDialog>
{/if}

<style>
  details { margin-top: 1rem; }
  summary { display: flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 600; list-style: none; user-select: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: grid; margin-inline-start: auto; color: var(--muted); transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease); }
  details[open] .chev { transform: rotate(180deg); }
  p, small { font-size: .8rem; color: var(--muted); }
  .totals { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: .8rem; }
  dd { margin: .3rem 0; font-size: 1.15rem; }
  .controls { display: flex; gap: .5rem; flex-wrap: wrap; align-items: end; margin: .7rem 0; }
  label { display: grid; gap: .3rem; }
  button, select { border: 1px solid var(--line-strong); background: var(--raised); color: var(--text); padding: .45rem; border-radius: .3rem; }
  button:disabled { opacity: .5; }
  .table { overflow: auto; }
  table { border-collapse: collapse; font-size: .8rem; width: 100%; margin: .8rem 0; }
  th, td { text-align: start; padding: .35rem; border-bottom: 1px solid var(--line-strong); }
  ul { list-style: none; padding: 0; max-height: 22rem; overflow: auto; }
  li { display: flex; justify-content: space-between; align-items: center; gap: .6rem; padding: .6rem 0; border-bottom: 1px solid var(--line-strong); overflow-wrap: anywhere; }
  small { display: block; }
</style>
