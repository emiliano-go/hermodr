<!-- A profile picture shown large: the cached preview at once, the full-size picture when it arrives. -->
<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";

  let { jid, preview, alt, onclose }: { jid: string; preview: string; alt: string; onclose: () => void } = $props();

  let full = $state<string | null>(null);
  $effect(() => {
    full = null;
    invoke<string | null>("avatar", { jid, full: true })
      .then((p) => (full = p))
      .catch(() => {});
  });
</script>

<svelte:window
  onkeydowncapture={(e) => {
    if (e.key !== "Escape") return;
    // Only the picture closes; the panel or card under it stays.
    e.stopImmediatePropagation();
    onclose();
  }} />

<button class="lightbox" aria-label="Close picture" onclick={onclose}>
  <img src={convertFileSrc(full ?? preview)} {alt} />
</button>

<style>
  .lightbox {
    position: fixed;
    inset: 0;
    z-index: 400;
    display: grid;
    place-items: center;
    padding: 40px;
    border: 0;
    background: var(--scrim);
    cursor: zoom-out;
    animation: lightbox-in calc(0.18s * var(--motion-scale)) var(--ease) both;
  }
  .lightbox img {
    width: min(640px, 100%, calc(100vh - 80px));
    aspect-ratio: 1;
    object-fit: contain;
    border-radius: 12px;
    box-shadow: var(--shadow);
  }
  @keyframes lightbox-in {
    from {
      opacity: 0;
    }
  }
</style>
