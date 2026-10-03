<!-- Everything below the message list: reply/edit previews, the staged-file
  tray, mention and emoji completion, the composer form and the attachment
  preview sheet. Moved out of +page.svelte. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { LocalizedError } from "$lib/i18n/errors";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Button from "$lib/ui/Button.svelte";
  import ExpressionPicker, { type PickerTab } from "$lib/composer/ExpressionPicker.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import ImageCropper from "$lib/composer/ImageCropper.svelte";
  import VideoPlayer from "$lib/media/VideoPlayer.svelte";
  import VoiceRecorder, { type Recording } from "$lib/composer/VoiceRecorder.svelte";
  import { backgroundPress } from "$lib/utils/press";
  import { imagePreview } from "$lib/utils/files";
  import { replyIcon } from "$lib/utils/message";
  import type { Emoji } from "$lib/utils/emoji";
  import type { PendingMedia, StoredMessage } from "$lib/utils/models";
  import { scheduled } from "$lib/state/scheduled.svelte";
  import { tick } from "svelte";
  import Soundboard from "$lib/soundboard/Soundboard.svelte";
  import SlashCommandMenu from "$lib/composer/SlashCommandMenu.svelte";
  import { slashToken, replaceSlashToken, type SlashCommandId, type SlashSelection } from "$lib/utils/slash-commands";
  import { canChooseMediaQuality } from "$lib/utils/media-quality";
  import { isAlbumSelection } from "$lib/utils/upload";
  import { broadcastSendReason } from "$lib/utils/broadcast";
  import type { MediaQuality } from "$lib/utils/wire";
  import ScheduleDialog from "./ScheduleDialog.svelte";
  import CameraCapture from "./CameraCapture.svelte";
  import QuickRepliesMenu from "$lib/chat/QuickRepliesMenu.svelte";
  import { quickReplies } from "$lib/state/quick-replies.svelte";
  import type { QuickReplyScope } from "$lib/utils/quick-replies";
  import type { QuickReply } from "$lib/utils/wire";

  let {
    draft = $bindable(),
    composerInput = $bindable(),
    replyingTo,
    replyAuthor,
    replySnippet,
    editing,
    pending,
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
    takereply,
    onpickeremoji,
    onpickersent,
    onpickererror,
    onstage,
    oncreatekind,
    oninput,
    onbeforeinput = () => {},
    onkey,
    onsend,
    onschedule = async () => false,
    oncancelreply,
    oncanceledit,
    onremove,
    ontoggleonce,
    onsendvoice,
    onvoiceerror,
    onreceipts,
    ontyping,
    receiptsHidden,
    typingHidden,
    account = null,
    generation = 0,
    disabled = false,
    connected = !disabled,
    defaultQuality = "hd",
    onsoundclip = async () => { throw new LocalizedError({ kind: "postal_error", code: "error.content.audio_clip_sending_is_unavailable", params: {} }); },
    onslashcommand = () => {},
    onsharecontacts = () => {},
    onquickreply = () => {},
  }: {
    draft: string;
    composerInput: HTMLTextAreaElement | undefined;
    replyingTo: StoredMessage | null;
    replyAuthor: string;
    replySnippet: string;
    editing: { original: string } | null;
    pending: PendingMedia[];
    recording: boolean;
    mentionMatches: {
      jid: string;
      name: string;
      username: string | null;
      number: string | null;
      aliases: string[];
      token: string;
    }[];
    mentionIndex: number;
    onselectmention: (person: { jid: string; name: string; token: string }) => void;
    emojiToken: { query: string; start: number } | null;
    emojiMatches: Emoji[];
    emojiIndex: number;
    onselectemoji: (emoji: string) => void;
    pickerTab: PickerTab | null;
    selectedChat: string;
    enqueue: <T>(task: (signal: AbortSignal) => Promise<T>) => Promise<T>;
    /** Takes the reply being composed as send arguments, clearing it. */
    takereply: () => Record<string, string>;
    onpickeremoji: (emoji: string) => void;
    onpickersent: () => void;
    onpickererror: (message: string | LocalizedError) => void;
    onstage: (file: File) => void;
    oncreatekind: (kind: "poll" | "event") => void;
    oninput: (event: Event) => void;
    onbeforeinput?: (event: InputEvent) => void;
    onkey: (event: KeyboardEvent) => void;
    onsend: () => void;
    onschedule?: (dueAt: number) => Promise<boolean>;
    oncancelreply: () => void;
    oncanceledit: () => void;
    onremove: (id: number) => void;
    ontoggleonce: (id: number) => void;
    onsendvoice: (note: Recording) => void;
    onvoiceerror: (message: string | LocalizedError) => void;
    onreceipts: () => void;
    ontyping: () => void;
    receiptsHidden: boolean;
    typingHidden: boolean;
    account?: string | null;
    generation?: number;
    disabled?: boolean;
    connected?: boolean;
    defaultQuality?: MediaQuality;
    onsoundclip?: (file: File, scope: { account: string; chat: string; generation: number }) => Promise<void>;
    onslashcommand?: (command: SlashCommandId) => void;
    onsharecontacts?: () => void;
    onquickreply?: (scope: QuickReplyScope, reply: QuickReply) => void;
  } = $props();

  let attachMenu = $state(false);
  let attachTimer: ReturnType<typeof setTimeout> | null = null;
  function openAttach() {
    if (disabled) return;
    if (attachTimer) { clearTimeout(attachTimer); attachTimer = null; }
    attachMenu = true;
  }
  function scheduleAttachClose() {
    if (attachTimer) clearTimeout(attachTimer);
    attachTimer = setTimeout(() => { attachMenu = false; attachTimer = null; }, 150);
  }
  function closeAttach() {
    if (attachTimer) { clearTimeout(attachTimer); attachTimer = null; }
    attachMenu = false;
  }
  let toolsMenu = $state(false);
  let toolsTimer: ReturnType<typeof setTimeout> | null = null;
  function openTools() {
    if (!account || !selectedChat) return;
    if (toolsTimer) { clearTimeout(toolsTimer); toolsTimer = null; }
    toolsMenu = true;
  }
  function scheduleToolsClose() {
    if (toolsTimer) clearTimeout(toolsTimer);
    toolsTimer = setTimeout(() => { toolsMenu = false; toolsTimer = null; }, 150);
  }
  function closeTools() {
    if (toolsTimer) { clearTimeout(toolsTimer); toolsTimer = null; }
    toolsMenu = false;
  }
  let soundboardOpen = $state(false);
  let cameraOpen = $state(false);
  function submitComposer(event: SubmitEvent) {
    event.preventDefault();
    if (!disabled && !document.querySelector("dialog[open]")) onsend();
  }
  function enqueuePicker<T>(task: (signal: AbortSignal) => Promise<T>): Promise<T> {
    if (disabled) return Promise.reject(new LocalizedError({ kind: "postal_error", code: "error.content.message_sending_is_disabled_here", params: {} }));
    return enqueue((signal) => {
      if (disabled) throw new LocalizedError({ kind: "postal_error", code: "error.content.message_sending_is_disabled_here", params: {} });
      return task(signal);
    });
  }
  let caret = $state({ start: 0, end: 0 });
  let dismissedSlash = $state<string | null>(null);
  const activeSlash = $derived(slashToken(draft, caret.start, caret.end));
  const slashKey = $derived(activeSlash ? JSON.stringify(activeSlash) : null);
  const slashDisabled = $derived({ location: t("content.location_sending_is_unavailable"), "keep-in-chat": t("content.keep_in_chat_is_unavailable"),
    ...(!selectedChat.endsWith("@g.us") ? { "mention-all": t("content.mention_all_is_available_in_groups") } : {}) });
  $effect(() => { void account; void selectedChat; void generation; soundboardOpen = false; cameraOpen = false; dismissedSlash = null; closeTools(); });
  $effect(() => { if (disabled || editing) cameraOpen = false; });
  $effect(() => { if (disabled) { closeAttach(); pickerTab = null; scheduling = false; } });

  function updateCaret() {
    if (composerInput && (caret.start !== composerInput.selectionStart || caret.end !== composerInput.selectionEnd)) {
      caret = { start: composerInput.selectionStart, end: composerInput.selectionEnd };
    }
  }
  async function chooseSlash(selection: SlashSelection) {
    if (disabled || selection.account !== account || selection.chat !== selectedChat || selection.generation !== generation
      || !composerInput || composerInput.selectionStart !== selection.token.end || composerInput.selectionEnd !== selection.token.end) return;
    const replacement = selection.command === "mention-all" ? "@all " : "";
    const next = replaceSlashToken(draft, selection.token, replacement);
    if (next === null) return;
    draft = next;
    dismissedSlash = slashKey;
    if (selection.command === "poll" || selection.command === "event") oncreatekind(selection.command);
    else if (selection.command === "sticker" || selection.command === "gif") pickerTab = selection.command;
    else onslashcommand(selection.command);
    await tick();
    if (selection.account !== account || selection.chat !== selectedChat || selection.generation !== generation) return;
    const at = selection.token.start + replacement.length;
    composerInput?.setSelectionRange(at, at);
    updateCaret();
  }
  let scheduling = $state(false);
  let schedulingChat = "";
  $effect(() => { if (selectedChat !== schedulingChat) { scheduling = false; schedulingChat = selectedChat; } });

  /** A plain draft can be scheduled; while one can, the clock schedules it. */
  const canSchedule = $derived(
    !disabled && !!draft.trim() && !editing && !replyingTo && pending.length === 0,
  );
  let filePicker: HTMLInputElement | undefined = $state();

  function attach(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    if (disabled) return;
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
    item.quality = canChooseMediaQuality(file) ? item.quality ?? defaultQuality : undefined;
    item.url = await imagePreview(file);
    cropping = false;
  }
</script>

{#if replyingTo}
  <div class="reply-preview">
    {#if !replyingTo.spoiler && replyingTo.media_kind === "image" && replyingTo.media_path}
      <img
        class="reply-thumb"
        src={convertFileSrc(replyingTo.media_path)}
        alt=""
      />
    {:else if replyingTo.media_kind}
      <span class="reply-icon">{replyIcon(replyingTo.media_kind)}</span>
    {/if}
    <span class="reply-body">
      <span class="reply-to">{t("content.replying_to")} {replyAuthor}</span>
      <span class="reply-snippet">{replySnippet}</span>
    </span>
    <Button
      variant="icon"
      icon="x"
      iconSize={16}
      title={t("content.cancel_reply")}
      aria-label={t("content.cancel_reply")}
      onclick={oncancelreply} />
  </div>
{/if}

{#if editing}
  <div class="reply-preview editing-preview">
    <span class="reply-icon"><Icon name="edit" size={16} /></span>
    <span class="reply-body">
      <span class="reply-to">{t("content.editing_message")}</span>
      <span class="reply-snippet">{editing.original}</span>
    </span>
    <Button
      variant="icon"
      icon="x"
      iconSize={16}
      title={t("content.cancel_edit")}
      aria-label={t("content.cancel_edit")}
      onclick={oncanceledit} />
  </div>
{/if}

{#if pending.length > 0}
  <p class="attachment-mode">{pending[0].retry?.parentId ? t("content.continue_album_original_captions_and_reply_kept_draft_stays_in_the_compo")
    : pending[0].retry ? t("content.retry_unsent_files_original_captions_and_reply_kept_draft_stays_in_the_c")
    : isAlbumSelection(pending) ? t("content.send_as_one_album_2_8_photos_or_videos") : t("content.send_as_separate_files_albums_support_2_8_ordinary_photos_or_videos")}</p>
  <div class="pending">
    {#each pending as item (item.id)}
      <div class="pending-item">
        <button class="pending-thumb" title={t("content.preview_and_caption")} onclick={() => (previewId = item.id)}>
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
        {#if item.kind !== "other"}
          <button
            type="button"
            class="once-toggle"
            class:active={item.once}
            aria-pressed={item.once}
            title={item.once ? t("content.sent_as_view_once_tap_for_normal") : t("content.send_as_view_once")}
            aria-label={t("content.view_once")}
            onclick={() => ontoggleonce(item.id)}>1</button>
        {/if}
        <span class="pending-name" title={item.file.name}><bdi dir="auto">{item.file.name}</bdi></span>
        {#if canChooseMediaQuality(item.file)}
          <select class="quality" aria-label={t("content.upload_quality_for", { name: item.file.name })} value={item.quality ?? defaultQuality}
            onchange={(event) => { item.quality = event.currentTarget.value as MediaQuality; }}>
            <option value="standard">{t("content.standard")}</option><option value="hd">{t("content.hd_original")}</option>
          </select>
        {/if}
        {#if item.caption}
          <span class="pending-caption"><bdi dir="auto">{item.caption}</bdi></span>
        {/if}
        <Button
          variant="icon"
          icon="x"
          iconSize={12}
          cls="remove"
          title={t("content.remove")}
          aria-label={t("content.remove")}
          onclick={() => onremove(item.id)} />
      </div>
    {/each}
    <span class="pending-status">{t("content.type_a_caption_below_then_press_enter")}</span>
  </div>
{/if}

{#if !disabled && mentionMatches.length > 0}
  <div class="mentions">
    {#each mentionMatches as person, i (person.jid)}
      <button
        type="button"
        class="mention"
        class:active={i === mentionIndex}
        onclick={() => onselectmention(person)}
        onmouseenter={() => (mentionIndex = i)}>
        <bdi dir="auto">{person.name}</bdi>
        <!-- The alias that matched, else their username. Either way it is a
          hint: the row is addressed by name either way. -->
        {#if person.token !== person.name}<span class="mention-handle"> - @{person.token}</span
          >{:else if person.username && person.username !== person.name}<span class="mention-handle"
            >@{person.username}</span
          >{/if}
      </button>
    {/each}
  </div>
{/if}

<div class="composer-area">
{#if activeSlash && slashKey !== dismissedSlash && !editing && !recording && !disabled}
  {#key `${account}/${selectedChat}/${generation}`}
    <SlashCommandMenu input={composerInput} token={activeSlash} {account} chat={selectedChat} {generation}
      disabled={slashDisabled} onchoose={(selection) => { void chooseSlash(selection); }} onclose={() => { dismissedSlash = slashKey; }} />
  {/key}
{/if}
<Soundboard open={soundboardOpen} {account} chat={selectedChat} {generation} disabled={disabled || !!editing || recording}
  onsend={async (file, scope) => {
    if (disabled) throw new LocalizedError({ kind: "postal_error", code: "error.content.message_sending_is_disabled_here", params: {} });
    await onsoundclip(file, scope);
  }} onclose={() => { soundboardOpen = false; }} />
{#if !disabled && emojiToken && emojiMatches.length > 0}
  <div class="suggest" role="listbox" aria-label={t("content.emoji_suggestions")}>
    <span class="suggest-title">{t("content.emoji_matching")}{emojiToken.query}</span>
    {#each emojiMatches as e, i (e.emoji)}
      <button
        type="button"
        class="suggest-row"
        class:active={i === emojiIndex}
        role="option"
        aria-selected={i === emojiIndex}
        onmousedown={(event) => event.preventDefault()}
        onmouseenter={() => (emojiIndex = i)}
        onclick={() => onselectemoji(e.emoji)}>
        <span class="suggest-emoji">{e.emoji}</span>
        :{e.shortcodes.find((c) => c.startsWith(emojiToken?.query.toLowerCase() ?? "")) ?? e.shortcodes[0] ?? e.label}:
      </button>
    {/each}
  </div>
{/if}
{#if pickerTab && !disabled}
  <ExpressionPicker
    {disabled}
    chat={selectedChat}
    bind:tab={pickerTab}
    enqueue={enqueuePicker}
    {takereply}
    onemoji={onpickeremoji}
    onsent={onpickersent}
    onerror={onpickererror}
    onclose={() => (pickerTab = null)} />
{/if}
<form class="composer" onsubmit={submitComposer}>
  {#if recording}
    <VoiceRecorder
      {disabled}
      onsend={(note) => { if (!disabled) onsendvoice(note); }}
      oncancel={() => (recording = false)}
      onerror={onvoiceerror} />
  {:else}
  {#if attachMenu && !disabled}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- Sibling of the hover wrapper: nested inside, its fullscreen hit area
      would keep the pointer "inside" the wrapper and mouseleave would never fire. -->
    <div class="attach-catcher" role="presentation" onclick={() => closeAttach()}></div>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="attach-wrap"
    role="group"
    aria-label={t("content.attach")}
    onmouseenter={openAttach}
    onmouseleave={scheduleAttachClose}
    onfocusin={openAttach}
    onfocusout={scheduleAttachClose}
    onkeydown={(event) => { if (event.key === "Escape") closeAttach(); }}>
  <Button
    variant="icon"
    icon="plus"
    iconSize={22}
    cls="attach"
    active={attachMenu}
    title={t("content.attach")}
    aria-label={t("content.attach")}
    aria-haspopup="menu"
    aria-expanded={attachMenu}
    {disabled}
    onclick={() => { if (!disabled) attachMenu = !attachMenu; }} />
  {#if attachMenu && !disabled}
    <div class="attach-menu" role="menu" aria-label={t("content.attach")}>
      <button type="button" role="menuitem" disabled={disabled || editing !== null || !account}
        onclick={() => { closeAttach(); onsharecontacts(); }}><Icon name="user" size={18} /> {t("content.share_contacts")}</button>
      <button type="button" role="menuitem" disabled={disabled || editing !== null || !account}
        onclick={() => { closeAttach(); cameraOpen = true; }}><Icon name="image" size={18} /> {t("content.take_a_photo")}</button>
      <button
        type="button"
        role="menuitem"
        {disabled}
        onclick={() => {
          if (disabled) return;
          closeAttach();
          filePicker?.click();
        }}><Icon name="paperclip" size={18} /> {t("content.upload_a_file")}</button>
      <button
        type="button"
        role="menuitem"
        {disabled}
        onclick={() => {
          if (disabled) return;
          closeAttach();
          oncreatekind("poll");
        }}><Icon name="poll" size={18} /> {t("content.create_poll")}</button>
      <button
        type="button"
        role="menuitem"
        {disabled}
        onclick={() => {
          if (disabled) return;
          closeAttach();
          oncreatekind("event");
        }}><Icon name="calendar" size={18} /> {t("content.create_event")}</button>
    </div>
  {/if}
  </div>
  <input
    class="file-input"
    type="file"
    multiple
    {disabled}
    bind:this={filePicker}
    onchange={attach}
  />
  <textarea
    bind:this={composerInput}
    value={draft}
    {disabled}
    onbeforeinput={(event) => { if (!disabled) onbeforeinput(event); }}
    oninput={(event) => { if (disabled) return; oninput(event); dismissedSlash = null; updateCaret(); }}
    onkeyup={updateCaret}
    onclick={updateCaret}
    onselect={updateCaret}
    onkeydown={(event) => { if (!disabled) onkey(event); }}
    rows="1"
    placeholder={editing ? t("content.edit_message") : pending.length > 0 ? t("content.add_a_caption_optional") : t("content.type_a_message")}
  ></textarea>
  <div class="composer-tools">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="tools-wrap"
      role="group"
      aria-label={t("content.more_messaging_options")}
      onmouseenter={openTools}
      onmouseleave={scheduleToolsClose}
      onfocusin={openTools}
      onfocusout={scheduleToolsClose}
      onkeydown={(event) => { if (event.key === "Escape") closeTools(); }}>
      <Button
        variant="icon"
        icon="sliders"
        iconSize={20}
        active={toolsMenu}
        title={t("content.more_messaging_options")}
        aria-label={t("content.more_messaging_options")}
        aria-haspopup="menu"
        aria-expanded={toolsMenu}
        disabled={!account || !selectedChat}
        onclick={() => { if (account && selectedChat) toolsMenu = !toolsMenu; }} />
      {#if toolsMenu && account && selectedChat}
        <div class="tools-menu" role="menu" aria-label={t("content.more_messaging_options")}>
          <QuickRepliesMenu {account} chat={selectedChat} {generation} requestKey={quickReplies.key}
            dataScope={quickReplies.scope(selectedChat)} replies={quickReplies.replies} loading={quickReplies.loading}
            syncing={quickReplies.syncing} error={quickReplies.error} {connected} menuItem
            disabled={disabled || !account || !!editing || recording}
            onopen={closeTools}
            onselect={(scope, reply) => { if (!disabled) onquickreply(scope, reply); }}
            onsync={(scope) => { void quickReplies.sync(scope); }} />
          <Button
            variant="menu"
            icon="clock"
            iconSize={18}
            title={canSchedule ? t("content.schedule_this_message") : t("content.scheduled_messages")}
            aria-label={canSchedule ? t("content.schedule_this_message") : t("content.scheduled_messages")}
            onclick={() => {
              closeTools();
              if (canSchedule) scheduling = true;
              else scheduled.open = true;
            }}>{canSchedule ? t("content.schedule_this_message") : t("content.scheduled_messages")}</Button>
          <Button
            variant="menu"
            icon={receiptsHidden ? "eyeOff" : "eye"}
            iconSize={18}
            active={receiptsHidden}
            pressed={receiptsHidden}
            title={receiptsHidden ? t("content.read_receipts_hidden_here") : t("content.hide_read_receipts_here")}
            aria-label={t("content.hide_read_receipts_here")}
            onclick={() => { closeTools(); onreceipts(); }}>{receiptsHidden ? t("content.read_receipts_hidden_here") : t("content.hide_read_receipts_here")}</Button>
          <Button
            variant="menu"
            icon={typingHidden ? "keyboardOff" : "keyboard"}
            iconSize={18}
            active={typingHidden}
            pressed={typingHidden}
            title={typingHidden ? t("content.typing_not_sent_here") : t("content.stop_sending_typing_here")}
            aria-label={t("content.stop_sending_typing_here")}
            onclick={() => { closeTools(); ontyping(); }}>{typingHidden ? t("content.typing_not_sent_here") : t("content.stop_sending_typing_here")}</Button>
        </div>
      {/if}
    </div>
    <Button variant="icon" icon="volume" iconSize={20} active={soundboardOpen} title={t("content.soundboard")} aria-label={t("content.soundboard")}
      onclick={() => { soundboardOpen = !soundboardOpen; }} />
    <Button
      variant="icon"
      cls="tool-text"
      active={pickerTab === "gif"}
      title={t("content.gifs")}
      {disabled}
      onclick={() => { if (!disabled) pickerTab = pickerTab === "gif" ? null : "gif"; }}>{t("content.gif")}</Button>
    <Button
      variant="icon"
      icon="sticker"
      iconSize={20}
      active={pickerTab === "sticker"}
      title={t("content.stickers")}
      aria-label={t("content.stickers")}
      {disabled}
      onclick={() => { if (!disabled) pickerTab = pickerTab === "sticker" ? null : "sticker"; }} />
    <Button
      variant="icon"
      icon="smile"
      iconSize={20}
      active={pickerTab === "emoji"}
      title={t("content.emoji")}
      aria-label={t("content.emoji")}
      {disabled}
      onclick={() => { if (!disabled) pickerTab = pickerTab === "emoji" ? null : "emoji"; }} />
  </div>
  {#if !draft.trim() && pending.length === 0}
    <Button
      variant="send"
      icon="mic"
      iconSize={19}
      title={t("content.record_a_voice_message")}
      aria-label={t("content.record_a_voice_message")}
      {disabled}
      onclick={() => { if (!disabled) recording = true; }} />
  {:else}
    <Button variant="send" icon="send" iconSize={18} type="submit" title={t("content.send")} aria-label={t("content.send")} {disabled} />
  {/if}
  {/if}
</form>
{#if cameraOpen && account && !disabled}
  {#key `${account}:${selectedChat}:${generation}`}
    <CameraCapture {account} chat={selectedChat} {generation}
      onstage={(file, scope) => {
        if (disabled || editing || scope.account !== account || scope.chat !== selectedChat || scope.generation !== generation) throw new LocalizedError({ kind: "postal_error", code: "error.content.camera_attachment_target_changed", params: {} });
        onstage(file);
      }} onclose={() => { cameraOpen = false; }} />
  {/key}
{/if}
</div>

{#if scheduling && !disabled}
  <ScheduleDialog text={draft} onsave={(_text, dueAt) => {
    if (disabled) throw new LocalizedError({ kind: "postal_error", code: "error.content.message_scheduling_is_disabled_here", params: {} });
    return onschedule(dueAt);
  }} onclose={() => (scheduling = false)} />
{/if}

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
      aria-label={t("content.attachment_preview")}>
      {#if previewItem.kind === "image" && cropping}
        <ImageCropper
          file={previewItem.file}
          onapply={(file) => void replacePending(previewItem!.id, file)}
          oncancel={() => (cropping = false)} />
      {:else if previewItem.kind === "image"}
        <img class="preview-large" src={previewItem.url} alt={previewItem.file.name} />
        <Button variant="ghost" cls="crop-button" onclick={() => (cropping = true)}>{t("content.crop_or_resize")}</Button>
      {:else if previewItem.kind === "video"}
        <div class="preview-video">
          <VideoPlayer src={previewItem.url} autoplay={false} />
        </div>
      {:else}
        <span class="file-icon large"><Icon name="file" size={56} /></span>
      {/if}
      <span class="pending-name"><bdi dir="auto">{previewItem.file.name}</bdi></span>
      <input
        class="caption"
        value={previewItem.caption}
        oninput={(e) => previewItem && (previewItem.caption = e.currentTarget.value)}
        placeholder={t("content.add_a_caption")}
        autocomplete="off"
      />
      <Button variant="primary" onclick={() => (previewId = null)}>{t("content.done")}</Button>
    </div>
  </div>
{/if}

<style>
  .attachment-mode { margin: 4px 8px; color: var(--muted); font-size: .85em; }
  .quality { width: 100%; padding: 3px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); font-size: 0.6875rem; }
  .reply-preview {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 14px;
    padding: 8px 8px 8px 12px;
    background: var(--surface);
    border-inline-start: 3px solid var(--accent);
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    font-size: 0.75rem;
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
    font-size: 1rem;
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
    font-size: 0.8125rem;
  }
  .pending :global(.remove) {
    position: absolute;
    top: -6px;
    inset-inline-end: -6px;
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
  .once-toggle {
    position: absolute;
    top: 4px;
    inset-inline-start: 4px;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg) 75%, transparent);
    color: var(--muted);
    font: inherit;
    font-size: 0.75rem;
    font-weight: 700;
    cursor: pointer;
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .once-toggle:hover {
    color: var(--text);
  }
  .once-toggle.active {
    background: var(--accent);
    color: var(--accent-ink);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .pending-name {
    font-size: 0.6875rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pending-caption {
    font-size: 0.6875rem;
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
    text-align: start;
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
    margin-inline-start: 6px;
    color: var(--muted);
    font-size: 0.7812rem;
  }
  .composer-area {
    position: relative;
    flex: none;
  }
  /* Discord-style completion list over the composer. */
  .suggest {
    position: absolute;
    inset-inline-start: 12px;
    inset-inline-end: 12px;
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
    font-size: 0.7188rem;
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
    font-size: 0.9062rem;
    text-align: start;
    cursor: pointer;
  }
  .suggest-row.active {
    background: var(--raised);
  }
  .suggest-emoji {
    font-size: 1.25rem;
    width: 26px;
    text-align: center;
  }
  .attach-catcher {
    position: fixed;
    inset: 0;
    z-index: 55;
  }
  .attach-wrap {
    position: relative;
    display: flex;
    align-items: center;
    /* Above the fullscreen catcher so the trigger keeps its hover/clicks;
       taps anywhere else land on the catcher and close the menu. */
    z-index: 56;
  }
  .attach-menu {
    position: absolute;
    inset-inline-start: 0;
    bottom: calc(100% + 16px);
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
  /* Hover bridge: covers the gap so moving into the menu never leaves it. */
  .attach-menu::after {
    content: "";
    position: absolute;
    top: 100%;
    inset-inline-start: 0;
    inset-inline-end: 0;
    height: 20px;
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
    font-size: 0.9062rem;
    text-align: start;
    cursor: pointer;
  }
  .attach-menu button :global(svg) {
    color: var(--accent);
  }
  .attach-menu button:hover {
    background: var(--raised);
  }
  .tools-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .tools-menu {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    bottom: calc(100% + 16px);
    z-index: 56;
    display: flex;
    flex-direction: column;
    min-width: 230px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  /* Hover bridge: covers the gap so moving into the menu never leaves it. */
  .tools-menu::after {
    content: "";
    position: absolute;
    top: 100%;
    inset-inline-start: 0;
    inset-inline-end: 0;
    height: 20px;
  }
  .tools-menu :global(.btn-menu) {
    width: 100%;
    box-sizing: border-box;
  }
  .tools-menu :global(.btn-menu svg) {
    color: var(--accent);
    flex: none;
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
