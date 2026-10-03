<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onDestroy, untrack } from "svelte";
  import type { GroupSettingChange, GroupSettings as Settings } from "$lib/utils/wire";
  import ImageCropper from "$lib/composer/ImageCropper.svelte";
  import { session } from "$lib/state/session.svelte";
  import { base64Of } from "$lib/utils/files";
  import { saveGroupMetadata } from "./group-settings-actions";

  let { chat, account, onload, onchange, onpicture, onbusy = () => {} }: {
    chat: string;
    account: string;
    onload: () => Promise<Settings>;
    onchange: (change: GroupSettingChange) => Promise<void>;
    onpicture: (data: string) => Promise<void>;
    onbusy?: (busy: boolean) => void;
  } = $props();

  let settings = $state<Settings | null>(null);
  let subject = $state("");
  let description = $state("");
  let picture = $state<File | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let stale = $state(false);
  let error = $state<LocalizedError | string | null>(null);
  let outcome = $state("");
  let generation = 0;
  const disabled = $derived(loading || busy || stale);

  function scope() { return { chat, account, generation }; }
  function current(target: ReturnType<typeof scope>) {
    return target.chat === chat && target.account === account && account === session.activeAccount && target.generation === generation;
  }
  function paint(snapshot: Settings) {
    settings = snapshot; subject = snapshot.subject ?? ""; description = snapshot.description ?? ""; stale = false;
  }

  async function refresh() {
    if (loading || busy) return;
    const target = scope();
    if (!current(target)) { error = normalizeError({ kind: "postal_error", code: "error.group_settings_account", params: {} }); return; }
    loading = true; error = null;
    try { const snapshot = await onload(); if (current(target)) paint(snapshot); }
    catch (failure) { if (current(target)) error = normalizeError(failure); }
    finally { if (current(target)) loading = false; }
  }

  $effect(() => {
    chat; account; session.activeAccount; ++generation;
    settings = null; subject = ""; description = ""; picture = null;
    loading = false; busy = false; stale = false; error = null; outcome = "";
    untrack(() => { onbusy(false); void refresh(); });
  });
  onDestroy(() => { ++generation; onbusy(false); });

  async function save(write: () => Promise<void>, label: string, acknowledged: () => void) {
    if (!settings || disabled) return;
    const target = scope();
    if (!current(target)) return;
    busy = true; onbusy(true); error = null; outcome = "";
    try {
      const result = await saveGroupMetadata(write, onload, () => current(target), () => {
        acknowledged(); outcome = label;
      });
      if (!current(target)) return;
      if (result.snapshot) paint(result.snapshot);
      if (result.refreshError) { stale = true; error = normalizeError({ kind: "postal_error", code: "error.group_settings_refresh", params: {}, diagnostic: result.refreshError.diagnostic }); }
    } catch (failure) { if (current(target)) error = normalizeError(failure); }
    finally { if (current(target)) { busy = false; onbusy(false); } }
  }

  async function change(change: GroupSettingChange, label: string) {
    if (!settings) return;
    await save(() => onchange(change), label, () => {
      if (!settings) return;
      switch (change.kind) {
        case "subject": settings = { ...settings, subject: change.text }; break;
        case "description": settings = { ...settings, description: change.text }; break;
        case "announce": settings = { ...settings, announce: change.enabled }; break;
        case "locked": settings = { ...settings, locked: change.enabled }; break;
        case "approval": settings = { ...settings, approval: change.enabled }; break;
      }
    });
  }

  async function upload(file: File | null) {
    const target = scope();
    await save(async () => {
      const data = file ? await base64Of(file) : "";
      if (current(target)) await onpicture(data);
    }, "group.picture", () => { picture = null; });
  }

  function toggle(kind: "announce" | "locked" | "approval", input: HTMLInputElement, label: string) {
    if (!settings) return;
    const enabled = input.checked;
    input.checked = settings[kind];
    void change({ kind, enabled }, label);
  }
</script>

<div class="group-settings" aria-busy={busy || loading}>
  {#if (loading || !error) && !settings}<p>{t("group.settings_loading")}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if outcome}<p role="status">{t("group.setting_saved", { label: t(outcome) })}</p>{/if}
  {#if error}<button disabled={loading || busy} onclick={refresh}>{t("group.settings_reload")}</button>{/if}
  {#if settings}
    {#if !settings.member}<p>{t("group.not_member")}</p>{/if}
    <label>{t("ui.name")}<input value={subject} disabled={!settings.can_edit_info || disabled} oninput={(event) => subject = event.currentTarget.value} /></label>
    {#if settings.can_edit_info}
      <div class="actions"><span>{Array.from(subject).length}/100</span>
        <button disabled={disabled || !subject.trim() || Array.from(subject).length > 100 || subject === (settings.subject ?? "")}
          onclick={() => change({ kind: "subject", text: subject }, "group.name")}>{t("group.name_save")}</button></div>
    {/if}
    <label>{t("group.description")}<textarea rows="4" value={description} disabled={!settings.can_edit_info || disabled}
      oninput={(event) => description = event.currentTarget.value}></textarea></label>
    {#if settings.can_edit_info}
      <div class="actions"><span>{Array.from(description).length}/2048</span>
        <button disabled={disabled || Array.from(description).length > 2048 || description === (settings.description ?? "")}
          onclick={() => { if (settings) void change({ kind: "description", text: description || null, previous_id: settings.description_id }, "group.description"); }}>{t("group.description_save")}</button></div>
    {/if}
    {#if settings.can_edit_picture}
      <div class="picture">
        <label>{t("group.picture")}<input type="file" accept="image/*" disabled={disabled}
          onchange={(event) => { picture = event.currentTarget.files?.[0] ?? null; event.currentTarget.value = ""; }} /></label>
        <button disabled={disabled} onclick={() => upload(null)}>{t("group.picture_remove")}</button>
      </div>
      {#if picture}
        <ImageCropper file={picture} square sizes={false} applyLabel={t("group.picture_save")}
          onapply={(file) => { if (!disabled) void upload(file); }} oncancel={() => { if (!busy) picture = null; }} />
      {/if}
    {/if}
    {#if settings.community}<p>{t("group.community_no_chat")}</p>
    {:else}<div class="setting"><span>{t("group.send_admin_only")}</span>
      {#if settings.admin}<input type="checkbox" aria-label={t("group.send_admin_only")} checked={settings.announce} disabled={disabled}
        onchange={(event) => toggle("announce", event.currentTarget, "group.message_permissions")} />
      {:else}<span>{settings.announce ? t("ui.on") : t("ui.off")}</span>{/if}</div>{/if}
    <div class="setting"><span>{t("group.edit_admin_only")}</span>
      {#if settings.admin}<input type="checkbox" aria-label={t("group.edit_admin_only")} checked={settings.locked} disabled={disabled}
        onchange={(event) => toggle("locked", event.currentTarget, "group.info_permissions")} />
      {:else}<span>{settings.locked ? t("ui.on") : t("ui.off")}</span>{/if}</div>
    <div class="setting"><span>{t("group.approve_join")}</span>
      {#if settings.admin}<input type="checkbox" aria-label={t("group.approve_join")} checked={settings.approval} disabled={disabled}
        onchange={(event) => toggle("approval", event.currentTarget, "group.join_approval")} />
      {:else}<span>{settings.approval ? t("ui.on") : t("ui.off")}</span>{/if}</div>
  {/if}
</div>

<style>
  .group-settings { display: flex; flex-direction: column; gap: 10px; }
  label { display: flex; flex-direction: column; gap: 6px; }
  p { margin: 0; }
  input:not([type="checkbox"]), textarea { box-sizing: border-box; width: 100%; padding: 8px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; font: inherit; }
  .setting, .actions { display: flex; flex-direction: row; align-items: center; justify-content: space-between; gap: 10px; }
  .actions span { color: var(--muted); font-size: 12px; }
  button { padding: 7px 10px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; font: inherit; cursor: pointer; }
  button:disabled, input:disabled, textarea:disabled { opacity: .5; cursor: default; }
  .error { color: var(--danger); }
  .picture { display: flex; flex-direction: column; gap: 8px; }
</style>
