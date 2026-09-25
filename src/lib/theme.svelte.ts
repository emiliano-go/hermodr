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
export type Theme = { id: string; name: string; tokens: Tokens };
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
  const copy = { id: newId("theme"), name, tokens: { ...theme.tokens } };
  customization.themes.push(copy);
  customization.theme = copy.id;
  return customization.themes[customization.themes.length - 1];
}

/** A Svelte transition length scaled by the active theme's `motion-scale`. */
export function motion(ms: number): number {
  const scale = Number.parseFloat(activeTheme().tokens["motion-scale"] ?? "1");
  return Number.isFinite(scale) && scale >= 0 ? ms * scale : ms;
}

/** Writes the theme onto the document root, clearing tokens it does not set. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement.style;
  for (const { key } of TOKENS) {
    const value = theme.tokens[key];
    if (value) root.setProperty(`--${key}`, value);
    else root.removeProperty(`--${key}`);
  }
}

export function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(customization));
  } catch {
    // Storage can be full or blocked; the session keeps working unsaved.
  }
}
