<script lang="ts">
  import { t, formatRelative, formatNumber } from "$lib/i18n/localizer";
  import type { BubbleApi, LiveLocation, StoredMessage } from "$lib/utils/models";
  import { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import MessageText from "$lib/messages/MessageText.svelte";

  /** A live location share: the map snapshot, how fresh it is, and the facts. */
  let { message, api }: { message: StoredMessage; api: BubbleApi } = $props();

  const NOTHING: LiveLocation = {
    lat: 0, lng: 0, accuracy: null, speed: null, heading: null, sequence: null,
    started_at: 0, updated_at: 0, expires_at: null, ended: true,
  };
  const live = $derived(message.live_location ?? NOTHING);
  // Older rows kept the maps URL in the text; the card composes its own.
  const caption = $derived(
    message.text
      .split("\n")
      .filter((line) => !line.startsWith("https://maps.google.com"))
      .join("\n")
      .trim(),
  );

  let now = $state(Date.now());
  const expired = $derived(live.expires_at !== null && Math.floor(now / 1000) >= live.expires_at);
  const ended = $derived(live.ended || expired);
  /** Ticks while the share is live, so the age and countdown stay honest. */
  $effect(() => {
    if (ended) return;
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  function ago(seconds: number) {
    if (seconds < 5) return t("content.just_now");
    if (seconds < 60) return formatRelative(-seconds, "second");
    if (seconds < 3600) return formatRelative(-Math.round(seconds / 60), "minute");
    if (seconds < 86400) return formatRelative(-Math.round(seconds / 3600), "hour");
    return formatRelative(-Math.round(seconds / 86400), "day");
  }
  const updated = $derived(ago(Math.max(0, Math.floor(now / 1000) - live.updated_at)));
  const endsIn = $derived.by(() => {
    if (live.expires_at === null || ended) return null;
    const left = live.expires_at - Math.floor(now / 1000);
    if (left <= 0) return null;
    return left >= 3600 ? t("content.value_h", { param0: (Math.round(left / 3600)) }) : t("content.value_min", { param0: (Math.max(1, Math.round(left / 60))) });
  });

  const coords = $derived(`${live.lat.toFixed(5)}, ${live.lng.toFixed(5)}`);
  const mapsUrl = $derived(`https://maps.google.com/?q=${live.lat},${live.lng}`);
  const kmh = $derived(live.speed !== null && live.speed >= 0.5 ? Math.round(live.speed * 3.6) : null);
  const COMPASS = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
  const heading = $derived(
    live.heading === null ? null : `${t("content.compass_" + (COMPASS[Math.round(live.heading / 22.5) % 16]?.toLowerCase() ?? "unknown"))} · ${formatNumber(Math.round(live.heading))}°`,
  );

  async function copyCoords() {
    try {
      await navigator.clipboard.writeText(coords);
    } catch {
      // Clipboard access may be denied; the coordinates stay visible to copy by hand.
    }
  }
</script>

<div class="loc" class:ended>
  <button class="map" title={t("content.open_in_maps")} onclick={() => api.onopenurl(mapsUrl)}>
    {#if message.media_thumb}
      <img src={mediaSrc(message.media_thumb)} alt={t("content.map")} />
    {:else}
      <span class="map-empty"><Icon name="pin" size={22} /></span>
    {/if}
    <span class="map-pin"><Icon name="pin" size={14} /></span>
  </button>
  <div class="body">
    <div class="head">
      <span class="state">{ended ? t("content.live_location_ended") : t("content.live_location")}</span>
      <span class="age">{t("content.updated")} {updated}{#if endsIn}{t("content.ends_in")} {endsIn}{/if}</span>
    </div>
    {#if caption}
      <MessageText
        text={caption}
        mine={message.from_me}
        toWire={api.toWire}
        targetOf={api.targetOf}
        avatarOf={api.avatarOf}
        onprofile={api.onprofile}
        onopenurl={api.onopenurl} />
    {/if}
    <div class="facts">
      <span><bdi dir="ltr">{coords}</bdi></span>
      {#if live.accuracy !== null}<span>±{formatNumber(live.accuracy)} {t("content.m")}</span>{/if}
      {#if kmh !== null}<span>{formatNumber(kmh)} {t("content.km_h")}</span>{/if}
      {#if heading !== null}<span>{heading}</span>{/if}
    </div>
    <div class="actions">
      <button onclick={() => api.onopenurl(mapsUrl)}>{t("content.open_in_maps")}</button>
      <button onclick={copyCoords}>{t("content.copy_coordinates")}</button>
    </div>
  </div>
</div>

<style>
  .loc {
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-width: 300px;
    border-radius: 8px;
    overflow: hidden;
    background: rgba(0, 0, 0, 0.16);
  }
  .map {
    position: relative;
    padding: 0;
    border: 0;
    background: var(--bg);
    cursor: pointer;
    line-height: 0;
  }
  .map img {
    width: 100%;
    height: 130px;
    object-fit: cover;
    display: block;
  }
  .map-empty {
    display: grid;
    place-items: center;
    height: 130px;
    color: var(--muted);
  }
  .map-pin {
    position: absolute;
    inset-inline-end: 6px;
    bottom: 6px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: rgba(6, 8, 10, 0.65);
    color: #fff;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 7px 9px 8px;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.7812rem;
    font-weight: 700;
    color: var(--accent-text);
  }
  .ended .state {
    color: var(--muted);
  }
  /* The pulsing dot is the one thing that says "this is still moving". */
  .state::before {
    content: "";
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-text);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .ended .state::before {
    background: var(--faint);
    animation: none;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
  @media (prefers-reduced-motion: reduce) {
    .state::before { animation: none; }
  }
  .age {
    font-size: 0.7188rem;
    color: var(--muted);
    text-align: end;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    font-size: 0.75rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 2px;
  }
  .actions button {
    padding: 3px 8px;
    border: 0;
    border-radius: 6px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-size: 0.7188rem;
    cursor: pointer;
  }
  .actions button:hover {
    color: var(--accent-text);
  }
</style>
