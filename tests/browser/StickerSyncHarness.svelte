<script lang="ts">
  import StickerSync from "$lib/media/StickerSync.svelte";
  import type { StickerLibrary, StickerResyncReport } from "$lib/utils/wire";
  import type { StickerScope } from "$lib/utils/sticker-sync";
  let account = $state("alpha"), chat = $state("one"), generation = $state(1), connected = $state(true);
  let failLoad = $state(false), failSync = $state(false), partial = $state(true), defer = $state(false), version = $state(0), waiting = $state(0);
  let calls = $state<string[]>([]);
  const releases: (() => void)[] = [];
  const library: StickerLibrary = { packs: [{ pack_id: "shared", name: "Known shared pack", publisher: "Synthetic", tray_path: null, origin: "shared", updated_at: 1 }], favorites: [], recent: [], catalog_complete: false };
  async function delay() {
    if (!defer) return;
    waiting++;
    await new Promise<void>((yes) => releases.push(yes));
    waiting--;
  }
  async function load(scope: StickerScope): Promise<StickerLibrary> {
    const fail = failLoad;
    calls = [...calls, `load ${JSON.stringify(scope)}`];
    await delay();
    if (fail) throw new Error("Synthetic cached-library failure");
    return library;
  }
  async function resync(scope: StickerScope): Promise<StickerResyncReport> {
    const fail = failSync, isPartial = partial;
    calls = [...calls, `resync ${JSON.stringify(scope)}`];
    await delay();
    if (fail) throw new Error("Synthetic snapshot failure");
    return { packs: 1, stickers: 3, known_packs: isPartial ? 2 : 1, packs_changed: 1, stickers_changed: 2, skipped_stickers: isPartial ? 1 : 0,
      app_state_synced: !isPartial, app_state_retryable: isPartial, app_state_fatal: false, app_state_error: isPartial ? "Synthetic retryable snapshot state" : null,
      pack_failures: isPartial ? [{ pack_id: "failed-pack", error: "Synthetic pack refresh failure" }] : [], mirror_verified: false, catalog_complete: false };
  }
  function release() { releases.splice(0).forEach((yes) => yes()); }
</script>

<h1>Synthetic sticker sync</h1>
<button onclick={() => (account = account === "alpha" ? "beta" : "alpha")}>Switch account</button>
<button onclick={() => (chat = chat === "one" ? "two" : "one")}>Switch chat</button>
<button onclick={() => generation++}>Replace service</button>
<button onclick={() => version++}>Refresh cache</button>
<button onclick={release} disabled={!waiting}>Release requests {waiting}</button>
<label><input type="checkbox" bind:checked={connected} /> Connected</label>
<label><input type="checkbox" bind:checked={defer} /> Defer requests</label>
<label><input type="checkbox" bind:checked={failLoad} /> Fail cache reads</label>
<label><input type="checkbox" bind:checked={failSync} /> Fail resync</label>
<label><input type="checkbox" bind:checked={partial} /> Retryable partial report</label>
<p>Scope {account} / {chat} / {generation}</p>
<StickerSync {account} {chat} {generation} {connected} {version} showPacks onload={load} onresync={resync}
  onlibrary={(scope) => (calls = [...calls, `loaded ${JSON.stringify(scope)}`])}
  onsynced={(scope) => (calls = [...calls, `accepted ${JSON.stringify(scope)}`])} />
<pre aria-label="Synthetic sticker operations">{calls.join("\n")}</pre>
