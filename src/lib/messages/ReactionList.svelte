<!-- Who reacted to one message, a section per emoji. Opened from the bubble's
     menu or by clicking the pill under a message. Your own section carries an
     X that takes your reaction back, since the pill no longer does that. -->
<script lang="ts" module>
  export type Reactor = { jid: string; label: string; avatar: string | null; self: boolean };
  export type ReactorGroup = { emoji: string; people: Reactor[] };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Icon from "$lib/ui/Icon.svelte";

  let {
    groups,
    onprofile,
    onremove,
    onclose,
  }: {
    /** In the order the emoji first arrived; ours leads its own group. */
    groups: ReactorGroup[];
    /** Opens someone's contact card at the click, as the member lists do. */
    onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
    /** Takes back our own reaction; a sender only ever holds one. */
    onremove: () => void;
    onclose: () => void;
  } = $props();

  const total = $derived(groups.reduce((n, g) => n + g.people.length, 0));
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte:ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Reactions"
    transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      <h2>Reactions{#if total}<span class="count">{total}</span>{/if}</h2>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    {#if total === 0}
      <p class="empty">Nobody has reacted to this message.</p>
    {:else}
      <div class="body">
        {#each groups as group (group.emoji)}
          <section>
            <h3>
              <span class="emoji">{group.emoji}</span><span class="n">{group.people.length}</span>
              {#if group.people.some((p) => p.self)}
                <button
                  class="remove"
                  title="Remove your reaction"
                  aria-label="Remove your reaction"
                  onclick={onremove}><Icon name="x" size={14} /></button>
              {/if}
            </h3>
            <ul>
              {#each group.people as person (person.jid)}
                <li>
                  <button
                    class="reactor-row"
                    title="Profile of {person.label}"
                    aria-label="Profile of {person.label}"
                    onclick={(e) => onprofile(person.jid, person.label, e, person.self)}>
                    <Avatar cls="reactor" src={person.avatar} label={person.label} seed={person.jid} />
                    <span class="who">{person.label}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 275;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }
  .dialog {
    width: min(400px, 92vw);
    max-height: min(560px, 80vh);
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 12px 8px 20px;
  }
  h2 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .count {
    color: var(--muted);
    font-size: 13.5px;
    font-weight: 400;
  }
  .close {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--raised);
    color: var(--text);
  }
  .empty {
    margin: 0;
    padding: 24px 12px 32px;
    text-align: center;
    color: var(--muted);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px 8px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 8px 12px 4px;
    color: var(--muted);
    font-size: 12.5px;
    font-weight: 600;
  }
  .emoji {
    font-size: 15px;
  }
  .remove {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-left: auto;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .remove:hover {
    background: var(--raised);
    color: var(--text);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .reactor-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 5px 12px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background calc(0.12s * var(--motion-scale));
  }
  .reactor-row:hover {
    background: var(--surface);
  }
  .who {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14.5px;
  }
  /* Avatar renders this class inside its own template, so it needs the
     unscoped selector to reach it. */
  :global(.reactor) {
    flex: none;
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: 14px;
    font-weight: 500;
    letter-spacing: 0.02em;
    user-select: none;
  }
  :global(img.reactor) {
    object-fit: cover;
    background: var(--raised);
  }
</style>
