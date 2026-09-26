/**
 * Composer keybindings, editable from Settings and persisted on this device.
 *
 * A binding is stored as a short string like `Ctrl+ArrowUp` so it survives in
 * localStorage readably. Matching is exact on the modifier set, so `ArrowUp`
 * and `Ctrl+ArrowUp` never collide.
 */

export type Action = "cancelReply" | "historyPrev" | "historyNext" | "editLast";

export type Binding = {
  key: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
};

export const ACTIONS: { id: Action; label: string; description: string }[] = [
  { id: "cancelReply", label: "Cancel reply / edit", description: "Clears a staged reply or an in-progress edit." },
  { id: "historyPrev", label: "Previous sent message", description: "Recalls the previous message you sent into the composer." },
  { id: "historyNext", label: "Next sent message", description: "Walks forward through recalled messages." },
  { id: "editLast", label: "Edit last message", description: "Loads your last sent message into the composer for editing." },
];

const DEFAULTS: Record<Action, string> = {
  cancelReply: "Escape",
  historyPrev: "ArrowUp",
  historyNext: "ArrowDown",
  editLast: "Ctrl+ArrowUp",
};

const KEY = "hermodr.keybinds";

function parse(text: string): Binding {
  const parts = text.split("+").map((p) => p.trim());
  const key = parts.pop() || "Escape";
  const mods = parts.map((p) => p.toLowerCase());
  return {
    key,
    ctrl: mods.includes("ctrl"),
    alt: mods.includes("alt"),
    shift: mods.includes("shift"),
    meta: mods.includes("meta") || mods.includes("cmd") || mods.includes("super"),
  };
}

/** Modifier order matches how the label is shown. */
export function format(binding: Binding): string {
  const parts: string[] = [];
  if (binding.ctrl) parts.push("Ctrl");
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  if (binding.meta) parts.push("Meta");
  parts.push(binding.key);
  return parts.join("+");
}

/** A short label for buttons, with arrows and escape prettified. */
export function label(binding: Binding): string {
  const names: Record<string, string> = {
    Escape: "Esc",
    ArrowUp: "↑",
    ArrowDown: "↓",
    ArrowLeft: "←",
    ArrowRight: "→",
    " ": "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Del",
  };
  const parts: string[] = [];
  if (binding.ctrl) parts.push("Ctrl");
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  if (binding.meta) parts.push("Meta");
  const key = binding.key.length === 1 ? binding.key.toUpperCase() : binding.key;
  parts.push(names[binding.key] ?? key);
  return parts.join("+");
}

function load(): Record<Action, Binding> {
  const saved: Partial<Record<Action, string>> = {};
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (raw && typeof raw === "object") {
      for (const { id } of ACTIONS) if (typeof raw[id] === "string") saved[id] = raw[id];
    }
  } catch {
    // Unreadable storage falls back to the defaults.
  }
  const result = {} as Record<Action, Binding>;
  for (const { id } of ACTIONS) result[id] = parse(saved[id] ?? DEFAULTS[id]);
  return result;
}

export const keybinds: Record<Action, Binding> = $state(load());

export function save() {
  try {
    const raw: Record<string, string> = {};
    for (const { id } of ACTIONS) raw[id] = format(keybinds[id]);
    localStorage.setItem(KEY, JSON.stringify(raw));
  } catch {
    // Storage can be full or blocked; the session keeps working unsaved.
  }
}

export function setBinding(action: Action, binding: Binding) {
  keybinds[action] = binding;
  save();
}

export function resetBindings() {
  for (const { id } of ACTIONS) keybinds[id] = parse(DEFAULTS[id]);
  save();
}

export function resetBinding(action: Action) {
  keybinds[action] = parse(DEFAULTS[action]);
  save();
}

export function isDefault(action: Action): boolean {
  return format(keybinds[action]) === DEFAULTS[action];
}

/** Turns a keydown into a binding, or `null` while only modifiers are held. */
export function bindingFromEvent(event: KeyboardEvent): Binding | null {
  if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return null;
  return {
    key: event.key.length === 1 ? event.key.toLowerCase() : event.key,
    ctrl: event.ctrlKey,
    alt: event.altKey,
    shift: event.shiftKey,
    meta: event.metaKey,
  };
}

function same(a: Binding | null, b: Binding): boolean {
  if (!a) return false;
  return a.key === b.key && a.ctrl === b.ctrl && a.alt === b.alt && a.shift === b.shift && a.meta === b.meta;
}

export function matches(event: KeyboardEvent, binding: Binding): boolean {
  return same(bindingFromEvent(event), binding);
}

/** Actions that share a binding, as a set of action ids. */
export function conflicting(): Set<Action> {
  const seen = new Map<string, Action>();
  const conflicts = new Set<Action>();
  for (const { id } of ACTIONS) {
    const text = format(keybinds[id]);
    const other = seen.get(text);
    if (other) {
      conflicts.add(other);
      conflicts.add(id);
    } else {
      seen.set(text, id);
    }
  }
  return conflicts;
}
