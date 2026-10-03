/**
 * Per-device Accessibility preferences and the Accessibility-mode master preset.
 *
 * Persisted in localStorage (like keybinds and zoom), applied to
 * `document.documentElement` as `data-a11y-*` attributes and `--a11y-*` /
 * `--font-scale` CSS variables. OS preferences (`prefers-reduced-motion`,
 * `prefers-contrast`, `prefers-color-scheme` via the theme) are honoured when
 * a setting is `"system"`.
 */

export type TriState = "off" | "on" | "system";
export type TargetSize = "comfortable" | "large";
export type AnnounceMode = "off" | "concise" | "detailed";
export type FontChoice = "system" | "legible";

export interface AccessibilityPrefs {
  /** Master preset. When on, every pref below holds its most accessible value. */
  enabled: boolean;
  /** True once the first-launch prompt has been answered (enable or skip). */
  promptSeen: boolean;
  /** Snapshot of prefs before Accessibility mode was enabled, for reversible off. */
  snapshot: AccessibilityPrefs | null;
  reduceMotion: TriState;
  /** GIFs / autoplaying stickers / videos become tap-to-play (WCAG 2.2.2). */
  pauseAnimatedMedia: boolean;
  highContrast: TriState;
  reduceTransparency: boolean;
  targetSize: TargetSize;
  alwaysShowFocus: boolean;
  enhancedFocus: boolean;
  colorBlindPalette: boolean;
  /** 100–200, applied as a rem-based `--font-scale`. */
  textScale: number;
  /** WCAG 1.4.12 spacing (line-height / letter / word). */
  textSpacing: boolean;
  fontChoice: FontChoice;
  showShortcutHints: boolean;
  /** WCAG 2.1.4: single-character shortcuts can be turned off. */
  charShortcutsEnabled: boolean;
  /** Media autoplay; off in Accessibility mode. */
  autoplayVideos: boolean;
  announcements: AnnounceMode;
}

const KEY = "postal.accessibility";

export const ACCESSIBLE_VALUES: Omit<AccessibilityPrefs, "enabled" | "promptSeen" | "snapshot"> = {
  reduceMotion: "on",
  pauseAnimatedMedia: true,
  highContrast: "on",
  reduceTransparency: true,
  targetSize: "large",
  alwaysShowFocus: true,
  enhancedFocus: true,
  colorBlindPalette: true,
  textScale: 125,
  textSpacing: false,
  fontChoice: "legible",
  showShortcutHints: true,
  charShortcutsEnabled: false,
  autoplayVideos: false,
  announcements: "detailed",
};

export const DEFAULTS: Omit<AccessibilityPrefs, "snapshot"> = {
  enabled: false,
  promptSeen: false,
  reduceMotion: "system",
  pauseAnimatedMedia: false,
  highContrast: "system",
  reduceTransparency: false,
  targetSize: "comfortable",
  alwaysShowFocus: false,
  enhancedFocus: false,
  colorBlindPalette: false,
  textScale: 100,
  textSpacing: false,
  fontChoice: "system",
  showShortcutHints: true,
  charShortcutsEnabled: true,
  autoplayVideos: true,
  announcements: "concise",
};

function clampTextScale(v: unknown): number {
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isFinite(n)) return 100;
  return Math.min(200, Math.max(100, Math.round(n)));
}

function sanitize(raw: unknown): AccessibilityPrefs {
  const base: AccessibilityPrefs = { ...DEFAULTS, snapshot: null };
  if (!raw || typeof raw !== "object") return base;
  const r = raw as Record<string, unknown>;
  const tri = (v: unknown, fb: TriState): TriState => (v === "off" || v === "on" || v === "system" ? v : fb);
  const bool = (v: unknown, fb: boolean): boolean => (typeof v === "boolean" ? v : fb);
  let snapshot: AccessibilityPrefs | null = null;
  if (r.snapshot && typeof r.snapshot === "object") {
    try {
      snapshot = sanitize(r.snapshot);
      snapshot.snapshot = null;
    } catch {
      snapshot = null;
    }
  }
  return {
    enabled: bool(r.enabled, false),
    promptSeen: bool(r.promptSeen, false),
    snapshot,
    reduceMotion: tri(r.reduceMotion, DEFAULTS.reduceMotion),
    pauseAnimatedMedia: bool(r.pauseAnimatedMedia, false),
    highContrast: tri(r.highContrast, DEFAULTS.highContrast),
    reduceTransparency: bool(r.reduceTransparency, false),
    targetSize: r.targetSize === "large" ? "large" : "comfortable",
    alwaysShowFocus: bool(r.alwaysShowFocus, false),
    enhancedFocus: bool(r.enhancedFocus, false),
    colorBlindPalette: bool(r.colorBlindPalette, false),
    textScale: clampTextScale(r.textScale),
    textSpacing: bool(r.textSpacing, false),
    fontChoice: r.fontChoice === "legible" ? "legible" : "system",
    showShortcutHints: bool(r.showShortcutHints, true),
    charShortcutsEnabled: bool(r.charShortcutsEnabled, true),
    autoplayVideos: bool(r.autoplayVideos, true),
    announcements: r.announcements === "off" || r.announcements === "concise" || r.announcements === "detailed"
      ? r.announcements
      : "concise",
  };
}

function load(): AccessibilityPrefs {
  try {
    return sanitize(JSON.parse(localStorage.getItem(KEY) ?? "null"));
  } catch {
    return { ...DEFAULTS, snapshot: null };
  }
}

export const accessibility: AccessibilityPrefs = $state(load());

export function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(accessibility));
  } catch {
    // Storage can be full or blocked; the session keeps working unsaved.
  }
}

function withoutSnapshot(p: AccessibilityPrefs): Omit<AccessibilityPrefs, "snapshot"> {
  const { snapshot: _omit, ...rest } = p;
  return rest;
}

/** Enable the master preset, keeping a snapshot so disabling restores prior values. */
export function enableAccessibilityMode() {
  if (!accessibility.snapshot) {
    accessibility.snapshot = { ...withoutSnapshot(accessibility), snapshot: null };
  }
  Object.assign(accessibility, { ...ACCESSIBLE_VALUES, enabled: true, promptSeen: true });
  save();
  applyAccessibility();
}

/** Disable the preset and restore the values from before it was enabled. */
export function disableAccessibilityMode() {
  const snap = accessibility.snapshot;
  if (snap) {
    const { snapshot: _omit, ...rest } = snap;
    Object.assign(accessibility, { ...rest, enabled: false, snapshot: null });
  } else {
    accessibility.enabled = false;
  }
  save();
  applyAccessibility();
}

export function setAccessibilityMode(on: boolean) {
  if (on) enableAccessibilityMode();
  else disableAccessibilityMode();
}

// --- OS integration ---------------------------------------------------------

const motionQuery = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
const contrastQuery = typeof matchMedia === "function"
  ? (matchMedia("(prefers-contrast: more)") ?? null)
  : null;

export function osReducedMotion(): boolean {
  return motionQuery?.matches ?? false;
}

export function osHighContrast(): boolean {
  return contrastQuery?.matches ?? false;
}

export function wantsReducedMotion(p = accessibility): boolean {
  if (p.reduceMotion === "on") return true;
  if (p.reduceMotion === "off") return false;
  return osReducedMotion();
}

export function wantsHighContrast(p = accessibility): boolean {
  if (p.highContrast === "on") return true;
  if (p.highContrast === "off") return false;
  return osHighContrast();
}

export function effectiveAnnouncements(p = accessibility): AnnounceMode {
  return p.announcements;
}

// --- Colour helpers (high-contrast token transform + ratio checks) -----------

function parseColor(value: string): [number, number, number, number] | null {
  const v = value.trim();
  const hex = /^#([0-9a-f]{6})$/i.exec(v);
  if (hex) {
    const n = Number.parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const hex3 = /^#([0-9a-f]{3})$/i.exec(v);
  if (hex3) {
    const [r, g, b] = hex3[1].split("").map((c) => Number.parseInt(c + c, 16));
    return [r, g, b, 1];
  }
  const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(v);
  if (m) return [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : Number(m[4])];
  const hsl = /^hsla?\(\s*([\d.]+)\s*[, ]\s*([\d.]+)%\s*[, ]\s*([\d.]+)%/i.exec(v);
  if (hsl) {
    const h = ((Number(hsl[1]) % 360) + 360) % 360 / 360;
    const s = Math.min(1, Number(hsl[2]) / 100);
    const l = Math.min(1, Number(hsl[3]) / 100);
    const f = (n: number) => {
      const k = (n + h * 12) % 12;
      const a = s * Math.min(l, 1 - l);
      return Math.round((l - a * Math.max(-1, Math.min(k - 3, Math.min(9 - k, 1)))) * 255);
    };
    return [f(0), f(8), f(4), 1];
  }
  if (v === "white" || v === "#fff" || v === "#ffffff") return [255, 255, 255, 1];
  if (v === "black" || v === "#000" || v === "#000000") return [0, 0, 0, 1];
  if (v === "transparent") return [0, 0, 0, 0];
  return null;
}

function luminance([r, g, b]: [number, number, number]): number {
  const f = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** WCAG contrast ratio of two opaque colours. */
export function contrastRatio(a: string, b: string): number | null {
  const ca = parseColor(a);
  const cb = parseColor(b);
  if (!ca || !cb) return null;
  const la = luminance([ca[0], ca[1], ca[2]]);
  const lb = luminance([cb[0], cb[1], cb[2]]);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

function toHex(r: number, g: number, b: number): string {
  return "#" + [r, g, b].map((v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0")).join("");
}

/** Move `fg` away from `bg` until `ratio` is met (or the extremum is hit). */
export function ensureContrast(fg: string, bg: string, ratio: number): string {
  const parsed = parseColor(fg);
  const bgParsed = parseColor(bg);
  if (!parsed || !bgParsed) return fg;
  if ((contrastRatio(fg, bg) ?? 0) >= ratio) return fg;
  const bgLum = luminance([bgParsed[0], bgParsed[1], bgParsed[2]]);
  // Dark background -> lighten towards white; light background -> darken towards black.
  const target: [number, number, number] = bgLum < 0.4 ? [255, 255, 255] : [0, 0, 0];
  let [r, g, b] = [parsed[0], parsed[1], parsed[2]];
  for (let i = 0; i < 24; i++) {
    r += (target[0] - r) * 0.25;
    g += (target[1] - g) * 0.25;
    b += (target[2] - b) * 0.25;
    const candidate = toHex(r, g, b);
    if ((contrastRatio(candidate, bg) ?? 0) >= ratio) return candidate;
  }
  return toHex(target[0], target[1], target[2]);
}

/**
 * High-contrast token transform layered over the active theme (not a separate
 * theme). Pushes text tokens to 4.5:1 and non-text tokens to 3:1 against
 * their surfaces, and removes translucency.
 */
export function highContrastTokens(tokens: Record<string, string>): Record<string, string> {
  const out = { ...tokens };
  const bg = tokens["chat-bg"] ?? tokens.bg ?? "#000000";
  const surface = tokens.surface ?? bg;
  const bubble = tokens.bubble ?? surface;
  const mine = tokens["bubble-mine"] ?? bubble;
  const scheme = tokens.scheme ?? "dark";
  const light = scheme === "light";
  // Opaque surfaces first: translucency cannot guarantee contrast.
  for (const key of ["bg", "chat-bg", "surface", "raised", "raised-2", "bubble", "bubble-mine"] as const) {
    const c = parseColor(out[key] ?? "");
    if (c && c[3] < 1) {
      out[key] = toHex(c[0], c[1], c[2]);
    }
  }
  const bg2 = out["chat-bg"] ?? bg;
  const surface2 = out.surface ?? surface;
  const bubble2 = out.bubble ?? bubble;
  const mine2 = out["bubble-mine"] ?? mine;
  out.text = ensureContrast(out.text ?? (light ? "#111b21" : "#e9edef"), bg2, 7);
  out.muted = ensureContrast(out.muted ?? "#667781", bg2, 4.5);
  out.faint = ensureContrast(out.faint ?? "#8696a0", surface2, 4.5);
  out.link = ensureContrast(out.link ?? "#53bdeb", bubble2, 4.5);
  out.accent = ensureContrast(out.accent ?? "#00a884", bg2, 3);
  out["accent-hover"] = ensureContrast(out["accent-hover"] ?? out.accent, bg2, 3);
  out["accent-text"] = ensureContrast(out["accent-text"] ?? out.accent, bg2, 4.5);
  out.danger = ensureContrast(out.danger ?? "#f15c6d", bg2, 4.5);
  out.mention = ensureContrast(out.mention ?? "#f0b232", bg2, 4.5);
  out["mention-pill"] = ensureContrast(out["mention-pill"] ?? out.link, bubble2, 4.5);
  out.replying = ensureContrast(out.replying ?? "#00a884", bg2, 3);
  out.line = ensureContrast(out.line ?? "#222d34", bg2, 3);
  out["line-soft"] = out.line;
  out["line-strong"] = ensureContrast(out["line-strong"] ?? "#3b4a54", bg2, 3);
  // Status ticks reuse the link colour; keep them in sync so the transform
  // cannot drift them apart.
  if (out.link) out["status-tick"] = out.link;
  void mine2;
  return out;
}

// --- DOM application ----------------------------------------------------------

/** Apply prefs to the document root. Idempotent; safe to call on every change. */
export function applyAccessibility(p = accessibility) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const reduced = wantsReducedMotion(p);
  const contrast = wantsHighContrast(p);
  root.dataset.a11yMode = p.enabled ? "on" : "off";
  root.dataset.a11yMotion = reduced ? "reduce" : "full";
  root.dataset.a11yContrast = contrast ? "more" : "no-preference";
  root.dataset.a11yTransparency = p.reduceTransparency ? "reduce" : "full";
  root.dataset.a11yTargets = p.targetSize;
  root.dataset.a11yFocus = p.alwaysShowFocus ? "always" : "auto";
  root.dataset.a11yFocusThick = p.enhancedFocus ? "on" : "off";
  root.dataset.a11yColorBlind = p.colorBlindPalette ? "on" : "off";
  root.dataset.a11ySpacing = p.textSpacing ? "on" : "off";
  root.dataset.a11yFont = p.fontChoice;
  root.dataset.a11yHints = p.showShortcutHints ? "on" : "off";
  root.dataset.a11yAnnounce = p.announcements;
  root.dataset.a11yAutoplay = p.autoplayVideos ? "on" : "off";
  root.dataset.a11yPauseMedia = p.pauseAnimatedMedia ? "on" : "off";
  const scale = clampTextScale(p.textScale) / 100;
  root.style.setProperty("--font-scale", String(scale));
  // Root font size is the rem base: text scales, media (px-sized) does not.
  root.style.setProperty("--a11y-font-scale", String(scale));
  root.style.fontSize = `${16 * scale}px`;
  if (p.textSpacing) {
    root.style.setProperty("--a11y-line-height", "1.5");
    root.style.setProperty("--a11y-letter-spacing", "0.12em");
    root.style.setProperty("--a11y-word-spacing", "0.16em");
  } else {
    root.style.removeProperty("--a11y-line-height");
    root.style.removeProperty("--a11y-letter-spacing");
    root.style.removeProperty("--a11y-word-spacing");
  }
  root.style.setProperty("--a11y-target-min", p.targetSize === "large" ? "44px" : "24px");
  root.classList.toggle("a11y-reduce-motion", reduced);
  root.classList.toggle("a11y-high-contrast", contrast);
  root.classList.toggle("a11y-reduce-transparency", p.reduceTransparency);
  root.classList.toggle("a11y-large-targets", p.targetSize === "large");
  root.classList.toggle("a11y-always-focus", p.alwaysShowFocus);
  root.classList.toggle("a11y-focus-thick", p.enhancedFocus);
  root.classList.toggle("a11y-color-blind", p.colorBlindPalette);
  root.classList.toggle("a11y-text-spacing", p.textSpacing);
  root.classList.toggle("a11y-legible-font", p.fontChoice === "legible");
  // Motion is honoured even before the theme layer runs: mirror the theme's
  // `no-motion` contract here.
  if (reduced) {
    root.classList.add("no-motion");
    root.style.setProperty("--motion-scale", "0");
  } else {
    root.classList.remove("no-motion");
    root.style.removeProperty("--motion-scale");
  }
}

if (typeof window !== "undefined") {
  motionQuery?.addEventListener?.("change", () => applyAccessibility());
  contrastQuery?.addEventListener?.("change", () => applyAccessibility());
}

// --- Screen-reader announcements ------------------------------------------------

export interface Announcement {
  text: string;
  assertive?: boolean;
}

type AnnounceListener = (a: Announcement) => void;
const announceListeners = new Set<AnnounceListener>();

export function onAnnouncement(fn: AnnounceListener): () => void {
  announceListeners.add(fn);
  return () => { announceListeners.delete(fn); };
}

function emit(text: string, assertive = false) {
  const message = text.trim();
  if (!message) return;
  for (const fn of announceListeners) {
    try { fn({ text: message, assertive }); } catch { /* one listener must not break others */ }
  }
}

/**
 * Announce a new incoming message. Suppressed when announcements are off or
 * while the initial sync backlog is still landing (`gateReady === false`),
 * so the backlog is not read message by message.
 */
export function announceMessage(sender: string, preview: string, gateReady: boolean, opts?: { mention?: boolean }) {
  const mode = effectiveAnnouncements();
  if (mode === "off" || !gateReady) return;
  const who = sender.trim() || "Unknown chat";
  const what = preview.trim().slice(0, 220) || "New message";
  if (mode === "concise" && !opts?.mention) {
    emit(`New message from ${who}`);
  } else {
    emit(opts?.mention ? `Mention from ${who}: ${what}` : `New message from ${who}: ${what}`);
  }
}

export function announceTyping(sender: string, gateReady: boolean) {
  if (effectiveAnnouncements() !== "detailed" || !gateReady || !sender.trim()) return;
  emit(`${sender.trim()} is typing`);
}

export function announceStatus(text: string, gateReady: boolean, assertive = false) {
  if (effectiveAnnouncements() === "off" || !gateReady) return;
  emit(text, assertive);
}

/** Meaningful alt text for media: sender + media kind + caption. */
export function mediaAltText(sender: string, kind: string, caption: string | null | undefined): string {
  const who = sender.trim() || "Unknown sender";
  const what = { image: "photo", video: "video", gif: "GIF", audio: "voice message", sticker: "sticker", document: "document" }[kind] ?? kind;
  const cap = (caption ?? "").trim();
  return cap ? `${who} sent ${what}: ${cap.slice(0, 220)}` : `${who} sent ${what}`;
}
