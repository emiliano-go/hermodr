<!-- WhatsApp formatting: blocks, inline runs and mention pills, with the
  time's reserved space after the last line. Moved out of +page.svelte. -->
<script lang="ts">
  import Avatar from "$lib/Avatar.svelte";
  import { blocks, type Inline } from "$lib/format";
  import type { MentionTarget } from "$lib/models";

  let {
    text,
    mine,
    toWire,
    targetOf,
    avatarOf,
    onprofile,
    onopenurl,
  }: {
    text: string;
    mine: boolean;
    /** Rewrites `@Name` tokens to the wire's `@<number>` form. */
    toWire: (text: string) => string;
    /** Who an `@<user>` token names. */
    targetOf: (user: string) => MentionTarget;
    /** A JID's cached picture path, fetched on first use. */
    avatarOf: (jid: string) => string | null;
    onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
    onopenurl: (url: string) => void;
  } = $props();
</script>

{#snippet runs(nodes: Inline[])}{#each nodes as n, i (i)}{#if n.kind === "text"}{n.text}{:else if n.kind === "link"}<a
      class="link"
      href={n.url}
      onclick={(e) => {
        e.preventDefault();
        onopenurl(n.url);
      }}>{n.url}</a
    >{:else if n.kind === "code"}<code class="inline-code">{n.text}</code>{:else if n.kind === "mention"}{@render
      mentionPill(n.user)}{:else if n.kind === "bold"}<strong
      >{@render runs(n.children)}</strong
    >{:else if n.kind === "italic"}<em>{@render runs(n.children)}</em>{:else}<s
      >{@render runs(n.children)}</s
    >{/if}{/each}{/snippet}

{#snippet mentionPill(user: string)}{@const target = targetOf(user)}<button
    type="button"
    class="mention-pill"
    class:self={target.self}
    onclick={(e) => onprofile(target.jid, target.name, e, target.self)}
    ondblclick={(e) => e.stopPropagation()}
    ><Avatar
      src={avatarOf(target.jid)}
      label={target.name}
      seed={target.jid}
      cls="mention-initials"
      iconFallback
      iconSize={11}
    />@{target.name}</button
  >{/snippet}

{#snippet lines(list: Inline[][])}{#each list as line, i (i)}{#if i > 0}<br />{/if}{@render runs(line)}{/each}{/snippet}

<span class="text"
  >{#each blocks(toWire(text)) as block, i (i)}{#if block.kind === "pre"}<pre class="pre">{block.text}</pre
      >{:else if block.kind === "quote"}<span class="quote-block">{@render lines(block.lines)}</span
      >{:else if block.kind === "list"}{#if block.ordered}<ol class="fmt-list">
          {#each block.items as item, j (j)}<li>{@render runs(item)}</li>{/each}
        </ol>{:else}<ul class="fmt-list">
          {#each block.items as item, j (j)}<li>{@render runs(item)}</li>{/each}
        </ul>{/if}{:else}{#if i > 0}<br />{/if}{@render lines(block.lines)}{/if}{/each}<span
    class="meta-spacer"
    class:mine></span
  ></span
>

<style>
  .link {
    color: var(--link);
    cursor: pointer;
  }
  /* Keep the spaces the sender typed, and wrap long tokens. */
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .inline-code,
  .pre {
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 0.92em;
  }
  .inline-code {
    padding: 1px 4px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }
  .mention-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 5px 0 2px;
    border-radius: 4px;
    vertical-align: bottom;
    font-weight: 500;
    color: var(--mention-pill);
    background: var(--mention-pill-soft);
    white-space: nowrap;
    border: 0;
    font: inherit;
    line-height: inherit;
    cursor: pointer;
  }
  .mention-pill:hover {
    text-decoration: underline;
  }
  .mention-pill.self {
    color: var(--mention);
    background: var(--mention-self-soft);
  }
  .pre {
    display: block;
    margin: 2px 0;
    padding: 6px 8px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--text) 8%, transparent);
    white-space: pre-wrap;
  }
  .quote-block {
    display: block;
    margin: 2px 0;
    padding-left: 8px;
    border-left: 3px solid color-mix(in srgb, var(--text) 30%, transparent);
    color: var(--muted);
  }
  .fmt-list {
    margin: 2px 0;
    padding-left: 20px;
    white-space: normal;
  }
</style>
