<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { onMount } from "svelte";
  import { slashCommandKey, slashCommands, type SlashCommandId, type SlashSelection, type SlashToken } from "$lib/utils/slash-commands";

  let { input, token, account, chat, generation, disabled = {
    location: t("content.location_sending_is_unavailable"), "keep-in-chat": t("content.keep_in_chat_is_unavailable"),
  }, onchoose, onclose }: {
    input: HTMLTextAreaElement | undefined;
    token: SlashToken;
    account: string | null;
    chat: string | null;
    generation: number;
    disabled?: Partial<Record<SlashCommandId, string>>;
    onchoose: (selection: SlashSelection) => void;
    onclose: () => void;
  } = $props();
  let owner = $state<{ account: string; chat: string; generation: number } | null>(null);
  let index = $state(0);
  const active = $derived(!!owner && owner.account === account && owner.chat === chat && owner.generation === generation);
  const options = $derived(slashCommands(token.query));
  const enabled = $derived(options.filter((command) => !disabled[command.id]));
  const selected = $derived(enabled[index]?.id ?? null);
  onMount(() => { if (account && chat) owner = { account, chat, generation }; });
  $effect(() => { void token.raw; void enabled; index = 0; });
  $effect(() => { if (owner && !active) onclose(); });

  function current() {
    return active && input && input.selectionStart === token.end && input.selectionEnd === token.end
      && input.value.slice(token.start, token.end) === token.raw;
  }
  function choose(command: SlashCommandId) {
    if (!current() || !owner || disabled[command]) return;
    onchoose({ command, token: { ...token }, ...owner });
  }
  function onKey(event: KeyboardEvent) {
    if (!current() || event.keyCode === 229) return;
    const next = slashCommandKey(event, index, enabled.length);
    if (next === null) return;
    event.preventDefault(); event.stopImmediatePropagation();
    if (next === "close") onclose();
    else if (next === "choose" && selected) choose(selected);
    else if (typeof next === "number") index = next;
  }
  $effect(() => {
    if (!input || !active) return;
    const target = input;
    target.addEventListener("keydown", onKey, true);
    return () => target.removeEventListener("keydown", onKey, true);
  });
  $effect(() => {
    if (!input || !active) return;
    const target = input, controls = target.getAttribute("aria-controls"), descendant = target.getAttribute("aria-activedescendant");
    target.setAttribute("aria-controls", "slash-command-menu");
    if (selected) target.setAttribute("aria-activedescendant", `slash-command-${selected}`);
    else target.removeAttribute("aria-activedescendant");
    return () => {
      if (target.getAttribute("aria-controls") !== "slash-command-menu") return;
      if (controls === null) target.removeAttribute("aria-controls"); else target.setAttribute("aria-controls", controls);
      if (descendant === null) target.removeAttribute("aria-activedescendant"); else target.setAttribute("aria-activedescendant", descendant);
    };
  });
</script>

{#if active}
  <div id="slash-command-menu" class="slash-menu" role="listbox" aria-label={t("content.slash_commands")}>
    {#each options as command (command.id)}
      <button type="button" role="option" tabindex="-1" id="slash-command-{command.id}" disabled={!!disabled[command.id]}
        aria-selected={command.id === selected} class:active={command.id === selected}
        onmousedown={(event) => event.preventDefault()} onmouseenter={() => { const at = enabled.findIndex((option) => option.id === command.id); if (at >= 0) index = at; }}
        onclick={() => choose(command.id)}>
        <span class="name">/{command.id}</span><span>{disabled[command.id] ?? command.description}</span>
      </button>
    {/each}
    {#if options.length === 0}<p>{t("content.no_commands_match")}{token.query}.</p>{/if}
  </div>
{/if}

<style>
  .slash-menu { max-height: 280px; overflow-y: auto; padding: 5px; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); }
  button { display: flex; width: 100%; align-items: center; gap: 16px; padding: 9px 12px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--muted); text-align: start; font: inherit; cursor: pointer; }
  button.active, button:hover:not(:disabled) { background: var(--raised); }
  button:disabled { opacity: .5; cursor: default; }
  .name { min-width: 112px; color: var(--text); }
  p { padding: 8px 12px; margin: 0; color: var(--muted); }
</style>
