<script lang="ts" module>
  export type Poll = {
    id: string;
    name: string;
    options: string[];
    multi: boolean;
    votes: { voter: string; options: string[] }[];
  };
</script>

<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";

  let {
    poll,
    question,
    namer,
    picture,
    onvote,
  }: {
    poll: Poll | undefined;
    /** Shown while the poll's definition is unknown here. */
    question: string;
    /** A voter's display name. */
    namer: (jid: string) => string;
    /** A voter's cached picture path, fetched on first use. */
    picture: (jid: string) => string | null;
    onvote: (options: string[]) => Promise<void>;
  } = $props();

  let busy = $state(false);
  let details = $state(false);

  const mine = $derived(poll?.votes.find((v) => v.voter === "@me")?.options ?? []);
  const voters = $derived(poll?.votes.filter((v) => v.options.length > 0) ?? []);
  const tally = $derived(
    (poll?.options ?? []).map((option) => ({
      option,
      who: voters.filter((v) => v.options.includes(option)).map((v) => v.voter),
    })),
  );
  const total = $derived(Math.max(1, voters.length));

  async function toggle(option: string) {
    if (!poll || busy) return;
    const next = poll.multi
      ? mine.includes(option)
        ? mine.filter((o) => o !== option)
        : [...mine, option]
      : mine.includes(option)
        ? []
        : [option];
    busy = true;
    try {
      await onvote(next);
    } finally {
      busy = false;
    }
  }
</script>

{#snippet face(jid: string, size: number)}
  {@const path = picture(jid)}
  {#if path}
    <img class="face" style="--size: {size}px" src={convertFileSrc(path)} alt="" />
  {:else}
    <span class="face blank" style="--size: {size}px">{namer(jid).replace(/[^\p{L}]/gu, "").slice(0, 1).toUpperCase()}</span>
  {/if}
{/snippet}

<div class="poll">
  <span class="question">{poll?.name ?? question}</span>
  <span class="hint">{poll?.multi ? "Select one or more" : "Select one"}</span>
  {#if poll}
    {#each tally as { option, who } (option)}
      <button
        class="option"
        class:chosen={mine.includes(option)}
        disabled={busy}
        title={who.map(namer).join(", ")}
        onclick={() => toggle(option)}>
        <span class="check" class:round={!poll.multi}></span>
        <span class="option-body">
          <span class="option-row">
            <span class="option-name">{option}</span>
            <span class="faces">
              {#each who.slice(0, 3) as voter (voter)}{@render face(voter, 18)}{/each}
            </span>
            <span class="count">{who.length}</span>
          </span>
          <span class="bar"><span style="width: {(who.length / total) * 100}%"></span></span>
        </span>
      </button>
    {/each}
    {#if details}
      <div class="details">
        {#each tally.filter((t) => t.who.length > 0) as { option, who } (option)}
          <div class="detail">
            <span class="detail-head">{option} <span class="count">{who.length}</span></span>
            {#each who as voter (voter)}
              <span class="voter">{@render face(voter, 24)}{namer(voter)}</span>
            {/each}
          </div>
        {/each}
      </div>
    {/if}
    <span class="footer">
      {voters.length}
      {voters.length === 1 ? "vote" : "votes"}
      {#if voters.length > 0}
        · <button class="link" onclick={() => (details = !details)}>{details ? "Hide votes" : "View votes"}</button>
      {/if}
    </span>
  {:else}
    <span class="hint">This poll's details did not reach this device.</span>
  {/if}
</div>

<style>
  .poll {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 260px;
    padding: 4px 2px;
  }
  .question {
    font-weight: 600;
    font-size: 15px;
  }
  .hint,
  .footer {
    font-size: 12.5px;
    color: var(--muted);
  }
  .footer {
    text-align: center;
    padding-top: 4px;
    border-top: 1px solid color-mix(in srgb, var(--text) 12%, transparent);
  }
  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 2px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .option:disabled {
    cursor: progress;
  }
  .check {
    flex: none;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    border: 2px solid var(--muted);
    border-radius: 4px;
    box-sizing: border-box;
  }
  .check.round {
    border-radius: 50%;
  }
  .chosen .check {
    border-color: var(--accent);
    background: var(--accent);
    box-shadow: inset 0 0 0 3px var(--bubble);
  }
  .option-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .option-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .count {
    color: var(--muted);
    font-size: 13px;
  }
  .option-name {
    flex: 1;
  }
  .faces {
    display: flex;
  }
  .faces > :global(* + *) {
    margin-left: -6px;
  }
  .face {
    width: var(--size);
    height: var(--size);
    flex: none;
    border-radius: 50%;
    object-fit: cover;
    box-shadow: 0 0 0 2px var(--bubble);
  }
  .face.blank {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--text);
    font-size: calc(var(--size) * 0.5);
    font-weight: 600;
  }
  .details {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 8px 0 4px;
    border-top: 1px solid color-mix(in srgb, var(--text) 12%, transparent);
  }
  .detail {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .detail-head {
    display: flex;
    justify-content: space-between;
    font-weight: 600;
    font-size: 13px;
  }
  .voter {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13.5px;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--link);
    font: inherit;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .bar {
    height: 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 12%, transparent);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width calc(0.25s * var(--motion-scale)) var(--ease);
  }
</style>
