<script lang="ts">
  import { formatNumber as localeNumber } from "$lib/i18n/localizer";
  import { formatDate as localeDate, formatTime as localeTime } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onDestroy } from "svelte";
  import type { ChatEvent } from "$lib/utils/models";
  import Icon from "$lib/ui/Icon.svelte";
  import Avatar from "$lib/ui/Avatar.svelte";
  import { broadcastSendReason } from "$lib/utils/broadcast";
  import { quizScopeMatches, type QuizScope } from "$lib/utils/quiz-poll";

  let {
    event,
    title,
    onrespond,
    onopenurl,
    onedit,
    oncancel,
    chat = null,
    names = (jid) => jid,
    picture = () => null,
    scope = null,
    pinned = false,
  }: {
    event: ChatEvent | undefined;
    title: string;
    onrespond: (response: string, extraGuestCount?: number) => Promise<void>;
    onopenurl: (url: string) => void;
    /** Present only on our own events. */
    onedit?: () => void;
    oncancel?: () => Promise<void>;
    chat?: string | null;
    names?: (jid: string) => string;
    picture?: (jid: string) => string | null;
    scope?: QuizScope | null;
    pinned?: boolean;
  } = $props();

  let busy = $state(false);
  let failed = $state<LocalizedError | string | null>("");
  let details = $state(false);
  let extraGuestCount = $state<number | undefined>(undefined);
  let generation = 0, alive = true, ownerKey = "", guestSource = "";
  const answers = $derived([["going", t("content.going")], ["maybe", t("content.maybe")], ["not_going", t("content.can_t_go")]] as const);
  const liveScope = $derived(scope ?? { account: null, chat, generation: 0, requestKey: event?.id });
  const sendReason = $derived(broadcastSendReason(liveScope.chat));
  const mine = $derived(event?.responses.find((row) => row.responder === "@me") ?? null);
  const groups = $derived(answers.map(([value, label]) => {
    const rows = event?.responses.filter((row) => row.response === value) ?? [];
    return { value, label, rows, guests: rows.reduce((sum, row) => sum + (knownGuests(row.extra_guest_count) ? row.extra_guest_count : 0), 0) };
  }));
  const responseReason = $derived(sendReason ?? (!event ? t("content.this_event_s_details_did_not_reach_this_device") : event.canceled
    ? t("content.this_event_was_canceled") : !event.can_respond ? event.invitation
      ? t("content.this_invitation_is_read_only_on_this_device") : t("content.this_event_cannot_be_answered_on_this_device") : null));
  const validGuests = $derived(event?.extra_guests_allowed !== true || extraGuestCount === undefined || knownGuests(extraGuestCount));

  $effect(() => {
    const key = JSON.stringify([liveScope.account, liveScope.chat, liveScope.generation, liveScope.requestKey, event?.id]);
    if (key === ownerKey) return;
    ownerKey = key;
    generation++;
    busy = details = false;
    failed = "";
  });
  $effect(() => {
    const key = JSON.stringify([liveScope.account, liveScope.chat, liveScope.generation, liveScope.requestKey, event?.id, mine?.extra_guest_count]);
    if (busy || key === guestSource) return;
    guestSource = key;
    extraGuestCount = knownGuests(mine?.extra_guest_count) ? mine!.extra_guest_count! : undefined;
  });
  onDestroy(() => { alive = false; generation++; });

  function knownGuests(value: number | null | undefined): value is number {
    return value != null && Number.isInteger(value) && value >= 0 && value <= 2147483647;
  }

  function when(start: number, end: number | null) {
    const from = new Date(start * 1000);
    const to = end == null ? null : new Date(end * 1000);
    if (!Number.isFinite(from.getTime()) || to && !Number.isFinite(to.getTime())) return t("content.event_time_unavailable");
    const date = (d: Date) => localeDate((d).getTime() / 1000, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
    const time = (d: Date) => localeTime((d).getTime() / 1000, { hour: "2-digit", minute: "2-digit" });
    return to ? `${date(from)}, ${time(from)} – ${from.toDateString() === to.toDateString() ? time(to) : `${date(to)}, ${time(to)}`}` : `${date(from)}, ${time(from)}`;
  }

  async function run(task: () => Promise<void>) {
    if (busy || responseReason || !alive) return;
    const owner = { ...liveScope }, id = event?.id, revision = generation;
    const current = () => alive && revision === generation && id === event?.id && quizScopeMatches(owner, liveScope);
    busy = true;
    failed = "";
    try {
      await task();
    } catch (cause) {
      if (current()) failed = normalizeError(cause);
    } finally {
      if (current()) busy = false;
    }
  }

  function respond(answer: string) {
    if (!answers.some(([value]) => value === answer) || !validGuests) return;
    const guests = event?.extra_guests_allowed === true && answer !== "not_going" ? extraGuestCount : undefined;
    return run(() => onrespond(answer, guests));
  }
</script>

<div class="event" class:canceled={event?.canceled}>
  <span class="badge"><Icon name="calendar" size={14} /> {t("content.event")}</span>
  {#if pinned}<span class="badge"><Icon name="pin" size={14} /> {t("content.pinned_event")}</span>{/if}
  <span class="name">{event?.name ?? title}</span>
  {#if event?.canceled}<span class="muted">{t("content.this_event_was_canceled")}</span>{/if}
  {#if event?.start != null}
    <span class="line"><Icon name="clock" size={14} /> {when(event.start, event.end)}</span>
  {:else if event?.end != null}
    <span class="line">{t("content.ends")} {when(event.end, null)}</span>
  {/if}
  {#if event?.location}
    <span class="line"><Icon name="pin" size={14} /> {event.location}</span>
  {/if}
  {#if event?.link}
    <button class="line link" onclick={() => onopenurl(event.link!)}>
      <Icon name="external" size={14} /> {t("content.join_call")}
    </button>
  {/if}
  {#if event?.description}<p class="description">{event.description}</p>{/if}
  {#if event}
    {#if event.invitation}<span class="muted">{t("content.event_invitation")}</span>{/if}
    {#if event.is_scheduled_call != null}<span class="muted">{t("content.scheduled_call")} {event.is_scheduled_call ? t("content.yes") : t("content.no")}</span>{/if}
    {#if event.has_reminder != null}<span class="muted">{t("content.reminder")} {event.has_reminder ? t("content.enabled") : t("content.none")}</span>{/if}
    {#if event.reminder_offset_sec != null}<span class="muted">{t("content.reminder_offset")} {localeNumber(event.reminder_offset_sec)} {t("content.seconds")}</span>{/if}
    {#if event.has_reminder === true && event.reminder_offset_sec == null}<span class="muted">{t("content.reminder_offset_unavailable")}</span>{/if}
    <span class="muted">{event.extra_guests_allowed == null ? t("content.extra_guest_allowance_unavailable") : event.extra_guests_allowed ? t("content.extra_guests_allowed") : t("content.extra_guests_are_not_allowed")}</span>
    <span class="muted">{groups.map((group) => t("content.response_count", { count: group.rows.length, response: group.label }) + (group.guests ? " (+" + t("content.extra_guest_count", { count: group.guests }) + ")" : "")).join(" · ")}</span>
    <span class="your-response">{t("content.your_response")} {answers.find(([value]) => value === mine?.response)?.[1] ?? t("content.not_responded")}
      {#if knownGuests(mine?.extra_guest_count)} · {t("content.extra_guest_count", { count: mine!.extra_guest_count })}{/if}</span>
    <details bind:open={details}>
      <summary>{t("content.attendees")}</summary>
      {#each groups as group (group.value)}
        <section class="attendees" aria-label={t("content.value_attendees", { param0: (group.label) })}>
          <h3>{group.label} ({localeNumber(group.rows.length)})</h3>
          {#if !group.rows.length}<span class="muted">{t("content.no_responses_received")}</span>{/if}
          <ul>{#each group.rows as row (row.responder)}
            {@const name = row.responder === "@me" ? t("content.you") : names(row.responder)}
            {@const path = picture(row.responder)}
            <li><Avatar label={name} seed={row.responder} src={path} /><span><bdi dir="auto">{name}</bdi>
              {#if knownGuests(row.extra_guest_count)}<small>{t("content.extra_guest_count", { count: row.extra_guest_count })}</small>
              {:else if event.extra_guests_allowed === true}<small>{t("content.guest_count_unavailable")}</small>{/if}</span></li>
          {/each}</ul>
        </section>
      {/each}
    </details>
    {#if responseReason}<span class="muted" role="status">{responseReason}</span>{/if}
    {#if event.extra_guests_allowed === true && !event.canceled && event.can_respond}
      <label class="guests">{t("content.your_extra_guests_optional")}<input type="number" min="0" max="2147483647" step="1"
        bind:value={extraGuestCount} disabled={busy || !!sendReason} /></label>
      {#if !validGuests}<span class="error" role="alert">{t("content.enter_a_whole_guest_count_from_0_to_2147483647")}</span>{/if}
    {/if}
    <div class="answers">
      {#each answers as [value, label] (value)}
        <button class="answer" class:chosen={mine?.response === value} disabled={busy || !!responseReason || !validGuests} title={responseReason ?? undefined} onclick={() => respond(value)}>
          {label}
        </button>
      {/each}
    </div>
    {#if mine}
      <button class="link" disabled title={t("content.clearing_an_rsvp_is_unavailable_with_the_current_protocol_api")}>{t("content.clear_response")}</button>
      <span class="muted">{t("content.clearing_an_rsvp_is_unavailable_with_the_current_protocol_api")}</span>
    {/if}
    {#if failed}<span class="error" role="alert">{failed}</span>{/if}
    {#if onedit && oncancel}
      <div class="owner">
        <button class="link" disabled={busy || !!responseReason} title={responseReason ?? undefined}
          onclick={() => { if (!busy && !responseReason) onedit?.(); }}>{t("content.edit")}</button>
        <button
          class="link danger"
          disabled={busy || !!responseReason}
          title={responseReason ?? undefined}
          onclick={() => run(() => oncancel!())}>{t("content.cancel_event")}</button>
      </div>
    {/if}
  {:else if !event}
    <span class="muted">{t("content.this_event_s_details_did_not_reach_this_device")}</span>
  {/if}
</div>

<style>
  .event {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: min(260px, 100%);
    padding: 4px 2px;
  }
  .canceled .name {
    text-decoration: line-through;
  }
  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--accent);
  }
  .name {
    font-size: 1rem;
    font-weight: 600;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.8438rem;
    color: var(--muted);
  }
  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--link);
    font: inherit;
    font-size: 0.8438rem;
    cursor: pointer;
  }
  .description {
    margin: 2px 0;
    white-space: pre-wrap;
    font-size: 0.8438rem;
  }
  .muted {
    font-size: 0.7812rem;
    color: var(--muted);
  }
  .answers {
    display: flex;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in srgb, var(--text) 12%, transparent);
  }
  .answer {
    flex: 1;
    padding: 6px 8px;
    border: 1px solid color-mix(in srgb, var(--text) 18%, transparent);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
  }
  .answer:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }
  .answer.chosen {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .owner {
    display: flex;
    justify-content: flex-end;
    gap: 14px;
  }
  .danger {
    color: var(--danger);
  }
  .error { color: var(--danger); font-size: 0.8125rem; overflow-wrap: anywhere; }
  .your-response { font-size: 0.8125rem; }
  summary { cursor: pointer; font-size: 0.8125rem; color: var(--link); }
  details[open] { max-height: 280px; overflow-y: auto; }
  .attendees h3 { margin: 8px 0; font-size: 0.8125rem; }
  .attendees ul { margin: 0; padding: 0; list-style: none; }
  .attendees li { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 0.8125rem; }
  .attendees :global(.avatar) { width: 24px; height: 24px; flex-shrink: 0; font-size: 0.6875rem; }
  small { display: block; color: var(--muted); }
  .guests { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.8125rem; }
  .guests input { width: 80px; padding: 5px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--surface); color: var(--text); font: inherit; }
</style>
