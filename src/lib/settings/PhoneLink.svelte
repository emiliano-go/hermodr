<!-- Phone-number linking, the alternative to scanning the QR beside it. The
  parent owns the flow state; this renders the switch, the number form and the
  issued code. -->
<script lang="ts">
  import { t, formatNumber } from "$lib/i18n/localizer";
  import { locale } from "$lib/i18n/locale.svelte";
  import { normalizeError } from "$lib/i18n/errors";
  const countryNames = $derived(new Intl.DisplayNames([locale.language], { type: "region" }));
  import Button from "$lib/ui/Button.svelte";
  import { COUNTRIES, composeE164, countryFlag, defaultCountryIso } from "$lib/utils/country-codes";

  let {
    active = false,
    code = null,
    expiresAt = null,
    error = null,
    busy = false,
    manual = false,
    onactivate = () => {},
    ondeactivate = () => {},
    onrequest = () => {},
    onrefresh = () => {},
  }: {
    active?: boolean;
    code?: string | null;
    expiresAt?: number | null;
    error?: { message: string; throttled: boolean; unavailable: boolean } | null;
    busy?: boolean;
    manual?: boolean;
    onactivate?: () => void;
    ondeactivate?: () => void;
    onrequest?: (phone: string) => void;
    onrefresh?: () => void;
  } = $props();

  let iso = $state(defaultCountryIso());
  let national = $state("");
  const country = $derived(COUNTRIES.find((c) => c.iso === iso));
  const phone = $derived(composeE164(country?.dial ?? "", national));
  const canRequest = $derived(!!phone && !busy);

  // Countdown for the displayed code; the server also refreshes codes on its own.
  let now = $state(Date.now());
  $effect(() => {
    void code;
    if (!code) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const secondsLeft = $derived(expiresAt ? Math.max(0, Math.ceil((expiresAt - now) / 1000)) : 0);
  const expired = $derived(!!code && secondsLeft <= 0);
  const formatted = $derived(code ? `${code.slice(0, 4)}-${code.slice(4)}` : "");
  const countdown = $derived(
    `${formatNumber(Math.floor(secondsLeft / 60), { useGrouping: false })}:${formatNumber(secondsLeft % 60, { minimumIntegerDigits: 2, useGrouping: false })}`,
  );

  function errorText(value: { message: string; throttled: boolean; unavailable: boolean }) {
    if (value.unavailable) return t("settings.phone_unavailable");
    if (value.throttled) return t("settings.phone_throttled");
    return normalizeError(value.message).message;
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (canRequest && phone) onrequest(phone);
  }
</script>

{#if !active}
  <button class="toggle" type="button" onclick={onactivate}>{t("settings.phone_link")}</button>
{:else if code}
  <div class="code" aria-label={t("settings.pairing_code")}><bdi dir="ltr">{formatted}</bdi></div>
  <p class="hint">
    {expired || manual ? t("settings.code_expired") : t("settings.code_expires", { time: countdown })}
    {t("settings.phone_steps")}
  </p>
  {#if expired || manual}
    <Button variant="primary" onclick={onrefresh}>{t("settings.code_new")}</Button>
  {/if}
  <button class="toggle" type="button" onclick={ondeactivate}>{t("settings.use_qr")}</button>
{:else}
  <form class="number" onsubmit={submit}>
    <span class="fields">
      <select class="field country" bind:value={iso} aria-label={t("settings.country")}>
        <option value="">{t("settings.country")}</option>
        {#each COUNTRIES as item (item.iso)}
          <option value={item.iso}>{countryFlag(item.iso)} {countryNames.of(item.iso) ?? item.name} +{item.dial}</option>
        {/each}
      </select>
      <input
        class="field"
        type="tel" dir="ltr"
        inputmode="tel"
        autocomplete="tel-national"
        placeholder={t("contact.phone_number")}
        aria-label={t("contact.phone_number")}
        bind:value={national} />
    </span>
    <Button variant="primary" type="submit" disabled={!canRequest}>
      {busy ? t("settings.requesting") : t("settings.code_get")}
    </Button>
  </form>
  {#if error}<p class="error" role="alert">{errorText(error)}</p>{/if}
  <p class="hint">{t("settings.phone_hint")}</p>
  <button class="toggle" type="button" onclick={ondeactivate}>{t("settings.use_qr")}</button>
{/if}

<style>
  .toggle {
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent-text);
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
  }
  .toggle:hover {
    text-decoration: underline;
  }
  .number {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .fields {
    display: flex;
    gap: 8px;
    width: 100%;
  }
  .country {
    flex: 1 1 55%;
    min-width: 0;
  }
  .fields input {
    flex: 1 1 45%;
    min-width: 0;
  }
  .code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 1.875rem;
    font-weight: 600;
    letter-spacing: 3px;
    color: var(--text);
    background: var(--surface);
    border-radius: var(--radius-sm);
    padding: 12px 18px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
    line-height: 1.5;
    text-align: center;
  }
  .error {
    margin: 0;
    color: var(--danger, #e5484d);
    font-size: 0.8125rem;
    text-align: center;
  }
</style>
