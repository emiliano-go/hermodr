<script lang="ts">
  import { onDestroy } from "svelte";
  import Button from "$lib/Button.svelte";

  let {
    file,
    square = false,
    sizes = true,
    onapply,
    oncancel,
    applyLabel = "Apply",
    altLabel,
    onalt,
  }: {
    file: Blob & { name?: string };
    /** Locks the crop to a square, as stickers need. */
    square?: boolean;
    /** Offers downscaling the result by its long edge. */
    sizes?: boolean;
    onapply: (result: File) => void;
    oncancel: () => void;
    applyLabel?: string;
    /** A second way to use the result, such as saving instead of sending. */
    altLabel?: string;
    onalt?: (result: File) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  const src = URL.createObjectURL(file);
  onDestroy(() => URL.revokeObjectURL(src));

  let image: HTMLImageElement | undefined = $state();
  let natural = $state({ w: 1, h: 1 });
  /** The crop in fractions of the image. */
  let crop = $state({ x: 0, y: 0, w: 1, h: 1 });
  let longEdge = $state(0);
  let drag: { mode: string; x: number; y: number; start: typeof crop } | null = null;

  function loaded() {
    if (!image) return;
    natural = { w: image.naturalWidth, h: image.naturalHeight };
    if (square) {
      const side = Math.min(natural.w, natural.h);
      crop = {
        w: side / natural.w,
        h: side / natural.h,
        x: (1 - side / natural.w) / 2,
        y: (1 - side / natural.h) / 2,
      };
    }
  }

  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  function down(mode: string, e: PointerEvent) {
    e.stopPropagation();
    drag = { mode, x: e.clientX, y: e.clientY, start: { ...crop } };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (!drag || !image) return;
    const dx = (e.clientX - drag.x) / image.clientWidth;
    const dy = (e.clientY - drag.y) / image.clientHeight;
    const s = drag.start;
    const min = 0.05;
    if (drag.mode === "move") {
      crop = { ...s, x: clamp(s.x + dx, 0, 1 - s.w), y: clamp(s.y + dy, 0, 1 - s.h) };
      return;
    }
    // Corner handles: n/s and e/w say which edges follow the pointer.
    let { x, y, w, h } = s;
    if (drag.mode.includes("w")) {
      const nx = clamp(s.x + dx, 0, s.x + s.w - min);
      w = s.w + (s.x - nx);
      x = nx;
    }
    if (drag.mode.includes("e")) w = clamp(s.w + dx, min, 1 - s.x);
    if (drag.mode.includes("n")) {
      const ny = clamp(s.y + dy, 0, s.y + s.h - min);
      h = s.h + (s.y - ny);
      y = ny;
    }
    if (drag.mode.includes("s")) h = clamp(s.h + dy, min, 1 - s.y);
    if (square) {
      // Equal sides in pixels, limited by whichever edge runs out first.
      const side = Math.min(w * natural.w, h * natural.h);
      const nw = side / natural.w;
      const nh = side / natural.h;
      if (drag.mode.includes("w")) x = s.x + s.w - nw;
      if (drag.mode.includes("n")) y = s.y + s.h - nh;
      w = nw;
      h = nh;
    }
    crop = { x, y, w, h };
  }

  async function apply(use: (result: File) => void = onapply) {
    const sx = Math.round(crop.x * natural.w);
    const sy = Math.round(crop.y * natural.h);
    const sw = Math.max(1, Math.round(crop.w * natural.w));
    const sh = Math.max(1, Math.round(crop.h * natural.h));
    const scale = longEdge ? Math.min(1, longEdge / Math.max(sw, sh)) : 1;
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(sw * scale));
    canvas.height = Math.max(1, Math.round(sh * scale));
    canvas.getContext("2d")!.drawImage(image!, sx, sy, sw, sh, 0, 0, canvas.width, canvas.height);
    // Keep transparency where the source may have it; photos stay JPEG.
    const png = square || /png|webp|gif/.test(file.type);
    const type = png ? "image/png" : "image/jpeg";
    const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, type, 0.92));
    if (!blob) return;
    const base = (file.name ?? "image").replace(/\.[^.]+$/, "");
    use(new File([blob], `${base}.${png ? "png" : "jpg"}`, { type }));
  }
</script>

<div class="cropper">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="frame" onpointermove={move} onpointerup={() => (drag = null)}>
    <img bind:this={image} {src} alt="" draggable="false" onload={loaded} />
    <div
      class="crop"
      style="left: {crop.x * 100}%; top: {crop.y * 100}%; width: {crop.w * 100}%; height: {crop.h * 100}%"
      onpointerdown={(e) => down("move", e)}>
      {#each ["nw", "ne", "sw", "se"] as corner (corner)}
        <span class="handle {corner}" onpointerdown={(e) => down(corner, e)}></span>
      {/each}
    </div>
  </div>
  <div class="controls">
    <span class="dims">{Math.round(crop.w * natural.w)} × {Math.round(crop.h * natural.h)}</span>
    {#if sizes}
      <select class="field" bind:value={longEdge} aria-label="Size">
        <option value={0}>Original size</option>
        <option value={2560}>Large (2560)</option>
        <option value={1600}>Medium (1600)</option>
        <option value={1024}>Small (1024)</option>
      </select>
    {/if}
    <span class="spacer"></span>
    <Button variant="ghost" type="button" onclick={oncancel}>Cancel</Button>
    {#if altLabel && onalt}
      <Button variant="ghost" type="button" onclick={() => apply(onalt)}>{altLabel}</Button>
    {/if}
    <Button variant="primary" type="button" onclick={() => apply()}>{applyLabel}</Button>
  </div>
</div>

<style>
  .cropper {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
  }
  .frame {
    position: relative;
    align-self: center;
    line-height: 0;
    user-select: none;
    touch-action: none;
  }
  img {
    max-width: min(640px, 100%);
    max-height: var(--crop-max-height, 60vh);
    display: block;
  }
  .crop {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid #fff;
    box-shadow: 0 0 0 9999px color-mix(in srgb, var(--scrim) 80%, transparent);
    cursor: move;
  }
  .handle {
    position: absolute;
    width: 14px;
    height: 14px;
    background: #fff;
    border-radius: 3px;
  }
  /* Handles sit inside the crop so the frame's clipping never hides them. */
  .nw {
    left: -2px;
    top: -2px;
    cursor: nwse-resize;
  }
  .ne {
    right: -2px;
    top: -2px;
    cursor: nesw-resize;
  }
  .sw {
    left: -2px;
    bottom: -2px;
    cursor: nesw-resize;
  }
  .se {
    right: -2px;
    bottom: -2px;
    cursor: nwse-resize;
  }
  .frame {
    overflow: hidden;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .dims {
    font-size: 12.5px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .spacer {
    flex: 1;
  }
  .field {
    padding: 6px 8px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text);
    font: inherit;
  }
</style>
