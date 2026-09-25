<script lang="ts" module>
  export type Recording = { blob: Blob; seconds: number; waveform: number[]; viewOnce: boolean };
</script>

<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    onsend,
    oncancel,
    onerror,
  }: {
    onsend: (recording: Recording) => void;
    oncancel: () => void;
    onerror: (message: string) => void;
  } = $props();

  const LIVE_BARS = 36;
  const PAUSE = -1;
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let context: AudioContext | null = null;
  let analyser: AnalyserNode | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  const chunks: Blob[] = [];
  /** Every level sampled, which becomes the note's waveform. */
  const history: number[] = [];

  let ready = $state(false);
  let paused = $state(false);
  let once = $state(false);
  let elapsed = $state(0);
  let live = $state<number[]>([]);
  let lastTick = 0;

  onMount(async () => {
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
      });
    } catch {
      onerror("Microphone access was refused or no microphone is connected.");
      return oncancel();
    }
    startRecorder();
    context = new AudioContext();
    analyser = context.createAnalyser();
    analyser.fftSize = 1024;
    context.createMediaStreamSource(stream).connect(analyser);
    const samples = new Float32Array(analyser.fftSize);
    lastTick = performance.now();
    timer = setInterval(() => {
      const now = performance.now();
      if (paused || !analyser) return void (lastTick = now);
      elapsed += (now - lastTick) / 1000;
      lastTick = now;
      analyser.getFloatTimeDomainData(samples);
      let sum = 0;
      for (const s of samples) sum += s * s;
      const level = Math.sqrt(sum / samples.length);
      history.push(level);
      live = [...live, level].slice(-LIVE_BARS);
    }, 100);
    ready = true;
  });

  function startRecorder() {
    recorder = new MediaRecorder(stream!, { mimeType: "audio/webm;codecs=opus", audioBitsPerSecond: 32000 });
    recorder.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
    recorder.start(250);
  }

  /** Throws away what was recorded and starts again. */
  function recordOver() {
    stopPreview();
    if (recorder && recorder.state !== "inactive") {
      recorder.ondataavailable = null;
      recorder.onstop = null;
      recorder.stop();
    }
    chunks.length = 0;
    history.length = 0;
    live = [];
    elapsed = 0;
    paused = false;
    lastTick = performance.now();
    startRecorder();
  }

  let preview: HTMLAudioElement | undefined = $state();
  let previewUrl: string | null = null;
  let previewing = $state(false);
  let previewAt = $state(0);

  /** Plays what has been recorded so far; only offered while paused. */
  async function togglePreview() {
    if (!preview) return;
    if (previewing) return stopPreview();
    // Flush the chunk in progress so the preview reaches the pause.
    recorder?.requestData();
    await new Promise((r) => setTimeout(r, 60));
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = URL.createObjectURL(new Blob(chunks, { type: "audio/webm" }));
    preview.src = previewUrl;
    previewing = true;
    await preview.play().catch(() => (previewing = false));
  }
  function stopPreview() {
    preview?.pause();
    previewing = false;
    previewAt = 0;
  }

  function release() {
    stopPreview();
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    clearInterval(timer);
    stream?.getTracks().forEach((t) => t.stop());
    void context?.close();
    stream = null;
    context = null;
  }
  onDestroy(release);

  function togglePause() {
    if (!recorder) return;
    if (paused) {
      stopPreview();
      recorder.resume();
    } else {
      recorder.pause();
      // A gap in the live levels marks where the recording was paused.
      live = [...live, PAUSE].slice(-LIVE_BARS);
    }
    paused = !paused;
  }

  /** 64 levels from 0 to 100, WhatsApp's waveform format. */
  function waveform() {
    const top = Math.max(...history, 1e-6);
    return Array.from({ length: 64 }, (_, i) => {
      const from = Math.floor((i * history.length) / 64);
      const to = Math.max(from + 1, Math.floor(((i + 1) * history.length) / 64));
      const slice = history.slice(from, to);
      const peak = slice.length ? Math.max(...slice) : 0;
      return Math.round((peak / top) * 100);
    });
  }

  function finish() {
    if (!recorder || elapsed < 0.5) return;
    const active = recorder;
    active.onstop = () => {
      const seconds = Math.max(1, Math.round(elapsed));
      const recording = { blob: new Blob(chunks, { type: "audio/webm" }), seconds, waveform: waveform(), viewOnce: once };
      release();
      onsend(recording);
    };
    active.stop();
  }

  function cancel() {
    if (recorder && recorder.state !== "inactive") {
      recorder.onstop = null;
      recorder.stop();
    }
    release();
    oncancel();
  }

  function clock(seconds: number) {
    return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
  }
  const top = $derived(Math.max(0.02, ...live));

  function finishPreview() {
    previewing = false;
    previewAt = 0;
  }
</script>

<div class="recorder" role="group" aria-label="Recording a voice message">
  <button type="button" class="icon" title="Discard" aria-label="Discard recording" onclick={cancel}>
    <Icon name="trash" size={20} />
  </button>
  {#if paused}
    <button
      type="button"
      class="icon"
      title="Record over"
      aria-label="Discard and record again"
      onclick={recordOver}><Icon name="back10" size={18} /></button>
    <button
      type="button"
      class="icon preview"
      title={previewing ? "Stop" : "Listen"}
      aria-label={previewing ? "Stop listening" : "Listen to the recording"}
      onclick={togglePreview}><Icon name={previewing ? "pause" : "play"} size={18} filled /></button>
    <input
      class="scrub"
      type="range"
      min="0"
      max={elapsed}
      step="0.05"
      value={previewAt}
      aria-label="Preview position"
      oninput={(e) => {
        if (preview) preview.currentTime = Number(e.currentTarget.value);
      }} />
    <span class="time">{clock(previewing ? previewAt : elapsed)}</span>
  {:else}
    <span class="dot"></span>
    <span class="time">{clock(elapsed)}</span>
    <span class="levels" aria-hidden="true">
      {#each Array(LIVE_BARS - live.length) as _, i (i)}<span class="level idle"></span>{/each}
      {#each live as level, i (i)}<span
          class="level"
          class:gap={level === PAUSE}
          style="height: {level === PAUSE ? 100 : Math.max(8, (level / top) * 100)}%"></span
        >{/each}
    </span>
  {/if}
  <audio
    bind:this={preview}
    ontimeupdate={() => preview && (previewAt = preview.currentTime)}
    onended={finishPreview}></audio>
  <button
    type="button"
    class="icon pause"
    title={paused ? "Resume" : "Pause"}
    aria-label={paused ? "Resume recording" : "Pause recording"}
    disabled={!ready}
    onclick={togglePause}><Icon name={paused ? "mic" : "pause"} size={20} filled={!paused} /></button>
  <button
    type="button"
    class="icon once"
    class:on={once}
    title="View once"
    aria-label="View once"
    aria-pressed={once}
    onclick={() => (once = !once)}>1</button>
  <button type="button" class="send" title="Send" aria-label="Send voice message" disabled={!ready} onclick={finish}>
    <Icon name="send" size={18} />
  </button>
</div>

<style>
  .recorder {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    min-height: 52px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }
  .pause {
    color: var(--danger);
  }
  .once {
    border: 2px dashed currentColor;
    width: 28px;
    height: 28px;
    font-size: 12px;
    font-weight: 700;
  }
  .once.on {
    color: var(--accent);
    border-style: solid;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--danger);
    animation: blink 1s ease-in-out infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.25;
    }
  }
  .time {
    min-width: 42px;
    font-size: 16px;
    font-variant-numeric: tabular-nums;
  }
  .levels {
    display: flex;
    align-items: center;
    gap: 2px;
    width: 180px;
    height: 28px;
  }
  .level {
    flex: 1;
    border-radius: 2px;
    background: var(--muted);
  }
  .level.idle {
    height: 3px;
    background: var(--faint);
  }
  .level.gap {
    flex: 0 0 2px;
    background: var(--danger);
    opacity: 0.6;
  }
  .preview {
    color: var(--accent);
  }
  .scrub {
    width: 180px;
    accent-color: var(--accent);
  }
  .send {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-ink);
    cursor: pointer;
  }
  .send:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
