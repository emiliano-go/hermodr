// Generated from Rust Serde DTOs. Run pnpm generate:wire.
export type Account = { id: string, label: string,
/**
 * Learned once the account connects, so its picture shows while inactive.
 */
jid: string | null,
/**
 * Whether the optional Android instance has been paired. Kept while it is
 * stopped so the toggle can say so without starting it.
 */
once_paired: boolean, };
export type AccountsView = { accounts: Array<Account>, active: string | null, };
export type Activation = "eager" | "lazy";
export type AdminReport = { id: string,
/**
 * The message as stored here, when this device has it.
 */
message: StoredMessage | null, reporters: Array<[string, number]>, };
export type ArchiveManifest = { format: string, version: number, messages: number, attachments: number, missing_attachments: number, };
export type ArchiveReport = { directory: string, messages: number, attachments: number, missing_attachments: number, };
export type BooleanProp = { name: string, code: number, default: boolean, value: boolean | null, };
export type ChatMarks = { reactions: Array<Reaction>, starred: Array<string>, pinned: string | null, polls: Array<Poll>, events: Array<Event>,
/**
 * View-once messages and whether each was opened (or sent by us, which counts).
 */
view_once: Array<ViewOnce>,
/**
 * Ids of messages that arrived marked as forwarded.
 */
forwarded: Array<string>,
/**
 * Ids of messages their sender edited.
 */
edited: Array<string>, };
export type ChatRetention = { max_age_hours: RetentionLimit, max_messages: RetentionLimit,
/**
 * Whether scrolling to the top asks the phone for older messages.
 */
on_demand: boolean, };
export type ChatSettings = {
/**
 * The chat's auto download override, `None` when it follows the global one.
 */
auto_download: boolean | null, retention: ChatRetention,
/**
 * Typing and read receipt overrides, `None` when following the global ones.
 */
send_typing: boolean | null, send_receipts: boolean | null, };
export type ChatStorage = { chat: string, name: string | null, bytes: number, by_kind: { [key in string]: number }, };
export type ChatSummary = { chat: string,
/**
 * Resolved display name, when one has been learned.
 */
display_name: string | null, last_message_at: number, last_text: string,
/**
 * Whether the last message was sent by the account owner.
 */
last_from_me: boolean,
/**
 * Resolved name of the last message's sender, when known.
 */
last_sender_name: string | null,
/**
 * The last message's sender, for when no name is known.
 */
last_sender: string,
/**
 * What the last message carried (`image`, `video`, …), if not only text.
 */
last_media_kind: string | null, message_count: number,
/**
 * Incoming messages the user has not seen yet.
 */
unread_count: number,
/**
 * Unread messages that mention us.
 */
mention_count: number,
/**
 * Whether the chat is pinned, mirrored from the account.
 */
pinned: boolean,
/**
 * Archived, mirrored from the account.
 */
archived: boolean,
/**
 * Muted until this Unix time in seconds; -1 is indefinitely, 0 not muted.
 */
muted_until: number,
/**
 * Marked unread by hand, mirrored from the account.
 */
marked_unread: boolean, };
export type CleanupResult = { files: number, bytes: number, };
export type ConnectionState = { started: boolean, connected: boolean, qr: string | null, };
export type ContactIdentity = { saved_name: string | null, push_name: string | null, username: string | null, number: string | null, own: boolean, };
export type Contributions = { commands: Array<string>, transcription?: TranscriptionContribution | null, };
export type DiskRetention = { max_age_hours: RetentionLimit, max_messages_per_chat: RetentionLimit, };
export type Event = { id: string, name: string, description: string | null, start: number | null, end: number | null, location: string | null, link: string | null, canceled: boolean, responses: Array<EventResponse>, };
export type EventForm = { name: string, description: string | null, start: number | null, end: number | null, location: string | null, link: string | null, canceled: boolean, };
export type EventResponse = { responder: string,
/**
 * `going`, `not_going` or `maybe`.
 */
response: string, };
export type GalleryCursor = { timestamp: number, sort_order: number, chat: string, id: string, };
export type GalleryFilter = { chat: string | null, kind: GalleryKind | null, from_me: boolean | null, since: number | null, until: number | null, };
export type GalleryItem = { message: StoredMessage, urls: Array<string>, };
export type GalleryKind = "image" | "video" | "audio" | "document" | "sticker" | "gif" | "link";
export type GalleryPage = { items: Array<GalleryItem>, next_cursor: GalleryCursor | null, };
export type GroupHistoryOffer = { enabled: boolean, reason: string | null, max_messages: number, time_window_seconds: number, };
export type GroupHistoryResult = { state: string, message: string, retry_id: string | null, };
export type GroupInfo = { subject: string | null, description: string | null, created_at: number | null,
/**
 * Name and address of whoever created the group.
 */
owner: string | null, owner_jid: string | null, participants: Array<Participant>,
/**
 * Whether members may report messages to the group's admins.
 */
allow_admin_reports: boolean,
/**
 * Only admins can send messages (announcement mode).
 */
announce: boolean,
/**
 * Only admins can edit the group's name, picture and description.
 */
locked: boolean,
/**
 * A community's parent group, which has no conversation of its own.
 */
community: boolean,
/**
 * The community's announcement group.
 */
announcements: boolean,
/**
 * The community this group belongs to, and that community's name.
 */
parent: string | null, parent_name: string | null,
/**
 * We are an admin of this group.
 */
admin: boolean,
/**
 * We may send messages here.
 */
can_send: boolean,
/**
 * Members may add participants, not just admins.
 */
members_can_add: boolean, };
export type GroupJoinRequest = { jid: string, name: string, request_time: number | null, };
export type GroupKind = { community: boolean, announcements: boolean, parent: string | null, };
export type GroupMemberAddResult = { participants: Array<ParticipantChange>, history: GroupHistoryResult, };
export type HintChange = "arrival" | "content" | "status";
export type HostMessage<E = JsonValue> = { "type": "hello", api_version: number, capabilities: Array<string>, } | { "type": "event", seq: number, event: E, } | { "type": "error", id: JsonValue, error: string, } | { "type": "transcribe", id: number, provider: string, chat: string, message_id: string, mime: string, duration_ms: number, audio: string, config: TranscriptionConfig, } | { "type": "install_model", id: number, url: string, sha256: string, filename: string, data_directory: string, };
export type InviteInfo = { jid: string, subject: string | null, description: string | null, size: number, created_at: number | null,
/**
 * Joining needs an admin's approval.
 */
approval: boolean, community: boolean,
/**
 * We are already in it.
 */
joined: boolean, picture: string | null, };
export type Joined = { jid: string,
/**
 * An admin still has to approve the request.
 */
pending: boolean, };
export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
export type LinkedDevice = { jid: string, device_id: number, is_current: boolean, can_unlink: boolean, };
export type LiveLocation = { lat: number, lng: number,
/**
 * The sender's own accuracy estimate, in metres.
 */
accuracy: number | null,
/**
 * Movement speed in metres per second.
 */
speed: number | null,
/**
 * Travel direction, degrees clockwise from magnetic north.
 */
heading: number | null,
/**
 * The sender's update counter; higher is newer.
 */
sequence: number | null,
/**
 * When the share started (the first message's timestamp).
 */
started_at: number,
/**
 * When the last position was received.
 */
updated_at: number,
/**
 * When the share is expected to end, when the message carried one.
 */
expires_at: number | null,
/**
 * Stopped by the sender or expired; the last position is kept.
 */
ended: boolean, };
export type MediaAction = "copy_image" | "save" | "open";
export type MessageCursor = { timestamp: number, id: string, sort_order: number, };
export type MessagePage = { messages: Array<StoredMessage>, has_more: boolean, };
export type MessagePageDirection = "before" | "after" | "through";
export type MessageReceipt = { recipient: string, name: string | null, delivered_at: number | null, read_at: number | null, played_at: number | null, };
export type OnceState = {
/**
 * Whether a device was ever linked; survives the instance being stopped.
 */
paired: boolean,
/**
 * Whether a pairing session was asked for and is waiting for the scan.
 */
pairing: boolean, running: boolean, connected: boolean, qr: string | null, };
export type Participant = {
/**
 * JID to put in `mentioned_jid` and to mention in the text.
 */
jid: string,
/**
 * Display name, from the address book when known.
 */
name: string,
/**
 * Whether the member is a group admin.
 */
admin: boolean,
/**
 * Whether the member created the group (a super admin).
 */
owner: boolean,
/**
 * Phone number, when known.
 */
number: string | null,
/**
 * WhatsApp username, when the member has one.
 */
username: string | null,
/**
 * The member's own tag in this group, such as "Long live EclipseOS".
 */
label: string | null, };
export type ParticipantChange = { jid: string,
/**
 * Whether the server accepted this participant.
 */
ok: boolean,
/**
 * The server's code, such as `403` or `409`, when it did not.
 */
code: string | null,
/**
 * The server's text for the refusal.
 */
error: string | null,
/**
 * The add was accepted but still needs an admin's approval.
 */
pending: boolean, };
export type PluginInfo = { enabled: boolean, state: string, error: string | null, id: string, name: string, version: string, api_version: number, entrypoint: string, activation: Activation, idle_timeout_secs: number | null, capabilities: Array<string>, contributes: Contributions, };
export type PluginReply = { "type": "ready", name: string, } | { "type": "ack", seq: number, } | { "type": "log", level: string, message: string, } | { "type": "call", id: JsonValue, } | { "type": "event", } | { "type": "transcript", id: number, provider: string, text: string, language: string | null, } | { "type": "transcribe_error", id: number, message: string, } | { "type": "model_installed", id: number, filename: string, } | { "type": "model_error", id: number, message: string, };
export type PluginsView = { plugins: Array<PluginInfo>, directory: string, errors: Array<string>, };
export type Poll = { id: string, name: string, options: Array<string>,
/**
 * More than one option may be chosen.
 */
multi: boolean, votes: Array<PollVote>, };
export type PollVote = { voter: string, options: Array<string>, };
export type Profile = { name: string, about: string | null,
/**
 * The account's username, without the `@`, if one is set or reserved.
 */
username: string | null,
/**
 * Reserved but not yet active.
 */
username_reserved: boolean,
/**
 * Privacy category (`last`, `profile`, `readreceipts`, …) to its value.
 */
privacy: { [key in string]: string }, };
export type ProviderConsent = { plugin_id: string, provider: string, };
export type ProviderKind = "local" | "cloud";
export type Reaction = { target: string, sender: string, emoji: string, };
export type RetentionLimit = { "kind": "inherit" } | { "kind": "unlimited" } | { "kind": "limited", "value": number };
export type ScheduledMessage = { id: string, chat: string, text: string, mentions: Array<string>, due_at: number, status: string, error: string | null, attempted: boolean, };
export type SearchResult = { jid: string, name: string,
/**
 * The JID's user part, so the UI can show "number - name".
 */
number: string,
/**
 * `contact` or `group`.
 */
kind: string,
/**
 * Whether the name came from the address book.
 */
saved: boolean,
/**
 * Whether the chat already has messages locally.
 */
has_messages: boolean,
/**
 * The contact's local aliases, which the UI may match on. Empty for a
 * group: an alias addresses a person, not a room.
 */
aliases: Array<string>, };
export type ServiceEvent = { "kind": "qrCode", code: string, } | { "kind": "connected" } | { "kind": "disconnected" } | { "kind": "loggedOut" } | { "kind": "message", message: StoredMessage, } | { "kind": "messageHint", chat: string, id: string, sender: string, from_me: boolean, fresh: boolean,
/**
 * What changed, so the UI knows whether a refetch is needed.
 */
change: HintChange,
/**
 * The delivery state a [`HintChange::Status`] change carries.
 */
status: string | null, } | { "kind": "retentionApplied", removed: number, } | { "kind": "namesUpdated", count: number, } | { "kind": "chatStateChanged", chat: string, } | { "kind": "chatPinRemoved", chat: string, } | { "kind": "syncing", pending: number, applied: number, } | { "kind": "initialSyncComplete", messages: number, chats: number, } | { "kind": "synced" } | { "kind": "historyLoaded", chats: Array<string>, } | { "kind": "historyProgress", percent: number, } | { "kind": "backfill", done: number, total: number, } | { "kind": "avatarChanged", jid: string, } | { "kind": "typing", chat: string, sender: string, state: string, } | { "kind": "presence", jid: string, online: boolean, last_seen: number | null, } | { "kind": "memberLabel", chat: string, jid: string, label: string, } | { "kind": "groupChanged", chat: string, } | { "kind": "favoritesChanged" } | { "kind": "marks", chat: string, } | { "kind": "storeChanged" } | { "kind": "stickerLibraryChanged", packs: boolean, favorites: boolean, recents: boolean, } | { "kind": "uploadProgress", token: string, sent: number, total: number, };
export type Sticker = {
/**
 * Base64 SHA-256 of the decrypted file: the app-state index key.
 */
filehash: string, pack_id: string | null, path: string | null, animated: boolean, lottie: boolean, emojis: Array<string>, favorite: boolean, recent_at: number | null, updated_at: number, };
export type StickerLibrary = { packs: Array<StickerPack>, favorites: Array<Sticker>, recent: Array<Sticker>, };
export type StickerPack = { pack_id: string, name: string | null, publisher: string | null, tray_path: string | null, origin: string | null, updated_at: number, };
export type StickerResyncReport = { packs: number, stickers: number, };
export type StorageCleanup = { "kind": "attachment", chat: string, id: string, quoted: boolean, } | { "kind": "chat_media", chat: string, } | { "kind": "cache" };
export type StorageFile = { chat: string, id: string, kind: string, filename: string, timestamp: number, quoted: boolean, bytes: number, available: boolean, };
export type StorageOrder = "largest" | "oldest";
export type StorageReport = { database_bytes: number, attachment_bytes: number, cache_bytes: number, other_bytes: number, total_files: number, chats: Array<ChatStorage>, files: Array<StorageFile>, };
export type StoredMessage = { spoiler: boolean,
/**
 * Resolved from `names` when read; never stored on the row.
 */
sender_name: string | null, text: string,
/**
 * The last position of a live location, updated in place as edits arrive.
 */
live_location: LiveLocation | null, chat: string, id: string, sender: string, timestamp: number, from_me: boolean,
/**
 * `image`, `video`, `audio`, `document`, `sticker`, `gif`, `poll`, `event`…
 */
media_kind: string | null,
/**
 * Absolute path to the downloaded media, if it was kept.
 */
media_path: string | null,
/**
 * The media's thumbnail, embedded in the message and available without
 * downloading the full file: a `data:` URI for received media (a few KB
 * in the row), a file path for older rows.
 */
media_thumb: string | null,
/**
 * Audio/voice-note length in seconds, when the message carries it. Lets
 * the bubble show the time before the file is decoded or played.
 */
media_duration: number | null,
/**
 * What this media was before `kind` was rewritten to `view_once`: the kind
 * is what decides which player a recovered photo, video or voice note needs.
 */
media_once_kind: string | null, reply_to_id: string | null, reply_to_text: string | null, reply_to_sender: string | null,
/**
 * Chat the quoted message lives in. Different from this chat for a private
 * reply, which is a direct message quoting a group message.
 */
reply_to_chat: string | null,
/**
 * Media kind of the quoted message, when it carried media.
 */
reply_to_kind: string | null,
/**
 * The quoted media's thumbnail, when one was available.
 */
reply_to_thumb: string | null,
/**
 * The quoted message was view-once. A linked device never gets that media
 * any other way, so a reply quoting one is the only copy it will see.
 */
reply_to_view_once: boolean,
/**
 * Whether this account may take the copy a reply quotes. Not a view-once:
 * anyone may, as its media is an ordinary message of its own. A view-once:
 * only its author may, since the media is in the reply but was not sent to
 * anyone else.
 */
reply_to_recoverable: boolean,
/**
 * Where a recovered copy was written. Named after the quoted message, so
 * every reply quoting the same view-once shares one file.
 */
reply_to_path: string | null, preview_url: string | null, preview_title: string | null, preview_desc: string | null, preview_thumb: string | null,
/**
 * Site name; empty for received links, whose preview does not carry it.
 */
preview_site: string | null,
/**
 * The page's theme colour, for the embed's side bar.
 */
preview_color: string | null,
/**
 * Persistent first-seen order for messages sharing a wire timestamp.
 */
sort_order: number,
/**
 * Whether the user has seen this message.
 */
read: boolean,
/**
 * Whether the sender deleted the message for everyone.
 */
revoked: boolean,
/**
 * Deleted by the user on this device only. The row is kept and shown
 * greyed out; nothing about it leaves this computer.
 */
deleted: boolean,
/**
 * Whether the message mentions us (directly or via @all).
 */
mentioned: boolean,
/**
 * Delivery state of a message we sent: `pending`, `sent`, `delivered` or
 * `read`. `None` for incoming messages.
 */
status: string | null,
/**
 * The protocol's stub type name, such as `E2E_IDENTITY_CHANGED`.
 */
system_kind: string | null,
/**
 * The stub's parameters, usually the JIDs it is about.
 */
system_params: Array<string>, };
export type StoredTranscript = { chat: string, id: string, text: string, language: string | null, provider: string, created_at: number, };
export type Target = { chat: string, id: string, sender: string, fromMe: boolean, };
export type TranscriptionConfig = { data_directory: string | null, whisper_executable: string | null, decoder_executable: string | null, model: string | null, model_sha256: string | null, language: string | null, cloud_consent: boolean, api_key: string | null, timeout_secs: number | null, idle_timeout_secs: number | null, };
export type TranscriptionContribution = { id: string, providers: Array<TranscriptionProvider>, };
export type TranscriptionEvent = { account_id: string, chat: string, id: string, status: string, transcript: StoredTranscript | null, error: string | null, };
export type TranscriptionProvider = { id: string, name: string, kind: ProviderKind, transmits_audio: boolean, requires_key: boolean, };
export type TranscriptionSettings = { plugin_id: string | null, provider: string, whisper_executable: string | null, decoder_executable: string | null, model: string | null, model_sha256: string | null, language: string | null, idle_timeout_secs: number | null, };
export type TranscriptionView = { settings: TranscriptionSettings, plugins: Array<PluginInfo>, cloud_consents: Array<ProviderConsent>, key_configured: boolean, data_directory: string | null, errors: Array<string>, };
export type UiSettings = { retention: DiskRetention, message_window_size: number,
/**
 * Requests deep history during pairing, independently of disk retention.
 */
request_full_history: boolean,
/**
 * Where downloaded media is stored. Empty disables downloads.
 */
media_dir: string | null,
/**
 * Cold storage for the message archive. Empty keeps it inside the app
 * data folder. Changing it moves the existing archive on the next start.
 */
history_dir: string | null,
/**
 * Whether to download incoming media automatically.
 */
auto_download_media: boolean, auto_transcribe: boolean,
/**
 * Whether to warn when a video goes out without a preview.
 */
warn_missing_video_preview: boolean,
/**
 * Whether others see "typing…" while we write.
 */
send_typing: boolean,
/**
 * Whether senders learn we read or played their messages. Off covers
 * groups too, which WhatsApp's own read-receipt privacy does not.
 */
send_receipts: boolean,
/**
 * Whether messages are kept on disk. Off keeps them in memory for this run only.
 */
keep_history: boolean,
/**
 * Skip the initial-sync loading screen and show the chat UI immediately.
 * Off holds the loading screen until the initial backlog is applied.
 */
skip_loading_screen: boolean,
/**
 * Whether archived chats stay archived when a new message arrives. Off
 * moves the chat back to the main list.
 */
keep_archived: boolean,
/**
 * Whether the optional Android instance runs: a second link that fetches
 * one-time media the External companion never receives. Off stops it
 * without unlinking; the link stays paired for next time.
 */
android_instance: boolean,
/**
 * Global kill switch for desktop notifications. Muted chats never
 * notify, whatever this is set to.
 */
notifications_enabled: boolean,
/**
 * Whether the chat list keeps its order while the pointer is over it.
 * Previews still update in place; the new order applies once the pointer
 * leaves or a chat is opened. Off reorders immediately.
 */
freeze_chat_list_on_hover: boolean,
/**
 * Log the library's keepalive pings and transport frames, so a stalled
 * link is diagnosable. Applies the next time Postal starts.
 */
verbose_whatsapp_logs: boolean, };
export type UserProfile = { jid: string,
/**
 * Saved, push, business or user name; `None` when only the number is known.
 */
name: string | null,
/**
 * Phone number digits, when known.
 */
number: string | null, username: string | null, about: string | null,
/**
 * Verified business name, for business accounts.
 */
business: string | null, };
export type ViewOnce = { id: string, opened: boolean,
/**
 * Whether this view-once can be shown: the file is already on disk, or a
 * reply quotes it carrying a copy.
 */
available: boolean, };
