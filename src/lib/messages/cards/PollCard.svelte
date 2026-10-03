<script lang="ts">
  import { formatNumber as localeNumber } from "$lib/i18n/localizer";
  import { LocalizedError, messageText, normalizeError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import type { Poll } from "$lib/utils/models";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { quizScopeMatches, type QuizScope } from "$lib/utils/quiz-poll";
  import { session } from "$lib/state/session.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { broadcastSendReason } from "$lib/utils/broadcast";

  let {
    poll,
    question,
    namer,
    picture,
    onvote,
    scope = null,
  }: {
    poll: Poll | undefined;
    /** Shown while the poll's definition is unknown here. */
    question: string;
    /** A voter's display name. */
    namer: (jid: string) => string;
    /** A voter's cached picture path, fetched on first use. */
    picture: (jid: string) => string | null;
    onvote: (options: string[]) => Promise<void>;
    scope?: QuizScope | null;
  } = $props();

  let busy = $state(false);
  let details = $state(false);
  let failed = $state<LocalizedError | string | null>("");
  let generation = 0;
  const liveScope = $derived(scope ?? { account: session.activeAccount, chat: chats.selectedChat, generation: messages.accountGeneration, requestKey: poll?.id });
  const quiz = $derived(poll?.quiz ?? null);
  const multi = $derived(!quiz && !!poll?.multi);
  const sendReason = $derived(broadcastSendReason(scope?.chat));
  const canVote = $derived(!!poll && !sendReason && (!quiz || quiz.can_vote));

  const mine = $derived(poll?.votes.find((v) => v.voter === "@me")?.options ?? []);
  const voters = $derived(poll?.votes.filter((v) => v.options.length > 0) ?? []);
  const tally = $derived(
    (poll?.options ?? []).map((option) => ({
      option,
      who: voters.filter((v) => v.options.includes(option)).map((v) => v.voter),
    })),
  );
  const total = $derived(Math.max(1, voters.length));
  function voterName(jid: string): string { return jid === "@me" ? t("content.you") : namer(jid); }
  $effect(() => {
    void poll?.id; void liveScope.account; void liveScope.chat; void liveScope.generation; void liveScope.requestKey;
    ++generation;
    busy = details = false;
    failed = "";
    return () => { ++generation; };
  });

  async function toggle(option: string) {
    if (!poll || busy || !canVote) return;
    const owner = { ...liveScope }, id = poll.id, revision = generation;
    const current = () => revision === generation && poll?.id === id && quizScopeMatches(owner, liveScope);
    const next = multi
      ? mine.includes(option)
        ? mine.filter((o) => o !== option)
        : [...mine, option]
      : mine.includes(option)
        ? []
        : [option];
    busy = true;
    failed = "";
    try {
      await onvote(next);
    } catch (cause) {
      if (current()) failed = normalizeError(cause);
    } finally {
      if (current()) busy = false;
    }
  }
</script>

{#snippet face(jid: string, size: number)}
  {@const path = picture(jid)}
  {#if path}
    <img class="face" style="--size: {size}px" src={convertFileSrc(path)} alt="" />
  {:else}
    <span class="face blank" style="--size: {size}px">{voterName(jid).replace(/[^\p{L}]/gu, "").slice(0, 1).toUpperCase()}</span>
  {/if}
{/snippet}

<div class="poll">
  <span class="question">{poll?.name ?? question}</span>
  {#if quiz}<span class="hint">{t("content.quiz_one_correct_answer")}</span>{/if}
  <span class="hint">{sendReason ?? (multi ? t("content.select_one_or_more") : t("content.select_one"))}</span>
  {#if poll}
    {#each tally as { option, who } (option)}
      <button
        class="option"
        class:chosen={mine.includes(option)}
        disabled={busy || !canVote}
        title={sendReason ?? who.map(voterName).join(", ")}
        onclick={() => toggle(option)}>
        <span class="check" class:round={!multi}></span>
        <span class="option-body">
          <span class="option-row">
            <span class="option-name">{option}</span>
            <span class="faces">
              {#each who.slice(0, 3) as voter (voter)}{@render face(voter, 18)}{/each}
            </span>
            <span class="count">{localeNumber(who.length)}</span>
          </span>
          <span class="bar"><span style="width: {(who.length / total) * 100}%"></span></span>
        </span>
      </button>
    {/each}
    {#if busy}<p class="hint" role="status">{t("content.sending_answer")}</p>{/if}
    {#if failed}<p class="error" role="alert">{failed}</p>{/if}
    {#if quiz}
      <p class="feedback" role="status">{t(quiz.my_correct === true ? "content.quiz_correct" : quiz.my_correct === false ? "content.quiz_incorrect" : "content.quiz_unavailable")}</p>
      <p class="hint">{quiz.correct_option === null ? t("content.correct_answer_is_undisclosed") : t("content.correct_answer_value", { param0: (quiz.correct_option) })}</p>
      {#if !quiz.can_vote}<p class="hint" role="status">{t("content.voting_unavailable_on_this_device")}</p>{/if}
      {#if !quiz.results_complete}<p class="hint" role="status">{t("content.results_are_incomplete_on_this_device")}</p>{/if}
      {#if quiz.error_ref || quiz.error}<p class="error" role="alert">{quiz.error_ref ? messageText(quiz.error_ref) : t("error.operation_failed")}</p>{/if}
      {#if quiz.diagnostic || quiz.error}<details><summary>{t("content.technical_details")}</summary><pre dir="ltr">{quiz.diagnostic ?? quiz.error}</pre></details>{/if}
    {/if}
    {#if details}
      <div class="details">
        {#each tally.filter((t) => t.who.length > 0) as { option, who } (option)}
          <div class="detail">
            <span class="detail-head">{option} <span class="count">{localeNumber(who.length)}</span></span>
            {#each who as voter (voter)}
              <span class="voter">{@render face(voter, 24)}{voterName(voter)}</span>
            {/each}
          </div>
        {/each}
      </div>
    {/if}
    <span class="footer">
      {t("content.vote_count", { count: voters.length })}
      {#if voters.length > 0}
        · <button class="link" onclick={() => (details = !details)}>{details ? t("content.hide_votes") : t("content.view_votes")}</button>
      {/if}
    </span>
  {:else}
    <span class="hint">{t("content.this_poll_s_details_did_not_reach_this_device")}</span>
  {/if}
</div>

<style>
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  .poll {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: min(260px, 100%);
    max-width: 100%;
    padding: 4px 2px;
  }
  .question {
    font-weight: 600;
    font-size: 0.9375rem;
    overflow-wrap: anywhere;
  }
  .feedback, .error { margin: 0; font-size: 0.8125rem; overflow-wrap: anywhere; }
  .error { color: var(--danger, #ef7777); }
  .hint,
  .footer {
    font-size: 0.7812rem;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  p.hint { margin: 0; }
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
    text-align: start;
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
    min-width: 0;
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
    font-size: 0.8125rem;
  }
  .option-name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .faces {
    display: flex;
  }
  .faces > :global(* + *) {
    margin-inline-start: -6px;
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
    font-size: 0.8125rem;
  }
  .voter {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.8438rem;
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
