<!-- Who is typing in the open chat, as a bubble at the end of the list.
  Moved out of +page.svelte. The list precomputes each typer's label and hue;
  only pictures resolve here. -->
<script lang="ts">
  import Avatar from "$lib/Avatar.svelte";
  import Icon from "$lib/Icon.svelte";
  import TypingDots from "$lib/TypingDots.svelte";

  export type Typer = { sender: string; state: string; label: string; hue: number };

  let {
    typers,
    isGroup,
    avatarOf,
  }: {
    typers: Typer[];
    isGroup: boolean;
    /** A JID's cached picture path, fetched on first use. */
    avatarOf: (jid: string) => string | null;
  } = $props();

  const shown = $derived(typers.slice(0, 3));
</script>

<div class="bubble typing-bubble first" style="--hue: {typers[0].hue}">
  {#if isGroup}
    {#each shown as typer (typer.sender)}
      <div class="typing-row" style="--hue: {typer.hue}">
        <span class="sender-avatar"
          ><Avatar src={avatarOf(typer.sender)} label={typer.label} seed={typer.sender} /></span
        >
        <span class="typer">{typer.label}</span>
        {#if typer.state === "recording"}
          <span class="recording"><Icon name="mic" size={15} /> recording audio…</span>
        {:else}
          <TypingDots />
        {/if}
      </div>
    {/each}
    {#if typers.length > shown.length}
      <span class="typing-more">and {typers.length - shown.length} more…</span>
    {/if}
  {:else if typers[0].state === "recording"}
    <span class="recording"><Icon name="mic" size={15} /> recording audio…</span>
  {:else}
    <TypingDots />
  {/if}
</div>

<style>
  .bubble {
    flex-shrink: 0;
    align-self: flex-start;
    position: relative;
    max-width: 65%;
    min-width: 0;
    background: var(--bubble);
    border-radius: var(--radius-sm);
    border-top-left-radius: 0;
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
    margin-left: var(--pad-l);
    margin-right: var(--pad-r);
  }
  .typing-bubble {
    margin-top: 10px;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  /* The tail marks the run, as on other people's bubbles. */
  .typing-bubble::before {
    content: "";
    position: absolute;
    top: 0;
    width: 9px;
    height: 13px;
    background: inherit;
    left: -8px;
    clip-path: polygon(0 0, 100% 0, 100% 100%);
  }
  .typing-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .typer {
    font-size: 12.8px;
    font-weight: 500;
    line-height: 22px;
    color: hsl(var(--hue) 65% 68%);
  }
  .typing-more {
    color: var(--muted);
    font-size: 12.8px;
    padding-left: 2px;
  }
  .recording {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    font-size: 13px;
  }
</style>
