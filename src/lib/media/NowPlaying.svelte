<script lang="ts">
  import { mediaClock } from "$lib/media/clock";
  import { t } from "$lib/i18n/localizer";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/ui/Icon.svelte";
  import { player } from "$lib/state/player.svelte";

  let bar: HTMLDivElement | undefined = $state();
  let scrubbing = false;

  function seekTo(e: PointerEvent) {
    if (!bar || !player.duration) return;
    const rect = bar.getBoundingClientRect();
    player.seek((e.clientX - rect.left) / rect.width);
  }

  const clock = (seconds: number) => mediaClock(seconds);
</script>

{#if player.track}
  <div class="player" role="group" aria-label={t("content.voice_message_playing")}>
    <div class="line">
      <span class="who">
        {#if player.track.avatar}
          <img src={convertFileSrc(player.track.avatar)} alt="" />
        {:else}
          <span class="blank">{player.track.initials}</span>
        {/if}
      </span>
      <span class="name" title={player.track.title}><bdi dir="auto">{player.track.title}</bdi></span>
      <button class="rate" title={t("content.playback_speed")} onclick={() => player.cycleRate()}>{player.rate}×</button>
      <button class="tool" title={t("content.close_player")} aria-label={t("content.close_player")} onclick={() => player.stop()}>
        <Icon name="x" size={16} />
      </button>
    </div>
    <div class="line">
      <button
        class="tool"
        class:failed={player.failed}
        disabled={player.failed || player.loading}
        title={player.failed ? t("content.could_not_play_this_file") : player.paused ? t("content.play") : t("content.pause")}
        aria-label={player.paused ? t("content.play") : t("content.pause")}
        onclick={() => player.toggle()}>
        {#if player.failed}!{:else}<Icon name={player.paused ? "play" : "pause"} size={17} filled />{/if}
      </button>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="bar"
        bind:this={bar}
        onpointerdown={(e) => {
          scrubbing = true;
          bar!.setPointerCapture(e.pointerId);
          seekTo(e);
        }}
        onpointermove={(e) => scrubbing && seekTo(e)}
        onpointerup={() => (scrubbing = false)} dir="ltr">
        <span class="fill" style="width: {player.progress * 100}%"></span>
        <span class="knob" style="left: {player.progress * 100}%"></span>
      </div>
      <span class="time" dir="ltr"><bdi dir="ltr">{clock(player.position)}</bdi> / <bdi dir="ltr">{clock(player.duration)}</bdi></span>
    </div>
  </div>
{/if}

<style>
  .player {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 10px;
    border-top: 1px solid color-mix(in srgb, var(--text) 8%, transparent);
    background: var(--surface);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .who {
    flex: none;
    width: 28px;
    height: 28px;
  }
  .who img,
  .blank {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
  }
  .blank {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    color: var(--text);
    font-size: 0.75rem;
    font-weight: 600;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.8125rem;
    color: var(--text);
  }
  .rate {
    flex: none;
    min-width: 36px;
    height: 22px;
    padding: 0 7px;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 14%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
  }
  .tool {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .tool:hover:not(:disabled) {
    color: var(--text);
  }
  .tool.failed {
    color: var(--danger);
    font-weight: 700;
  }
  .tool:disabled {
    cursor: progress;
    opacity: 0.6;
  }
  .bar {
    position: relative;
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--text) 14%, transparent);
    cursor: pointer;
    touch-action: none;
  }
  .fill {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    border-radius: 2px;
    background: var(--accent);
  }
  .knob {
    position: absolute;
    top: 50%;
    width: 10px;
    height: 10px;
    margin: -5px 0 0 -5px;
    border-radius: 50%;
    background: var(--accent);
    pointer-events: none;
  }
  .time {
    flex: none;
    font-size: 0.6875rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
