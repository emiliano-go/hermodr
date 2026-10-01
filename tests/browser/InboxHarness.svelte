<script lang="ts">
  import UnifiedInbox from "../../src/lib/chat/UnifiedInbox.svelte";
  import type { ChatSummary } from "../../src/lib/utils/wire";
  import type { InboxAction } from "../../src/lib/utils/inbox";

  const base: ChatSummary = { chat: "", display_name: null, last_message_at: 1000, last_text: "Synthetic preview", last_from_me: false,
    last_sender_name: null, last_sender: "", last_media_kind: null, message_count: 2, unread_count: 0, mention_count: 0,
    pinned: false, archived: false, muted_until: 0, marked_unread: false };
  let account = $state("alpha");
  let request = $state(0);
  let connected = $state(true);
  let loading = $state(false);
  let loadError = $state(false);
  let available = $state(true);
  let labelsLoading = $state(false);
  let writable = $state(true);
  let empty = $state(false);
  let defer = $state(false);
  let fail = $state(false);
  let pending = $state(10);
  let applied = $state(4);
  let calls = $state<string[]>([]);
  let waiting = $state(0);
  const releases: (() => void)[] = [];
  const makeRows = (prefix: string): ChatSummary[] => [
    { ...base, chat: `${prefix}-alice@s`, display_name: `${prefix} Alice`, unread_count: 2, mention_count: 1 },
    { ...base, chat: `${prefix}-bob@s`, display_name: `${prefix} Bob`, archived: true, muted_until: -1 },
    { ...base, chat: `${prefix}-clara@s`, display_name: `${prefix} Clara`, marked_unread: true, archived: true },
    { ...base, chat: `${prefix}-dara@s`, display_name: `${prefix} Dara` },
    { ...base, chat: `${prefix}-ordinary@s`, display_name: `${prefix} Ordinary` },
  ];
  let records = $state<Record<string, ChatSummary[]>>({ alpha: makeRows("alpha"), beta: makeRows("beta") });
  let associations = $state<Record<string, string[]>>({ "alpha-alice@s": ["work"], "alpha-bob@s": ["work"], "alpha-dara@s": ["personal"],
    "beta-alice@s": ["work"], "beta-bob@s": ["work"], "beta-dara@s": ["personal"] });

  async function action(owner: string, chat: string, action: InboxAction) {
    calls = [...calls, `${owner} ${chat} ${JSON.stringify(action)}`];
    const refused = fail;
    if (defer) await new Promise<void>((resolve) => { releases.push(resolve); waiting = releases.length; });
    if (refused) throw new Error(`Synthetic action refusal for ${owner}`);
    if (action.kind === "label") {
      const previous = associations[chat] ?? [];
      associations = { ...associations, [chat]: action.applied ? [...new Set([...previous, action.label])] : previous.filter((id) => id !== action.label) };
    } else {
      records = { ...records, [owner]: records[owner].map((row) => row.chat !== chat ? row : action.kind === "read"
        ? { ...row, unread_count: action.read ? 0 : row.unread_count, mention_count: action.read ? 0 : row.mention_count, marked_unread: !action.read }
        : action.kind === "archive" ? { ...row, archived: action.archived }
        : { ...row, muted_until: action.seconds < 0 ? -1 : action.seconds === 0 ? 0 : Math.floor(Date.now() / 1000) + action.seconds }) };
    }
  }
  function release() { const next = releases.shift(); waiting = releases.length; next?.(); }
</script>

<h1>Synthetic inbox</h1>
<div class="controls">
  <button onclick={() => (account = account === "alpha" ? "beta" : "alpha")}>Switch account</button>
  <button onclick={() => request++}>Bump request</button>
  <button onclick={release} disabled={!waiting}>Release actions {waiting}</button>
  <label><input type="checkbox" bind:checked={connected} /> Connected</label>
  <label><input type="checkbox" bind:checked={loading} /> Loading</label>
  <label><input type="checkbox" bind:checked={loadError} /> Load error</label>
  <label><input type="checkbox" bind:checked={available} /> Labels available</label>
  <label><input type="checkbox" bind:checked={labelsLoading} /> Labels loading</label>
  <label><input type="checkbox" bind:checked={writable} /> Labels writable</label>
  <label><input type="checkbox" bind:checked={empty} /> Empty chats</label>
  <label><input type="checkbox" bind:checked={defer} /> Defer actions</label>
  <label><input type="checkbox" bind:checked={fail} /> Fail actions</label>
  <button onclick={() => { pending = 0; applied = 0; }}>Finish catch-up</button>
</div>
<pre aria-label="Synthetic operations">{calls.join("\n")}</pre>
<UnifiedInbox {account} requestKey={request} {connected} chats={empty ? [] : records[account]} {loading}
  error={loadError ? "Synthetic inbox refresh failure" : ""} labels={available ? [{ id: "work", name: "Work" }, { id: "personal", name: "Personal" }] : null}
  labelsByChat={associations} labelsWritable={writable} {labelsLoading}
  chatLabelOf={(chat) => chat.display_name ?? chat.chat} formatTime={(timestamp) => `${timestamp}s`}
  syncPending={pending} syncApplied={applied} onaction={action} onopen={(chat, mention) => (calls = [...calls, `open ${account} ${chat} ${!!mention}`])}
  onretry={() => (loadError = false)} />

<style>
  :global(body) { font-family: sans-serif; max-width: 860px; margin: 20px auto; --bg: #fff; --surface: #f3f4f5; --raised-2: #e7ebee; --text: #222; --muted: #555;
    --line: #ddd; --line-strong: #aaa; --radius-sm: 5px; --accent: #067a68; --accent-text: #05695b; --danger: #a00; }
  .controls { display: flex; flex-wrap: wrap; gap: 10px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
