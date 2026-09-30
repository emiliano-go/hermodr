// Generated synthetic Serde fixture. Run pnpm generate:wire.
import type { StoredMessage, ServiceEvent, UiSettings, HostMessage, ArchiveReport } from './wire';
export const fixture = {
  "archive": {
    "attachments": 1,
    "directory": "synthetic",
    "messages": 7,
    "missing_attachments": 0
  },
  "event": {
    "chat": "synthetic@invalid",
    "fresh": false,
    "from_me": true,
    "id": "fixture",
    "kind": "messageHint",
    "sender": "synthetic@invalid"
  },
  "message": {
    "chat": "synthetic@invalid",
    "deleted": false,
    "from_me": true,
    "id": "fixture",
    "live_location": null,
    "media_duration": null,
    "media_kind": "image",
    "media_once_kind": null,
    "media_path": null,
    "media_thumb": null,
    "mentioned": false,
    "preview_color": null,
    "preview_desc": null,
    "preview_site": null,
    "preview_thumb": null,
    "preview_title": null,
    "preview_url": "https://example.invalid/",
    "read": false,
    "reply_to_chat": null,
    "reply_to_id": "quoted",
    "reply_to_kind": null,
    "reply_to_path": null,
    "reply_to_recoverable": false,
    "reply_to_sender": null,
    "reply_to_text": null,
    "reply_to_thumb": null,
    "reply_to_view_once": false,
    "revoked": false,
    "sender": "",
    "sender_name": null,
    "sort_order": 7,
    "status": null,
    "system_kind": null,
    "system_params": [],
    "text": "",
    "timestamp": 0
  },
  "plugin": {
    "event": {
      "chat": "synthetic@invalid",
      "fresh": false,
      "from_me": true,
      "id": "fixture",
      "kind": "messageHint",
      "sender": "synthetic@invalid"
    },
    "seq": 7,
    "type": "event"
  },
  "settings": {
    "android_instance": false,
    "auto_download_media": true,
    "freeze_chat_list_on_hover": true,
    "history_dir": null,
    "keep_archived": true,
    "keep_history": true,
    "media_dir": null,
    "message_window_size": 500,
    "notifications_enabled": true,
    "request_full_history": false,
    "retention": {
      "max_age_hours": {
        "kind": "unlimited"
      },
      "max_messages_per_chat": {
        "kind": "unlimited"
      }
    },
    "send_receipts": true,
    "send_typing": true,
    "skip_loading_screen": false,
    "verbose_whatsapp_logs": true,
    "warn_missing_video_preview": true
  }
} satisfies { message: StoredMessage; event: ServiceEvent; settings: UiSettings; plugin: HostMessage<ServiceEvent>; archive: ArchiveReport };
