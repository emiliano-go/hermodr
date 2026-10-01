import { ACTIONS, format, keybinds, label, matches } from "../utils/keybinds.svelte.ts";
import type { Binding } from "../utils/keybinds.svelte.ts";
import type { SoundClip } from "./library.ts";

export function soundBinding(slot: number | null): Binding | null {
  return slot !== null && Number.isInteger(slot) && slot >= 1 && slot <= 9
    ? { key: String(slot), ctrl: true, alt: true, shift: false, meta: false } : null;
}

export function soundShortcutLabel(slot: number | null): string { const binding = soundBinding(slot); return binding ? label(binding) : "None"; }

export function soundShortcutConflict(slot: number | null, clips: SoundClip[], id?: string): string | null {
  const binding = soundBinding(slot);
  if (!binding) return null;
  const action = ACTIONS.find(({ id }) => format(keybinds[id]) === format(binding));
  if (action) return `This shortcut is assigned to ${action.label.toLowerCase()}.`;
  if (clips.some((clip) => clip.id !== id && clip.shortcut === slot)) return "This shortcut is assigned to another clip.";
  return null;
}

export function shortcutClip(event: KeyboardEvent, clips: SoundClip[]): SoundClip | null {
  if (event.defaultPrevented || event.repeat || event.isComposing || event.keyCode === 229 || event.getModifierState?.("AltGraph")) return null;
  const clip = clips.find((clip) => { const binding = soundBinding(clip.shortcut); return binding && matches(event, binding); });
  return clip && !soundShortcutConflict(clip.shortcut, clips, clip.id) ? clip : null;
}
