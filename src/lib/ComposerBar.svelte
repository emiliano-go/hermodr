<!-- Everything below the message list: reply/edit previews, the staged-file
  tray, mention and emoji completion, the composer form and the attachment
  preview sheet. Moved out of +page.svelte. -->
<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Button from "$lib/Button.svelte";
  import ExpressionPicker, { type PickerTab } from "$lib/ExpressionPicker.svelte";
  import Icon from "$lib/Icon.svelte";
  import ImageCropper from "$lib/ImageCropper.svelte";
  import VideoPlayer from "$lib/VideoPlayer.svelte";
  import VoiceRecorder, { type Recording } from "$lib/VoiceRecorder.svelte";
  import { backgroundPress } from "$lib/press";
  import { imagePreview } from "$lib/files";
  import { replyIcon } from "$lib/message";
  import type { Emoji } from "$lib/emoji";
  import type { PendingMedia, StoredMessage } from "$lib/models";

  let {
    draft = $bindable(),
    composerInput = $bindable(),
    replyingTo,
    replyAuthor,
    replySnippet,
    editing,
    pending,
    sendOnce = $bindable(),
    recording = $bindable(),
    mentionMatches,
    mentionIndex = $bindable(),
    onselectmention,
    emojiToken,
    emojiMatches,
    emojiIndex = $bindable(),
    onselectemoji,
    pickerTab = $bindable(),
    selectedChat,
    enqueue,
    onpickeremoji,
    onpickersent,
    onpickererror,
    onstage,
    oncreatekind,
    oninput,
    onkey,
    onsend,
    oncancelreply,
    oncanceledit,
    onremove,
    onsendvoice,
    onvoiceerror,
    onreceipts,
    ontyping,
    receiptsHidden,
    typingHidden,
  }: {
    draft: string;
    composerInput: HTMLTextAreaElement | undefined;
    replyingTo: StoredMessage | null;
    replyAuthor: string;
    replySnippet: string;
    editing: { original: string } | null;
    pending: PendingMedia[];
    sendOnce: boolean;
    recording: boolean;
    mentionMatches: { jid: string; name: string; username: string | null; number: string | null }[];
    mentionIndex: number;
    onselectmention: (person: { jid: string; name: string }) => void;
    emojiToken: { query: string; start: number } | null;
    emojiMatches: Emoji[];
    emojiIndex: number;
    onselectemoji: (emoji: string) => void;
    pickerTab: PickerTab | null;
    selectedChat: string;
    enqueue: <T>(task: () => Promise<T>) => Promise<T>;
    onpickeremoji: (emoji: string) => void;
    onpickersent: () => void;
    onpickererror: (message: string) => void;
    onstage: (file: File) => void;
    oncreatekind: (kind: "poll" | "event") => void;
    oninput: (event: Event) => void;
    onkey: (event: KeyboardEvent) => void;
    onsend: () => void;
    oncancelreply: () => void;
    oncanceledit: () => void;
    onremove: (id: number) => void;
    onsendvoice: (note: Recording) => void;
    onvoiceerror: (message: string) => void;
    onreceipts: () => void;
    ontyping: () => void;
    receiptsHidden: boolean;
    typingHidden: boolean;
  } = $props();

  let attachMenu = $state(false);
  let filePicker: HTMLInputElement | undefined = $state();

  function attach(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    for (const file of files) onstage(file);
  }

  let cropping = $state(false);
  let previewId = $state<number | null>(null);
  let previewItem = $derived(pending.find((p) => p.id === previewId) ?? null);
  $effect(() => {
    void previewId;
    cropping = false;
  });
  /**
   * The scrim, the sheet's own padding and the video's letterbox close the preview; the image, the
   * player's controls, the crop button, the caption field, the name and "Done" never do. While the
   * cropper is open the sheet's gaps would throw the crop away, so only the scrim counts.
   */
  const previewDismiss = backgroundPress(
    (target) =>
      target instanceof Element &&
      target.matches(cropping ? ".sheet-backdrop" : ".sheet-backdrop, .preview-sheet, .preview-video"),
  );
  /** Swaps a staged image for its cropped or resized version. */
  async function replacePending(id: number, file: File) {
    const item = pending.find((p) => p.id === id);
    if (!item) return;
    if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    item.file = file;
    item.url = await imagePreview(file);
    cropping = false;
  }
</script>

{#if replyingTo}
  <div class="reply-preview">
    {#if replyingTo.media_kind === "image" && replyingTo.media_path}
      <img
        class="reply-thumb"
        src={convertFileSrc(replyingTo.media_path)}
        alt=""
      />
    {:else if replyingTo.media_kind}
      <span class="reply-icon">{replyIcon(replyingTo.media_kind)}</span>
    {/if}
    <span class="reply-body">
      <span class="reply-to">Replying to {replyAuthor}</span>
      <span class="reply-snippet">{replySnippet}</span>
    </span>
    <Button
      variant="icon"
      icon="x"
      iconSize={16}
      title="Cancel reply"
      aria-label="Cancel reply"
      onclick={oncancelreply} />
  </div>
{/if}

{#if editing}
  <div class="reply-preview editing-preview">
    <span class="reply-icon"><Icon name="edit" size={16} /></span>
    <span class="reply-body">
      <span class="reply-to">Editing message</span>
      <span class="reply-snippet">{editing.original}</span>
    </span>
    <Button
      variant="icon"
      icon="x"
      iconSize={16}
      title="Cancel edit"
      aria-label="Cancel edit"
      onclick={oncanceledit} />
  </div>
{/if}

{#if pending.length > 0}
  <div class="pending">
    {#each pending as item (item.id)}
      <div class="pending-item">
        <button class="pending-thumb" title="Preview and caption" onclick={() => (previewId = item.id)}>
          {#if item.kind === "image"}
            <img src={item.url} alt={item.file.name} />
          {:else if item.kind === "video"}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video src={item.url} preload="metadata" muted></video>
            <span class="play-badge">▶</span>
          {:else}
            <span class="file-icon"><Icon name="file" size={26} /></span>
          {/if}
        </button>
        <span class="pending-name" title={item.file.name}>{item.file.name}</span>
        {#if item.caption}
          <span class="pending-caption">{item.caption}</span>
        {/if}
        <Button
          variant="icon"
          icon="x"
          iconSize={12}
          cls="remove"
          title="Remove"
          aria-label="Remove"
          onclick={() => onremove(item.id)} />
      </div>
    {/each}
    <span class="pending-status">Type a caption below, then press Enter.</span>
  </div>
{/if}

{#if mentionMatches.length > 0}
  <div class="mentions">
    {#each mentionMatches as person, i (person.jid)}
      <button
        type="button"
        class="mention"
        class:active={i === mentionIndex}
        onclick={() => onselectmention(person)}
        onmouseenter={() => (mentionIndex = i)}>
        {person.name}
        {#if person.username && person.username !== person.name}<span class="mention-handle"
            >@{person.username}</span
          >{/if}
      </button>
    {/each}
  </div>
{/if}

<div class="composer-area">
{#if emojiToken && emojiMatches.length > 0}
  <div class="suggest" role="listbox" aria-label="Emoji suggestions">
    <span class="suggest-title">Emoji matching :{emojiToken.query}</span>
    {#each emojiMatches as e, i (e.emoji)}
      <button
        type="button"
        class="suggest-row"
        class:active={i === emojiIndex}
        role="option"
        aria-selected={i === emojiIndex}
        onmouseenter={() => (emojiIndex = i)}
        onclick={() => onselectemoji(e.emoji)}>
        <span class="suggest-emoji">{e.emoji}</span>
        :{e.shortcodes.find((c) => c.startsWith(emojiToken?.query.toLowerCase() ?? "")) ?? e.shortcodes[0] ?? e.label}:
      </button>
    {/each}
  </div>
{/if}
{#if pickerTab}
  <ExpressionPicker
    chat={selectedChat}
    bind:tab={pickerTab}
    {enqueue}
    onemoji={onpickeremoji}
    onsent={onpickersent}
    onerror={onpickererror}
    onclose={() => (pickerTab = null)} />
{/if}
<form class="composer" onsubmit={(e) => (e.preventDefault(), onsend())}>
  {#if recording}
    <VoiceRecorder
      onsend={onsendvoice}
      oncancel={() => (recording = false)}
      onerror={onvoiceerror} />
  {:else}
  <Button
    variant="icon"
    icon="plus"
    iconSize={22}
    cls="attach"
    active={attachMenu}
    title="Attach"
    aria-label="Attach"
    aria-expanded={attachMenu}
    onclick={() => (attachMenu = !attachMenu)} />
  {#if attachMenu}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="attach-catcher" role="presentation" onclick={() => (attachMenu = false)}></div>
    <div class="attach-menu" role="menu">
      <button
        type="button"
        role="menuitem"
        onclick={() => {
          attachMenu = false;
          filePicker?.click();
        }}><Icon name="paperclip" size={18} /> Upload a file</button>
      <button
        type="button"
        role="menuitem"
        onclick={() => {
          attachMenu = false;
          oncreatekind("poll");
        }}><Icon name="poll" size={18} /> Create poll</button>
      <button
        type="button"
        role="menuitem"
        onclick={() => {
          attachMenu = false;
          oncreatekind("event");
        }}><Icon name="calendar" size={18} /> Create event</button>
    </div>
  {/if}
  <input
    class="file-input"
    type="file"
    multiple
    bind:this={filePicker}
    onchange={attach}
  />
  <textarea
    bind:this={composerInput}
    value={draft}
    oninput={oninput}
    onkeydown={onkey}
    rows="1"
    placeholder={editing ? "Edit message" : pending.length > 0 ? "Add a caption (optional)" : "Type a message"}
  ></textarea>
  <div class="composer-tools">
    <Button
      variant="icon"
      icon={receiptsHidden ? "eyeOff" : "eye"}
      iconSize={20}
      active={receiptsHidden}
      pressed={receiptsHidden}
      title={receiptsHidden ? "Read receipts hidden here" : "Hide read receipts here"}
      aria-label="Hide read receipts here"
      onclick={onreceipts} />
    <Button
      variant="icon"
      icon="keyboard"
      iconSize={20}
      active={typingHidden}
      pressed={typingHidden}
      title={typingHidden ? "Typing not sent here" : "Stop sending typing here"}
      aria-label="Stop sending typing here"
      onclick={ontyping} />
    <Button
      variant="icon"
      cls="tool-text"
      active={pickerTab === "gif"}
      title="GIFs"
      onclick={() => (pickerTab = pickerTab === "gif" ? null : "gif")}>GIF</Button>
    <Button
      variant="icon"
      icon="sticker"
      iconSize={20}
      active={pickerTab === "sticker"}
      title="Stickers"
      aria-label="Stickers"
      onclick={() => (pickerTab = pickerTab === "sticker" ? null : "sticker")} />
    <Button
      variant="icon"
      icon="smile"
      iconSize={20}
      active={pickerTab === "emoji"}
      title="Emoji"
      aria-label="Emoji"
      onclick={() => (pickerTab = pickerTab === "emoji" ? null : "emoji")} />
    {#if pending.some((p) => p.kind !== "other")}
      <Button
        variant="icon"
        cls="once-toggle"
        active={sendOnce}
        pressed={sendOnce}
        title="View once"
        aria-label="View once"
        onclick={() => (sendOnce = !sendOnce)}>1</Button>
    {/if}
  </div>
  {#if !draft.trim() && pending.length === 0}
    <Button
      variant="send"
      icon="mic"
      iconSize={19}
      title="Record a voice message"
      aria-label="Record a voice message"
      onclick={() => (recording = true)} />
  {:else}
    <Button variant="send" icon="send" iconSize={18} type="submit" title="Send" aria-label="Send" />
  {/if}
  {/if}
</form>
</div>

{#if previewItem}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="sheet-backdrop"
    role="presentation"
    onpointerdown={previewDismiss.down}
    onclick={(e) => previewDismiss.click(e) && (previewId = null)}>
    <div
      class="sheet preview-sheet"
      role="dialog"
      aria-modal="true"
      aria-label="Attachment preview">
      {#if previewItem.kind === "image" && cropping}
        <ImageCropper
          file={previewItem.file}
          onapply={(file) => void replacePending(previewItem!.id, file)}
          oncancel={() => (cropping = false)} />
      {:else if previewItem.kind === "image"}
        <img class="preview-large" src={previewItem.url} alt={previewItem.file.name} />
        <Button variant="ghost" cls="crop-button" onclick={() => (cropping = true)}>Crop or resize</Button>
      {:else if previewItem.kind === "video"}
        <div class="preview-video">
          <VideoPlayer src={previewItem.url} autoplay={false} />
        </div>
      {:else}
        <span class="file-icon large"><Icon name="file" size={56} /></span>
      {/if}
      <span class="pending-name">{previewItem.file.name}</span>
      <input
        class="caption"
        value={previewItem.caption}
        oninput={(e) => previewItem && (previewItem.caption = e.currentTarget.value)}
        placeholder="Add a caption"
        autocomplete="off"
      />
      <Button variant="primary" onclick={() => (previewId = null)}>Done</Button>
    </div>
  </div>
{/if}

<style>
  .reply-preview {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 14px;
    padding: 8px 8px 8px 12px;
    background: var(--surface);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .reply-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .reply-to {
    color: var(--accent-text);
    font-weight: 600;
  }
  .reply-body span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reply-thumb {
    width: 32px;
    height: 32px;
    object-fit: cover;
    border-radius: 4px;
    flex: none;
  }
  .reply-icon {
    font-size: 16px;
    flex: none;
  }
  .pending {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: 12px;
    padding: 8px 14px;
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .pending-item {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 120px;
  }
  .pending-thumb {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    width: 120px;
    height: 72px;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    cursor: pointer;
  }
  .pending-status {
    align-self: center;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 13px;
  }
  .pending :global(.remove) {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 20px;
    height: 20px;
    min-width: 0;
    min-height: 0;
    border-radius: 999px;
    background: var(--raised-2);
    color: var(--text);
    box-shadow: 0 0 0 2px var(--surface);
  }
  .pending-thumb img,
  .pending-thumb video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .pending-name {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pending-caption {
    font-size: 11px;
    color: var(--accent-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mentions {
    display: flex;
    flex-direction: column;
    max-height: 180px;
    overflow-y: auto;
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .mention {
    text-align: left;
    background: transparent;
    border: 0;
    color: inherit;
    font: inherit;
    padding: 7px 14px;
    cursor: pointer;
  }
  .mention.active,
  .mention:hover {
    background: var(--raised);
  }
  .mention-handle {
    margin-left: 6px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .composer-area {
    position: relative;
    flex: none;
  }
  /* Discord-style completion list over the composer. */
  .suggest {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: calc(100% + 6px);
    z-index: 50;
    display: flex;
    flex-direction: column;
    max-height: 360px;
    overflow-y: auto;
    padding: 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .suggest-title {
    padding: 2px 8px 6px;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .suggest-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    text-align: left;
    cursor: pointer;
  }
  .suggest-row.active {
    background: var(--raised);
  }
  .suggest-emoji {
    font-size: 20px;
    width: 26px;
    text-align: center;
  }
  .attach-catcher {
    position: fixed;
    inset: 0;
    z-index: 55;
  }
  .attach-menu {
    position: absolute;
    left: 10px;
    bottom: calc(100% + 6px);
    z-index: 56;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .attach-menu button {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    text-align: left;
    cursor: pointer;
  }
  .attach-menu button :global(svg) {
    color: var(--accent);
  }
  .attach-menu button:hover {
    background: var(--raised);
  }
  .composer-tools {
    display: flex;
    align-items: center;
    align-self: center;
    gap: 2px;
  }
  .composer-tools :global(.btn-icon) {
    width: 38px;
    height: 38px;
  }
  .file-input {
    display: none;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 5px 16px 5px 10px;
    min-height: 62px;
    box-sizing: border-box;
    background: var(--surface);
    flex: none;
  }
  .composer > input:not(.file-input),
  .composer > textarea {
    flex: 1;
    align-self: center;
    background: var(--raised);
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: 9px 12px;
    color: inherit;
    font: inherit;
  }
  .composer > textarea {
    resize: none;
    line-height: 1.4;
    field-sizing: content;
    box-sizing: border-box;
    max-height: 140px;
  }
  .composer > textarea::placeholder {
    color: var(--faint);
  }
  .sheet-backdrop {
    position: fixed;
    inset: 0;
    z-index: 250;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 20px 22px;
    width: min(460px, 90vw);
    max-height: 86vh;
    overflow-y: auto;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .preview-sheet {
    width: min(680px, 80vw);
  }
  .preview-large {
    max-width: 100%;
    max-height: 60vh;
    border-radius: 8px;
    object-fit: contain;
  }
  /* VideoPlayer sizes itself to a size container. */
  .preview-video {
    width: 100%;
    height: 50vh;
    container-type: size;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  /* Arrives through Button's `cls`, so it must pierce into the child. */
  :global(.crop-button) {
    align-self: center;
  }  .file-icon {
    color: var(--muted);
  }
  .caption {
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 8px 10px;
    color: inherit;
    font: inherit;
  }
</style>
