<script lang="ts" module>
  export type ChatEvent = {
    id: string;
    name: string;
    description: string | null;
    start: number | null;
    end: number | null;
    location: string | null;
    link: string | null;
    canceled: boolean;
    responses: { responder: string; response: string }[];
  };
</script>

<script lang="ts">
  import Icon from "$lib/Icon.svelte";

  let {
    event,
    title,
    onrespond,
    onopenurl,
    onedit,
    oncancel,
  }: {
    event: ChatEvent | undefined;
    title: string;
    onrespond: (response: string) => Promise<void>;
    onopenurl: (url: string) => void;
    /** Present only on our own events. */
    onedit?: () => void;
    oncancel?: () => Promise<void>;
  } = $props();

  let busy = $state(false);
  const mine = $derived(event?.responses.find((r) => r.responder === "@me")?.response ?? null);
  const going = $derived(event?.responses.filter((r) => r.response === "going").length ?? 0);
  const maybe = $derived(event?.responses.filter((r) => r.response === "maybe").length ?? 0);

  function when(start: number, end: number | null) {
    const from = new Date(start * 1000);
    const date = from.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
    const time = (d: Date) => d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return end ? `${date}, ${time(from)} – ${time(new Date(end * 1000))}` : `${date}, ${time(from)}`;
  }

  async function respond(answer: string) {
    busy = true;
    try {
      await onrespond(answer);
    } finally {
      busy = false;
    }
  }
</script>

<div class="event" class:canceled={event?.canceled}>
  <span class="badge"><Icon name="calendar" size={14} /> Event</span>
  <span class="name">{event?.name ?? title}</span>
  {#if event?.canceled}<span class="muted">This event was canceled.</span>{/if}
  {#if event?.start}
    <span class="line"><Icon name="clock" size={14} /> {when(event.start, event.end)}</span>
  {/if}
  {#if event?.location}
    <span class="line"><Icon name="pin" size={14} /> {event.location}</span>
  {/if}
  {#if event?.link}
    <button class="line link" onclick={() => onopenurl(event.link!)}>
      <Icon name="external" size={14} /> Join call
    </button>
  {/if}
  {#if event?.description}<p class="description">{event.description}</p>{/if}
  {#if event && !event.canceled}
    <span class="muted">{going} going{maybe ? ` · ${maybe} maybe` : ""}</span>
    <div class="answers">
      {#each [["going", "Going"], ["maybe", "Maybe"], ["not_going", "Can't go"]] as [value, label] (value)}
        <button class="answer" class:chosen={mine === value} disabled={busy} onclick={() => respond(value)}>
          {label}
        </button>
      {/each}
    </div>
    {#if onedit && oncancel}
      <div class="owner">
        <button class="link" onclick={onedit}>Edit</button>
        <button
          class="link danger"
          disabled={busy}
          onclick={async () => {
            busy = true;
            try {
              await oncancel();
            } finally {
              busy = false;
            }
          }}>Cancel event</button>
      </div>
    {/if}
  {:else if !event}
    <span class="muted">This event's details did not reach this device.</span>
  {/if}
</div>

<style>
  .event {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 260px;
    padding: 4px 2px;
  }
  .canceled .name {
    text-decoration: line-through;
  }
  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .name {
    font-size: 16px;
    font-weight: 600;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13.5px;
    color: var(--muted);
  }
  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--link);
    font: inherit;
    font-size: 13.5px;
    cursor: pointer;
  }
  .description {
    margin: 2px 0;
    white-space: pre-wrap;
    font-size: 13.5px;
  }
  .muted {
    font-size: 12.5px;
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
    font-size: 13px;
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
</style>
