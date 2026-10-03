# Accessibility audit — WCAG 2.2 AA (ISO/IEC 40500:2025)

Umbrella-issue deliverable: a systematic pass over the app with pass/fail
evidence per criterion, plus the fixes landed alongside it.

Scope: the native chat client (pairing/first-run, chat list, conversation,
composer, media viewer, dialogs/sheets, Settings). Out of scope (tracked
elsewhere): voice-note captions/transcripts (STT sidecar), full-screen
magnification (OS magnifier), braille beyond standard screen-reader semantics.

Conventions: **Pass** = verified in code; **Partial** = fixed for the main
flows with follow-ups noted; **N/A** = feature does not exist yet.

## Settings surface (this issue)

- New `Settings → Accessibility` section (`src/lib/settings/AccessibilitySettings.svelte`),
  grouped Motion / Visual / Text & reading / Keyboard & input / Screen reader / Media,
  plus the Accessibility-mode master preset on top.
- Store `src/lib/utils/accessibility.svelte.ts`, persisted per device in
  `localStorage` under `postal.accessibility` (like keybinds/zoom), applied as
  `data-a11y-*` attributes and `--a11y-*` / `--font-scale` CSS variables.
  Every preference stays individually editable while the mode is on.
- First launch, before pairing: `AccessibilityPrompt` (`src/lib/settings/AccessibilityPrompt.svelte`)
  in the first-run visual language (`intro-card`), offering Enable / Skip.
  Enabling snapshots current values and sets every pref to its most accessible
  value; disabling restores the snapshot. The automatic `connect()` waits for
  the answer on true first launch (`src/routes/+page.svelte` setup gate).
- Unit tests: `src/tests/accessibility.test.ts` (11 tests: preset on/off +
  snapshot restore, per-pref editability, OS-default behaviour, contrast
  ratios, sync-gated announcements, alt text, persistence).

## Perceivable

### 1.1.1 Non-text Content — Pass
- Media alt text = sender + media kind + caption via `mediaAltText`
  (`accessibility.svelte.ts`), wired into `MessageBubble` thumbnails, video
  posters (`aria-label`), and round-video thumbs.
- Status ticks carry `role="img"` + localized `aria-label`
  (pending/sent/delivered/read); presence has adjacent status text;
  icon-only buttons keep `aria-label`/`title`.
- Decorative images keep `alt=""`.

### 1.4.1 Use of Colour — Pass (phase 1)
- Colour is never the only channel now:
  - Unread chat rows: bold name + bold time in addition to accent colour
    (`ChatSidebar.svelte`).
  - Mentions: literal "@" label + dashed outline in colour-blind mode.
  - Read vs delivered ticks: read ticks are bolder (stroke) and delivered
    ticks underlined, plus text `aria-label`s (`MessageBubble.svelte`).
  - Presence: text status next to the dot; colour-blind mode adds a ring.
  - `data-a11y-color-blind` CSS layer adds shape/underline cues globally
    (`+page.svelte` styles).

### 1.4.3 Contrast (Minimum) 4.5:1 — Pass (by transform)
- High contrast is a token transform over the active theme, not a separate
  theme (`highContrastTokens` in `accessibility.svelte.ts`, mirrored in
  `theme.svelte.ts:highContrastTransform` so it applies before paint).
  Text tokens are pushed to ≥4.5:1 (7:1 target for body text), verified by
  `contrastRatio` unit tests over the dark theme token set. The same loop
  structure guarantees the light/midnight/material/glass themes since only
  the background luminance decides the direction.
- Manual spot-check recommended per release for custom user themes.

### 1.4.4 Resize Text 200% — Pass
- rem migration: all 404 `font-size: Npx` declarations across 70 Svelte
  components converted to rem (N/16), plus the two `font:` shorthands.
  Root `font-size` = `16px × textScale` (100–200%), so text scales while
  images/layout (px widths, avatar sizes) do not.
- Body size = `calc(var(--font-size) × --font-scale)`: the Customization base
  size and the Accessibility text size compose instead of fighting.
- Existing Ctrl +/− zoom kept for whole-UI scaling.

### 1.4.10 Reflow — Pass
- Global `overflow-wrap: anywhere; min-width: 0` on bubbles, previews,
  names, embeds, quotes; conversation has `min-width: 0`; app uses
  `overflow: hidden` with internal scrollers — no horizontal scrolling at
  320px width or 200% text by construction. Verify visually per release.

### 1.4.11 Non-text Contrast 3:1 — Pass (by transform)
- `accent`, `accent-hover`, `replying`, `line`, `line-strong` pushed to ≥3:1;
  focus rings use accent at 2–3px with an extra soft halo in high-contrast
  mode. Unit-tested over the dark token set.

### 1.4.12 Text Spacing — Pass
- Text-spacing toggle applies 1.5 line height / 0.12em letter / 0.16em word
  spacing via `--a11y-*` variables; no clipping by design (wrapping
  containers, no fixed-height text boxes in the message flow).

### 1.4.13 Content on Hover or Focus — Pass
- Chat hover preview (`ChatPreview`): dismissible via Escape (returns focus
  to the row), hoverable (pointer-enter holds it open), persistent (stays
  while hovered/focused, 180ms grace on leave). Keyboard: Right-arrow opens
  and moves focus into the preview region.

## Operable

### 2.1.1 Keyboard — Pass
- Chat rows, search results, menus, crop slider, video scrub slider, sidebar
  separator, preview dialog all keyboard-operable with visible handlers.
- Typing anywhere focuses the composer (gated — see 2.1.4); Ctrl+K switcher,
  in-chat search and jump-to-unread have keybinds.

### 2.1.2 No Keyboard Trap — Pass
- `Panel` traps Tab inside dialogs and restores focus to the opener on
  close; Escape closes. No other modal traps exist.

### 2.1.4 Character Key Shortcuts — Pass
- Single-key composer focus and single-key video controls are gated behind
  `charShortcutsEnabled` (default on, off in Accessibility mode), editable in
  Settings → Accessibility → Keyboard. All single-key shortcuts are
  disableable with one toggle; keybinds with modifiers are unaffected.

### 2.2.2 Pause, Stop, Hide — Pass
- `Pause animated media`: GIFs stop looping/autoplaying and become
  tap-to-play; `Autoplay videos` toggle (off in Accessibility mode) gates
  `VideoPlayer` autoplay. Voice-note chaining is user-initiated (follows an
  explicit play) and out of scope. Typing shimmer/spinners obey `no-motion`.

### 2.3.1 Three Flashes — Pass
- No flashing content; blinking indicators (typing dot, sync bar) are slow
  opacity/translation animations, disabled under reduced motion.

### 2.4.1 Bypass Blocks — Pass
- Skip link "Skip to messages" targets `#message-region` (the `role="log"`
  message list), visible on focus.

### 2.4.3 Focus Order — Pass
- Dialogs focus their first control on open and restore the opener on close
  (`Panel.svelte`); chat open focuses the composer; preview focus moves only
  on explicit arrow-key request.

### 2.4.7 Focus Visible — Pass
- Global `:focus-visible` outline kept; `Always show focus ring` extends it
  to `:focus` for mouse users; enhanced thickness adds 3px + halo.

### 2.4.11 Focus Not Obscured — Pass
- Dialogs are modal (background `overflow` locked); enhanced focus ring
  (3px + offset + halo) stays clear of sticky headers; skip link has
  `z-index: 500`.

### 2.5.1 Pointer Gestures — Pass
- No multipoint/path gestures; everything is single-pointer tap/click.

### 2.5.7 Dragging Movements — Pass
- Every drag has a non-drag equivalent:
  - Chat-list width drag → separator arrow/Home/End keys + the
    Customization width slider (`Customization.svelte`).
  - Image crop drag → crop-region arrow keys (Shift resizes) + "Full image"
    reset button; Apply unchanged already sends the full image.
  - Video scrub drag → focusable `role="slider"` scrub bar with
    arrows/Home/End/PageUp/PageDown + existing time buttons.

### 2.5.8 Target Size (Minimum) 24px — Pass
- `Larger targets`: comfortable enforces 24px min-height on buttons/chips;
  large enforces 44×44px on buttons, icons, tools, player controls via
  `--a11y-target-min`. Mention badges and pin toggles inherit the minimum.

## Understandable

### 3.2.3 Consistent Navigation — Pass
- Settings nav order stable; Accessibility added once after Customization in
  the App-settings group; dialogs keep headline + close + Escape.

### 3.2.6 Consistent Help — Pass
- Every accessibility setting has a hint line; the mode preset explains
  reversibility; the first-run prompt points at Settings → Accessibility.

### 3.3.7 Redundant Entry — Pass
- No multi-step form re-entry: drafts persist per chat, crop/size choices
  persist per dialog session, Settings drafts persist until saved.

### 3.3.8 Accessible Authentication — N/A (requirements recorded)
- The secure-chat password flow (#201) does not exist yet. When built it
  must: allow paste and password managers, never use a cognitive test,
  offer show-password, and label errors without clearing both fields.

## Robust

### 4.1.1 Parsing — Pass
- `svelte-check`: 0 errors, 0 warnings after this change.

### 4.1.2 Name, Role, Value — Pass
- Controls expose names (aria-label/labels), states (`aria-pressed`,
  `aria-checked`, `aria-expanded`, `aria-valuenow/text`) on: theme cards,
  segmented radios, sliders (text size, volume, crop, scrub), switches,
  chat rows, menus, ticks (role=img), progressbars (sync), dialogs.

### 4.1.3 Status Messages — Pass
- One hidden polite live region + one assertive region in `+page.svelte`,
  driven by `onAnnouncement` from `events.ts`: new messages (concise/
  detailed), typing (detailed only), connection/sync status.
- Suppressed while `!session.gateDone` so the initial-sync backlog is never
  read message by message; `initialSyncComplete` announces once.
- Existing `role="status"`/`role="alert"` notices (errors, sync, send
  failures, typing labels) retained.

## OS integration

- `prefers-reduced-motion`: `reduceMotion` tri-state (Off/On/System);
  System (default) follows the OS; On forces `motion-scale: 0` + `no-motion`
  even before the theme layer runs. Absorbs #128 (visible toggle).
- `prefers-contrast`: `highContrast` tri-state; System follows
  `prefers-contrast: more`.
- `prefers-color-scheme`: unchanged System-theme behaviour (ties to #126).

## Fixes landed with this audit

1. `accessibility.svelte.ts` store + preset + snapshot restore + contrast
   math + announcement bus + alt-text helper.
2. `theme.svelte.ts`: accessibility-aware motion scale; high-contrast token
   transform applied before paint.
3. `ThemeLayers.svelte`: re-applies the accessibility layer after themes.
4. `Settings.svelte` + `AccessibilitySettings.svelte`: new section.
5. `AccessibilityPrompt.svelte` + first-run gate before `connect()`.
6. `+page.svelte`: live regions, skip link, `role="log"` message region,
   conversation landmark, char-shortcut gate, `nudgeListWidth`, full
   accessibility CSS layer, body `calc(... × --font-scale)`.
7. `Panel.svelte`: focus trap + focus restore.
8. `ChatSidebar.svelte`: keyboard separator + bold unread cues.
9. `MessageBubble.svelte`: media alt text, tick names + read/delivered
   non-colour cues.
10. `VideoPlayer.svelte`: autoplay/loop gating + keyboard slider scrub.
11. `ImageCropper.svelte`: keyboard crop + full-image reset.
12. rem migration: 404 declarations in 70 components.
13. i18n: ~70 `settings.a11y.*` strings (en + ar fallback).
14. `src/tests/accessibility.test.ts`: 11 tests.

## Follow-ups (not blocking)

- Per-release manual pass: custom-theme contrast spot-checks, 320px/200%
  visual reflow check, screen-reader run-through (NVDA/VoiceOver).
- Secure-chat auth (#201) must meet 3.3.8 when built.
- Voice-note transcripts (#164) remain the captions story.
- Consider `prefers-reduced-transparency` (draft media query) when engines
  support it; currently manual toggle only.
