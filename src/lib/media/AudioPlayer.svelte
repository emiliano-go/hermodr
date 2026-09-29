<script lang="ts">
  import { untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/ui/Icon.svelte";
  import { BARS, player, type VoiceTrack } from "$lib/state/player.svelte";

  let {
    path,
    /** Stored length in seconds, so the time shows before the file is decoded. */
    duration: storedDuration = null,
    avatar = null,
    initials = "",
    mine = false,
    play = false,
    /** Sender name for the sidebar player. */
    title = "",
    /** The next note in a chain: cue and a short pause before it starts. */
    chained = false,
    onplayed,
    onended,
    onpaused,
  }: {
    path: string;
    duration?: number | null;
    /** The sender's picture, shown beside the note as WhatsApp does. */
    avatar?: string | null;
    initials?: string;
    /** Our own notes never show as unplayed. */
    mine?: boolean;
    /** Start as soon as this turns true, for the queue behind a voice note. */
    play?: boolean;
    title?: string;
    chained?: boolean;
    /** Called the first time the note plays here, for the played receipt. */
    onplayed?: () => void;
    /** Called when the note reaches its end on its own, to chain to the next. */
    onended?: () => void;
    /** Called on a manual pause, so a chain does not survive it. */
    onpaused?: () => void;
  } = $props();
  // svelte-ignore state_referenced_locally
  let heard = $state(mine || player.heard(path));
  let wave: HTMLDivElement | undefined = $state();
  let scrubbing = false;

  const active = $derived(player.track?.path === path);
  const shape = $derived(player.shapes[path] ?? null);
  const duration = $derived(
    (active ? player.duration : 0) || shape?.duration || storedDuration || 0,
  );
  const current = $derived(active ? player.position : 0);
  const progress = $derived(duration ? Math.min(1, current / duration) : 0);
  const paused = $derived(!active || player.paused);
  const loading = $derived(active && player.loading);
  const failed = $derived(active && player.failed);
  const rate = $derived(player.rate);

  function tune(): VoiceTrack {
    return {
      path,
      duration: storedDuration,
      avatar,
      initials,
      title,
      autoplay: chained,
      onplayed: () => {
        heard = true;
        onplayed?.();
      },
      // A player with nothing to chain to (a recovered one-time note) closes
      // itself once the audio reaches the end.
      onended: onended ?? (() => player.stop()),
      onpaused,
    };
  }

  function start() {
    if (!failed) void player.play(tune());
  }

  function toggle() {
    if (failed) return;
    if (active && !player.paused) player.pause();
    else start();
  }

  // The parent chains voice notes by flipping `play` on the next one; only that
  // change matters, not the playback state starting depends on.
  $effect(() => {
    if (play) untrack(() => start());
  });

  function seekTo(e: PointerEvent) {
    if (!wave || !duration) return;
    const rect = wave.getBoundingClientRect();
    player.seek(Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)));
  }

  function clock(seconds: number) {
    if (!Number.isFinite(seconds)) return "0:00";
    return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
  }
</script>

<div class="voice" class:heard>
  <button
    class="toggle"
    class:failed
    onclick={toggle}
    disabled={failed || loading}
    title={failed ? "Could not play this file" : paused ? "Play" : "Pause"}
    aria-label={paused ? "Play" : "Pause"}>
    {#if failed}!{:else}<Icon name={paused ? "play" : "pause"} size={22} filled />{/if}
  </button>

  <div class="middle">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="wave"
      bind:this={wave}
      onpointerdown={(e) => {
        scrubbing = true;
        wave!.setPointerCapture(e.pointerId);
        seekTo(e);
      }}
      onpointermove={(e) => scrubbing && seekTo(e)}
      onpointerup={() => (scrubbing = false)}>
      {#each shape?.peaks ?? Array(BARS).fill(0.15) as peak, i (i)}
        <span class="bar" class:played={i / BARS < progress} style="height: {Math.round(peak * 100)}%"></span>
      {/each}
      <span class="knob" style="left: {progress * 100}%"></span>
    </div>
    <!-- Until the file decodes, the length is unknown; 0:00 would read as an empty note. -->
    <span class="time">{duration || current > 0 ? clock(paused && current === 0 ? duration : current) : "--:--"}</span>
  </div>

  {#if !paused || current > 0}
    <button class="rate" title="Playback speed" onclick={() => player.cycleRate()}>{rate}×</button>
  {:else}
    <span class="who">
      {#if avatar}
        <img src={convertFileSrc(avatar)} alt="" />
      {:else}
        <span class="blank">{initials}</span>
      {/if}
      <span class="mic"><Icon name="mic" size={11} /></span>
    </span>
  {/if}
</div>

<style>
  /* Controls sit on the waveform's line; the time hangs below it, as in WhatsApp. */
  .voice {
    --played: var(--accent);
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-rows: 34px auto;
    align-items: center;
    column-gap: 10px;
    width: 300px;
    max-width: 100%;
    padding: 4px 2px 0;
  }
  .voice.heard {
    --played: var(--link);
  }
  .middle {
    display: contents;
  }
  .toggle {
    grid-row: 1;
    grid-column: 1;
  }
  .wave {
    grid-row: 1;
    grid-column: 2;
  }
  .time {
    grid-row: 2;
    grid-column: 2;
  }
  .rate,
  .who {
    grid-row: 1;
    grid-column: 3;
  }
  .who {
    grid-row: 1 / span 2;
  }
  .toggle {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
    font-weight: 700;
  }
  .toggle:hover:not(:disabled) {
    color: var(--text);
  }
  .toggle.failed {
    color: var(--danger);
  }
  .toggle:disabled {
    cursor: progress;
    opacity: 0.6;
  }
  .wave {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 26px;
    cursor: pointer;
    touch-action: none;
  }
  .bar {
    flex: 1;
    min-height: 3px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--text) 32%, transparent);
  }
  .bar.played {
    background: var(--played);
  }
  .knob {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 50%;
    background: var(--played);
    pointer-events: none;
  }
  .time {
    font-size: 11px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .rate {
    flex: none;
    min-width: 40px;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 14%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .who {
    position: relative;
    flex: none;
    width: 46px;
    height: 46px;
  }
  .who img,
  .blank {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
  }
  .blank {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--text);
    font-weight: 600;
  }
  .mic {
    position: absolute;
    left: -4px;
    bottom: -2px;
    display: grid;
    place-items: center;
    color: var(--played);
  }
</style>
