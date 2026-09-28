<script lang="ts">
  import { invoke } from "$lib/ipc";
  import { session } from "$lib/state/session.svelte";
  import type { ChatSummary } from "$lib/models";
  type Report = { directory: string; messages: number; attachments: number; missing_attachments: number };
  let chats = $state<ChatSummary[]>([]);
  let chat = $state("");
  let busy = $state(false);
  let error = $state("");
  let result = $state("");
  async function loadChats(event: Event) {
    if (!(event.currentTarget as HTMLDetailsElement).open) return;
    try { chats = await invoke<ChatSummary[]>("chats"); } catch { chats = []; }
  }
  async function run(restore: boolean) {
    if (busy) return;
    busy = true;
    error = result = "";
    try {
      const report = restore ? await invoke<Report | null>("restore_local_backup")
        : await invoke<Report | null>("export_archive", { chat: chat || null });
      if (!report) return;
      result = `${restore ? "Restored into a separate account" : "Exported"}: ${report.messages} messages, ${report.attachments} attachment${report.attachments === 1 ? "" : "s"}. ${report.directory}`;
      if (report.missing_attachments) result += ` Missing attachments: ${report.missing_attachments}. These files were unavailable when backed up.`;
      if (restore) await session.loadAccounts();
    } catch (failure) { error = String(failure); }
    finally { busy = false; }
  }
</script>

<details class="archives" ontoggle={loadChats}>
  <summary>Export and local backup</summary>
  <p>Export a conversation as JSON with its downloaded attachments, or back up this account’s message store, marks and aliases.</p>
  <label>Export scope
    <select bind:value={chat} disabled={busy}>
      <option value="">Whole account backup</option>
      {#each chats as item (item.chat)}<option value={item.chat}>{item.display_name ?? item.chat}</option>{/each}
    </select>
  </label>
  <div class="actions">
    <button class="button" disabled={busy} onclick={() => run(false)}>{chat ? "Export conversation…" : "Back up account…"}</button>
    <button class="button" disabled={busy} onclick={() => run(true)}>Restore into new account…</button>
  </div>
  <p>Backups contain private messages and media, but no login credentials. Keep the folder private. Restore adds a separate account; existing accounts stay intact. Select “Restored backup” in Accounts and link the same WhatsApp account again. Current disk retention applies after linking.</p>
  {#if busy}<p role="status">Working…</p>{/if}
  {#if result}<p role="status">{result}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</details>

<style>
  .archives { margin-top: 1rem; border-top: 1px solid var(--border); padding-top: 1rem; }
  summary { cursor: pointer; font-weight: 600; }
  p { font-size: .85rem; color: var(--muted); overflow-wrap: anywhere; }
  label { display: grid; gap: .4rem; }
  select { padding: .5rem; background: var(--raised); color: var(--text); border: 1px solid var(--border); }
  .actions { display: flex; flex-wrap: wrap; gap: .5rem; margin-top: .75rem; }
  [role="alert"] { color: #f66; }
</style>
