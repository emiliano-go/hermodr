<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import type { QuickReply } from "$lib/utils/wire";
  import { filterQuickReplies, quickReplyScopeMatches, type QuickReplyScope } from "$lib/utils/quick-replies";

  let { account, chat, generation, requestKey, dataScope, replies, loading = false, syncing = false, error = null,
    connected, disabled = false, onselect, onsync }: {
    account: string | null; chat: string; generation: number; requestKey: string | number; dataScope: QuickReplyScope | null;
    replies: readonly QuickReply[]; loading?: boolean; syncing?: boolean; error?: string | null; connected: boolean; disabled?: boolean;
    onselect: (scope: QuickReplyScope, reply: QuickReply) => void;
    onsync: (scope: QuickReplyScope) => void;
  } = $props();

  const id = $props.id();
  let dialog: HTMLDialogElement;
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();
  let mounted = true;
  let openedScope = $state.raw<QuickReplyScope | null>(null);
  let query = $state("");
  let selected = $state(0);
  let composing = $state(false);
  const active = $derived(quickReplyScopeMatches(openedScope, account, chat, generation, requestKey));
  const ready = $derived(active && quickReplyScopeMatches(dataScope, account, chat, generation, requestKey));
  const rows = $derived(ready ? filterQuickReplies(replies, query) : []);
  const choices = $derived(rows.filter((reply) => reply.message.trim()));
  const indices = $derived(new Map(choices.map((reply, index) => [reply.id, index] as const)));
  const chosen = $derived(choices[selected]?.id ?? null);

  function current(scope: QuickReplyScope) {
    return mounted && scope === openedScope && quickReplyScopeMatches(scope, account, chat, generation, requestKey) && active;
  }

  function open(event: MouseEvent) {
    if (!account || !chat || disabled) return;
    const scope = { account, chat, generation, requestKey };
    openedScope = scope;
    query = "";
    selected = 0;
    composing = false;
    (event.currentTarget as HTMLButtonElement).focus();
    dialog.showModal();
    void tick().then(() => { if (current(scope) && dialog.open) input?.focus(); });
  }

  function close() {
    openedScope = null;
    dialog.close();
  }

  function choose(scope: QuickReplyScope, id: string) {
    if (!current(scope) || !ready || disabled) return;
    const reply = replies.find((reply) => reply.id === id);
    if (!reply?.message.trim()) return;
    close();
    onselect(scope, reply);
  }

  function select(event: MouseEvent) {
    const button = event.currentTarget as HTMLButtonElement;
    if (!openedScope || !list?.contains(button) || !button.dataset.replyId) return;
    choose(openedScope, button.dataset.replyId);
  }

  function sync(scope: QuickReplyScope) {
    if (!current(scope) || !connected || syncing || loading || disabled) return;
    onsync(scope);
  }

  function key(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.isComposing || event.keyCode === 229 || composing) return;
    if (event.key === "Escape") {
      event.preventDefault(); event.stopPropagation(); close(); return;
    }
    if (event.target !== input || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key === "Enter") {
      event.preventDefault(); event.stopPropagation();
      if (openedScope && chosen) choose(openedScope, chosen);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault(); event.stopPropagation();
      if (choices.length) selected = (selected + (event.key === "ArrowDown" ? 1 : -1) + choices.length) % choices.length;
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault(); event.stopPropagation(); selected = event.key === "Home" ? 0 : Math.max(0, choices.length - 1);
    }
  }

  $effect(() => { void query; selected = 0; });
  $effect(() => {
    if (selected >= choices.length) selected = Math.max(0, choices.length - 1);
    if (chosen) list?.querySelector<HTMLElement>(`[data-selected="true"]`)?.scrollIntoView({ block: "nearest" });
  });
  $effect(() => { if (openedScope && (!active || disabled)) close(); });
  onDestroy(() => { mounted = false; });
</script>

<button type="button" class="trigger" title="Quick replies" aria-label="Quick replies" aria-haspopup="dialog"
  aria-controls={id} aria-expanded={active} disabled={!account || !chat || disabled} onclick={open}>
  <Icon name="message" size={18} />
</button>
<dialog bind:this={dialog} {id} aria-labelledby="{id}-title" onkeydown={key}
  oncancel={(event) => { event.preventDefault(); if (!composing) close(); }} onclose={() => { if (!dialog.open) openedScope = null; }}>
  {#if active && openedScope}
    <header><h2 id="{id}-title">Stored quick replies</h2>
      <button type="button" class="close" aria-label="Close quick replies" onclick={close}><Icon name="x" size={18} /></button>
    </header>
    <p class="note">Choose a reply to insert its text into your draft.</p>
    <div class="sync-row"><button type="button" disabled={!connected || loading || syncing || disabled} onclick={() => { if (openedScope) sync(openedScope); }}>
      {syncing ? "Requesting sync…" : "Sync from phone"}
    </button>{#if !connected}<span>Connect this account to sync.</span>{/if}</div>
    <p class="note">Sync requests phone updates. More replies may arrive after the request finishes.</p>
    {#if ready && loading}<p class="note" role="status">Loading stored quick replies…</p>{/if}
    {#if ready && error}<p class="error" role="alert">{error}</p>{/if}
    <input bind:this={input} bind:value={query} type="search" role="combobox" aria-label="Find quick replies"
      oncompositionstart={() => (composing = true)} oncompositionend={() => (composing = false)}
      aria-autocomplete="list" aria-expanded="true" aria-controls="{id}-results"
      aria-activedescendant={chosen ? `${id}-option-${selected}` : undefined}
      placeholder="Search shortcuts, messages or keywords" />
    <ul bind:this={list} id="{id}-results" role="listbox" aria-label="Stored quick replies" aria-busy={ready && loading}>
      {#each rows as reply (reply.id)}
        {@const index = indices.get(reply.id) ?? -1}
        <li role="presentation"><button type="button" role="option" id={index >= 0 ? `${id}-option-${index}` : undefined}
          aria-selected={reply.id === chosen} data-selected={reply.id === chosen} class:active={reply.id === chosen}
          disabled={disabled || !reply.message.trim()} tabindex="-1" data-reply-id={reply.id}
          onmouseenter={(event) => { const at = indices.get(event.currentTarget.dataset.replyId || ""); if (at !== undefined) selected = at; }} onclick={select}>
          <span class="shortcut">{reply.shortcut ? `/${reply.shortcut.replace(/^\/+/, "")}` : "Quick reply"}</span>
          <span class="message">{reply.message || "No text available"}</span>
        </button></li>
      {/each}
    </ul>
    {#if !loading && rows.length === 0}<p class="note" role="status">
      {ready ? query.trim() ? "No quick replies match this search." : "No quick replies stored on this device." : "Quick replies have not loaded for this chat."}
    </p>{/if}
  {/if}
</dialog>

<style>
  button { font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .trigger { display: grid; place-items: center; width: 34px; height: 34px; padding: 0; flex-shrink: 0; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--muted); }
  .trigger:hover:not(:disabled), .close:hover { background: var(--raised); color: var(--text); }
  dialog { width: min(440px, calc(100vw - 24px)); max-height: calc(100dvh - 24px); margin: auto; padding: 18px; box-sizing: border-box; overflow: auto; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: 17px; }
  .close { display: grid; place-items: center; padding: 5px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--muted); }
  .note, .error { margin: 10px 0; font-size: 12px; overflow-wrap: anywhere; }
  .note, .sync-row span { color: var(--muted); }
  .error { color: var(--danger); }
  .sync-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: 12px; }
  .sync-row button { padding: 6px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); color: var(--text); }
  input { width: 100%; min-width: 0; box-sizing: border-box; margin: 4px 0 8px; padding: 8px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--bg); color: var(--text); font: inherit; }
  ul { list-style: none; max-height: min(300px, 45dvh); overflow-y: auto; margin: 0; padding: 0; }
  li button { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; width: 100%; min-width: 0; padding: 10px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--text); text-align: left; }
  li button.active, li button:hover:not(:disabled) { background: var(--raised); }
  .shortcut, .message { width: 100%; overflow-wrap: anywhere; }
  .shortcut { font-weight: 600; font-size: 13px; }
  .message { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; overflow: hidden; white-space: pre-wrap; color: var(--muted); font-size: 12px; }
</style>
