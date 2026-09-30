<script lang="ts">
  import { MEDIA_TYPES } from "$lib/utils/auto-download";
  import type { MediaAutoDownload } from "$lib/utils/wire";
  let { value, onchange }: { value: MediaAutoDownload; onchange: (next: MediaAutoDownload) => void } = $props();
</script>

<div class="downloads">
  <p>Download incoming media automatically. Each chat can override any type.</p>
  {#each MEDIA_TYPES as [kind, label]}
    <label>
      <span>{label}</span>
      <input class="switch" type="checkbox" checked={value[kind]}
        onchange={(event) => onchange({ ...value, [kind]: event.currentTarget.checked })} />
    </label>
  {/each}
</div>

<style>
  .downloads { display: flex; flex-direction: column; gap: .8rem; }
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .85rem; margin: 0; }
  input { accent-color: var(--accent); }
</style>
