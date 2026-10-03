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
/**
 * The picture behind the app, stored in IndexedDB, darkened by `dim` (0–1);
 * `v` changes when the picture does. `image` is where older builds kept it.
 */
export type Background = { dim: number; v?: number; image?: string };
export type Density = "compact" | "comfortable" | "cozy";
type Saved = {
  theme: string;
  themes: Theme[];
  extensions: Extension[];
  background?: Background | null;
  density?: Density;
  /** Chat list width in pixels. */
  listWidth?: number;
  /** Chats with their own picture, stored in IndexedDB; `v` changes when the picture does. */
  chatBackgrounds?: Record<string, { dim: number; v: number }>;
};

/** A picture file downscaled to `max` px as a JPEG data URL. */
export async function pictureDataUrl(file: File, max = 1920): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, max / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext("2d")!.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/jpeg", 0.82);
}

// Per-chat pictures live in IndexedDB: several would overflow localStorage,
// and a failed save there would also drop theme changes.
function pictureStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("hermodr-pictures", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("pictures");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const db = open.result;
      let transaction: IDBTransaction | undefined;
      try {
        transaction = db.transaction("pictures", mode);
        let result!: T;
        let failure: DOMException | null = null;
        transaction.oncomplete = () => { db.close(); resolve(result); };
        transaction.onerror = transaction.onabort = () => { db.close(); reject(transaction?.error ?? failure ?? new Error("Picture transaction aborted.")); };
        const request = run(transaction.objectStore("pictures"));
        request.onsuccess = () => { result = request.result; };
        request.onerror = () => { failure = request.error; };
      } catch (error) {
        try { transaction?.abort(); } catch {}
        db.close();
        reject(error);
      }
    };
  });
}
const APP_PICTURE = "*app";
export const appPicture = () => pictureStore<string | undefined>("readonly", (s) => s.get(APP_PICTURE));
export async function setAppPicture(image: string) {
  await pictureStore("readwrite", (s) => s.put(image, APP_PICTURE));
  customization.background = { dim: customization.background?.dim ?? 0.25, v: Date.now() };
}
export async function removeAppPicture() {
  await pictureStore("readwrite", (s) => s.delete(APP_PICTURE));
  customization.background = null;
}

export const chatPicture = (jid: string) => pictureStore<string | undefined>("readonly", (s) => s.get(jid));
export async function setChatPicture(jid: string, image: string) {
  await pictureStore("readwrite", (s) => s.put(image, jid));
  customization.chatBackgrounds = {
    ...customization.chatBackgrounds,
    [jid]: { dim: customization.chatBackgrounds?.[jid]?.dim ?? 0.25, v: Date.now() },
  };
}
export async function removeChatPicture(jid: string) {
  await pictureStore("readwrite", (s) => s.delete(jid));
  const { [jid]: _, ...rest } = customization.chatBackgrounds ?? {};
  customization.chatBackgrounds = rest;
}

/**
 * Displacement map for the glass lens, one axis per colour channel: 128 is
 * no shift, and the rim ramps to the extremes so the backdrop bends inward at
 * the edges, as Apple's Liquid Glass does. Used by the `#glass-*` filters in +page.
 */
export function lensMap(axis: "x" | "y") {
  const color = (v: number) => (axis === "x" ? `rgb(${v},0,0)` : `rgb(0,${v},0)`);
  const stops: [number, number][] = [
    [0, 255], [0.05, 205], [0.14, 150], [0.26, 128], [0.74, 128], [0.86, 106], [0.95, 50], [1, 0],
  ];
  return (
    "data:image/svg+xml;utf8," +
    encodeURIComponent(
      `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100' preserveAspectRatio='none'>` +
        `<linearGradient id='g' x2='${axis === "x" ? 1 : 0}' y2='${axis === "y" ? 1 : 0}'>` +
        stops.map(([at, v]) => `<stop offset='${at}' stop-color='${color(v)}'/>`).join("") +
        `</linearGradient><rect width='100' height='100' fill='url(#g)'/></svg>`,
    )
  );
}

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
/* The lens: engines without SVG backdrop filters (WebKitGTK) keep the plain blur above,
   or none for the pieces below. */
.menu, .sheet, .modal, .intro-card, .attach-menu, .account-menu, .card {
  backdrop-filter: url(#glass-lg) blur(4px) saturate(170%) brightness(1.06);
}
.bubble, .reply-preview {
  backdrop-filter: url(#glass-md) blur(2px) saturate(165%) brightness(1.05);
}
.conversation header, .composer > textarea, .search, .chip, .day span, .send.ready {
  backdrop-filter: url(#glass-sm) blur(2px) saturate(165%) brightness(1.05);
}
.menu, .sheet, .modal, .intro-card, .attach-menu, .account-menu, .search, .chip, .bubble {
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.14), inset 0 1px 0 rgba(255, 255, 255, 0.22),
    0 6px 24px rgba(0, 0, 0, 0.22) !important;
}
.embed, .quote { box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.16); }
.bubble.first::before { display: none !important; }
.bubble { border-radius: var(--radius-sm) !important; }
.send.ready, .badge:not(.mention-badge) {
  background: linear-gradient(180deg, var(--accent-hover), var(--accent)) !important;
  color: var(--accent-ink) !important;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35), 0 4px 14px color-mix(in srgb, var(--accent) 45%, transparent);
}
.composer > textarea { border-radius: 999px !important; box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12); }

/* The wallpaper layer moves by transform alone, which the compositor does without repainting. */
body::before, .stage::before {
  will-change: transform;
  animation: glass-drift calc(40s * var(--motion-scale)) ease-in-out infinite alternate;
}
body:has(.backdrop, .sheet-backdrop)::before { animation-play-state: paused; }
@keyframes glass-drift {
  from { transform: translate3d(0, 0, 0) scale(1); }
  50% { transform: translate3d(4%, -3%, 0) scale(1.06); }
  to { transform: translate3d(-3%, 4%, 0) scale(1.03); }
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

const KEY = "postal.customization";
const systemScheme = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;
let systemDark = $state(systemScheme?.matches ?? false);
systemScheme?.addEventListener("change", (event) => { systemDark = event.matches; });

function load(): Saved {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (saved && Array.isArray(saved.themes) && Array.isArray(saved.extensions)) return saved;
  } catch {
    // Unreadable storage falls back to the defaults.
  }
  return { theme: "system", themes: [], extensions: [] };
}

export const customization: Saved = $state(load());

// Older builds kept the app picture in localStorage; move it to IndexedDB.
const legacyPicture = customization.background?.image;
if (legacyPicture) void setAppPicture(legacyPicture).catch(() => {});

export function allThemes(): Theme[] {
  const preset = BUILT_IN.find((theme) => theme.id === (systemDark ? "dark" : "light"))!;
  return [{ ...preset, id: "system", name: "System" }, ...BUILT_IN, ...customization.themes];
}

export function activeTheme(): Theme {
  return allThemes().find((t) => t.id === customization.theme) ?? BUILT_IN[0];
}

export function isBuiltIn(theme: Theme) {
  return theme.id === "system" || BUILT_IN.some((t) => t.id === theme.id);
}

export function themeCss(theme: Theme): string {
  const variables = TOKENS.filter(({ key }) => theme.tokens[key])
    .map(({ key }) => `  --${key}: ${theme.tokens[key]} !important;`).join("\n");
  return `:root {\n${variables}\n}\n${theme.css ?? ""}`;
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

/** Reads the accessibility reduce-motion preference without importing the store (avoids a cycle). */
function accessibilityWantsReducedMotion(): boolean | null {
  try {
    const raw = JSON.parse(localStorage.getItem("postal.accessibility") ?? "null");
    if (!raw || typeof raw !== "object") return null;
    if (raw.reduceMotion === "on") return true;
    if (raw.reduceMotion === "off") return false;
    return null;
  } catch {
    return null;
  }
}

/** Reads the accessibility high-contrast preference without importing the store. */
function accessibilityWantsHighContrast(): boolean {
  try {
    const raw = JSON.parse(localStorage.getItem("postal.accessibility") ?? "null");
    if (!raw || typeof raw !== "object") return false;
    if (raw.highContrast === "on") return true;
    if (raw.highContrast === "off") return false;
  } catch {
    return false;
  }
  return false;
}

/** The theme's `motion-scale`, or 0 when reduced motion is requested (setting or OS). */
function motionScale(theme: Theme): number {
  const pref = accessibilityWantsReducedMotion();
  if (pref === true) return 0;
  if (pref === false) {
    // Explicit "off" keeps theme motion even under an OS reduce request.
  } else if (reducedMotion?.matches) {
    return 0;
  }
  const scale = Number.parseFloat(theme.tokens["motion-scale"] ?? "1");
  return Number.isFinite(scale) && scale >= 0 ? scale : 1;
}

/** A Svelte transition length scaled by the active theme's `motion-scale`. */
export function motion(ms: number): number {
  return ms * motionScale(activeTheme());
}

/**
 * Writes the theme onto the document root, clearing tokens it does not set.
 * At scale 0 `no-motion` stops decorative animations with fixed durations.
 * Progress spinners independently honor the OS reduced-motion preference.
 * When the accessibility high-contrast preference (or its OS "system"
 * fallback) is active, tokens pass through the high-contrast transform first.
 */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  let tokens = theme.tokens;
  if (wantsHighContrastTokens()) {
    tokens = highContrastTransform(tokens);
  }
  for (const { key } of TOKENS) {
    const value = (tokens as Record<string, string>)[key];
    if (value) root.style.setProperty(`--${key}`, value);
    else root.style.removeProperty(`--${key}`);
  }
  const scale = motionScale(theme);
  // The accessibility layer owns motion when it requests reduce; otherwise the
  // theme scale applies.
  if (typeof document !== "undefined" && document.documentElement.dataset.a11yMotion === "reduce") {
    root.style.setProperty("--motion-scale", "0");
    root.classList.add("no-motion");
  } else {
    root.style.setProperty("--motion-scale", String(scale));
    root.classList.toggle("no-motion", scale === 0);
  }
}

/** Whether the high-contrast token transform applies right now. */
function wantsHighContrastTokens(): boolean {
  try {
    const raw = JSON.parse(localStorage.getItem("postal.accessibility") ?? "null");
    if (raw && typeof raw === "object") {
      if (raw.highContrast === "on") return true;
      if (raw.highContrast === "off") return false;
    }
  } catch {
    // Fall through to the OS preference.
  }
  if (typeof matchMedia === "function") {
    try {
      if (matchMedia("(prefers-contrast: more)").matches) return true;
    } catch {
      // Unsupported query; no transform.
    }
  }
  return accessibilityWantsHighContrast();
}

/** Local high-contrast transform (mirrors the accessibility store; avoids a module cycle). */
function highContrastTransform(tokens: Record<string, string>): Record<string, string> {
  const parse = (value: string): [number, number, number] | null => {
    const v = value.trim();
    const h = /^#([0-9a-f]{6})$/i.exec(v);
    if (h) {
      const n = Number.parseInt(h[1], 16);
      return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    }
    const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(v);
    return m ? [+m[1], +m[2], +m[3]] : null;
  };
  const lum = ([r, g, b]: [number, number, number]): number => {
    const f = (c: number) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const ratio = (a: string, b: string): number => {
    const ca = parse(a);
    const cb = parse(b);
    if (!ca || !cb) return 0;
    const la = lum(ca);
    const lb = lum(cb);
    const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
    return (hi + 0.05) / (lo + 0.05);
  };
  const hex = (r: number, g: number, b: number): string =>
    "#" + [r, g, b].map((x) => Math.max(0, Math.min(255, Math.round(x))).toString(16).padStart(2, "0")).join("");
  const ensure = (fg: string, bg: string, want: number): string => {
    if (ratio(fg, bg) >= want) return fg;
    const cb = parse(bg) ?? [0, 0, 0];
    const dark = lum(cb) < 0.4;
    const t: [number, number, number] = dark ? [255, 255, 255] : [0, 0, 0];
    const cf = parse(fg) ?? [128, 128, 128];
    let [r, g, b] = cf;
    for (let i = 0; i < 24; i++) {
      r += (t[0] - r) * 0.25;
      g += (t[1] - g) * 0.25;
      b += (t[2] - b) * 0.25;
      const c = hex(r, g, b);
      if (ratio(c, bg) >= want) return c;
    }
    return hex(t[0], t[1], t[2]);
  };
  const out = { ...tokens };
  for (const key of ["bg", "chat-bg", "surface", "raised", "raised-2", "bubble", "bubble-mine"] as const) {
    const c = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)$/i.exec(out[key] ?? "");
    if (c) out[key] = hex(+c[1], +c[2], +c[3]);
  }
  const bg = out["chat-bg"] ?? out.bg ?? "#000000";
  const surface = out.surface ?? bg;
  const bubble = out.bubble ?? surface;
  const light = (out.scheme ?? "dark") === "light";
  out.text = ensure(out.text ?? (light ? "#111b21" : "#e9edef"), bg, 7);
  out.muted = ensure(out.muted ?? "#667781", bg, 4.5);
  out.faint = ensure(out.faint ?? "#8696a0", surface, 4.5);
  out.link = ensure(out.link ?? "#53bdeb", bubble, 4.5);
  out.accent = ensure(out.accent ?? "#00a884", bg, 3);
  out["accent-hover"] = ensure(out["accent-hover"] ?? out.accent, bg, 3);
  out["accent-text"] = ensure(out["accent-text"] ?? out.accent, bg, 4.5);
  out.danger = ensure(out.danger ?? "#f15c6d", bg, 4.5);
  out.mention = ensure(out.mention ?? "#f0b232", bg, 4.5);
  out["mention-pill"] = ensure(out["mention-pill"] ?? out.link, bubble, 4.5);
  out.replying = ensure(out.replying ?? "#00a884", bg, 3);
  out.line = ensure(out.line ?? "#222d34", bg, 3);
  out["line-soft"] = out.line;
  out["line-strong"] = ensure(out["line-strong"] ?? "#3b4a54", bg, 3);
  return out;
}

reducedMotion?.addEventListener("change", () => applyTheme(activeTheme()));

export function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(customization));
  } catch {
    // Storage can be full or blocked; the session keeps working unsaved.
  }
}
