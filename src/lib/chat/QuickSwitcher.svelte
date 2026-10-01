<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { members } from "$lib/state/members.svelte";
  import { plain } from "$lib/utils/format";
  import { quickChats, quickSwitcherKey, messageSnippet, type QuickChat, type QuickSwitchTarget } from "$lib/utils/quick-switcher";
  import type { ChatSummary, SearchResult, StoredMessage } from "$lib/utils/wire";

  let { account, chats, onload, onmessages, onchoose, onclose }: {
    account: string;
    chats: ChatSummary[];
    onload: () => Promise<SearchResult[]>;
    onmessages: (query: string) => Promise<StoredMessage[]>;
    onchoose: (target: QuickSwitchTarget) => void | Promise<void>;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let input: HTMLInputElement;
  let list: HTMLUListElement;
  const openedAccount = untrack(() => account);
  let mounted = true;
  const current = () => mounted && account === openedAccount;
  let query = $state("");
  let directory = $state.raw<SearchResult[]>([]);
  let messages = $state.raw<StoredMessage[]>([]);
  let messageQuery = $state("");
  let selected = $state(0);
  let loading = $state(true);
  let searching = $state(false);
  let busy = $state(false);
  let catalogError = $state<string | null>(null);
  let error = $state<string | null>(null);

  const recent = $derived.by((): QuickChat[] => [...chats].sort((a, b) => b.last_message_at - a.last_message_at).map((chat) => ({
    jid: chat.chat, name: chat.display_name ?? chat.chat.split("@")[0], number: chat.chat.split("@")[0],
    kind: chat.chat.endsWith("@newsletter") ? "channel" : chat.chat.endsWith("@g.us") ? "group" : "chat",
    aliases: members.aliasesFor(chat.chat),
  })));
  const chatMatches = $derived(quickChats(recent, directory, query));
  const rows = $derived(account === openedAccount ? [
    ...chatMatches.map((chat) => ({ key: `chat:${chat.jid}`, chat: chat.jid, label: chat.name,
      title: members.displayName(chat.name, chat.jid), messageId: undefined as string | undefined,
      kind: chat.jid.endsWith("@newsletter") ? "channel" : chat.kind, snippet: "" })),
    ...(query.trim() && messageQuery === query.trim() ? messages : []).map((message) => {
      const label = chats.find((chat) => chat.chat === message.chat)?.display_name ?? message.chat.split("@")[0];
      return { key: JSON.stringify([message.chat, message.id]), chat: message.chat, messageId: message.id,
        label, title: members.displayName(label, message.chat), kind: "message",
        snippet: message.spoiler ? "[Spoiler]" : messageSnippet(plain(message.text, (user) => members.mentionName(user)), query),
      };
    }),
  ] : []);

  onMount(() => {
    dialog.showModal();
    input.focus();
    void onload().then((found) => {
      if (current()) { directory = found; selected = 0; }
    }).catch((failure) => { if (current()) catalogError = String(failure); })
      .finally(() => { if (current()) loading = false; });
    return () => { mounted = false; dialog.close(); };
  });

  $effect(() => { if (account !== openedAccount) close(); });
  $effect(() => {
    const value = query.trim();
    let active = true;
    selected = 0;
    messages = [];
    messageQuery = "";
    searching = !!value;
    error = null;
    if (!value) return;
    const timer = setTimeout(async () => {
      try {
        const found = await onmessages(value);
        if (active && current()) { messages = found.slice(0, 50); messageQuery = value; }
      } catch (failure) {
        if (active && current()) error = `Could not search messages: ${String(failure)}`;
      } finally {
        if (active && current()) searching = false;
      }
    }, 180);
    return () => { active = false; clearTimeout(timer); };
  });
  $effect(() => {
    if (selected >= rows.length) selected = Math.max(0, rows.length - 1);
    list?.children[selected]?.firstElementChild?.scrollIntoView({ block: "nearest" });
  });

  async function choose(target: QuickSwitchTarget) {
    if (busy || !current()) return;
    busy = true;
    error = null;
    try {
      await onchoose(target);
      if (current()) close();
    } catch (failure) {
      if (current()) error = String(failure);
    } finally { if (current()) busy = false; }
  }

  function close() {
    dialog.close();
    onclose();
  }

  function key(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter" && event.target !== input) return;
    const action = quickSwitcherKey(event.key, selected, rows.length);
    if (action === null) return;
    event.preventDefault();
    event.stopPropagation();
    if (action === "close") close();
    else if (action === "choose") void choose(rows[selected]);
    else { selected = action; input.focus(); }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="quick-switcher-title" onkeydown={key}
  oncancel={(event) => { event.preventDefault(); close(); }}>
  <header>
    <h2 id="quick-switcher-title">Quick switcher</h2>
    <button type="button" class="close" aria-label="Close quick switcher" onclick={close}>×</button>
  </header>
  <input bind:this={input} bind:value={query} role="combobox" aria-label="Find chats and messages"
    aria-autocomplete="list" aria-expanded="true" aria-controls="quick-switcher-results"
    aria-activedescendant={rows[selected] ? `quick-switcher-option-${selected}` : undefined}
    placeholder="Find chats, contacts, groups, channels or messages" disabled={busy} />
  <p class="heading">{query.trim() ? "Chats and messages" : "Recent chats"}</p>
  <ul bind:this={list} id="quick-switcher-results" role="listbox" aria-label="Quick switcher results" aria-busy={loading || searching}>
    {#each rows as row, index (row.key)}
      <li role="presentation">
        <button id={`quick-switcher-option-${index}`} type="button" role="option" tabindex="-1"
          aria-selected={selected === index} class:active={selected === index} disabled={busy}
          onmouseenter={() => (selected = index)} onclick={() => void choose(row)}>
          <span class="label">{row.title}</span><span class="kind">{row.kind}</span>
          {#if row.snippet}<span class="snippet">{row.snippet}</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
  {#if loading || searching}<p class="status" role="status">{searching ? "Searching messages…" : "Loading contacts…"}</p>{/if}
  {#if !rows.length && !loading && !searching}<p class="status">{query.trim() ? "No matches found." : "No recent chats yet."}</p>{/if}
  {#if catalogError}<p class="error" role="alert">Could not load contacts: {catalogError}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <footer><span>↑ ↓ Move</span><span>Enter Open</span><span>Esc Close</span></footer>
</dialog>

<style>
  dialog { width: min(580px, calc(100vw - 32px)); max-height: calc(100vh - 64px); padding: 16px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--bg); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
  h2 { margin: 0; font-size: 16px; font-weight: 600; }
  .close { width: 28px; height: 28px; border: 0; border-radius: 50%; background: transparent; color: var(--muted); font: inherit; font-size: 20px; cursor: pointer; }
  .close:hover { background: var(--raised); color: var(--text); }
  input { box-sizing: border-box; width: 100%; padding: 10px 12px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--surface); color: var(--text); font: inherit; }
  .heading { margin: 12px 4px 6px; color: var(--muted); font-size: 12px; }
  ul { max-height: min(420px, 55vh); margin: 0; padding: 0; overflow-y: auto; list-style: none; }
  li button { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 12px; box-sizing: border-box; width: 100%; padding: 10px 12px; border: 0; border-radius: 6px; background: transparent; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
  li button.active { background: var(--raised); }
  .label, .snippet { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kind, .snippet, footer, .status { color: var(--muted); font-size: 12px; }
  .kind { text-transform: capitalize; }
  .snippet { grid-column: 1 / -1; }
  .status, .error { margin: 10px 4px; }
  .error { color: var(--danger); font-size: 12px; }
  footer { display: flex; gap: 18px; margin-top: 12px; }
</style>
