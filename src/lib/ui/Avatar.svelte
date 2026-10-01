<!-- One profile picture with an initials fallback, wherever it is shown. -->
<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import { hue, initials } from "$lib/utils/avatar";

  let {
    src,
    label,
    seed,
    cls = "avatar",
    version,
    iconFallback = false,
    iconSize = 11,
  }: {
    src: string | null;
    label: string;
    /** Value the fallback colour is derived from; defaults to the label. */
    seed?: string;
    /** Class carrying the size/shape for this spot (avatar, choice-avatar, …). */
    cls?: string;
    /** Bumped when the picture file keeps its name but changed. */
    version?: number;
    /** Show a person icon instead of initials when the label has no letters. */
    iconFallback?: boolean;
    iconSize?: number;
  } = $props();

  const tint = $derived(hue(seed ?? label));
  const short = $derived(initials(label));
  let authorized = $state<string | null>(null);
  $effect(() => {
    const path = src;
    let current = true;
    authorized = null;
    if (path) void invoke<Record<string, string | null>>("authorize_media_assets", { paths: [path] })
      .then((paths) => { if (current) authorized = paths[path] ?? null; }).catch(() => {});
    return () => { current = false; };
  });
  const url = $derived(authorized ? convertFileSrc(authorized) + (version !== undefined ? `?v=${version}` : "") : null);
  const showIcon = $derived(iconFallback && !/\p{L}/u.test(label));
</script>

{#if url}
  <img class={cls} src={url} alt="" />
{:else}
  <span class={cls} style="--hue: {tint}">
    {#if showIcon}<Icon name="user" size={iconSize} />{:else}{short}{/if}
  </span>
{/if}
