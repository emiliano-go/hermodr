<script lang="ts">
  import CreateDialog from "$lib/chat/CreateDialog.svelte";
  import PollCard from "$lib/messages/cards/PollCard.svelte";
  import { quizScopeMatches, type QuizScope } from "$lib/utils/quiz-poll";
  import type { Poll } from "$lib/utils/wire";

  let account = $state("alpha"), chat = $state("group@g.us"), generation = $state(1), requestKey = $state(1);
  const scope = $derived<QuizScope>({ account, chat, generation, requestKey });
  let creator = $state(false), defer = $state(false), fail = $state(false), partial = $state(false), missingKey = $state(false);
  let waiting = $state(0), calls = $state<string[]>([]);
  let votes = $state<Poll["votes"]>([]), answer = $state<string | null>(null), correct = $state<boolean | null>(null);
  const releases: (() => void)[] = [];
  const poll = $derived<Poll>({ id: "synthetic-quiz", name: "Which answer is correct?", options: ["Alpha", "Beta"], multi: false, votes,
    quiz: { correct_option: answer, my_correct: correct, results_complete: !partial && !missingKey,
      error: missingKey ? "Synthetic missing quiz key" : partial ? "Synthetic partial aggregate" : null, can_vote: !missingKey } });
  async function delay() {
    if (!defer) return;
    waiting++;
    await new Promise<void>((yes) => releases.push(yes));
    waiting--;
  }
  function release() { releases.splice(0).forEach((yes) => yes()); }
  async function create(value: unknown) {
    const owner = { ...scope }, refused = fail;
    calls = [...calls, `create ${JSON.stringify(owner)} ${JSON.stringify(value)}`];
    await delay();
    if (refused) throw new Error("Synthetic create refusal");
    if (quizScopeMatches(owner, scope)) calls = [...calls, "create accepted"];
  }
  async function vote(options: string[]) {
    const owner = { ...scope }, refused = fail;
    calls = [...calls, `vote ${JSON.stringify(owner)} ${JSON.stringify(options)}`];
    await delay();
    if (refused) throw new Error("Synthetic vote refusal");
    if (!quizScopeMatches(owner, scope)) return;
    votes = [...votes.filter((vote) => vote.voter !== "@me"), { voter: "@me", options }];
    answer = options.length ? "Beta" : null;
    correct = options.length ? options[0] === "Beta" : null;
  }
  $effect(() => { void account; void chat; void generation; votes = []; answer = null; correct = null; });
  function key(event: KeyboardEvent) {
    if (!event.ctrlKey || !event.altKey) return;
    const code = event.key.toLowerCase();
    if (code === "a") account = account === "alpha" ? "beta" : "alpha";
    else if (code === "c") chat = chat === "group@g.us" ? "other@g.us" : "group@g.us";
    else if (code === "g") generation++;
    else if (code === "q") requestKey++;
    else if (code === "r") release();
    else return;
    event.preventDefault();
  }
</script>

<svelte:window onkeydown={key} />
<h1>Synthetic quiz polls</h1>
<button onclick={() => { requestKey++; creator = true; }}>Open creator</button>
<button onclick={() => (votes = [...votes.filter((vote) => vote.voter !== "peer@lid"), { voter: "peer@lid", options: ["Beta"] }])}>Deliver aggregate vote</button>
<button onclick={release} disabled={!waiting}>Release requests {waiting}</button>
<label><input type="checkbox" bind:checked={defer} /> Defer callbacks</label>
<label><input type="checkbox" bind:checked={fail} /> Refuse callback</label>
<label><input type="checkbox" bind:checked={partial} /> Partial results</label>
<label><input type="checkbox" bind:checked={missingKey} /> Missing key</label>
<p>Scope {account} / {chat} / {generation} / {requestKey}</p>
<p>Ctrl+Alt+A account; C chat; G generation; Q request; R release.</p>
<div class="card"><PollCard {poll} question={poll.name} {scope} namer={(jid) => `Synthetic ${jid}`} picture={() => null} onvote={vote} /></div>
<pre aria-label="Synthetic quiz operations">{calls.join("\n")}</pre>
{#if creator}<CreateDialog kind="poll" {scope} oncreate={create} onclose={() => { creator = false; calls = [...calls, "creator closed"]; }} />{/if}

<style>
  :global(:root) { --bg: #192129; --text: #e6ebef; --muted: #aab6bf; --line-strong: #44525d; --accent: #59b99d; --bubble: #25343e; --raised: #2b3943; --raised-2: #35424c; --danger: #ef7777; --scrim: #0009; --radius-lg: 12px; --radius-sm: 6px; --shadow: 0 8px 30px #0005; }
  :global(body) { margin: 16px; background: var(--bg); color: var(--text); font: 14px system-ui; }
  .card { max-width: 420px; margin: 16px 0; padding: 12px; background: var(--bubble); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
