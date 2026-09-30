<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import Spinner from "$lib/ui/Spinner.svelte";
  import { members } from "$lib/state/members.svelte";
  import { storageSize, type StorageReport, type StorageCleanup } from "$lib/utils/storage";
  let open = $state(false);
  let report = $state<StorageReport | null>(null);
  let busy = $state(false);
  let error = $state("");
  let notice = $state("");
  let chat = $state("");
  let order = $state("largest");
  let offset = $state(0);
  let pending = $state<{ action: StorageCleanup; title: string } | null>(null);
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
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function cleanup() {
    if (!pending || busy) return;
    busy = true;
    error = "";
    notice = "";
    try {
      const result = await invoke<import("$lib/utils/wire").CleanupResult>("storage_cleanup", { action: pending.action });
      notice = `Removed ${result.files} ${result.files === 1 ? "file" : "files"} · ${storageSize(result.bytes)}`;
    } catch (e) { error = String(e); }
    finally { pending = null; await refresh(false); }
  }

  $effect(() => { if (open) void untrack(refresh); });
</script>

<details bind:open>
  <summary><span>Storage Manager</span><span class="chev"><Icon name="chevronDown" size={16} /></span></summary>
  <p>Local files in the current media folder. Message text and history stay when attachments are removed.</p>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if notice}<p role="status">{notice}</p>{/if}
  <button disabled={busy} onclick={() => refresh()}>{#if busy}<Spinner />{/if} Refresh storage</button>
  {#if report}
    <dl class="totals">
      <div><dt>Message database</dt><dd>{storageSize(report.database_bytes)}</dd></div>
      <div><dt>Attachments</dt><dd>{storageSize(report.attachment_bytes)}</dd></div>
      <div><dt>Profile picture cache</dt><dd>{storageSize(report.cache_bytes)}</dd></div>
      <div><dt>Other local files</dt><dd>{storageSize(report.other_bytes)}</dd></div>
    </dl>
    <p>Shared files count once in the attachment total. Other files, including saved sticker originals, are left untouched.</p>
    <div class="controls">
      <label>Chat
        <select bind:value={chat} disabled={busy} onchange={() => { offset = 0; void refresh(); }}>
          <option value="">All chats</option>
          {#each report.chats as row (row.chat)}<option value={row.chat}>{members.displayName(row.name, row.chat)}</option>{/each}
        </select>
      </label>
      <button disabled={busy || !chat} onclick={() => { pending = { action: { kind: "chat_media", chat }, title: "Remove this chat's local attachments?" }; }}>Clean chat media…</button>
      <button disabled={busy || report.cache_bytes === 0} onclick={() => { pending = { action: { kind: "cache" }, title: "Clear cached profile pictures?" }; }}>Clear cache…</button>
    </div>
    <div class="table">
      <table aria-label="Usage by chat">
        <thead><tr><th>Chat</th><th>Videos</th><th>Images</th><th>Documents</th><th>Audio</th><th>Total</th></tr></thead>
        <tbody>{#each usages as row (row.chat)}
          <tr><td>{members.displayName(row.name, row.chat)}</td>{#each ["video", "image", "document", "audio"] as kind}<td>{storageSize(row.by_kind[kind] ?? 0)}</td>{/each}<td>{storageSize(row.bytes)}</td></tr>
        {/each}</tbody>
      </table>
    </div>
    {#if !chat && report.chats.length > 100}<p>Showing the 100 largest chats. Select a chat to see its usage.</p>{/if}
    <label>Sort files
      <select bind:value={order} disabled={busy} onchange={() => { offset = 0; void refresh(); }}>
        <option value="largest">Largest first</option><option value="oldest">Oldest first</option>
      </select>
    </label>
    <p>{report.total_files} attachment references · {offset + (report.files.length ? 1 : 0)}–{offset + report.files.length}</p>
    <ul>{#each report.files as file (`${file.chat}/${file.id}/${file.quoted}`)}
      <li>
        <div>{file.filename}<small>{file.quoted ? "Quoted " : ""}{file.kind} · {new Date(file.timestamp * 1000).toLocaleDateString()} · {storageSize(file.bytes)}</small>
          {#if !file.available}<small>Missing or outside current media folder</small>{/if}
        </div>
        <button disabled={busy || !file.available} aria-label={`Delete local ${file.filename}`}
          onclick={() => { pending = { action: { kind: "attachment", chat: file.chat, id: file.id, quoted: file.quoted }, title: `Remove ${file.filename} from this device?` }; }}>Delete…</button>
      </li>
    {/each}</ul>
    <div class="controls">
      <button disabled={busy || offset === 0} onclick={() => { offset -= 50; void refresh(); }}>Previous files</button>
      <button disabled={busy || offset + 50 >= report.total_files} onclick={() => { offset += 50; void refresh(); }}>Next files</button>
    </div>
  {/if}
</details>

{#if pending}
  <ConfirmDialog label="Confirm storage cleanup" title={pending.title}
    hint="This deletes local files. Messages stay. Downloading an attachment again depends on the original still being available."
    onclose={() => { if (!busy) pending = null; }}>
    {#snippet actions()}
      <button disabled={busy} onclick={() => { pending = null; }}>Cancel</button>
      <button disabled={busy} onclick={cleanup}>{busy ? "Removing…" : "Remove local files"}</button>
    {/snippet}
  </ConfirmDialog>
{/if}

<style>
  details { margin-top: 1rem; }
  summary { display: flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 600; list-style: none; user-select: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: grid; margin-left: auto; color: var(--muted); transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease); }
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
  th, td { text-align: left; padding: .35rem; border-bottom: 1px solid var(--line-strong); }
  ul { list-style: none; padding: 0; max-height: 22rem; overflow: auto; }
  li { display: flex; justify-content: space-between; align-items: center; gap: .6rem; padding: .6rem 0; border-bottom: 1px solid var(--line-strong); overflow-wrap: anywhere; }
  small { display: block; }
</style>
