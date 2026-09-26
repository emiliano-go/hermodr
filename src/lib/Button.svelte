<!-- One button with variants for every shared button shape in the app.
  Renders `btn btn-<variant>` (kept stable for the few contextual size
  overrides in +page.svelte); spot modifiers (`attach`, `tool-text`, …)
  go through `cls`. `title`, ARIA attributes, `type`, `disabled` and
  handlers pass through via rest props. -->
<script lang="ts">
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import type { Snippet } from "svelte";

  export type ButtonVariant = "icon" | "primary" | "chip" | "send" | "menu" | "ghost";

  let {
    variant,
    icon,
    iconSize = 18,
    active = false,
    /** Filter-chip selection; also drives aria-selected for chips. */
    selected = false,
    /** Toggled state; also drives aria-pressed for icon buttons. */
    pressed,
    /** Filter-chip trailing count, shown only when set. */
    count,
    /** Destructive tone, currently only styled on ghost. */
    danger = false,
    /** Extra (global) modifier class for spot tweaks: attach, tool-text, … */
    cls = "",
    children,
    ...rest
  }: {
    variant: ButtonVariant;
    icon?: IconName;
    iconSize?: number;
    active?: boolean;
    selected?: boolean;
    pressed?: boolean;
    count?: number | string;
    danger?: boolean;
    cls?: string;
    children?: Snippet;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    [key: string]: any;
  } = $props();

  const on = $derived(active || selected);
  /** Chips are tabs; the tab role carries aria-selected, plain buttons must not have it. */
  const selectAttrs = $derived<{ role?: string; "aria-selected"?: boolean }>(
    variant === "chip" ? { role: "tab", "aria-selected": selected } : {},
  );
</script>

<button
  type="button"
  class="btn btn-{variant} {cls}"
  class:on
  class:is-danger={danger}
  aria-pressed={pressed}
  {...selectAttrs}
  {...rest}>
  {#if icon}<Icon name={icon} size={iconSize} />{/if}
  {@render children?.()}
  {#if count !== undefined && count !== null}<span class="chip-count">{count}</span>{/if}
</button>

<style>
  .btn {
    font: inherit;
    cursor: pointer;
  }
  /* Square muted icon button. */
  .btn-icon {
    display: inline-grid;
    place-items: center;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: var(--muted);
  }
  .btn-icon:hover {
    background: var(--raised);
    color: var(--text);
  }
  .btn-icon.on {
    color: var(--accent);
  }
  /* Filled accent call to action. */
  .btn-primary {
    background: var(--accent);
    color: var(--accent-ink);
    border: 0;
    border-radius: var(--radius-sm);
    padding: 8px 16px;
    font-weight: 600;
  }
  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* Subtle bordered button for secondary actions (Cancel, Duplicate, …). */
  .btn-ghost {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--raised);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    color: var(--text);
    font-size: 14px;
    padding: 8px 16px;
  }
  .btn-ghost:hover:not(:disabled) {
    background: var(--raised-2);
  }
  .btn-ghost.on {
    background: var(--raised-2);
  }
  .btn-ghost.is-danger {
    color: var(--danger);
  }
  .btn-ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* Round chat-list filter pill. */
  .btn-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--surface);
    border: 0;
    border-radius: 999px;
    color: var(--muted);
    font-size: 14px;
    padding: 5px 12px;
  }
  .btn-chip:hover {
    background: var(--raised);
  }
  .btn-chip.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .chip-count {
    font-size: 12px;
  }
  /* Round composer send/record button. */
  .btn-send {
    flex: none;
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    margin-bottom: 5px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--accent);
  }
  .btn-send:disabled {
    color: var(--faint);
    cursor: default;
  }
  /* Icon + label row in popup menus. */
  .btn-menu {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--text);
    font-size: 14px;
    text-align: left;
  }
  .btn-menu:hover {
    background: var(--raised);
  }
</style>
