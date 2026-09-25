/** Every CSS variable the UI is drawn with, as the Customization panel lists them. */
export const TOKENS = [
  { key: "bg", label: "Sidebar", group: "Surfaces" },
  { key: "chat-bg", label: "Chat background", group: "Surfaces" },
  { key: "surface", label: "Headers and bars", group: "Surfaces" },
  { key: "raised", label: "Hover and inputs", group: "Surfaces" },
  { key: "raised-2", label: "Selected", group: "Surfaces" },
  { key: "scrim", label: "Dialog backdrop", group: "Surfaces" },
  { key: "shadow", label: "Popup shadow", group: "Surfaces" },
  { key: "bubble", label: "Incoming bubble", group: "Bubbles" },
  { key: "bubble-mine", label: "Outgoing bubble", group: "Bubbles" },
  { key: "text", label: "Text", group: "Text" },
  { key: "muted", label: "Secondary text", group: "Text" },
  { key: "faint", label: "Faint text", group: "Text" },
  { key: "link", label: "Links and read ticks", group: "Text" },
  { key: "mention", label: "Mentions of you: bar and text", group: "Highlights" },
  { key: "mention-soft", label: "Mentions of you: row", group: "Highlights" },
  { key: "mention-self-soft", label: "Mentions of you: tag", group: "Highlights" },
  { key: "mention-pill", label: "Mention tag text", group: "Highlights" },
  { key: "mention-pill-soft", label: "Mention tag", group: "Highlights" },
  { key: "replying", label: "Replying to: bar", group: "Highlights" },
  { key: "replying-soft", label: "Replying to: row", group: "Highlights" },
  { key: "jump-soft", label: "Jumped-to message", group: "Highlights" },
  { key: "row-hover", label: "Message hover", group: "Highlights" },
  { key: "accent", label: "Accent", group: "Accent" },
  { key: "accent-hover", label: "Accent hover", group: "Accent" },
  { key: "accent-text", label: "Accent text", group: "Accent" },
  { key: "accent-ink", label: "Text on accent", group: "Accent" },
  { key: "accent-soft", label: "Accent tint", group: "Accent" },
  { key: "line", label: "Dividers", group: "Lines" },
  { key: "line-soft", label: "Soft dividers", group: "Lines" },
  { key: "line-strong", label: "Borders", group: "Lines" },
  { key: "danger", label: "Error", group: "Status" },
  { key: "danger-soft", label: "Error background", group: "Status" },
  { key: "radius-sm", label: "Bubble radius", group: "Shape and type" },
  { key: "radius", label: "Input radius", group: "Shape and type" },
  { key: "radius-lg", label: "Dialog radius", group: "Shape and type" },
  { key: "font", label: "Font family", group: "Shape and type" },
  { key: "font-size", label: "Font size", group: "Shape and type" },
  { key: "scheme", label: "Native controls (dark/light)", group: "Shape and type" },
  { key: "motion-scale", label: "Animation length (1 normal, 0 off, 2 slower)", group: "Motion" },
  { key: "ease", label: "Easing curve", group: "Motion" },
] as const;

export type Tokens = Record<string, string>;
/**
 * `css` is a structural layer (blur, shapes) that tokens alone cannot express;
 * `wallpaper` is a CSS background drawn behind the translucent surfaces.
 */
export type Theme = { id: string; name: string; tokens: Tokens; css?: string; wallpaper?: string };
export type Extension = { id: string; name: string; css: string; enabled: boolean };
type Saved = { theme: string; themes: Theme[]; extensions: Extension[] };

const shape = {
  "radius-sm": "7.5px",
  radius: "8px",
  "radius-lg": "10px",
  font: '"Segoe UI", "Helvetica Neue", system-ui, sans-serif',
  "font-size": "14.2px",
  "motion-scale": "1",
  ease: "cubic-bezier(0.2, 0.8, 0.2, 1)",
};

const GLASS_WALLPAPER =
  "radial-gradient(1100px 760px at 8% 0%, #5b3cc4 0%, transparent 62%), " +
  "radial-gradient(900px 700px at 100% 100%, #0a84ff 0%, transparent 58%), " +
  "radial-gradient(700px 560px at 85% 8%, rgba(255, 55, 95, 0.45) 0%, transparent 60%), #0b0d1a";

/**
 * Translucent layers over a vivid wallpaper, with a specular top edge. Edges
 * are inset shadows, never borders, so the theme does not move anything.
 * Only small floating surfaces blur: the wallpaper is already soft, and
 * re-blurring whole panels over a moving layer every frame is what lags.
 */
const GLASS_CSS = `
.menu, .sheet, .modal, .intro-card, .attach-menu, .account-menu, .card {
  backdrop-filter: blur(24px) saturate(170%);
  -webkit-backdrop-filter: blur(24px) saturate(170%);
}
.menu, .sheet, .modal, .intro-card, .attach-menu, .account-menu, .search, .chip, .bubble {
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.14), inset 0 1px 0 rgba(255, 255, 255, 0.22),
    0 6px 24px rgba(0, 0, 0, 0.22) !important;
}
.embed, .quote { box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.16); }
.bubble.first::before { display: none !important; }
.bubble { border-radius: var(--radius-sm) !important; }
.send.ready, .badge:not(.mention-badge) {
  background: linear-gradient(180deg, #409cff, #0a84ff) !important;
  color: #fff !important;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35), 0 4px 14px rgba(10, 132, 255, 0.45);
}
.composer > textarea { border-radius: 999px !important; box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12); }

/* The wallpaper layer moves by transform alone, which the compositor does without repainting. */
body::before, .stage::before {
  will-change: transform;
  animation: glass-drift calc(40s * var(--motion-scale)) ease-in-out infinite alternate;
}
body:has(.backdrop, .sheet-backdrop)::before { animation-play-state: paused; }
@keyframes glass-drift {
  from { transform: translate3d(0, 0, 0) rotate(0deg) scale(1); }
  50% { transform: translate3d(5%, -4%, 0) rotate(9deg) scale(1.1); }
  to { transform: translate3d(-4%, 5%, 0) rotate(-7deg) scale(1.05); }
}
.bubble, .chip, .menu .item, .chat-row {
  background-image: linear-gradient(115deg, transparent 35%, rgba(255, 255, 255, 0.14) 50%, transparent 65%) !important;
  background-size: 260% 100% !important;
  background-repeat: no-repeat !important;
  background-position: 120% 0 !important;
  transition: background-position calc(0.9s * var(--motion-scale)) cubic-bezier(0.32, 0.72, 0, 1) !important;
}
.msg-row:hover .bubble, .chip:hover, .menu .item:hover, .chat-row:hover {
  background-position: -20% 0 !important;
}
.icon, .chip, .send, .badge, .confirm-actions button {
  transition: transform calc(0.4s * var(--motion-scale)) cubic-bezier(0.34, 1.56, 0.64, 1),
    background-color calc(0.2s * var(--motion-scale)) ease !important;
}
.icon:hover, .chip:hover, .send:hover { transform: translateY(-1px) scale(1.06); }
.icon:active, .chip:active, .send:active, .confirm-actions button:active { transform: scale(0.94) !important; }
.menu:not(.closing), .sheet, .attach-menu, .account-menu {
  animation: glass-in calc(0.38s * var(--motion-scale)) cubic-bezier(0.34, 1.4, 0.64, 1) both !important;
}
@keyframes glass-in {
  from { opacity: 0; transform: scale(0.9); filter: blur(8px); }
}
`;

/** Material 3: tonal surfaces, no borders or tails, pill controls and a FAB-like send button. */
const MATERIAL_CSS = `
.bubble { border-radius: var(--radius-sm) !important; box-shadow: none !important; }
.bubble.first::before { display: none !important; }
.bubble:not(.mine) { border-bottom-left-radius: 6px !important; }
.bubble:not(.mine):not(.first) { border-top-left-radius: 6px !important; }
.bubble.mine { border-bottom-right-radius: 6px !important; }
.bubble.mine:not(.first) { border-top-right-radius: 6px !important; }
.chats ul { padding: 0 8px !important; }
.chat-row { border-radius: 16px !important; }
.chat-row::after { display: none !important; }
.chip { border-radius: 8px !important; box-shadow: inset 0 0 0 1px var(--line-strong); }
.chip.active { box-shadow: none; }
.send.ready { border-radius: 16px !important; background: var(--bubble-mine) !important; color: var(--accent-hover) !important; }
.composer > textarea { border-radius: 28px !important; }
.primary, .button.primary, .confirm-actions button { border-radius: 999px !important; }
.day span { border-radius: 999px !important; box-shadow: none !important; }
.quote, .embed { border-radius: 12px !important; }
`;

export const BUILT_IN: Theme[] = [
  {
    id: "dark",
    name: "Dark",
    tokens: {
      bg: "#111b21",
      "chat-bg": "#0b141a",
      surface: "#202c33",
      raised: "#2a3942",
      "raised-2": "#374248",
      scrim: "rgba(0, 0, 0, 0.6)",
      shadow: "0 2px 12px rgba(0, 0, 0, 0.45)",
      bubble: "#202c33",
      "bubble-mine": "#005c4b",
      text: "#e9edef",
      muted: "#8696a0",
      faint: "#667781",
      link: "#53bdeb",
      mention: "#f0b232",
      "mention-soft": "rgba(240, 178, 50, 0.1)",
      "mention-self-soft": "rgba(240, 178, 50, 0.24)",
      "mention-pill": "#53bdeb",
      "mention-pill-soft": "rgba(83, 189, 235, 0.18)",
      replying: "#00a884",
      "replying-soft": "rgba(0, 168, 132, 0.16)",
      "jump-soft": "rgba(0, 168, 132, 0.3)",
      "row-hover": "rgba(233, 237, 239, 0.03)",
      accent: "#00a884",
      "accent-hover": "#06cf9c",
      "accent-text": "#00a884",
      "accent-ink": "#111b21",
      "accent-soft": "rgba(0, 168, 132, 0.18)",
      line: "#222d34",
      "line-soft": "#1d282f",
      "line-strong": "#3b4a54",
      danger: "#f15c6d",
      "danger-soft": "#3b1e24",
      ...shape,
      scheme: "dark",
    },
  },
  {
    id: "light",
    name: "Light",
    tokens: {
      bg: "#ffffff",
      "chat-bg": "#efeae2",
      surface: "#f0f2f5",
      raised: "#e9edef",
      "raised-2": "#d1d7db",
      scrim: "rgba(11, 20, 26, 0.4)",
      shadow: "0 2px 12px rgba(11, 20, 26, 0.16)",
      bubble: "#ffffff",
      "bubble-mine": "#d9fdd3",
      text: "#111b21",
      muted: "#667781",
      faint: "#8696a0",
      link: "#027eb5",
      mention: "#c98a00",
      "mention-soft": "rgba(201, 138, 0, 0.1)",
      "mention-self-soft": "rgba(201, 138, 0, 0.2)",
      "mention-pill": "#027eb5",
      "mention-pill-soft": "rgba(2, 126, 181, 0.12)",
      replying: "#00a884",
      "replying-soft": "rgba(0, 168, 132, 0.12)",
      "jump-soft": "rgba(0, 168, 132, 0.22)",
      "row-hover": "rgba(17, 27, 33, 0.04)",
      accent: "#00a884",
      "accent-hover": "#008069",
      "accent-text": "#008069",
      "accent-ink": "#ffffff",
      "accent-soft": "rgba(0, 168, 132, 0.14)",
      line: "#e9edef",
      "line-soft": "#f0f2f5",
      "line-strong": "#d1d7db",
      danger: "#ea0038",
      "danger-soft": "#fde8ec",
      ...shape,
      scheme: "light",
    },
  },
  {
    id: "midnight",
    name: "Midnight",
    tokens: {
      bg: "#000000",
      "chat-bg": "#000000",
      surface: "#121212",
      raised: "#1c1c1c",
      "raised-2": "#2a2a2a",
      scrim: "rgba(0, 0, 0, 0.7)",
      shadow: "0 2px 12px rgba(0, 0, 0, 0.6)",
      bubble: "#161616",
      "bubble-mine": "#01453a",
      text: "#ececec",
      muted: "#8a8a8a",
      faint: "#5f5f5f",
      link: "#53bdeb",
      mention: "#f0b232",
      "mention-soft": "rgba(240, 178, 50, 0.1)",
      "mention-self-soft": "rgba(240, 178, 50, 0.24)",
      "mention-pill": "#53bdeb",
      "mention-pill-soft": "rgba(83, 189, 235, 0.18)",
      replying: "#00a884",
      "replying-soft": "rgba(0, 168, 132, 0.16)",
      "jump-soft": "rgba(0, 168, 132, 0.3)",
      "row-hover": "rgba(236, 236, 236, 0.04)",
      accent: "#00a884",
      "accent-hover": "#06cf9c",
      "accent-text": "#00a884",
      "accent-ink": "#000000",
      "accent-soft": "rgba(0, 168, 132, 0.18)",
      line: "#1a1a1a",
      "line-soft": "#111111",
      "line-strong": "#333333",
      danger: "#f15c6d",
      "danger-soft": "#2b1215",
      ...shape,
      scheme: "dark",
    },
  },
  {
    id: "glass",
    name: "Liquid Glass",
    css: GLASS_CSS,
    wallpaper: GLASS_WALLPAPER,
    tokens: {
      bg: "rgba(18, 20, 38, 0.42)",
      "chat-bg": "rgba(10, 12, 26, 0.18)",
      surface: "rgba(255, 255, 255, 0.08)",
      raised: "rgba(255, 255, 255, 0.13)",
      "raised-2": "rgba(255, 255, 255, 0.2)",
      scrim: "rgba(4, 6, 16, 0.35)",
      shadow: "0 10px 40px rgba(0, 0, 0, 0.35)",
      bubble: "rgba(255, 255, 255, 0.12)",
      "bubble-mine": "rgba(10, 132, 255, 0.55)",
      text: "#f5f7ff",
      muted: "rgba(235, 240, 255, 0.68)",
      faint: "rgba(235, 240, 255, 0.45)",
      link: "#7cd4ff",
      mention: "#ffd60a",
      "mention-soft": "rgba(255, 214, 10, 0.12)",
      "mention-self-soft": "rgba(255, 214, 10, 0.26)",
      "mention-pill": "#7cd4ff",
      "mention-pill-soft": "rgba(124, 212, 255, 0.2)",
      replying: "#64b5ff",
      "replying-soft": "rgba(10, 132, 255, 0.18)",
      "jump-soft": "rgba(10, 132, 255, 0.3)",
      "row-hover": "rgba(255, 255, 255, 0.05)",
      accent: "#0a84ff",
      "accent-hover": "#409cff",
      "accent-text": "#7cc0ff",
      "accent-ink": "#ffffff",
      "accent-soft": "rgba(10, 132, 255, 0.24)",
      line: "rgba(255, 255, 255, 0.08)",
      "line-soft": "rgba(255, 255, 255, 0.05)",
      "line-strong": "rgba(255, 255, 255, 0.16)",
      danger: "#ff453a",
      "danger-soft": "rgba(255, 69, 58, 0.18)",
      ...shape,
      "radius-sm": "18px",
      radius: "14px",
      "radius-lg": "22px",
      font: '"SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
      ease: "cubic-bezier(0.32, 0.72, 0, 1)",
      scheme: "dark",
    },
  },
  {
    id: "material",
    name: "Material 3",
    css: MATERIAL_CSS,
    tokens: {
      bg: "#171d19",
      "chat-bg": "#0f1511",
      surface: "#1b211d",
      raised: "#252b27",
      "raised-2": "#303632",
      scrim: "rgba(0, 0, 0, 0.5)",
      shadow: "0 1px 3px rgba(0, 0, 0, 0.3), 0 4px 8px 3px rgba(0, 0, 0, 0.15)",
      bubble: "#252b27",
      "bubble-mine": "#005139",
      text: "#dfe4dd",
      muted: "#bfc9c1",
      faint: "#8a938c",
      link: "#a4cddd",
      mention: "#e7c26c",
      "mention-soft": "rgba(231, 194, 108, 0.1)",
      "mention-self-soft": "rgba(231, 194, 108, 0.22)",
      "mention-pill": "#a4cddd",
      "mention-pill-soft": "rgba(164, 205, 221, 0.16)",
      replying: "#8bd6b4",
      "replying-soft": "rgba(139, 214, 180, 0.14)",
      "jump-soft": "rgba(139, 214, 180, 0.26)",
      "row-hover": "rgba(223, 228, 221, 0.04)",
      accent: "#8bd6b4",
      "accent-hover": "#a6f2cf",
      "accent-text": "#8bd6b4",
      "accent-ink": "#003826",
      "accent-soft": "rgba(139, 214, 180, 0.16)",
      line: "#252b27",
      "line-soft": "#1d231f",
      "line-strong": "#404943",
      danger: "#ffb4ab",
      "danger-soft": "#5c1a17",
      ...shape,
      "radius-sm": "20px",
      radius: "16px",
      "radius-lg": "28px",
      font: '"Google Sans Text", "Roboto Flex", Roboto, "Segoe UI", system-ui, sans-serif',
      "font-size": "14.5px",
      ease: "cubic-bezier(0.2, 0, 0, 1)",
      scheme: "dark",
    },
  },
];

const KEY = "hermodr.customization";

function load(): Saved {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (saved && Array.isArray(saved.themes) && Array.isArray(saved.extensions)) return saved;
  } catch {
    // Unreadable storage falls back to the defaults.
  }
  return { theme: "dark", themes: [], extensions: [] };
}

export const customization: Saved = $state(load());

export function allThemes(): Theme[] {
  return [...BUILT_IN, ...customization.themes];
}

export function activeTheme(): Theme {
  return allThemes().find((t) => t.id === customization.theme) ?? BUILT_IN[0];
}

export function isBuiltIn(theme: Theme) {
  return BUILT_IN.some((t) => t.id === theme.id);
}

export function newId(prefix: string) {
  return `${prefix}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}

/** Copies a theme into an editable one and makes it active. */
export function duplicate(theme: Theme, name = `${theme.name} copy`): Theme {
  const copy = { id: newId("theme"), name, tokens: { ...theme.tokens }, css: theme.css, wallpaper: theme.wallpaper };
  customization.themes.push(copy);
  customization.theme = copy.id;
  return customization.themes[customization.themes.length - 1];
}

const reducedMotion = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;

/** The theme's `motion-scale`, or 0 when the OS asks for reduced motion. */
function motionScale(theme: Theme): number {
  if (reducedMotion?.matches) return 0;
  const scale = Number.parseFloat(theme.tokens["motion-scale"] ?? "1");
  return Number.isFinite(scale) && scale >= 0 ? scale : 1;
}

/** A Svelte transition length scaled by the active theme's `motion-scale`. */
export function motion(ms: number): number {
  return ms * motionScale(activeTheme());
}

/**
 * Writes the theme onto the document root, clearing tokens it does not set.
 * At scale 0 `no-motion` also stops the animations whose durations are fixed
 * (spinners, loading bars, shimmer), which `--motion-scale` cannot reach.
 */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  for (const { key } of TOKENS) {
    const value = theme.tokens[key];
    if (value) root.style.setProperty(`--${key}`, value);
    else root.style.removeProperty(`--${key}`);
  }
  const scale = motionScale(theme);
  root.style.setProperty("--motion-scale", String(scale));
  root.classList.toggle("no-motion", scale === 0);
}

reducedMotion?.addEventListener("change", () => applyTheme(activeTheme()));

export function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(customization));
  } catch {
    // Storage can be full or blocked; the session keeps working unsaved.
  }
}
