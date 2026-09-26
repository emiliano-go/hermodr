<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import Button from "$lib/Button.svelte";
  import Icon from "$lib/Icon.svelte";
  import type { ChatEvent } from "$lib/EventCard.svelte";

  let {
    kind,
    initial = null,
    oncreate,
    onclose,
  }: {
    kind: "poll" | "event";
    /** An event being edited instead of created. */
    initial?: ChatEvent | null;
    /** Resolves once sent; a rejection keeps the dialog open with the error. */
    oncreate: (value: unknown) => Promise<void>;
    onclose: () => void;
  } = $props();

  let question = $state("");
  let options = $state(["", ""]);
  let multi = $state(false);

  const pad = (n: number) => String(n).padStart(2, "0");
  const localDate = (s: number | null | undefined) => {
    if (!s) return "";
    const d = new Date(s * 1000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const localTime = (s: number | null | undefined) => {
    if (!s) return "";
    const d = new Date(s * 1000);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  };
  // svelte-ignore state_referenced_locally
  let name = $state(initial?.name ?? "");
  // svelte-ignore state_referenced_locally
  let description = $state(initial?.description ?? "");
  // svelte-ignore state_referenced_locally
  let date = $state(localDate(initial?.start));
  // svelte-ignore state_referenced_locally
  let startTime = $state(localTime(initial?.start));
  // svelte-ignore state_referenced_locally
  let endTime = $state(localTime(initial?.end));
  // svelte-ignore state_referenced_locally
  let location = $state(initial?.location ?? "");
  // svelte-ignore state_referenced_locally
  let link = $state(initial?.link ?? "");
  const heading = $derived(kind === "poll" ? "Create poll" : initial ? "Edit event" : "Create event");

  let busy = $state(false);
  let failed = $state<string | null>(null);

  const filled = $derived(options.map((o) => o.trim()).filter(Boolean));
  const valid = $derived(
    kind === "poll"
      ? question.trim() !== "" && filled.length >= 2 && new Set(filled).size === filled.length
      : name.trim() !== "",
  );

  // A trailing empty option is always offered, as WhatsApp does, up to twelve.
  $effect(() => {
    if (options.length < 12 && options[options.length - 1].trim()) options.push("");
  });

  /** Unix seconds for a date and a time of day, or null. */
  function seconds(day: string, time: string) {
    if (!day) return null;
    return Math.floor(new Date(`${day}T${time || "00:00"}`).getTime() / 1000);
  }

  async function submit() {
    if (!valid || busy) return;
    busy = true;
    failed = null;
    try {
      await oncreate(
        kind === "poll"
          ? { question: question.trim(), options: filled, multi }
          : {
              name: name.trim(),
              description: description.trim() || null,
              start: seconds(date, startTime),
              end: endTime ? seconds(date, endTime) : null,
              location: location.trim() || null,
              link: link.trim() || null,
            },
      );
      onclose();
    } catch (e) {
      failed = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <form
    class="dialog"
    aria-label={heading}
    transition:scale|global={{ start: 0.96, duration: motion(160) }}
    onsubmit={(e) => (e.preventDefault(), submit())}>
    <header>
      <h2>{heading}</h2>
      <button type="button" class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>

    {#if kind === "poll"}
      <label class="field-label">
        Question
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field" maxlength="255" bind:value={question} placeholder="Ask a question" autofocus />
      </label>
      <span class="field-label">Options</span>
      {#each options as _, i (i)}
        <div class="option-row">
          <input class="field" maxlength="100" bind:value={options[i]} placeholder="Option {i + 1}" />
          {#if options.length > 2 && options[i].trim()}
            <button type="button" class="remove" aria-label="Remove option" onclick={() => options.splice(i, 1)}>
              <Icon name="x" size={14} />
            </button>
          {/if}
        </div>
      {/each}
      {#if filled.length !== new Set(filled).size}<p class="error">Options must be different.</p>{/if}
      <label class="check-row">
        <input type="checkbox" bind:checked={multi} />
        Allow multiple answers
      </label>
    {:else}
      <label class="field-label">
        Name
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field" maxlength="100" bind:value={name} placeholder="Event name" autofocus />
      </label>
      <label class="field-label">
        Description
        <textarea class="field" rows="3" maxlength="2048" bind:value={description}></textarea>
      </label>
      <div class="when">
        <label class="field-label">Date <input class="field" type="date" bind:value={date} /></label>
        <label class="field-label">Starts <input class="field" type="time" bind:value={startTime} /></label>
        <label class="field-label">Ends <input class="field" type="time" bind:value={endTime} /></label>
      </div>
      <label class="field-label">
        Location
        <input class="field" bind:value={location} placeholder="Optional" />
      </label>
      <label class="field-label">
        Call link
        <input class="field" type="url" bind:value={link} placeholder="Optional" />
      </label>
    {/if}

    {#if failed}<p class="error">{failed}</p>{/if}
    <div class="actions">
      <Button variant="ghost" type="button" onclick={onclose}>Cancel</Button>
      <Button variant="primary" type="submit" disabled={!valid || busy}
        >{busy ? (initial ? "Saving…" : "Sending…") : initial ? "Save" : "Send"}</Button
      >
    </div>
  </form>
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
    width: min(460px, 92vw);
    max-height: 88vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 18px 20px;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .close,
  .remove {
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close {
    width: 32px;
    height: 32px;
  }
  .remove {
    width: 28px;
    height: 28px;
    flex: none;
  }
  .close:hover,
  .remove:hover {
    background: var(--raised);
    color: var(--text);
  }
  .field-label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .field {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 8px 10px;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    font-weight: 400;
  }
  textarea.field {
    resize: vertical;
  }
  .option-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .option-row .field {
    flex: 1;
  }
  .when {
    display: grid;
    grid-template-columns: 1.4fr 1fr 1fr;
    gap: 8px;
  }
  .check-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
  }
  .check-row input {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
    margin: 0;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 6px;
  }
  /* Footer actions live in $lib/Button.svelte (ghost/primary variants). */
</style>
