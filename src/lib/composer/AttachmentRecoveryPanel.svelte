<script lang="ts">
  import type { AttachmentRecovery } from "$lib/utils/models";
  import { recoveryKey } from "$lib/utils/attachment-recovery";

  let { records, onrestore, ondismiss, onsave }: {
    records: readonly AttachmentRecovery[];
    onrestore: (key: string) => void;
    ondismiss: (key: string) => void;
    onsave: (key: string, fileId: number) => void;
  } = $props();

  function action(event: MouseEvent, run: (key: string) => void) {
    const key = (event.currentTarget as HTMLElement).dataset.recovery;
    if (key) run(key);
  }
  function save(event: MouseEvent) {
    const node = event.currentTarget as HTMLElement;
    if (node.dataset.recovery && node.dataset.file) onsave(node.dataset.recovery, Number(node.dataset.file));
  }
</script>

{#each records as record (recoveryKey(record))}
  <section class="recovery" role="status" aria-label="Attachment send review">
    <strong>{record.parentUncertain ? "Album start is unconfirmed" : "Some file results are unknown"}</strong>
    <p>{record.parentUncertain ? "These files have not been submitted. Check this conversation before continuing the album."
      : "Check this conversation before sending these files again."}</p>
    {#if record.sentIds.length}<p>{record.sentIds.length} file(s) submitted. Delivery is tracked in the conversation.</p>{/if}
    <ul>{#each [...record.retryable, ...record.uncertain] as item (item.id)}
      <li><span>{item.file.name}</span><button type="button" data-recovery={recoveryKey(record)} data-file={item.id}
        aria-label={`Save copy of ${item.file.name}`} onclick={save}>Save copy</button></li>
    {/each}</ul>
    <div class="actions">
      {#if record.retryable.length}<button type="button" data-recovery={recoveryKey(record)} onclick={(event) => action(event, onrestore)}>Restore unsent files</button>{/if}
      <button type="button" data-recovery={recoveryKey(record)} onclick={(event) => action(event, ondismiss)}>Dismiss review</button>
    </div>
  </section>
{/each}

<style>
  .recovery { padding: 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); overflow-wrap: anywhere; }
  p { margin: 5px 0; color: var(--muted); }
  ul { padding: 0; margin: 8px 0; list-style: none; }
  li, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  li span { flex: 1; min-width: 0; }
  button { padding: 5px 8px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); color: var(--text); font: inherit; cursor: pointer; }
</style>
