<script lang="ts">
  import { formatNumber as localeNumber } from "$lib/i18n/localizer";
  import { t } from "$lib/i18n/localizer";
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
  <section class="recovery" role="status" aria-label={t("content.attachment_send_review")}>
    <strong>{record.parentUncertain ? t("content.album_start_is_unconfirmed") : t("content.some_file_results_are_unknown")}</strong>
    <p>{record.parentUncertain ? t("content.these_files_have_not_been_submitted_check_this_conversation_before_conti")
      : t("content.check_this_conversation_before_sending_these_files_again")}</p>
    {#if record.error}<p class="error">{record.error}</p>{/if}
    {#if record.diagnostic}<details><summary>{t("content.technical_details")}</summary><pre dir="ltr">{record.diagnostic}</pre></details>{/if}
    {#if record.sentIds.length}<p>{t("content.submitted_files", { count: record.sentIds.length })}</p>{/if}
    <ul>{#each [...record.retryable, ...record.uncertain] as item (item.id)}
      <li><span><bdi dir="auto">{item.file.name}</bdi></span><button type="button" data-recovery={recoveryKey(record)} data-file={item.id}
        aria-label={t("content.save_copy_of_value", { param0: (item.file.name) })} onclick={save}>{t("content.save_copy")}</button></li>
    {/each}</ul>
    <div class="actions">
      {#if record.retryable.length}<button type="button" data-recovery={recoveryKey(record)} onclick={(event) => action(event, onrestore)}>{t("content.restore_unsent_files")}</button>{/if}
      <button type="button" data-recovery={recoveryKey(record)} onclick={(event) => action(event, ondismiss)}>{t("content.dismiss_review")}</button>
    </div>
  </section>
{/each}

<style>
  .recovery { padding: 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); overflow-wrap: anywhere; }
  p { margin: 5px 0; color: var(--muted); }
  .error { color: var(--danger); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  ul { padding: 0; margin: 8px 0; list-style: none; }
  li, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  li span { flex: 1; min-width: 0; }
  button { padding: 5px 8px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--raised); color: var(--text); font: inherit; cursor: pointer; }
</style>
