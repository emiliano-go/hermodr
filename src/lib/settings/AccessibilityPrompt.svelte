<!-- First-launch Accessibility-mode prompt, shown before pairing. Keeps the
  first-run visual language (intro-card). -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import Button from "$lib/ui/Button.svelte";
  import Logo from "$lib/ui/Logo.svelte";
  import { accessibility, save, applyAccessibility, enableAccessibilityMode } from "$lib/utils/accessibility.svelte";

  let { ondone }: { ondone: () => void } = $props();

  function enable() {
    enableAccessibilityMode();
    ondone();
  }

  function skip() {
    accessibility.promptSeen = true;
    save();
    applyAccessibility();
    ondone();
  }
</script>

<div class="backdrop" role="presentation">
  <div class="intro-card a11y-prompt" role="dialog" aria-modal="true" aria-labelledby="a11y-prompt-title">
    <span class="intro-logo"><Logo size={40} /></span>
    <h2 id="a11y-prompt-title">{t("settings.a11y.prompt_title")}</h2>
    <p class="lede">{t("settings.a11y.prompt_body")}</p>
    <div class="actions">
      <Button variant="primary" onclick={enable}>{t("settings.a11y.prompt_enable")}</Button>
      <Button variant="ghost" onclick={skip}>{t("settings.a11y.prompt_skip")}</Button>
    </div>
    <p class="hint">{t("settings.a11y.prompt_reversible")}</p>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 400;
    display: grid;
    place-items: center;
    padding: 1.5rem;
    background: var(--scrim);
  }
  .intro-card.a11y-prompt {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    width: min(28.75rem, 100%);
    padding: 2.5rem 2.75rem;
    text-align: center;
    border: 1px solid var(--line-strong);
    border-radius: 1rem;
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .intro-logo { display: block; width: 2.5rem; height: 2.5rem; border-radius: 0.75rem; overflow: hidden; }
  h2 { margin: 0.5rem 0 0; font-size: 1.375rem; font-weight: 500; }
  .lede { margin: 0; color: var(--muted); font-size: 0.875rem; line-height: 1.5; }
  .actions { display: flex; gap: 0.75rem; margin-top: 0.5rem; }
  .hint { margin: 0; color: var(--faint); font-size: 0.75rem; }
</style>
