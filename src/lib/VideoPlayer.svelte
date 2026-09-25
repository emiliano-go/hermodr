<script lang="ts">
  import { onDestroy } from "svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    src,
    gif = false,
    autoplay = true,
    onerror,
  }: {
    src: string;
    /** GIFs loop silently and hide the sound controls. */
    gif?: boolean;
    autoplay?: boolean;
    onerror?: () => void;
  } = $props();

  const KEY = "hermodr.player";
  const RATES = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

  function load(): { volume: number; muted: boolean; rate: number } {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? "null");
      if (saved && typeof saved.volume === "number") return saved;
    } catch {
      // Unreadable storage falls back to the defaults.
    }
    return { volume: 1, muted: false, rate: 1 };
  }
  const saved = load();

  let video: HTMLVideoElement | undefined = $state();
  let box: HTMLDivElement | undefined = $state();
  let paused = $state(true);
  let current = $state(0);
  let duration = $state(0);
  let buffered: { start: number; end: number }[] = $state([]);
  let volume = $state(saved.volume);
  // svelte-ignore state_referenced_locally
  let muted = $state(gif || saved.muted);
  let rate = $state(saved.rate);
  // svelte-ignore state_referenced_locally
  let looping = $state(gif);
  let fullscreen = $state(false);
  let remaining = $state(false);
  let speedMenu = $state(false);
  let idle = $state(false);
  let hover = $state<{ x: number; time: number } | null>(null);
  let bar: HTMLDivElement | undefined = $state();
  let scrubbing = false;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (gif) return;
    try {
      localStorage.setItem(KEY, JSON.stringify({ volume, muted, rate }));
    } catch {
      // Storage can be full or blocked; the setting lasts this session.
    }
  });

  const loaded = $derived(duration ? Math.max(0, ...buffered.map((r) => r.end)) / duration : 0);
  const volumeIcon = $derived(muted || volume === 0 ? "volumeX" : volume < 0.5 ? "volumeLow" : "volume");

  function clock(seconds: number) {
    if (!Number.isFinite(seconds)) return "0:00";
    const s = Math.floor(seconds % 60).toString().padStart(2, "0");
    const m = Math.floor(seconds / 60) % 60;
    const h = Math.floor(seconds / 3600);
    return h ? `${h}:${m.toString().padStart(2, "0")}:${s}` : `${m}:${s}`;
  }

  function toggle() {
    if (!video) return;
    speedMenu = false;
    if (video.paused) void video.play();
    else video.pause();
  }

  function seek(to: number) {
    if (!video || !duration) return;
    video.currentTime = Math.min(duration, Math.max(0, to));
  }

  function setVolume(next: number) {
    volume = Math.min(1, Math.max(0, next));
    muted = volume === 0;
  }

  function stepRate(delta: number) {
    const at = RATES.indexOf(rate);
    rate = RATES[Math.min(RATES.length - 1, Math.max(0, (at < 0 ? 3 : at) + delta))];
  }

  async function toggleFullscreen() {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await box?.requestFullscreen();
  }

  async function togglePip() {
    if (!video) return;
    if (document.pictureInPictureElement) await document.exitPictureInPicture();
    else await video.requestPictureInPicture();
  }

  function fractionAt(e: PointerEvent) {
    const rect = bar!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
  }

  function onBarDown(e: PointerEvent) {
    scrubbing = true;
    bar!.setPointerCapture(e.pointerId);
    seek(fractionAt(e) * duration);
  }

  function onBarMove(e: PointerEvent) {
    const f = fractionAt(e);
    hover = { x: f * 100, time: f * duration };
    if (scrubbing) seek(f * duration);
  }

  /** Controls fade out after a moment of stillness while playing. */
  function wake() {
    idle = false;
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => (idle = !paused && !speedMenu && !scrubbing), 2500);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest?.("input:not([type=range]), textarea")) return;
    const handled = () => {
      e.preventDefault();
      e.stopPropagation();
      wake();
    };
    if (e.key === " " || e.key === "k") (toggle(), handled());
    else if (e.key === "j") (seek(current - 10), handled());
    else if (e.key === "l") (seek(current + 10), handled());
    else if (e.key === "," && paused) (seek(current - 1 / 30), handled());
    else if (e.key === "." && paused) (seek(current + 1 / 30), handled());
    else if (e.key === "<") (stepRate(-1), handled());
    else if (e.key === ">") (stepRate(1), handled());
    else if (e.key === "ArrowUp" && !gif) (setVolume(volume + 0.05), handled());
    else if (e.key === "ArrowDown" && !gif) (setVolume(volume - 0.05), handled());
    else if (e.key === "m" && !gif) ((muted = !muted), handled());
    else if (e.key === "f") (void toggleFullscreen(), handled());
    else if (/^[0-9]$/.test(e.key)) (seek((Number(e.key) / 10) * duration), handled());
  }

  onDestroy(() => clearTimeout(idleTimer));
</script>

<svelte:window onkeydown={onKey} />
<svelte:document onfullscreenchange={() => (fullscreen = document.fullscreenElement === box)} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="player"
  class:idle
  class:fullscreen
  bind:this={box}
  onpointermove={wake}
  onpointerleave={() => !paused && (idle = true)}>
  <!-- svelte-ignore a11y_media_has_caption, a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <video
    bind:this={video}
    {src}
    {autoplay}
    loop={looping}
    bind:paused
    bind:currentTime={current}
    bind:duration
    bind:buffered
    bind:volume
    bind:muted
    bind:playbackRate={rate}
    onclick={toggle}
    ondblclick={toggleFullscreen}
    onerror={() => onerror?.()}></video>

  {#if paused && current === 0}
    <button class="big-play" aria-label="Play" onclick={toggle}><Icon name="play" size={34} filled /></button>
  {/if}

  <div class="controls">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="bar"
      bind:this={bar}
      onpointerdown={onBarDown}
      onpointermove={onBarMove}
      onpointerup={() => (scrubbing = false)}
      onpointerleave={() => (hover = null)}>
      <div class="track">
        <div class="loaded" style="width: {loaded * 100}%"></div>
        <div class="played" style="width: {duration ? (current / duration) * 100 : 0}%"></div>
      </div>
      <div class="knob" style="left: {duration ? (current / duration) * 100 : 0}%"></div>
      {#if hover}
        <span class="hover-time" style="left: {hover.x}%">{clock(hover.time)}</span>
      {/if}
    </div>

    <div class="row">
      <button class="control" title={paused ? "Play (k)" : "Pause (k)"} aria-label={paused ? "Play" : "Pause"} onclick={toggle}>
        <Icon name={paused ? "play" : "pause"} size={20} filled />
      </button>
      <button class="control" title="Back 10 s (j)" aria-label="Back 10 seconds" onclick={() => seek(current - 10)}>
        <Icon name="back10" size={18} />
      </button>
      <button class="control" title="Forward 10 s (l)" aria-label="Forward 10 seconds" onclick={() => seek(current + 10)}>
        <Icon name="forward10" size={18} />
      </button>

      {#if !gif}
        <div class="volume">
          <button class="control" title="Mute (m)" aria-label={muted ? "Unmute" : "Mute"} onclick={() => (muted = !muted)}>
            <Icon name={volumeIcon} size={20} />
          </button>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            aria-label="Volume"
            value={muted ? 0 : volume}
            style="--fill: {(muted ? 0 : volume) * 100}%"
            oninput={(e) => setVolume(Number(e.currentTarget.value))} />
        </div>
      {/if}

      <button class="time" title="Show remaining time" onclick={() => (remaining = !remaining)}>
        {remaining ? `-${clock(duration - current)}` : clock(current)} / {clock(duration)}
      </button>

      <span class="spacer"></span>

      <div class="speed">
        <button
          class="control rate"
          title="Playback speed (&lt; &gt;)"
          aria-label="Playback speed"
          aria-expanded={speedMenu}
          onclick={() => (speedMenu = !speedMenu)}>{rate}×</button>
        {#if speedMenu}
          <div class="menu" role="menu">
            {#each RATES as r (r)}
              <button
                role="menuitemradio"
                aria-checked={r === rate}
                class:active={r === rate}
                onclick={() => {
                  rate = r;
                  speedMenu = false;
                }}>{r === 1 ? "Normal" : `${r}×`}</button>
            {/each}
          </div>
        {/if}
      </div>
      <button
        class="control"
        class:on={looping}
        title="Loop"
        aria-label="Loop"
        aria-pressed={looping}
        onclick={() => (looping = !looping)}><Icon name="repeat" size={18} /></button>
      {#if document.pictureInPictureEnabled}
        <button class="control" title="Picture in picture" aria-label="Picture in picture" onclick={togglePip}>
          <Icon name="pip" size={18} />
        </button>
      {/if}
      <button
        class="control"
        title={fullscreen ? "Exit full screen (f)" : "Full screen (f)"}
        aria-label={fullscreen ? "Exit full screen" : "Full screen"}
        onclick={toggleFullscreen}><Icon name={fullscreen ? "minimize" : "maximize"} size={18} /></button>
    </div>
  </div>
</div>

<style>
  .player {
    position: relative;
    display: flex;
    max-width: 100%;
    max-height: 100%;
    border-radius: 6px;
    overflow: hidden;
    background: #000;
  }
  .player.fullscreen {
    width: 100%;
    height: 100%;
    border-radius: 0;
    align-items: center;
    justify-content: center;
  }
  .player.idle {
    cursor: none;
  }
  /* The viewer's stage is a size container, so the video fits it exactly. */
  video {
    display: block;
    max-width: 100cqw;
    max-height: 100cqh;
    object-fit: contain;
    cursor: pointer;
  }
  .player.fullscreen video {
    width: 100%;
    height: 100%;
    max-width: none;
    max-height: none;
  }
  .big-play {
    position: absolute;
    inset: 0;
    margin: auto;
    width: 72px;
    height: 72px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    background: var(--scrim);
    color: #fff;
    cursor: pointer;
  }
  .big-play:hover {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .controls {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 18px 12px 8px;
    background: linear-gradient(transparent, var(--scrim));
    color: #fff;
    transition: opacity calc(0.2s * var(--motion-scale)) var(--ease);
  }
  .player.idle .controls {
    opacity: 0;
    pointer-events: none;
  }
  .bar {
    position: relative;
    height: 14px;
    display: flex;
    align-items: center;
    cursor: pointer;
    touch-action: none;
  }
  .track {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.22);
    overflow: hidden;
    transition: height calc(0.12s * var(--motion-scale)) var(--ease);
  }
  .bar:hover .track {
    height: 6px;
  }
  .loaded,
  .played {
    position: absolute;
    inset: 0 auto 0 0;
  }
  .loaded {
    background: rgba(255, 255, 255, 0.3);
  }
  .played {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    width: 12px;
    height: 12px;
    margin-left: -6px;
    border-radius: 50%;
    background: var(--accent);
    transform: scale(0);
    transition: transform calc(0.12s * var(--motion-scale)) var(--ease);
  }
  .bar:hover .knob {
    transform: scale(1);
  }
  .hover-time {
    position: absolute;
    bottom: 18px;
    transform: translateX(-50%);
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-top: 4px;
  }
  .control,
  .time {
    display: grid;
    place-items: center;
    min-width: 34px;
    height: 34px;
    padding: 0 6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .control:hover,
  .time:hover {
    background: rgba(255, 255, 255, 0.14);
  }
  .control.on {
    color: var(--accent);
  }
  .time {
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .rate {
    font-size: 13px;
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .volume {
    display: flex;
    align-items: center;
  }
  .volume input {
    width: 0;
    opacity: 0;
    transition:
      width calc(0.15s * var(--motion-scale)) var(--ease),
      opacity calc(0.15s * var(--motion-scale)) var(--ease);
    height: 4px;
    appearance: none;
    border-radius: 2px;
    background: linear-gradient(to right, #fff var(--fill), rgba(255, 255, 255, 0.3) var(--fill));
    cursor: pointer;
  }
  .volume:hover input,
  .volume input:focus-visible {
    width: 80px;
    opacity: 1;
    margin: 0 6px;
  }
  .volume input::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
  }
  .speed {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 6px);
    display: flex;
    flex-direction: column;
    min-width: 110px;
    padding: 4px;
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  .menu button {
    padding: 6px 10px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--raised);
  }
  .menu button.active {
    color: var(--accent);
    font-weight: 600;
  }
</style>
