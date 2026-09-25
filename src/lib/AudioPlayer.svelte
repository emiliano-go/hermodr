<script lang="ts" module>
  const BARS = 42;
  const RATES = [1, 1.5, 2];
  const RATE_KEY = "hermodr.voiceRate";
  /** Decoded shape and length per file, so scrolling back does not decode again. */
  const shapes = new Map<string, { peaks: number[]; duration: number }>();
  /** Only one voice note plays at a time, as in WhatsApp. */
  let playingNow: HTMLAudioElement | null = null;

  const HEARD_KEY = "hermodr.heardVoice";
  /** Notes already played here; unplayed ones keep WhatsApp's green. */
  const heardNotes: Set<string> = (() => {
    try {
      return new Set(JSON.parse(localStorage.getItem(HEARD_KEY) ?? "[]"));
    } catch {
      return new Set();
    }
  })();
  function rememberHeard(path: string) {
    heardNotes.add(path);
    try {
      localStorage.setItem(HEARD_KEY, JSON.stringify([...heardNotes].slice(-1000)));
    } catch {
      // Only this session remembers it then.
    }
  }

  function savedRate() {
    try {
      const rate = Number(localStorage.getItem(RATE_KEY));
      return RATES.includes(rate) ? rate : 1;
    } catch {
      return 1;
    }
  }

  async function shapeOf(bytes: Uint8Array) {
    const context = new AudioContext();
    try {
      const buffer = await context.decodeAudioData(bytes.slice().buffer);
      const data = buffer.getChannelData(0);
      const step = Math.max(1, Math.floor(data.length / BARS));
      const peaks = Array.from({ length: BARS }, (_, i) => {
        let sum = 0;
        for (let j = i * step; j < Math.min(data.length, (i + 1) * step); j++) sum += data[j] * data[j];
        return Math.sqrt(sum / step);
      });
      const top = Math.max(...peaks, 1e-6);
      return { peaks: peaks.map((p) => Math.max(0.08, p / top)), duration: buffer.duration };
    } finally {
      void context.close();
    }
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";
  import Icon from "$lib/Icon.svelte";

  let {
    path,
    avatar = null,
    initials = "",
    mine = false,
    onplayed,
  }: {
    path: string;
    /** The sender's picture, shown beside the note as WhatsApp does. */
    avatar?: string | null;
    initials?: string;
    /** Our own notes never show as unplayed. */
    mine?: boolean;
    /** Called the first time the note plays here, for the played receipt. */
    onplayed?: () => void;
  } = $props();
  // svelte-ignore state_referenced_locally
  let heard = $state(mine || heardNotes.has(path));

  let audio: HTMLAudioElement | undefined = $state();
  let src = $state<string | null>(null);
  let loading = $state(false);
  let failed = $state(false);
  let paused = $state(true);
  let url: string | null = null;
  let current = $state(0);
  let rate = $state(savedRate());
  // svelte-ignore state_referenced_locally
  let shape = $state(shapes.get(path) ?? null);
  let wave: HTMLDivElement | undefined = $state();
  let scrubbing = false;

  const duration = $derived(shape?.duration ?? 0);
  const progress = $derived(duration ? Math.min(1, current / duration) : 0);

  /** Type the media element expects, from the stored file extension. */
  function mime(path: string) {
    const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
    if (extension === "mp3") return "audio/mpeg";
    if (extension === "m4a" || extension === "aac" || extension === "mp4") return "audio/mp4";
    if (extension === "wav") return "audio/wav";
    return "audio/ogg";
  }

  // The bytes come through IPC because WebKitGTK's media pipeline cannot load
  // the asset scheme; they also give the waveform. Loading is deferred to the
  // first play so a chat full of voice notes reads nothing until asked.
  async function ensureLoaded() {
    if (src) return true;
    loading = true;
    try {
      const data = await invoke<string>("read_file", { path });
      const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
      url = URL.createObjectURL(new Blob([bytes], { type: mime(path) }));
      if (audio) audio.src = url;
      src = url;
      if (!shape) {
        // The waveform can decode while playback starts.
        shapeOf(bytes)
          .then((s) => {
            shape = s;
            shapes.set(path, s);
          })
          .catch(() => {});
      }
      return true;
    } catch {
      failed = true;
      return false;
    } finally {
      loading = false;
    }
  }

  onMount(() => () => {
    if (url) URL.revokeObjectURL(url);
    if (playingNow === audio) playingNow = null;
  });

  async function toggle() {
    if (!audio || failed) return;
    if (!audio.paused) return audio.pause();
    if (!src && !(await ensureLoaded())) return;
    if (playingNow && playingNow !== audio) playingNow.pause();
    playingNow = audio;
    audio.playbackRate = rate;
    try {
      await audio.play();
    } catch {
      failed = true;
      return;
    }
    if (!heard) {
      heard = true;
      rememberHeard(path);
      onplayed?.();
    }
  }

  // timeupdate fires a few times a second; follow the playhead every frame instead.
  $effect(() => {
    if (paused) return;
    let frame = requestAnimationFrame(function follow() {
      if (audio && !scrubbing) current = audio.currentTime;
      frame = requestAnimationFrame(follow);
    });
    return () => cancelAnimationFrame(frame);
  });

  function cycleRate() {
    rate = RATES[(RATES.indexOf(rate) + 1) % RATES.length];
    if (audio) audio.playbackRate = rate;
    try {
      localStorage.setItem(RATE_KEY, String(rate));
    } catch {
      // The speed lasts this session then.
    }
  }

  function seekTo(e: PointerEvent) {
    if (!audio || !wave || !duration) return;
    const rect = wave.getBoundingClientRect();
    audio.currentTime = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)) * duration;
    current = audio.currentTime;
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
    <span class="time">{clock(paused && current === 0 ? duration : current)}</span>
  </div>

  {#if !paused || current > 0}
    <button class="rate" title="Playback speed" onclick={cycleRate}>{rate}×</button>
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

  <audio
    bind:this={audio}
    src={src ?? undefined}
    preload="metadata"
    bind:paused
    ontimeupdate={() => !scrubbing && (current = audio?.currentTime ?? 0)}
    onerror={() => src && (failed = true)}
    onended={() => (current = 0)}></audio>
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
