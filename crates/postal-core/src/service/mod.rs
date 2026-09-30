//! The client service: connection lifecycle, typed events, and storage.
//!
//! This is the layer the UI talks to. It owns the protocol [`Bot`], converts
//! library events into [`ServiceEvent`]s the UI can render, and persists
//! messages through the [`MessageStore`] so retention stays enforced.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast;
use whatsapp_rust::{
    download::{Downloadable, MediaType},
    prelude::*,
    wacore::msg_secret::MsgSecretRetention,
    wacore::iq::privacy::{PrivacyCategory, PrivacyValue},
    wacore::types::presence::{ChatPresence, ChatPresenceMedia, ReceiptType},
    wacore::types::events::Event,
    wacore_binary::builder::NodeBuilder,
    wacore_binary::JidExt,
    CacheConfig,
    WAPatchName,
};

use crate::{
    history::HistoryPolicy,
    store::{LinkCard, LiveLocation, LocalState, Media, MessageHeader, MessageStore, StoreWorker, AliasWorker, Quote, DiskRetention, DiskRetentionManager, StoredMessage, Sticker, StickerPack},
};

mod audio;
mod connection;
mod contacts;
mod diagnostics;
mod storage;
mod archive;
pub use storage::{StorageReport, StorageCleanup, CleanupResult, StorageOrder};
mod groups;
mod gallery;
mod devices;
pub use devices::LinkedDevice;
mod favorites;
use favorites::Favorites;
mod pins;
mod history_pins;
use pins::Pins;
mod secret_edits;
pub mod transcription;
mod media_policy;
mod chat_unarchive;
mod group_invites;
mod blocked_contacts;
pub use blocked_contacts::BlockedContact;
mod group_create;
pub use group_create::{GroupCreateResult, GroupCreateParticipant, GroupCreateParticipantState};
mod group_requests;
pub use group_requests::GroupJoinRequest;
mod group_history;
mod group_history_policy;
mod scheduled;
pub use group_history::{GroupHistoryOffer, GroupHistoryResult, GroupMemberAddResult};
mod history;
mod inbound;
mod links;
mod media;
mod media_files;
mod media_codec;
mod media_download;
mod media_wire;
mod message_decode;
mod messages;
mod notices;
mod polls;
mod profile;
mod receipts;
mod stickers;
pub use stickers::{StickerLibrary, StickerResyncReport};
mod user_info;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod history_floor_tests;
#[cfg(test)]
mod protocol_tests;

use connection::*;
use contacts::*;
use history::*;
use inbound::*;
use links::*;
use media_codec::*;
use media_download::*;
use media_wire::*;
use message_decode::*;
use notices::*;
use polls::*;
use stickers::*;

/// Deletes recovered view-once files no stored message points at any more. Part
/// of the service's public surface so a caller holding only a store can run it.
pub use media_download::prune_quote_files;
pub use diagnostics::BooleanProp;

use crate::store::is_placeholder_name;

/// For store writes that must not stop the event loop but must not vanish
/// either: a failure is logged with the calling line.
trait Logged<T> {
    fn logged(self);
    fn observed(self) -> Option<T>;
}

impl<T, E: Into<anyhow::Error>> Logged<T> for std::result::Result<T, E> {
    #[track_caller]
    fn logged(self) {
        if let Err(e) = self {
            let e = e.into();
            let at = std::panic::Location::caller();
            log::error!(target: "postal_core::storage", "write failed at {}:{}: {e:#}", at.file(), at.line());
        }
    }

    #[track_caller]
    fn observed(self) -> Option<T> {
        match self.map_err(Into::into) {
            Ok(value) => Some(value),
            Err(e) if matches!(e.downcast_ref::<rusqlite::Error>(), Some(rusqlite::Error::QueryReturnedNoRows)) => None,
            Err(e) => {
                let at = std::panic::Location::caller();
                log::error!(target: "postal_core::storage", "operation failed at {}:{}: {e:#}", at.file(), at.line());
                None
            }
        }
    }
}

fn remove_cached_file(path: impl AsRef<Path>) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            Err::<(), _>(error).logged();
        }
    }
}

/// What a message hint asks the UI to do with the stored row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum HintChange {
    /// A new message is stored; the UI appends and follows it.
    Arrival,
    /// The row's content changed: an edit, media arriving, a kept one-time.
    Content,
    /// Only the delivery state changed; `status` carries the new one.
    Status,
}

/// Events the UI reacts to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum ServiceEvent {
    /// A pairing QR is ready to display.
    ///
    /// Every variant uses named fields: an internally tagged enum cannot
    /// represent a newtype variant holding a bare `String`, and serialization
    /// failure would silently drop the event.
    QrCode { code: String },
    Connected,
    Disconnected,
    /// WhatsApp revoked this device; the stored session can never sign in again.
    LoggedOut,
    /// A message was received or sent and stored.
    ///
    /// Boxed because `StoredMessage` is far larger than the other variants, and
    /// every clone of the enum is stored in the broadcast buffer.
    Message { message: Box<StoredMessage> },
    /// A lightweight invalidation for the same arrival: the row is already in
    /// the store, so the UI refetches instead of parsing a full payload.
    /// Emitted alongside `Message`; burst paths send only this.
    /// `fresh` is a new arrival (follow, typing clear, subject lookup);
    /// status-only updates (receipts, acks, media fill-in) send `false`.
    MessageHint {
        chat: String,
        id: String,
        sender: String,
        from_me: bool,
        fresh: bool,
        /// What changed, so the UI knows whether a refetch is needed.
        change: HintChange,
        /// The delivery state a [`HintChange::Status`] change carries.
        status: Option<String>,
    },
    /// Message history was changed by retention, so the UI should refresh.
    RetentionApplied { removed: usize },
    /// Address-book names were learned, so cached chats and messages now hold
    /// stale display names and should be refetched.
    NamesUpdated { count: usize },
    /// A chat's pin, archive, mute or unread mark changed from another device.
    ChatStateChanged { chat: String },
    ChatPinRemoved { chat: String },
    /// The offline backlog is draining; `pending` is how many messages the
    /// server announced at the start of the drain, `applied` how many have been
    /// stored so far.
    Syncing { pending: usize, applied: usize },
    /// The initial catch-up (offline drain and the initial history window) has
    /// been applied. The UI may leave its loading screen without landing in a UI
    /// that is still updating underneath it.
    InitialSyncComplete { messages: usize, chats: usize },
    /// The backlog finished draining.
    Synced,
    /// History sync stored older messages for these chats.
    ///
    /// Also sent when the phone answered with nothing new (or nothing at
    /// all): without it the UI's "load older" wait only ends on its timeout
    /// and reports a failure that never happened.
    HistoryLoaded { chats: Vec<String> },
    /// The phone's history sync after pairing is `percent` done.
    HistoryProgress { percent: u32 },
    /// A full-history backfill has finished `done` of `total` chats.
    Backfill { done: usize, total: usize },
    /// A chat's profile picture changed, so its cached avatar is stale.
    AvatarChanged { jid: String },
    /// Someone started or stopped typing; `state` is `typing`, `recording` or
    /// `paused`.
    Typing { chat: String, sender: String, state: String },
    /// A watched contact came online or went offline; `last_seen` when they share it.
    Presence { jid: String, online: bool, last_seen: Option<i64> },
    /// A group member changed their tag; empty means they cleared it.
    MemberLabel { chat: String, jid: String, label: String },
    /// A group's settings, admins, members or name changed.
    GroupChanged { chat: String },
    FavoritesChanged,
    /// Reactions, stars or the pinned message of a chat changed.
    Marks { chat: String },
    /// Store changes were missed (a lagging listener skipped events), so the
    /// UI should reload the chat list, marks and the open chat. The Android
    /// companion emits its store changes on its own stream; when that stream
    /// falls behind, nothing names the affected chats, so this asks for a
    /// blanket refresh instead.
    StoreChanged,
    /// Packs, favourites or recents in the sticker library changed.
    StickerLibraryChanged { packs: bool, favorites: bool, recents: bool },
    /// Bytes of an outgoing file sent so far, named by the caller's token.
    UploadProgress { token: String, sent: u64, total: u64 },
}

impl ServiceEvent {
    /// Lightweight invalidation for a stored message: the UI refetches the row
    /// instead of parsing a full payload per event.
    fn hint(message: &StoredMessage, fresh: bool) -> ServiceEvent {
        ServiceEvent::MessageHint {
            chat: message.header.chat.clone(),
            id: message.header.id.clone(),
            sender: message.header.sender.clone(),
            from_me: message.header.from_me,
            fresh,
            change: if fresh { HintChange::Arrival } else { HintChange::Content },
            status: None,
        }
    }

    /// A live message with its row, so the UI appends and notifies without
    /// fetching the row back.
    fn arrival(message: &StoredMessage) -> ServiceEvent {
        ServiceEvent::Message { message: Box::new(message.clone()) }
    }

    /// A delivery-state change. The UI patches the loaded row in place instead
    /// of refetching it, and the chat list does not need a refresh for it.
    fn status(message: &StoredMessage, status: &str) -> ServiceEvent {
        ServiceEvent::MessageHint {
            chat: message.header.chat.clone(),
            id: message.header.id.clone(),
            sender: message.header.sender.clone(),
            from_me: message.header.from_me,
            fresh: false,
            change: HintChange::Status,
            status: Some(status.to_string()),
        }
    }
}

/// The account's own profile and privacy, as the settings panel edits them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Profile {
    pub name: String,
    pub about: Option<String>,
    /// The account's username, without the `@`, if one is set or reserved.
    pub username: Option<String>,
    /// Reserved but not yet active.
    pub username_reserved: bool,
    /// Privacy category (`last`, `profile`, `readreceipts`, …) to its value.
    pub privacy: std::collections::BTreeMap<String, String>,
}

/// How [`WhatsAppService::send_media`] sends a file beyond its type.
#[derive(Debug, Default)]
pub struct SendOptions {
    /// A video that plays muted and looping, as WhatsApp's GIFs are.
    pub gif: bool,
    /// Opens once for the recipient, then is gone.
    pub view_once: bool,
    /// Present for a recorded voice note (an Ogg/Opus file).
    pub voice: Option<VoiceNote>,
    /// Carries WhatsApp's "Forwarded" label.
    pub forwarded: bool,
    /// JIDs the caption mentions as `@<number>`.
    pub mentions: Vec<String>,
    /// Names the upload in [`ServiceEvent::UploadProgress`], when the caller wants progress.
    pub progress: Option<String>,
}

#[derive(Debug)]
pub struct VoiceNote {
    pub seconds: u32,
    /// 64 loudness levels, 0 to 100, drawn as the note's waveform.
    pub waveform: Vec<u8>,
}

/// A group member, as the mention autocomplete needs it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Participant {
    /// JID to put in `mentioned_jid` and to mention in the text.
    pub jid: String,
    /// Display name, from the address book when known.
    pub name: String,
    /// Whether the member is a group admin.
    pub admin: bool,
    /// Whether the member created the group (a super admin).
    pub owner: bool,
    /// Phone number, when known.
    pub number: Option<String>,
    /// WhatsApp username, when the member has one.
    pub username: Option<String>,
    /// The member's own tag in this group, such as "Long live EclipseOS".
    pub label: Option<String>,
}

/// One row of the chat/contact search.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SearchResult {
    pub jid: String,
    pub name: String,
    /// The JID's user part, so the UI can show "number - name".
    pub number: String,
    /// `contact` or `group`.
    pub kind: String,
    /// Whether the name came from the address book.
    pub saved: bool,
    /// Whether the chat already has messages locally.
    pub has_messages: bool,
    /// The contact's local aliases, which the UI may match on. Empty for a
    /// group: an alias addresses a person, not a room.
    pub aliases: Vec<String>,
}

/// Everything the group info sidebar shows.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupInfo {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<u64>,
    /// Name and address of whoever created the group.
    pub owner: Option<String>,
    pub owner_jid: Option<String>,
    pub participants: Vec<Participant>,
    /// Whether members may report messages to the group's admins.
    pub allow_admin_reports: bool,
    /// Only admins can send messages (announcement mode).
    pub announce: bool,
    /// Only admins can edit the group's name, picture and description.
    pub locked: bool,
    /// A community's parent group, which has no conversation of its own.
    pub community: bool,
    /// The community's announcement group.
    pub announcements: bool,
    /// The community this group belongs to, and that community's name.
    pub parent: Option<String>,
    pub parent_name: Option<String>,
    /// We are an admin of this group.
    pub admin: bool,
    /// We may send messages here.
    pub can_send: bool,
    /// Members may add participants, not just admins.
    pub members_can_add: bool,
}

/// The server's answer for one participant of an add, remove, promote or
/// demote request.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ParticipantChange {
    pub jid: String,
    /// Whether the server accepted this participant.
    pub ok: bool,
    /// The server's code, such as `403` or `409`, when it did not.
    pub code: Option<String>,
    /// The server's text for the refusal.
    pub error: Option<String>,
    /// The add was accepted but still needs an admin's approval.
    pub pending: bool,
}

/// Builds the UI shape from the parts of a server response. Also used by the
/// tests, since the response type cannot be built outside the library.
pub fn participant_change(
    jid: String,
    status: Option<String>,
    error: Option<String>,
    pending: bool,
) -> ParticipantChange {
    ParticipantChange { jid, ok: error.is_none(), code: status, error, pending }
}

/// How a group sits in a community, for the chat list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupKind {
    pub community: bool,
    pub announcements: bool,
    pub parent: Option<String>,
}

/// What an invite link card shows about its group.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct InviteInfo {
    pub jid: String,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub size: u32,
    pub created_at: Option<u64>,
    /// Joining needs an admin's approval.
    pub approval: bool,
    pub community: bool,
    /// We are already in it.
    pub joined: bool,
    pub picture: Option<String>,
}

/// What a profile card shows about someone.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct UserProfile {
    pub jid: String,
    /// Saved, push, business or user name; `None` when only the number is known.
    pub name: Option<String>,
    /// Phone number digits, when known.
    pub number: Option<String>,
    pub username: Option<String>,
    pub about: Option<String>,
    /// Verified business name, for business accounts.
    pub business: Option<String>,
}

/// A message reported to a group's admins, with who reported it and when.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct AdminReport {
    pub id: String,
    /// The message as stored here, when this device has it.
    pub message: Option<StoredMessage>,
    pub reporters: Vec<(String, u64)>,
}

/// How the service should behave for one account.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Session database (protocol and crypto state).
    pub session_path: PathBuf,
    /// Message store database.
    pub messages_path: PathBuf,
    /// Local outbox, kept even when message history is disabled.
    pub scheduled_path: PathBuf,
    /// Synced favorites persist independently of message history.
    pub favorites_path: PathBuf,
    /// Contact alias database. Kept out of the message store so aliases
    /// survive `messages_path` being turned into an in-memory store.
    pub aliases_path: PathBuf,
    /// How much history to keep locally.
    pub retention: DiskRetention,
    /// Whether to pull the deep history sync during pairing.
    pub request_full_history: bool,
    /// Where downloaded media is written. `None` disables media downloads.
    pub media_dir: Option<PathBuf>,
    /// Whether incoming media is downloaded when it arrives. A chat can
    /// override this in the store.
    pub auto_download_types: crate::store::media_policy::MediaAutoDownload,
    /// Whether archived chats stay archived when a new message arrives. Off
    /// moves the chat back to the main list.
    pub keep_archived: bool,
    /// Whether to link as an Android phone, which makes WhatsApp send
    /// view-once media to this device. Only read at pairing.
    pub android_pair: bool,
    /// Whether an arriving view-once whose media this device can fetch is
    /// downloaded and kept as an ordinary attachment instead of one-time.
    pub keep_view_once: bool,
    /// Whether this link exists only to keep one-time media: it ingests
    /// nothing but view-once messages, refuses history and skips every other
    /// store change. The Android companion runs with this on, so its wake
    /// does not re-read the backlog the main link already stored.
    pub one_time_only: bool,
}

impl ServiceConfig {
    /// Sensible defaults for a single account under `data_dir`.
    pub fn under(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            session_path: data_dir.join("session.db"),
            messages_path: data_dir.join("messages.db"),
            scheduled_path: data_dir.join("scheduled.db"),
            favorites_path: data_dir.join("favorites.db"),
            aliases_path: data_dir.join("aliases.db"),
            retention: DiskRetention::default(),
            request_full_history: false,
            auto_download_types: Default::default(),
            keep_archived: true,
            android_pair: true,
            keep_view_once: true,
            one_time_only: false,
            media_dir: Some(data_dir.join("media")),
        }
    }
}

/// A running account client.
///
/// Dropping this stops the background task and closes the stores.
pub struct WhatsAppService {
    favorites: Favorites,
    pins: Pins,
    history_shares: group_history::PendingHistoryShares,
    user_info_slots: tokio::sync::Semaphore,
    client: Arc<Client>,
    store: StoreWorker,
    scheduled: crate::store::scheduled::ScheduledWorker,
    disk_retention: Arc<DiskRetentionManager>,
    /// Local, per-contact aliases, kept in their own file beside the messages.
    aliases: AliasWorker,
    // Broadcast send only fails with no subscribers, expected during shutdown.
    events: broadcast::Sender<ServiceEvent>,
    /// Fires the shutdown signal. `None` once it has been sent.
    shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Where downloaded media is written; `None` disables media.
    media_dir: Option<PathBuf>,
    /// Latest pairing code, kept so a subscriber that attaches after the code
    /// was issued can still display it. The QR is emitted during startup, which
    /// a late subscriber would otherwise miss entirely.
    qr: Arc<Mutex<Option<String>>>,
    connected: Arc<AtomicBool>,
    /// Whether archived chats stay archived when new messages arrive.
    keep_archived: Arc<AtomicBool>,
    media_auto_download: Arc<RwLock<crate::store::media_policy::MediaAutoDownload>>,
    /// Single-flight guard for a forced reconnect after a stall or sleep.
    reconnecting: Arc<AtomicBool>,
    /// Groups whose subject query failed: when to retry, and the wait that set it.
    subject_backoff: Mutex<std::collections::HashMap<String, (std::time::Instant, Duration)>>,
    /// JIDs the server already had no name for this run, so the UI's repeated
    /// lookups do not re-query it for the same people.
    nameless: Mutex<std::collections::HashSet<String>>,
    resolving: AtomicBool,
    /// Cached until a group update invalidates it; member tags are also patched live.
    group_cache: std::sync::Arc<Mutex<std::collections::HashMap<String, GroupInfo>>>,
    /// Every group the account is in; None means a refresh is needed.
    /// Shared with the event handler, which drops it when any group changes.
    groups_cache: Arc<Mutex<Option<Vec<whatsapp_rust::GroupOverview>>>>,
    /// Pending "load older" requests, request session to the chat it was for.
    /// Shared with the event handler, which completes one when the phone's
    /// history sync carries that session back.
    older_waits: Arc<Mutex<OlderWaits>>,
}

/// Seconds since the Unix epoch.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
