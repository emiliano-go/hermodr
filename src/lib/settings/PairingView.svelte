<!-- The pairing and loading screen: account chooser, resume progress and the
  QR pairing card. Moved out of +page.svelte. -->
<script lang="ts">
  import { t, formatNumber } from "$lib/i18n/localizer";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import Logo from "$lib/ui/Logo.svelte";
  import Spinner from "$lib/ui/Spinner.svelte";
  import type { Account } from "$lib/utils/models";
  import { displayName as phoneName } from "$lib/utils/phone";
  import PhoneLink from "$lib/settings/PhoneLink.svelte";

  let {
    qrSvg,
    started,
    connecting,
    connected,
    choosingAccount,
    accounts,
    activeAccount,
    accountAvatars,
    linked,
    finalizing,
    syncPending,
    syncApplied,
    syncPercent,
    syncTimedOut,
    pairCode = null,
    pairCodeExpiresAt = null,
    pairCodeError = null,
    pairCodeManual = false,
    pairCodeBusy = false,
    onconnect,
    onchoose,
    onswitch,
    onsettings,
    onrequestpaircode = () => {},
    onrefreshpaircode = () => {},
    oncancelpaircode = () => {},
  }: {
    qrSvg: string | null;
    started: boolean;
    connecting: boolean;
    connected: boolean;
    choosingAccount: boolean;
    accounts: Account[];
    activeAccount: string | null;
    accountAvatars: Record<string, string | null>;
    linked: Account | undefined;
    finalizing: boolean;
    syncPending: number;
    syncApplied: number;
    syncPercent: number;
    syncTimedOut: boolean;
    pairCode?: string | null;
    pairCodeExpiresAt?: number | null;
    pairCodeError?: { message: string; throttled: boolean; unavailable: boolean } | null;
    pairCodeManual?: boolean;
    pairCodeBusy?: boolean;
    onconnect: () => void;
    onchoose: (id: string) => void;
    onswitch: (id: string) => void;
    onsettings: () => void;
    onrequestpaircode?: (phone: string) => void;
    onrefreshpaircode?: () => void;
    oncancelpaircode?: () => void;
  } = $props();

  /** The right card shows the QR unless the user chose phone-number linking. */
  let phoneMode = $state(false);

  const stage = $derived(qrSvg ? 2 : started || connecting ? 1 : 0);
  // Only a completed link has a JID: the account being paired, one abandoned
  // mid-pairing, or one whose phone unlinked it is not an account yet.
  const registered = $derived(accounts.filter((a) => a.jid));
</script>

<div class="pairing">
  <div class="intro-glow" aria-hidden="true"></div>
  <header class="intro-head">
    <span class="intro-logo"><Logo size={48} /></span>
    <div>
      <h1>{t("ui.app_name")}</h1>
      <span class="intro-tag">{t("settings.pairing_tagline")}</span>
    </div>
    <Button
      variant="icon"
      icon="settings"
      iconSize={18}
      title={t("settings.title")}
      aria-label={t("settings.title")}
      cls="intro-settings"
      onclick={onsettings} />
  </header>

  {#if choosingAccount}
    <div class="intro-card resume">
      <h2>{t("settings.account_choose")}</h2>
      <span class="resume-who">{t("settings.accounts_multiple")}</span>
      <div class="account-choices">
        {#each registered as account (account.id)}
          <button class="account-choice" onclick={() => onchoose(account.id)}>
            <Avatar
              src={accountAvatars[account.id] ?? null}
              label={account.label}
              seed={account.id}
              cls="choice-avatar"
            />
            <span class="choice-text">
              <strong><bdi>{account.label}</bdi></strong>
              <small>{phoneName(null, account.jid!)}</small>
            </span>
            <Icon name="chevronRight" size={16} />
          </button>
        {/each}
      </div>
    </div>
  {:else if linked}
    <div class="intro-card resume">
      <Avatar
        src={accountAvatars[linked.id] ?? null}
        label={linked.label}
        seed={linked.id}
        cls="resume-avatar"
      />
      <h2>{started || connecting || connected ? t("settings.signing_in") : t("settings.welcome_back")}</h2>
      <span class="resume-who">{linked.label} · {phoneName(null, linked.jid!)}</span>
      {#if started || connecting || connected}
        <div class="resume-progress" role="status">
          <div class="resume-status">
            <span>
              {#if finalizing}
                {t("settings.finishing")}
              {:else if syncPending > 0}
                {t("settings.messages_loading")}
              {:else if connected}
                {t("settings.your_messages_loading")}
              {:else}
                {t("settings.connecting")}
              {/if}
            </span>
            {#if syncPending > 0}
              <span class="resume-count">{t("settings.sync_count", { applied: Math.min(syncApplied, syncPending), pending: syncPending })} · {formatNumber(syncPercent / 100, { style: "percent", maximumFractionDigits: 0 })}</span>
            {/if}
          </div>
          <div
            class="resume-bar"
            class:determinate={syncPending > 0}
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={syncPending > 0 ? syncPercent : undefined}>
            <span style:width={syncPending > 0 ? `${syncPercent}%` : null}></span>
          </div>
          {#if syncTimedOut}
            <span class="hint">{t("settings.sync_background")}</span>
          {/if}
        </div>
      {:else}
        <Button variant="primary" onclick={onconnect}>{t("settings.connect")}</Button>
      {/if}
      {#if registered.some((a) => a.id !== linked.id)}
        <div class="account-bar">
          {#each registered as account (account.id)}
            {#if account.id !== linked.id}
              <button class="account" title={t("settings.account_switch_to", { name: account.label })} onclick={() => onswitch(account.id)}>
                <Avatar
                  src={accountAvatars[account.id] ?? null}
                  label={account.label}
                  seed={account.id}
                  cls="account-initial"
                />
                {account.label}
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {:else}
  <div class="intro-card">
    <section class="intro-steps">
      <h2>{t("settings.link_computer")}</h2>
      <ol>
        <li><span class="num">1</span><span>{t("settings.phone_open_whatsapp")}</span></li>
        <li>
          <span class="num">2</span><span>{t("settings.phone_linked_devices")}.</span>
        </li>
        <li><span class="num">3</span><span>{t("settings.phone_link_device")}.</span></li>
        <li><span class="num">4</span><span>{t("settings.qr_scan_hint")}</span></li>
      </ol>
      <!-- Each stage lights up as the connection actually reaches it. -->
      <div class="intro-progress" aria-label={t("settings.connection_progress")}>
        {#each ["settings.connection_step", "settings.wait_phone", "settings.linked"] as label, i (label)}
          <!-- Not `.stage`: the wallpaper layer is drawn behind every `.stage` (+page.svelte). -->
          <span class="phase" class:done={stage > i} class:current={stage === i + 1 || (stage === 0 && i === 0)}>
            <span class="phase-dot"></span>{t(label)}
          </span>
        {/each}
      </div>
      {#if registered.length > 0}
        <div class="intro-accounts">
          <span class="intro-label">{t("settings.accounts_local")}</span>
          <div class="account-bar">
            {#each registered as account (account.id)}
              <button
                class="account"
                class:active={account.id === activeAccount}
                title={account.label}
                onclick={() => onswitch(account.id)}>
                <Avatar
                  src={accountAvatars[account.id] ?? null}
                  label={account.label}
                  seed={account.id}
                  cls="account-initial"
                />
                {account.label}
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </section>

    <section class="intro-code">
      {#if phoneMode}
        <PhoneLink
          active
          code={pairCode}
          expiresAt={pairCodeExpiresAt}
          error={pairCodeError}
          busy={pairCodeBusy}
          manual={pairCodeManual}
          onrequest={onrequestpaircode}
          onrefresh={onrefreshpaircode}
          ondeactivate={() => {
            phoneMode = false;
            oncancelpaircode();
          }} />
      {:else if qrSvg}
        <div class="qr" aria-label={t("settings.qr_label")}>
          {@html qrSvg}
          <span class="qr-logo"><Logo size={44} /></span>
        </div>
        <p class="hint">{t("settings.qr_refresh_hint")}</p>
        <PhoneLink onactivate={() => (phoneMode = true)} />
      {:else if started || connecting}
        <div class="qr qr-loading" aria-label={t("settings.qr_preparing")}><Spinner /></div>
        <p class="hint">{t("settings.qr_loading")}</p>
      {:else}
        <div class="qr qr-idle"><Icon name="message" size={48} /></div>
        <Button variant="primary" onclick={onconnect}>{t("settings.pairing_start")}</Button>
      {/if}
    </section>
  </div>
  {/if}

  <p class="intro-foot">
    {t("settings.pairing_privacy")}
  </p>
</div>

<style>
  .pairing {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 22px;
    padding: 32px 24px;
    box-sizing: border-box;
    overflow: auto;
    background: var(--chat-bg);
  }
  .intro-glow {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      radial-gradient(60% 50% at 15% 10%, var(--accent-soft), transparent 70%),
      radial-gradient(50% 40% at 90% 90%, color-mix(in srgb, var(--link) 12%, transparent), transparent 70%);
  }
  .intro-head,
  .intro-card,
  .intro-foot {
    position: relative;
    box-sizing: border-box;
    width: min(920px, 100%);
    flex: none;
  }
  .intro-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .intro-logo {
    display: block;
    flex: none;
    width: 48px;
    height: 48px;
    border-radius: 12px;
    overflow: hidden;
  }
  .intro-head h1 {
    margin: 0;
    font-size: 1.625rem;
    letter-spacing: -0.01em;
  }
  .intro-tag {
    color: var(--muted);
    font-size: 0.8438rem;
  }
  .intro-card {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 40px;
    padding: 40px 44px;
    border: 1px solid var(--line-strong);
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
    animation: intro-in calc(0.35s * var(--motion-scale)) var(--ease) both;
  }
  @keyframes intro-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
  @media (max-width: 760px) {
    .intro-card {
      grid-template-columns: 1fr;
      padding: 28px 22px;
    }
  }
  .intro-card.resume {
    grid-template-columns: 1fr;
    justify-items: center;
    gap: 10px;
    width: min(460px, 100%);
    text-align: center;
  }
  .intro-card.resume h2 {
    margin: 8px 0 0;
    font-size: 1.375rem;
    font-weight: 500;
  }
  .resume-who {
    color: var(--muted);
    font-size: 0.8438rem;
  }
  .account-choices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    margin-top: 16px;
  }
  .account-choice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
    transition:
      background calc(0.15s * var(--motion-scale)),
      border-color calc(0.15s * var(--motion-scale)),
      transform calc(0.15s * var(--motion-scale));
  }
  .account-choice:hover {
    background: var(--raised-2);
    border-color: var(--accent);
  }
  .account-choice:active {
    transform: scale(0.99);
  }
  .choice-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .choice-text small {
    color: var(--muted);
  }
  .resume-progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(320px, 100%);
    margin-top: 18px;
  }
  .resume-status {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    color: var(--muted);
    font-size: 0.7812rem;
  }
  .resume-count {
    font-variant-numeric: tabular-nums;
  }
  .resume-bar {
    height: 6px;
    border-radius: 999px;
    background: var(--raised);
    overflow: hidden;
  }
  .resume-bar span {
    display: block;
    width: 40%;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    animation: indeterminate 1.2s ease-in-out infinite;
  }
  .resume-bar.determinate span {
    animation: none;
    transition: width calc(0.25s * var(--motion-scale)) var(--ease);
  }
  @keyframes indeterminate {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(250%);
    }
  }
  .intro-card.resume .account-bar {
    margin-top: 14px;
    justify-content: center;
  }
  .intro-card.resume .account {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .account-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    overflow: hidden;
  }
  .account {
    background: transparent;
    border: 0;
    color: var(--muted);
    font: inherit;
    font-size: 0.75rem;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account:hover {
    color: var(--text);
  }
  .account.active {
    background: var(--raised);
    color: var(--text);
    font-weight: 600;
  }
  .intro-steps h2 {
    margin: 0 0 18px;
    font-size: 1.5rem;
    font-weight: 400;
  }
  .intro-steps ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    font-size: 0.9375rem;
    line-height: 1.45;
  }
  .intro-steps li {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .intro-steps .num {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--line-strong);
    color: var(--muted);
    font-size: 0.7812rem;
  }
  .intro-progress {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin-top: 26px;
    font-size: 0.7812rem;
    color: var(--faint);
  }
  .phase {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .phase-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .phase.current {
    color: var(--text);
  }
  .phase.current .phase-dot {
    background: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-soft);
    animation: blink-dot 1.4s ease-in-out infinite;
  }
  .phase.done {
    color: var(--muted);
  }
  .phase.done .phase-dot {
    background: var(--accent);
  }
  @keyframes blink-dot {
    50% {
      box-shadow: 0 0 0 7px transparent;
    }
  }
  .intro-accounts {
    margin-top: 26px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .intro-label {
    font-size: 0.75rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .intro-accounts .account-bar {
    justify-content: flex-start;
    flex-wrap: wrap;
  }
  .intro-accounts .account {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .intro-code {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    text-align: center;
    max-width: 280px;
  }
  .intro-foot {
    margin: 0;
    color: var(--faint);
    font-size: 0.7812rem;
    text-align: center;
  }
  .qr {
    position: relative;
    background: var(--bg);
    padding: 12px;
    border-radius: 12px;
    line-height: 0;
  }
  .qr-logo {
    position: absolute;
    top: 50%;
    left: 50%;
    display: block;
    width: 44px;
    height: 44px;
    transform: translate(-50%, -50%);
    border-radius: 10px;
    border: 4px solid var(--bg);
    background: var(--bg);
    overflow: hidden;
  }
  .qr-loading,
  .qr-idle {
    display: grid;
    place-items: center;
    width: 264px;
    height: 264px;
    box-sizing: border-box;
    color: var(--faint);
  }
  .qr-loading {
    background: linear-gradient(100deg, var(--bg) 40%, var(--raised) 50%, var(--bg) 60%) 0 0 / 300% 100%;
    animation: shimmer 1.4s linear infinite;
  }
  @keyframes shimmer {
    to {
      background-position: -150% 0;
    }
  }
</style>
