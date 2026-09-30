<script lang="ts">
  import AddMembers from "$lib/chat/AddMembers.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { GroupHistoryResult, GroupMemberAddResult } from "$lib/utils/models";
  import { historyFixture } from "./ipc";

  session.activeAccount = "history-account-a";
  let opened = $state(false);
  let chat = $state("1@g.us");
  let delayed = $state(false);
  let release = $state<(() => void) | null>(null);
  let operations = $state<{ action: string; chat: string; account: string | null; jids?: string[]; optedIn?: string[]; retryId?: string }[]>([]);
  let offers = $state("");

  async function add(jids: string[], optedIn: string[]): Promise<GroupMemberAddResult> {
    operations.push({ action: "add", chat, account: session.activeAccount, jids: [...jids], optedIn: [...optedIn] });
    if (delayed) await new Promise<void>((resolve) => { release = resolve; });
    return {
      participants: jids.map((jid) => ({ jid, ok: jid !== "101@s.whatsapp.net", pending: jid === "102@s.whatsapp.net", code: jid === "101@s.whatsapp.net" ? "403" : null, error: null })),
      history: optedIn.length > 0
        ? { state: "upload_failed", message: "Synthetic upload failed. Participant additions are unchanged.", retry_id: "retained-history-id" }
        : { state: "not_requested", message: "History was not requested.", retry_id: null },
    };
  }

  async function retry(retryId: string): Promise<GroupHistoryResult> {
    operations.push({ action: "retry", chat, account: session.activeAccount, retryId });
    if (delayed) await new Promise<void>((resolve) => { release = resolve; });
    return { state: "shared", message: "Synthetic server accepted history.", retry_id: null };
  }

  function reset() {
    opened = false;
    session.activeAccount = "history-account-a";
    chat = "1@g.us";
    delayed = false;
    release = null;
    operations = [];
    offers = "";
    historyFixture.enabled = true;
    historyFixture.delayNext = false;
    historyFixture.pending = [];
    historyFixture.offers = [];
  }
</script>

<div class="controls">
  <button onclick={reset}>Reset fixture</button>
  <button onclick={() => { opened = true; }}>Open member picker</button>
  <button onclick={() => { delayed = !delayed; }}>Toggle delayed operation</button>
  <button onclick={() => { release?.(); release = null; }}>Release operation</button>
  <button onclick={() => { historyFixture.enabled = false; }}>Disable next offer</button>
  <button onclick={() => { historyFixture.delayNext = true; }}>Delay next offer</button>
  <button onclick={() => { historyFixture.enabled = false; session.activeAccount = "history-account-b"; }}>Switch account and disable history</button>
  <button onclick={() => { historyFixture.enabled = false; chat = "2@g.us"; }}>Switch group and disable history</button>
  <button onclick={() => { historyFixture.pending.shift()?.(); }}>Release old offer</button>
  <button onclick={() => { offers = JSON.stringify(historyFixture.offers); }}>Show policy reads</button>
  <p>Only synthetic contacts, accounts and results. Active: {session.activeAccount}; group: {chat}; delayed: {String(delayed)}.</p>
  <output aria-label="Group operations">{JSON.stringify(operations)}</output>
  <output aria-label="Policy reads">{offers}</output>
</div>

{#if opened}
  <AddMembers {chat} title="Synthetic group" members={[]} avatars={{}} me="999@s.whatsapp.net"
    onavatar={() => {}} onadd={add} onretryhistory={retry} onclose={() => { opened = false; }} />
{/if}

<style>
  :global(:root) { --bg: #132029; --surface: #1b2c36; --raised: #29414d; --text: #eee; --muted: #b7c8d3; --accent: #2cd4a0; --accent-text: #0c241b; --line-strong: #547080; --scrim: #0008; --radius-lg: 12px; --shadow: 0 8px 40px #0008; }
  :global(body) { font: 14px system-ui; background: var(--bg); color: var(--text); }
  .controls { position: relative; z-index: 300; padding: 12px; background: var(--bg); }
  button { padding: 4px 8px; margin: 2px; }
  p { margin: 4px; }
  output { display: block; overflow-wrap: anywhere; font-size: 12px; }
</style>
