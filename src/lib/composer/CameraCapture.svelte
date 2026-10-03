<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { onMount, untrack } from "svelte";
  import Button from "$lib/ui/Button.svelte";
  import { cameraFailure, cameraPhoto, openCamera, stopCamera, type CameraScope } from "$lib/utils/camera";

  let { account, chat, generation, onstage, onclose }: {
    account: string; chat: string; generation: number;
    onstage: (file: File, scope: CameraScope) => void | Promise<void>;
    onclose: () => void;
  } = $props();
  const owner = untrack(() => ({ account, chat, generation }));
  let dialog: HTMLDialogElement, video: HTMLVideoElement;
  let stream: MediaStream | null = null;
  let live = true, request = 0;
  let ready = $state(false), loading = $state(true), capturing = $state(false), failed = $state<LocalizedError | string | null>("");
  const current = () => live && account === owner.account && chat === owner.chat && generation === owner.generation;

  function release() {
    stopCamera(stream); stream = null; ready = false;
    if (video) { video.pause(); video.srcObject = null; }
  }
  function close() {
    if (!live) return;
    live = false; request++; release(); dialog?.close(); onclose();
  }
  async function start() {
    const attempt = ++request;
    const active = () => current() && request === attempt;
    release(); loading = true; failed = "";
    try {
      const acquired = await openCamera(active);
      if (!acquired) return;
      if (!active()) { stopCamera(acquired); return; }
      stream = acquired; video.srcObject = acquired;
      try { await video.play(); }
      catch { if (active()) { release(); failed = new LocalizedError({ kind: "postal_error", code: "error.content.the_camera_preview_could_not_start_try_again", params: {} }); } }
    } catch (error) { if (active()) { release(); const failure = normalizeError(error); failed = failure.code === "error.operation_failed" ? new LocalizedError({ ...failure.descriptor, code: "error.content.the_camera_preview_could_not_start_try_again", diagnostic: cameraFailure(error) }) : failure; } }
    finally { if (active()) loading = false; }
  }
  async function capture() {
    if (!current() || !ready || loading || capturing) return;
    const attempt = request;
    capturing = true; failed = "";
    try {
      const file = await cameraPhoto(video);
      if (!current() || attempt !== request) return;
      await onstage(file, { ...owner });
      if (current() && attempt === request) close();
    } catch (error) {
      if (current() && attempt === request) { const failure = normalizeError(error); failed = failure.code === "error.operation_failed" ? new LocalizedError({ ...failure.descriptor, code: "error.content.the_photo_could_not_be_captured" }) : failure; }
    } finally { if (current() && attempt === request) capturing = false; }
  }
  $effect(() => { if (!current()) close(); });
  onMount(() => {
    const opener = document.activeElement;
    dialog.showModal(); void start();
    return () => {
      const restore = dialog.contains(document.activeElement) || document.activeElement === document.body;
      live = false; request++; release(); dialog.close();
      if (restore && opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  });
</script>

<dialog bind:this={dialog} aria-labelledby="camera-heading" aria-describedby="camera-description"
  oncancel={(event) => { event.preventDefault(); close(); }} onclose={close}>
  <header><h2 id="camera-heading">{t("content.take_a_photo")}</h2><Button variant="icon" icon="x" aria-label={t("content.close_camera")} onclick={close} /></header>
  <p id="camera-description">{t("content.the_photo_is_added_to_your_attachments_review_it_before_sending")}</p>
  <!-- svelte-ignore a11y_media_has_caption -->
  <video bind:this={video} autoplay muted playsinline aria-label={t("content.camera_preview")}
    onloadeddata={() => { ready = current() && video.readyState >= 2 && video.videoWidth > 0 && video.videoHeight > 0; }}></video>
  {#if loading}<p role="status">{t("content.requesting_camera_access")}</p>{/if}
  {#if failed}<p class="error" role="alert">{failed}</p>{/if}
  {#if failed instanceof LocalizedError && failed.diagnostic}<details><summary>{t("content.technical_details")}</summary><pre dir="ltr">{failed.diagnostic}</pre></details>{/if}
  <footer>
    <Button variant="ghost" onclick={close}>{t("content.cancel")}</Button>
    {#if failed && !loading}<Button variant="ghost" disabled={capturing} onclick={() => void start()}>{t("content.try_again")}</Button>{/if}
    <Button variant="primary" disabled={!ready || loading || capturing} onclick={() => void capture()}>{capturing ? t("content.capturing") : t("content.take_photo")}</Button>
  </footer>
</dialog>

<style>
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  dialog { width: min(560px, calc(100vw - 32px)); box-sizing: border-box; padding: 18px; border: 1px solid var(--line); border-radius: var(--radius-lg); color: var(--text); background: var(--surface); box-shadow: 0 8px 28px var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header, footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h2 { margin: 0; font-size: 1.125rem; }
  p { margin: 10px 0; color: var(--muted); font-size: 0.8125rem; }
  video { display: block; width: 100%; max-height: 50vh; object-fit: contain; background: #000; border-radius: var(--radius-sm); }
  footer { justify-content: flex-end; margin-top: 14px; }
  .error { color: var(--danger); }
</style>
