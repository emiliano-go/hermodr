<!-- The pairing and loading screen: account chooser, resume progress and the
  QR pairing card. Moved out of +page.svelte. -->
<script lang="ts">
  import Avatar from "$lib/Avatar.svelte";
  import Button from "$lib/Button.svelte";
  import Icon from "$lib/Icon.svelte";
  import Logo from "$lib/Logo.svelte";
  import Spinner from "$lib/Spinner.svelte";
  import type { Account } from "$lib/models";
  import { displayName as phoneName } from "$lib/phone";

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
    onconnect,
    onchoose,
    onswitch,
    onsettings,
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
    onconnect: () => void;
    onchoose: (id: string) => void;
    onswitch: (id: string) => void;
    onsettings: () => void;
  } = $props();

  const stage = $derived(qrSvg ? 2 : started || connecting ? 1 : 0);
</script>

<div class="pairing">
  <div class="intro-glow" aria-hidden="true"></div>
  <header class="intro-head">
    <span class="intro-logo"><Logo size={48} /></span>
    <div>
      <h1>Hermóðr</h1>
      <span class="intro-tag">WhatsApp, native on your desktop</span>
    </div>
    <Button
      variant="icon"
      icon="settings"
      iconSize={18}
      title="Settings"
      aria-label="Settings"
      cls="intro-settings"
      onclick={onsettings} />
  </header>

  {#if choosingAccount}
    <div class="intro-card resume">
      <h2>Choose an account</h2>
      <span class="resume-who">Several WhatsApp accounts are linked on this computer.</span>
      <div class="account-choices">
        {#each accounts.filter((a) => a.jid) as account (account.id)}
          <button class="account-choice" onclick={() => onchoose(account.id)}>
            <Avatar
              src={accountAvatars[account.id] ?? null}
              label={account.label}
              seed={account.id}
              cls="choice-avatar"
            />
            <span class="choice-text">
              <strong>{account.label}</strong>
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
      <h2>{started || connecting || connected ? "Signing in" : "Welcome back"}</h2>
      <span class="resume-who">{linked.label} · {phoneName(null, linked.jid!)}</span>
      {#if started || connecting || connected}
        <div class="resume-progress" role="status">
          <div class="resume-status">
            <span>
              {#if finalizing}
                Finishing up…
              {:else if syncPending > 0}
                Loading messages…
              {:else if connected}
                Loading your messages…
              {:else}
                Connecting to WhatsApp…
              {/if}
            </span>
            {#if syncPending > 0}
              <span class="resume-count">{Math.min(syncApplied, syncPending)} of {syncPending} · {syncPercent}%</span>
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
            <span class="hint">Still syncing in the background…</span>
          {/if}
        </div>
      {:else}
        <Button variant="primary" onclick={onconnect}>Connect</Button>
      {/if}
      {#if accounts.length > 1}
        <div class="account-bar">
          {#each accounts as account (account.id)}
            {#if account.id !== linked.id}
              <button class="account" title="Switch to {account.label}" onclick={() => onswitch(account.id)}>
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
      <h2>Link this computer</h2>
      <ol>
        <li><span class="num">1</span><span>Open <strong>WhatsApp</strong> on your phone.</span></li>
        <li>
          <span class="num">2</span><span>Tap <strong>Menu</strong> or <strong>Settings</strong>, then <strong>Linked devices</strong>.</span>
        </li>
        <li><span class="num">3</span><span>Tap <strong>Link a device</strong>.</span></li>
        <li><span class="num">4</span><span>Point your phone at this screen to scan the code.</span></li>
      </ol>
      <!-- Each stage lights up as the connection actually reaches it. -->
      <div class="intro-progress" aria-label="Connection progress">
        {#each ["Connecting to WhatsApp", "Waiting for your phone", "Linked"] as label, i (label)}
          <span class="stage" class:done={stage > i} class:current={stage === i + 1 || (stage === 0 && i === 0)}>
            <span class="stage-dot"></span>{label}
          </span>
        {/each}
      </div>
      {#if accounts.length > 0}
        <div class="intro-accounts">
          <span class="intro-label">Accounts on this computer</span>
          <div class="account-bar">
            {#each accounts as account (account.id)}
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
      {#if qrSvg}
        <div class="qr" aria-label="Pairing QR code">
          {@html qrSvg}
          <span class="qr-logo"><Logo size={44} /></span>
        </div>
        <p class="hint">The code refreshes by itself. Keep this window open while you scan.</p>
      {:else if started || connecting}
        <div class="qr qr-loading" aria-label="Preparing a pairing code"><Spinner /></div>
        <p class="hint">Getting a pairing code from WhatsApp…</p>
      {:else}
        <div class="qr qr-idle"><Icon name="message" size={48} /></div>
        <Button variant="primary" onclick={onconnect}>Start pairing</Button>
      {/if}
    </section>
  </div>
  {/if}

  <p class="intro-foot">
    Your messages stay end-to-end encrypted. History is kept only on this computer, within the limits
    you set in Settings.
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
    font-size: 26px;
    letter-spacing: -0.01em;
  }
  .intro-tag {
    color: var(--muted);
    font-size: 13.5px;
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
    font-size: 22px;
    font-weight: 500;
  }
  .resume-who {
    color: var(--muted);
    font-size: 13.5px;
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
    text-align: left;
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
    font-size: 12.5px;
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
    font-size: 12px;
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
    font-size: 24px;
    font-weight: 400;
  }
  .intro-steps ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    font-size: 15px;
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
    font-size: 12.5px;
  }
  .intro-progress {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin-top: 26px;
    font-size: 12.5px;
    color: var(--faint);
  }
  .stage {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .stage-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .stage.current {
    color: var(--text);
  }
  .stage.current .stage-dot {
    background: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-soft);
    animation: blink-dot 1.4s ease-in-out infinite;
  }
  .stage.done {
    color: var(--muted);
  }
  .stage.done .stage-dot {
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
    font-size: 12px;
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
    font-size: 12.5px;
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
