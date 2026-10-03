<!-- Per-device Accessibility settings, grouped per the umbrella issue. Every
  preference stays individually editable regardless of Accessibility mode. -->
<script lang="ts">
  import {
    accessibility,
    save,
    applyAccessibility,
    setAccessibilityMode,
    type TriState,
  } from "$lib/utils/accessibility.svelte";
  import { t } from "$lib/i18n/localizer";

  function persist() {
    save();
    applyAccessibility();
  }

  function setMode(on: boolean) {
    setAccessibilityMode(on);
  }

  function triLabel(v: TriState): string {
    return v === "on" ? t("settings.a11y.on") : v === "off" ? t("settings.a11y.off") : t("settings.a11y.system");
  }
</script>

<div class="a11y-mode" role="group" aria-labelledby="a11y-mode-title">
  <div>
    <span class="setting-title" id="a11y-mode-title">{t("settings.a11y.mode_title")}</span>
    <span class="setting-desc">{t("settings.a11y.mode_hint")}</span>
    {#if accessibility.enabled && accessibility.snapshot}
      <span class="setting-desc">{t("settings.a11y.mode_reversible")}</span>
    {/if}
  </div>
  <label class="mode-switch">
    <input class="switch" type="checkbox" checked={accessibility.enabled} onchange={(e) => setMode(e.currentTarget.checked)} />
    <span>{accessibility.enabled ? t("settings.a11y.on") : t("settings.a11y.off")}</span>
  </label>
</div>

<h3>{t("settings.a11y.motion_group")}</h3>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.reduce_motion")}</span>
    <span class="setting-desc">{t("settings.a11y.reduce_motion_hint")}</span>
  </div>
  <select class="field" value={accessibility.reduceMotion} onchange={(e) => { accessibility.reduceMotion = e.currentTarget.value as TriState; persist(); }} aria-label={t("settings.a11y.reduce_motion")}>
    <option value="off">{t("settings.a11y.off")}</option>
    <option value="on">{t("settings.a11y.on")}</option>
    <option value="system">{t("settings.a11y.system_follow")}</option>
  </select>
</label>
<p class="muted inline-note">{t("settings.a11y.reduce_motion_state", { state: triLabel(accessibility.reduceMotion) })}</p>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.pause_media")}</span>
    <span class="setting-desc">{t("settings.a11y.pause_media_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.pauseAnimatedMedia} onchange={persist} />
</label>

<h3>{t("settings.a11y.visual_group")}</h3>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.high_contrast")}</span>
    <span class="setting-desc">{t("settings.a11y.high_contrast_hint")}</span>
  </div>
  <select class="field" value={accessibility.highContrast} onchange={(e) => { accessibility.highContrast = e.currentTarget.value as TriState; persist(); }} aria-label={t("settings.a11y.high_contrast")}>
    <option value="off">{t("settings.a11y.off")}</option>
    <option value="on">{t("settings.a11y.on")}</option>
    <option value="system">{t("settings.a11y.system_follow")}</option>
  </select>
</label>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.reduce_transparency")}</span>
    <span class="setting-desc">{t("settings.a11y.reduce_transparency_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.reduceTransparency} onchange={persist} />
</label>
<div class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.target_size")}</span>
    <span class="setting-desc">{t("settings.a11y.target_size_hint")}</span>
  </div>
  <div class="segmented" role="radiogroup" aria-label={t("settings.a11y.target_size")}>
    <button role="radio" aria-checked={accessibility.targetSize === "comfortable"} class:active={accessibility.targetSize === "comfortable"} onclick={() => { accessibility.targetSize = "comfortable"; persist(); }}>{t("settings.a11y.comfortable")}</button>
    <button role="radio" aria-checked={accessibility.targetSize === "large"} class:active={accessibility.targetSize === "large"} onclick={() => { accessibility.targetSize = "large"; persist(); }}>{t("settings.a11y.large")}</button>
  </div>
</div>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.always_focus")}</span>
    <span class="setting-desc">{t("settings.a11y.always_focus_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.alwaysShowFocus} onchange={persist} />
</label>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.enhanced_focus")}</span>
    <span class="setting-desc">{t("settings.a11y.enhanced_focus_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.enhancedFocus} onchange={persist} />
</label>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.color_blind")}</span>
    <span class="setting-desc">{t("settings.a11y.color_blind_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.colorBlindPalette} onchange={persist} />
</label>

<h3>{t("settings.a11y.text_group")}</h3>
<div class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.text_size")}</span>
    <span class="setting-desc">{t("settings.a11y.text_size_hint")}</span>
  </div>
  <span class="unit-field">
    <input type="range" min="100" max="200" step="5" value={accessibility.textScale} aria-label={t("settings.a11y.text_size")}
      oninput={(e) => { accessibility.textScale = Number(e.currentTarget.value); persist(); }} />
    <span class="readout" aria-live="off">{accessibility.textScale}%</span>
  </span>
</div>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.text_spacing")}</span>
    <span class="setting-desc">{t("settings.a11y.text_spacing_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.textSpacing} onchange={persist} />
</label>
<div class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.font_choice")}</span>
    <span class="setting-desc">{t("settings.a11y.font_choice_hint")}</span>
  </div>
  <div class="segmented" role="radiogroup" aria-label={t("settings.a11y.font_choice")}>
    <button role="radio" aria-checked={accessibility.fontChoice === "system"} class:active={accessibility.fontChoice === "system"} onclick={() => { accessibility.fontChoice = "system"; persist(); }}>{t("settings.a11y.font_system")}</button>
    <button role="radio" aria-checked={accessibility.fontChoice === "legible"} class:active={accessibility.fontChoice === "legible"} onclick={() => { accessibility.fontChoice = "legible"; persist(); }}>{t("settings.a11y.font_legible")}</button>
  </div>
</div>

<h3>{t("settings.a11y.keyboard_group")}</h3>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.shortcut_hints")}</span>
    <span class="setting-desc">{t("settings.a11y.shortcut_hints_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.showShortcutHints} onchange={persist} />
</label>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.char_shortcuts")}</span>
    <span class="setting-desc">{t("settings.a11y.char_shortcuts_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.charShortcutsEnabled} onchange={persist} />
</label>
<div class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.drag_alternatives")}</span>
    <span class="setting-desc">{t("settings.a11y.drag_alternatives_hint")}</span>
  </div>
</div>

<h3>{t("settings.a11y.screen_reader_group")}</h3>
<div class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.announcements")}</span>
    <span class="setting-desc">{t("settings.a11y.announcements_hint")}</span>
  </div>
  <select class="field" value={accessibility.announcements} onchange={(e) => { accessibility.announcements = e.currentTarget.value as typeof accessibility.announcements; persist(); }} aria-label={t("settings.a11y.announcements")}>
    <option value="off">{t("settings.a11y.announce_off")}</option>
    <option value="concise">{t("settings.a11y.announce_concise")}</option>
    <option value="detailed">{t("settings.a11y.announce_detailed")}</option>
  </select>
</div>
<p class="muted inline-note">{t("settings.a11y.announcements_sync_note")}</p>

<h3>{t("settings.a11y.media_group")}</h3>
<label class="setting">
  <div>
    <span class="setting-title">{t("settings.a11y.autoplay")}</span>
    <span class="setting-desc">{t("settings.a11y.autoplay_hint")}</span>
  </div>
  <input class="switch" type="checkbox" bind:checked={accessibility.autoplayVideos} onchange={persist} />
</label>

<style>
  h3 {
    margin: 22px 0 0;
    font-size: 0.7188rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  h3:first-child { margin-top: 0; }
  .a11y-mode {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px;
    margin-bottom: 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
  }
  .a11y-mode > div { display: flex; flex-direction: column; gap: 3px; }
  .mode-switch { display: flex; align-items: center; gap: 10px; font-weight: 600; }
  .inline-note { margin: -8px 0 0; }
  .unit-field { display: flex; align-items: center; gap: 8px; }
  .readout { min-width: 3.2rem; text-align: end; font-variant-numeric: tabular-nums; color: var(--muted); }
  .segmented { display: flex; padding: 3px; background: var(--bg); border: 1px solid var(--line-strong); border-radius: var(--radius); }
  .segmented button {
    padding: 5px 14px; background: transparent; border: 0;
    border-radius: calc(var(--radius) - 2px); color: var(--muted);
    font: inherit; font-size: 0.8125rem; cursor: pointer;
  }
  .segmented button.active { background: var(--raised-2); color: var(--text); }
  input[type="range"] { width: 10.6rem; accent-color: var(--accent); }
</style>
