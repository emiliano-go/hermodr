<script lang="ts">
  import Settings, { type UiSettings } from "$lib/settings/Settings.svelte";
  import ChatSettings from "$lib/chat/ChatSettings.svelte";
  import { fixture } from "./ipc";
  let chatOpen = $state(false);
  let globalOpen = $state(false);
  let saved = $state("");
  let settings = $state<UiSettings>({
    message_window_size: 500,
    retention: { max_age_hours: { kind: "limited", value: 24 }, max_messages_per_chat: { kind: "limited", value: 500 } },
    request_full_history: false, auto_download_media: false, auto_transcribe: false, warn_missing_video_preview: true,
    auto_download_types: { image: false, video: false, audio: false, document: false, sticker: false, gif: false },
    media_dir: null, history_dir: null, send_typing: false, send_receipts: false, keep_history: true,
    skip_loading_screen: false, keep_archived: true, android_instance: false,
    notifications_enabled: true, freeze_chat_list_on_hover: true, verbose_whatsapp_logs: true,
  });
  const noop = () => {};
</script>

<button onclick={() => { chatOpen = true; }}>Test per-chat retention</button>
<button onclick={() => { globalOpen = true; }}>Test global retention</button>
<pre aria-label="Saved retention">{saved}</pre>
{#if chatOpen}
  <ChatSettings chat="synthetic@s.whatsapp.net" title="Synthetic contact"
    onchange={() => { saved = JSON.stringify(fixture.savedRetention); }}
    onclearchat={noop} ondeletechat={noop} onclose={() => { chatOpen = false; }} />
{/if}
{#if globalOpen}
  <Settings {settings} section="whatsapp" accounts={[]} active={null} me={null} meAvatar={null} accountAvatars={{}}
    onclose={() => { globalOpen = false; }}
    onsave={async (value) => { settings = value; saved = JSON.stringify(value.retention); globalOpen = false; }}
    onflush={noop} onclearhistory={noop} onrename={noop} onremove={noop} onadd={noop}
    onswitch={noop} onprivacy={noop} onpicture={noop} />
{/if}
