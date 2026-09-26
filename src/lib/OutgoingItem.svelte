<!-- One attachment on its way out, drawn at the end of its chat until the
  sent message replaces it. Moved out of +page.svelte with its own bubble
  look so it shares no scoped styles with MessageBubble. -->
<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import { motion } from "$lib/theme.svelte";
  import type { Outgoing } from "$lib/models";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";

  let { upload }: { upload: Outgoing } = $props();
</script>

<div class="out-row" in:fly={{ y: 48, duration: motion(260), easing: cubicOut }}>
  <div
    class="bubble mine first outgoing-upload"
    class:media-only={upload.kind !== "other" && !upload.caption}>
    <div class="upload-visual" class:file-upload={upload.kind === "other" || !upload.url}>
      {#if upload.kind === "image" && upload.url}
        <img class="media" src={upload.url} alt={upload.name} />
      {:else if upload.kind === "video" && upload.url}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video class="media" src={upload.url} preload="metadata" muted></video>
      {:else}
        <span class="upload-name"><Icon name="file" size={20} />{upload.name}</span>
      {/if}
      <span class="upload-ring" aria-label="Uploading, {Math.round(upload.progress * 100)}%">
        <svg viewBox="0 0 48 48" width="48" height="48">
          <circle class="ring-track" cx="24" cy="24" r="20" />
          <circle
            class="ring-fill"
            class:spinning={upload.progress === 0}
            cx="24"
            cy="24"
            r="20"
            stroke-dasharray="125.66"
            stroke-dashoffset={125.66 * (1 - (upload.progress || 0.12))} />
        </svg>
        <span class="ring-label">{upload.progress > 0 ? `${Math.round(upload.progress * 100)}%` : ""}</span>
      </span>
    </div>
    {#if upload.caption}<span class="upload-caption">{upload.caption}</span>{/if}
  </div>
</div>

<style>
  /* Same row slot as message rows, without their hover states. */
  .out-row {
    display: flex;
    flex-direction: column;
    padding: 1px var(--pad-l) 1px var(--pad-r);
  }
  .bubble {
    flex-shrink: 0;
    align-self: flex-end;
    position: relative;
    max-width: 65%;
    min-width: 0;
    background: var(--bubble-mine);
    border-radius: var(--radius-sm);
    border-top-right-radius: 0;
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
    padding: 6px 7px 8px 9px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    line-height: 19px;
    word-break: break-word;
    overflow-wrap: anywhere;
  }
  .bubble.first {
    margin-top: 10px;
  }
  /* The tail marks it as ours, as on a sent bubble. */
  .bubble.first::before {
    content: "";
    position: absolute;
    top: 0;
    width: 9px;
    height: 13px;
    background: inherit;
    right: -8px;
    clip-path: polygon(0 0, 100% 0, 0 100%);
  }
  .bubble.media-only {
    padding: 3px;
  }
  .media {
    max-width: 100%;
    max-height: 320px;
    width: auto;
    height: auto;
    object-fit: contain;
    border-radius: 6px;
    display: block;
  }
  .bubble.media-only .media {
    border-radius: calc(var(--radius-sm) - 2px);
  }
  .upload-visual {
    position: relative;
    display: grid;
    place-items: center;
    border-radius: 8px;
    overflow: hidden;
  }
  .upload-visual .media {
    display: block;
    max-width: 280px;
    max-height: 320px;
    filter: brightness(0.7);
  }
  .upload-visual.file-upload {
    min-width: 220px;
    min-height: 72px;
    gap: 8px;
    padding: 10px;
  }
  .upload-name {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .upload-ring {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .file-upload .upload-ring {
    position: static;
  }
  .upload-ring svg {
    grid-area: 1 / 1;
    transform: rotate(-90deg);
    border-radius: 50%;
    background: var(--scrim);
  }
  .upload-ring circle {
    fill: none;
    stroke-width: 3.5;
  }
  .ring-track {
    stroke: rgba(255, 255, 255, 0.2);
  }
  .ring-fill {
    stroke: #fff;
    stroke-linecap: round;
    transition: stroke-dashoffset calc(0.25s * var(--motion-scale)) linear;
  }
  .ring-fill.spinning {
    transform-origin: 24px 24px;
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ring-label {
    grid-area: 1 / 1;
    color: #fff;
    font-size: 11px;
    font-weight: 600;
  }
  .upload-caption {
    display: block;
    padding: 6px 4px 2px;
  }
</style>
