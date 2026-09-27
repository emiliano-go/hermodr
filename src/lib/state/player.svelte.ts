// Voice-note playback. The engine lives outside the message list, so a note
// keeps playing when its chat is left; AudioPlayer and NowPlaying drive it.
import { invoke } from "$lib/ipc";

export const BARS = 42;
const RATES = [1, 1.5, 2];
const RATE_KEY = "postal.voiceRate";
const HEARD_KEY = "postal.heardVoice";

export type VoiceShape = { peaks: number[]; duration: number };

/** What a play request needs: the file, how to label it, and its reactions. */
export type VoiceTrack = {
  path: string;
  /** Stored length, until the file decodes. */
  duration: number | null;
  avatar: string | null;
  initials: string;
  /** Sender name for the sidebar player. */
  title: string;
  /** The next note in a chain: cue, brief pause, then play. */
  autoplay?: boolean;
  onplayed?: () => void;
  onended?: () => void;
  onpaused?: () => void;
};

function savedRate() {
  try {
    const rate = Number(localStorage.getItem(RATE_KEY));
    return RATES.includes(rate) ? rate : 1;
  } catch {
    return 1;
  }
}

/** Notes already played here; unplayed ones keep WhatsApp's green. */
function loadHeard() {
  try {
    return new Set<string>(JSON.parse(localStorage.getItem(HEARD_KEY) ?? "[]"));
  } catch {
    return new Set<string>();
  }
}

/** Type the media element expects, from the stored file extension. */
function mime(path: string) {
  const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  if (extension === "mp3") return "audio/mpeg";
  if (extension === "m4a" || extension === "aac" || extension === "mp4") return "audio/mp4";
  if (extension === "wav") return "audio/wav";
  return "audio/ogg";
}

async function shapeOf(bytes: Uint8Array): Promise<VoiceShape> {
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

class PlayerState {
  track = $state<VoiceTrack | null>(null);
  paused = $state(true);
  loading = $state(false);
  failed = $state(false);
  position = $state(0);
  rate = $state(savedRate());
  /** Decoded length once the waveform is ready; the stored one covers before that. */
  duration = $state(0);
  /** Decoded shape per file, so scrolling back does not decode again. */
  shapes = $state<Record<string, VoiceShape>>({});

  #audio: HTMLAudioElement | null = null;
  #url: string | null = null;
  #seq = 0;
  #frame = 0;
  #heard = loadHeard();
  #cues: AudioContext | null = null;

  get progress() {
    return this.duration ? Math.min(1, this.position / this.duration) : 0;
  }

  heard(path: string) {
    return this.#heard.has(path);
  }

  /** A short cue between chained notes, and a different one when a chain ends. */
  playCue(kind: "next" | "end") {
    try {
      const context = (this.#cues ??= new AudioContext());
      if (context.state === "suspended") void context.resume();
      // Rising pair into the next note, falling pair when the queue is done.
      const tones = kind === "next" ? [660, 880] : [494, 330];
      const now = context.currentTime;
      tones.forEach((frequency, i) => {
        const at = now + i * 0.09;
        const oscillator = context.createOscillator();
        const gain = context.createGain();
        oscillator.type = "sine";
        oscillator.frequency.value = frequency;
        gain.gain.setValueAtTime(0.0001, at);
        gain.gain.exponentialRampToValueAtTime(0.2, at + 0.012);
        gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.085);
        oscillator.connect(gain).connect(context.destination);
        oscillator.start(at);
        oscillator.stop(at + 0.1);
      });
    } catch {
      // The cue is a nicety; playback continues without it.
    }
  }

  #element() {
    if (this.#audio) return this.#audio;
    const audio = (this.#audio = new Audio());
    audio.preload = "metadata";
    audio.ontimeupdate = () => {
      if (!this.loading) this.position = audio.currentTime;
    };
    audio.onerror = () => {
      // Only the source on screen now; a revoked previous one is not a failure.
      if (!this.track || !this.#url || audio.src !== this.#url || !audio.error) return;
      this.failed = true;
      this.paused = true;
    };
    audio.onpause = () => {
      // A pause queued while another note was starting is stale.
      if (!audio.paused) return;
      this.paused = true;
      cancelAnimationFrame(this.#frame);
      this.track?.onpaused?.();
    };
    audio.onended = () => {
      // An ended queued by the previous source is stale.
      if (!audio.ended) return;
      this.paused = true;
      this.position = 0;
      cancelAnimationFrame(this.#frame);
      this.track?.onended?.();
    };
    return audio;
  }

  /** Plays a note, resuming it when it is already the loaded one. */
  async play(track: VoiceTrack) {
    const audio = this.#element();
    if (this.track?.path === track.path && !this.failed) {
      // Refresh callbacks and label: this may be a new component for the same
      // file after the chat was reopened.
      this.track = track;
      if (this.loading || !this.paused) return;
      this.paused = false;
      audio.playbackRate = this.rate;
      try {
        await audio.play();
        this.#follow();
      } catch {
        // A pause during the handshake rejects with AbortError; a real media
        // failure arrives through onerror instead.
        this.paused = true;
      }
      return;
    }

    const seq = ++this.#seq;
    // A chained note waits through its cue before starting.
    if (track.autoplay) {
      this.playCue("next");
      await new Promise((resolve) => setTimeout(resolve, 300));
      if (seq !== this.#seq) return;
    }
    audio.pause();
    this.#revoke();
    this.track = track;
    this.paused = true;
    this.loading = true;
    this.failed = false;
    this.position = 0;
    this.duration = track.duration ?? 0;
    try {
      const data = await invoke<string>("read_file", { path: track.path });
      if (seq !== this.#seq) return;
      const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
      const cached = this.shapes[track.path];
      if (cached) this.duration = cached.duration;
      else void this.#decodeShape(track.path, bytes);
      this.#url = URL.createObjectURL(new Blob([bytes], { type: mime(track.path) }));
      audio.src = this.#url;
      audio.playbackRate = this.rate;
      await audio.play();
      if (seq !== this.#seq) {
        audio.pause();
        return;
      }
      this.paused = false;
      if (!this.#heard.has(track.path)) {
        this.#heard.add(track.path);
        this.#saveHeard();
        track.onplayed?.();
      }
      this.#follow();
    } catch {
      if (seq === this.#seq) {
        this.failed = true;
        this.paused = true;
      }
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }

  toggle() {
    if (!this.track) return;
    if (!this.paused) this.#element().pause();
    else void this.play(this.track);
  }

  pause() {
    this.#audio?.pause();
  }

  /** Seeks by fraction of the length; needs the length to be known. */
  seek(fraction: number) {
    const audio = this.#audio;
    if (!audio || !this.duration) return;
    audio.currentTime = Math.min(1, Math.max(0, fraction)) * this.duration;
    this.position = audio.currentTime;
  }

  cycleRate() {
    this.rate = RATES[(RATES.indexOf(this.rate) + 1) % RATES.length];
    if (this.#audio) this.#audio.playbackRate = this.rate;
    try {
      localStorage.setItem(RATE_KEY, String(this.rate));
    } catch {
      // The speed lasts this session then.
    }
  }

  /** Closes the player: stops, unloads and forgets the current note. */
  stop() {
    this.#seq++;
    const track = this.track;
    this.#audio?.pause();
    if (this.#audio) {
      this.#audio.removeAttribute("src");
      this.#audio.load();
    }
    this.#revoke();
    cancelAnimationFrame(this.#frame);
    this.track = null;
    this.paused = true;
    this.loading = false;
    this.failed = false;
    this.position = 0;
    this.duration = 0;
    // Closing is not pausing: an autoplay chain must not survive it.
    track?.onpaused?.();
  }

  #follow() {
    const audio = this.#audio;
    if (!audio) return;
    cancelAnimationFrame(this.#frame);
    const step = () => {
      if (!audio || audio.paused) return;
      this.position = audio.currentTime;
      this.#frame = requestAnimationFrame(step);
    };
    this.#frame = requestAnimationFrame(step);
  }

  async #decodeShape(path: string, bytes: Uint8Array) {
    try {
      const shape = await shapeOf(bytes);
      this.shapes[path] = shape;
      if (this.track?.path === path) this.duration = shape.duration;
    } catch {
      // The stored length still covers the note.
    }
  }

  #saveHeard() {
    try {
      localStorage.setItem(HEARD_KEY, JSON.stringify([...this.#heard].slice(-1000)));
    } catch {
      // Only this session remembers it then.
    }
  }

  #revoke() {
    if (!this.#url) return;
    URL.revokeObjectURL(this.#url);
    this.#url = null;
  }
}

export const player = new PlayerState();
