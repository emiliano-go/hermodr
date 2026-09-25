//! The client service: connection lifecycle, typed events, and storage.
//!
//! This is the layer the UI talks to. It owns the protocol [`Bot`], converts
//! library events into [`ServiceEvent`]s the UI can render, and persists
//! messages through the [`MessageStore`] so retention stays enforced.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast;
use whatsapp_rust::{
    download::{Downloadable, MediaType},
    media::{self, AudioOptions, DocumentOptions, ImageOptions, VideoOptions},
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
    store::{MessageStore, Retention, StoredMessage},
};

/// Asks the phone for `count` messages older than the oldest one stored in
/// `chat`; they arrive later as a history sync.
async fn fetch_older(client: &Arc<Client>, store: &MessageStore, chat: &str, count: i32) -> Result<()> {
    let Some((id, from_me, timestamp)) = store.oldest_message(chat)? else {
        return Ok(());
    };
    let jid: Jid = chat.parse()?;
    let session = client
        .fetch_message_history(&jid, &id, from_me, timestamp * 1000, count)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    eprintln!("[hermodr] asked the phone for {count} messages before {id} in {chat} (session {session})");
    Ok(())
}

/// Whether a chat may pull its past again for an unknown quote: once a minute,
/// so a burst of replies to the same old message asks the phone once.
fn recall_allowed(chat: &str) -> bool {
    use std::collections::HashMap;
    use std::time::Instant;
    static LAST: std::sync::OnceLock<Mutex<HashMap<String, Instant>>> = std::sync::OnceLock::new();
    let mut last = LAST.get_or_init(Default::default).lock().unwrap();
    let now = Instant::now();
    if last.get(chat).is_some_and(|at| now.duration_since(*at) < Duration::from_secs(60)) {
        return false;
    }
    last.insert(chat.to_string(), now);
    true
}

/// What this device asks for when it links: named as Hermóðr on the phone's
/// linked devices, and with full history a backfill of every chat but only
/// its recent days, telling the phone older history will be asked for on
/// demand (a reply to something older fetches that chat's past). Only read at
/// pairing; an existing link keeps what it was paired with.
use crate::store::is_placeholder_name;

/// Stores the LID and phone forms of one sender when a message carries both.
fn remember_lid_pn(store: &MessageStore, sender: &Jid, alt: Option<&Jid>) {
    let Some(alt) = alt else { return };
    let (lid, pn) = match (sender.is_lid(), alt.is_lid()) {
        (true, false) => (sender, alt),
        (false, true) => (alt, sender),
        _ => return,
    };
    let _ = store.set_lid_pn(&lid.user, &pn.user);
}

/// The other address form of a bare user JID, from the session or our own record of it.
async fn other_form(client: &Client, store: &MessageStore, bare: &Jid) -> Option<(String, String)> {
    if let Ok(Some(entry)) = client.get_lid_pn_entry(bare).await {
        let (lid, pn) = (entry.lid.to_string(), entry.phone_number.to_string());
        let _ = store.set_lid_pn(&lid, &pn);
        return Some((lid, pn));
    }
    store.lid_pn(&bare.user).ok().flatten()
}

fn pairing_props(full_history: bool) -> whatsapp_rust::wacore::store::DevicePropsOverride {
    use wa::device_props::{HistorySyncConfig, PlatformType};
    let props = whatsapp_rust::wacore::store::DevicePropsOverride::new()
        .with_os("Hermóðr")
        .with_platform_type(PlatformType::UWP);
    if !full_history {
        return props;
    }
    props.with_require_full_sync(true).with_history_sync_config(HistorySyncConfig {
        full_sync_days_limit: Some(2),
        recent_sync_days_limit: Some(2),
        on_demand_ready: Some(true),
        complete_on_demand_ready: Some(true),
        // WhatsApp Web's own claims, which the library's default also makes.
        inline_initial_payload_in_e2_ee_msg: Some(true),
        support_bot_user_agent_chat_history: Some(true),
        support_cag_reactions_and_polls: Some(true),
        support_recent_sync_chunk_message_count_tuning: Some(true),
        support_hosted_group_msg: Some(true),
        support_biz_hosted_msg: Some(true),
        support_fbid_bot_chat_history: Some(true),
        support_message_association: Some(true),
        support_call_log_history: Some(true),
        support_group_history: Some(true),
        support_manus_history: Some(true),
        support_hatch_history: Some(true),
        ..Default::default()
    })
}

/// Builds the cache configuration for a given retention window.
///
/// `msg_secrets` are the decryption keys kept so edits, reactions and poll
/// votes can still be applied to their parent message. The library's default
/// horizon is 30 days for text and 90 for polls, which on an account with
/// hundreds of thousands of messages grows the session database into the
/// hundreds of megabytes. Since Hermóðr only keeps messages for
/// [`Retention::max_age_hours`], keeping keys far beyond that window protects
/// add-ons for messages that no longer exist. The horizon is therefore capped
/// at the message window, with a floor of an hour so edits arriving slightly
/// after their parent are never lost.
fn cache_config_for(retention: &Retention) -> CacheConfig {
    let horizon = retention
        .max_age_hours
        .map(|hours| Duration::from_secs(u64::from(hours) * 3600))
        .unwrap_or_else(|| Duration::from_secs(30 * 86_400))
        .max(Duration::from_secs(3600));

    CacheConfig {
        msg_secret_retention: MsgSecretRetention {
            text: horizon,
            // Poll and bot secrets have no sender-side time window, but they
            // still only matter while the parent message is retained.
            poll_event: horizon,
            bot: horizon,
        },
        ..Default::default()
    }
}

/// Reclaims decryption secrets left behind by a longer retention horizon.
///
/// The library prunes `msg_secrets` by their stored `expires_at`, so rows
/// written before the horizon was shortened keep their original deadline and
/// the session database stays large. This drops secrets whose parent message is
/// already outside the retention window, and shortens the deadline on the rest
/// so they expire on our schedule.
///
/// Rows with no message timestamp, or with the "never expires" marker, are left
/// untouched: their age cannot be established, and discarding them could break
/// decryption for a message we still keep.
fn reclaim_oversized_secrets(
    session_path: &Path,
    retention: &Retention,
    store: &MessageStore,
) -> Result<usize> {
    let Some(hours) = retention.max_age_hours else {
        return Ok(0);
    };
    if !session_path.exists() {
        return Ok(0);
    }

    let conn = rusqlite::Connection::open(session_path)?;
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'msg_secrets'",
        [],
        |row| row.get(0),
    )?;
    if has_table == 0 {
        return Ok(0);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let horizon = i64::from(hours) * 3600;
    let cutoff = now - horizon;

    let removed = conn.execute(
        "DELETE FROM msg_secrets WHERE message_ts > 0 AND message_ts < ?1",
        [cutoff],
    )?;
    conn.execute(
        "UPDATE msg_secrets SET expires_at = ?1
         WHERE message_ts > 0 AND expires_at > ?1",
        [now + horizon],
    )?;

    // SQLite reuses freed pages rather than returning them to the filesystem,
    // so the file stays large after a bulk delete and a later run would find
    // nothing left to remove. Compacting on the freelist rather than on this
    // run's deletions covers the profile an earlier build already pruned. This
    // runs before the bot starts, so there is no concurrent access to disturb.
    let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let last = store.meta(SESSION_VACUUM_KEY)?.unwrap_or(0);
    if should_vacuum(free_pages, pages, now - last) {
        log::info!("compacting the session database: {free_pages} of {pages} pages free");
        let started = std::time::Instant::now();
        conn.execute_batch("VACUUM")?;
        store.set_meta(SESSION_VACUUM_KEY, now)?;
        log::info!("session database compacted in {:?}", started.elapsed());
    }

    Ok(removed)
}

/// `meta` key holding when the session database was last compacted (unix seconds).
const SESSION_VACUUM_KEY: &str = "session_vacuum_at";

/// VACUUM rewrites the whole file, so it waits for at least 1000 free pages
/// making up a fifth of the file, and runs at most once a week.
fn should_vacuum(free_pages: i64, pages: i64, since_last_secs: i64) -> bool {
    free_pages > 1_000 && free_pages * 5 >= pages && since_last_secs >= 7 * 86_400
}

/// Events the UI reacts to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
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
    /// Message history was changed by retention, so the UI should refresh.
    RetentionApplied { removed: usize },
    /// Address-book names were learned, so cached chats and messages now hold
    /// stale display names and should be refetched.
    NamesUpdated { count: usize },
    /// The offline backlog is draining; `pending` is how many messages the
    /// server announced at the start of the drain.
    Syncing { pending: usize },
    /// The backlog finished draining.
    Synced,
    /// History sync stored older messages for these chats.
    HistoryLoaded { chats: Vec<String> },
    /// A chat's profile picture changed, so its cached avatar is stale.
    AvatarChanged { jid: String },
    /// Someone started or stopped typing; `state` is `typing`, `recording` or
    /// `paused`.
    Typing { chat: String, sender: String, state: String },
    /// A watched contact came online or went offline; `last_seen` when they share it.
    Presence { jid: String, online: bool, last_seen: Option<i64> },
    /// A group member changed their tag; empty means they cleared it.
    MemberLabel { chat: String, jid: String, label: String },
    /// Reactions, stars or the pinned message of a chat changed.
    Marks { chat: String },
    /// Bytes of an outgoing file sent so far, named by the caller's token.
    UploadProgress { token: String, sent: u64, total: u64 },
}

/// The account's own profile and privacy, as the settings panel edits them.
#[derive(Debug, Clone, Serialize)]
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

/// How [`Service::send_media`] sends a file beyond its type.
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

/// Encrypted media handed to the uploader, reporting how far it has been read.
struct ProgressSource {
    data: Arc<[u8]>,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

impl whatsapp_rust::wacore::upload::UploadSource for ProgressSource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn reader_from(&self, offset: u64) -> std::io::Result<Box<dyn std::io::Read + Send>> {
        let mut cursor = std::io::Cursor::new(Arc::clone(&self.data));
        cursor.set_position(offset.min(self.len()));
        Ok(Box::new(CountingReader { inner: cursor, read: offset, report: Arc::clone(&self.report) }))
    }
}

struct CountingReader {
    inner: std::io::Cursor<Arc<[u8]>>,
    read: u64,
    report: Arc<dyn Fn(u64) + Send + Sync>,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.report)(self.read);
        Ok(n)
    }
}

/// Marks a message context as forwarded, keeping anything already in it (a quote).
fn forwarded_context(context: Option<Box<wa::ContextInfo>>) -> Box<wa::ContextInfo> {
    let mut context = context.unwrap_or_default();
    context.is_forwarded = Some(true);
    context.forwarding_score = Some(context.forwarding_score.unwrap_or(0) + 1);
    context
}

/// Whether a received message carries the "Forwarded" label.
fn is_forwarded(message: &wa::Message) -> bool {
    message_context(message).is_some_and(|c| c.is_forwarded == Some(true))
}

#[derive(Debug)]
pub struct VoiceNote {
    pub seconds: u32,
    /// 64 loudness levels, 0 to 100, drawn as the note's waveform.
    pub waveform: Vec<u8>,
}

/// A group member, as the mention autocomplete needs it.
#[derive(Debug, Clone, Serialize)]
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
}

/// Everything the group info sidebar shows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GroupInfo {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<u64>,
    pub participants: Vec<Participant>,
    /// Whether members may report messages to the group's admins.
    pub allow_admin_reports: bool,
}

/// What an invite link card shows about its group.
#[derive(Debug, Clone, Serialize)]
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
    /// How much history to keep locally.
    pub retention: Retention,
    /// Whether to pull the deep history sync during pairing.
    pub accept_full_history: bool,
    /// Where downloaded media is written. `None` disables media downloads.
    pub media_dir: Option<PathBuf>,
    /// Whether incoming media is downloaded when it arrives. A chat can
    /// override this in the store.
    pub auto_download_media: bool,
}

impl ServiceConfig {
    /// Sensible defaults for a single account under `data_dir`.
    pub fn under(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            session_path: data_dir.join("session.db"),
            messages_path: data_dir.join("messages.db"),
            retention: Retention::default(),
            accept_full_history: false,
            auto_download_media: true,
            media_dir: Some(data_dir.join("media")),
        }
    }
}

/// Fetches a group's subject over the `w:g2` namespace.
///
/// Group names are not carried on incoming messages, and the library exposes no
/// typed accessor for them in this version, so the query is issued directly.
async fn fetch_group_subject(client: &Client, group: &str) -> Option<String> {
    let jid: Jid = group.parse().ok()?;
    let node = NodeBuilder::new("iq")
        .attr("type", "get")
        .attr("xmlns", "w:g2")
        .attr("to", jid)
        .children([NodeBuilder::new("query")
            .attr("request", "interactive")
            .build()])
        .build();

    let response = client
        .send_iq_node(node, Some(Duration::from_secs(15)))
        .await
        .ok()?;

    // The subject lives on the <group> child of the response.
    let group_node = response.get().get_optional_child_by_tag(&["group"])?;
    group_node
        .attrs()
        .optional_string("subject")
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// A running account client.
///
/// Dropping this stops the background task and closes the stores.
pub struct Service {
    client: Arc<Client>,
    store: Arc<MessageStore>,
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
    /// Groups whose subject query failed: when to retry, and the wait that set it.
    subject_backoff: Mutex<std::collections::HashMap<String, (std::time::Instant, Duration)>>,
    resolving: AtomicBool,
    /// Group metadata for this run, so opening a chat does not re-query the
    /// server and trip its rate limit. Group membership changes rarely enough
    /// that a session-lifetime cache is fine.
    /// Shared with the event handler, which patches member tags as they change.
    group_cache: std::sync::Arc<Mutex<std::collections::HashMap<String, GroupInfo>>>,
    /// Every group the account is in, `(jid, subject)`, filled on first search.
    groups_cache: Mutex<Vec<(String, String)>>,
}

impl Service {
    /// Connects an account, pairing first if it has no session yet.
    ///
    /// Returns the service along with an event receiver that was registered
    /// before the connection attempt began. The pairing code is emitted during
    /// startup, so a receiver created afterwards would miss it; the returned one
    /// is guaranteed to see every event from the beginning.
    pub async fn start(config: ServiceConfig) -> Result<(Self, broadcast::Receiver<ServiceEvent>)> {
        let store = Arc::new(MessageStore::open(
            &config.messages_path,
            config.retention,
        )?);
        let (events, initial_rx) = broadcast::channel(256);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        if let Ok(removed) = reclaim_oversized_secrets(&config.session_path, &config.retention, &store) {
            if removed > 0 {
                println!("[service] reclaimed {removed} stale decryption secret(s)");
            }
        }

        let policy = if config.accept_full_history {
            HistoryPolicy::accept_everything()
        } else {
            HistoryPolicy::default()
        };

        let qr_state = Arc::new(Mutex::new(None));
        let connected_state = Arc::new(AtomicBool::new(false));
        // Whether the address book has already been replayed this run.
        let names_resynced = Arc::new(AtomicBool::new(false));
        // The client only exists once the bot is built, but the message handler
        // needs it to download media. A OnceLock bridges that ordering.
        let client_slot: Arc<std::sync::OnceLock<Arc<Client>>> =
            Arc::new(std::sync::OnceLock::new());

        let media_dir = config.media_dir.clone();
        let auto_download_default = config.auto_download_media;
        let store_for_events = store.clone();
        let events_for_events = events.clone();
        let connected_for_events = connected_state.clone();
        let client_for_events = client_slot.clone();
        let media_dir_for_events = media_dir.clone();
        let group_cache: Arc<Mutex<std::collections::HashMap<String, GroupInfo>>> = Arc::default();
        let group_cache_for_events = group_cache.clone();

        let bot = Bot::builder()
            .with_backend(SqliteStore::new(config.session_path.to_string_lossy().as_ref()).await?)
            .with_history_sync_admission(policy)
            .with_device_props(pairing_props(config.accept_full_history))
            .with_cache_config(cache_config_for(&config.retention))
            .on_qr_code({
                let events = events.clone();
                let qr_state = qr_state.clone();
                move |code, _timeout| {
                    let events = events.clone();
                    let qr_state = qr_state.clone();
                    async move {
                        *qr_state.lock().unwrap() = Some(code.clone());
                        let _ = events.send(ServiceEvent::QrCode { code });
                    }
                }
            })
            .on_connected({
                let events = events.clone();
                let qr_state = qr_state.clone();
                let connected_state = connected_state.clone();
                let names_resynced = names_resynced.clone();
                let store = store.clone();
                let session_path = config.session_path.clone();
                move |client| {
                    let events = events.clone();
                    let qr_state = qr_state.clone();
                    let connected_state = connected_state.clone();
                    let names_resynced = names_resynced.clone();
                    let store = store.clone();
                    let session_path = session_path.clone();
                    async move {
                        connected_state.store(true, Ordering::SeqCst);
                        // The code is spent once paired.
                        *qr_state.lock().unwrap() = None;
                        let _ = events.send(ServiceEvent::Connected);

                        // Saved contact names reach the client as app-state
                        // patches, and an already-paired session has none left
                        // to deliver. Replay the address book once per run so
                        // the names are learned.
                        if !names_resynced.swap(true, Ordering::SeqCst) {
                            let client = client.clone();
                            let events = events.clone();
                            let session_path = session_path.clone();
                            tokio::spawn(async move {
                                match client
                                    .resync_app_state_collection(WAPatchName::CriticalUnblockLow)
                                    .await
                                {
                                    Ok(_) => {
                                        backfill_lid_names(&session_path, &store);
                                        if let Ok(count) = store.saved_name_count() {
                                            println!(
                                                "[service] address book: {count} saved name(s)"
                                            );
                                            if count > 0 {
                                                let _ =
                                                    events.send(ServiceEvent::NamesUpdated { count });
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[service] contact resync failed: {e}");
                                    }
                                }
                                // Pins live in a different collection.
                                if let Err(e) = client
                                    .resync_app_state_collection(WAPatchName::RegularLow)
                                    .await
                                {
                                    eprintln!("[service] pin resync failed: {e}");
                                }
                            });
                        }
                    }
                }
            })
            .on_event_for(
                &[
                    EventKind::Messages,
                    EventKind::Disconnected,
                    EventKind::LoggedOut,
                    EventKind::Receipt,
                    EventKind::ServerAck,
                    EventKind::ContactUpdate,
                    EventKind::ContactRemoved,
                    EventKind::OfflineSyncPreview,
                    EventKind::OfflineSyncCompleted,
                    EventKind::PinUpdate,
                    // Unlisted kinds never reach the handler: history ("load
                    // older" included), typing and picture changes need these.
                    EventKind::HistorySync,
                    EventKind::ChatPresence,
                    EventKind::PictureUpdate,
                    EventKind::UndecryptableMessage,
                    EventKind::Presence,
                ],
                move |event, _client| {
                    let store = store_for_events.clone();
                    let events = events_for_events.clone();
                    let connected = connected_for_events.clone();
                    let client_for_events = client_for_events.clone();
                    let media_dir = media_dir_for_events.clone();
                    let group_cache = group_cache_for_events.clone();
                    async move {
                        match event.as_ref() {
                            Event::Messages(batch) => {
                                let _commit = store.batch();
                                let client = client_for_events.get().cloned();
                                // Our own addresses, so a mention can be
                                // recognised whichever form it uses.
                                let own: Vec<String> = client
                                    .as_deref()
                                    .map(|c| {
                                        [c.pn(), c.lid()]
                                            .into_iter()
                                            .flatten()
                                            .map(|j| j.to_non_ad().to_string())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                for inbound in batch.messages.iter() {
                                    // The envelope carries the sender's display
                                    // name, which is the only name source
                                    // available without a contacts query.
                                    let push_name = inbound.info.push_name.to_string();
                                    let chat = inbound.info.source.chat.to_string();
                                    let sender = inbound.info.source.sender.to_string();
                                    let is_group = inbound.info.source.is_group
                                        || chat.ends_with("@g.us");
                                    let from_me = inbound.info.source.is_from_me;

                                    // Status updates are not a conversation; keep
                                    // them out of the store so they never show up
                                    // as a chat.
                                    if chat == "status@broadcast" {
                                        continue;
                                    }

                                    // Address-book names are keyed by phone
                                    // number, but an LID-addressed chat names
                                    // its sender with a LID, so the two never
                                    // match on their own. The source carries
                                    // the other form; copy the name across so
                                    // the saved one is what gets shown.
                                    remember_lid_pn(
                                        &store,
                                        &inbound.info.source.sender,
                                        inbound.info.source.sender_alt.as_ref(),
                                    );
                                    if let Some(alt) =
                                        inbound.info.source.sender_alt.as_ref().map(|j| j.to_string())
                                    {
                                        let known =
                                            store.name_for(&alt).ok().flatten().filter(|n| !is_placeholder_name(n));
                                        let is_saved = known.is_some();
                                        // Fall back to the phone number, never
                                        // the unreadable LID.
                                        let name = known.unwrap_or_else(|| {
                                            alt.split('@').next().unwrap_or(&alt).to_string()
                                        });
                                        if is_saved {
                                            let _ = store.set_saved_name(&sender, &name);
                                            if !is_group && !from_me {
                                                let _ = store.set_saved_name(&chat, &name);
                                            }
                                        } else {
                                            // The bare number is only a placeholder;
                                            // it must not replace a push name that a
                                            // message without one would otherwise erase.
                                            let unnamed = |jid: &str| {
                                                store.name_for(jid).ok().flatten().is_none()
                                            };
                                            if unnamed(&sender) {
                                                let _ = store.set_name(&sender, &name);
                                            }
                                            if !is_group && !from_me && unnamed(&chat) {
                                                let _ = store.set_name(&chat, &name);
                                            }
                                        }
                                    }

                                    if !push_name.is_empty() {
                                        // Push names never override a saved one.
                                        let _ = store.set_name(&sender, &push_name);
                                        // A participant's JID has no device suffix
                                        // while a message's sender does, so store
                                        // the bare form too or the group member
                                        // list cannot find the name.
                                        if let Some((user, server)) = sender.split_once('@') {
                                            let bare = format!(
                                                "{}@{}",
                                                user.split(':').next().unwrap_or(user),
                                                server
                                            );
                                            if bare != sender {
                                                let _ = store.set_name(&bare, &push_name);
                                            }
                                        }
                                        // A one-to-one chat is named after its
                                        // contact. A group is named by its
                                        // subject, and a message we sent must
                                        // never name a chat after us, which is
                                        // what turned a group into our own name.
                                        if !is_group && !from_me {
                                            let _ = store.set_name(&chat, &push_name);
                                        }
                                    }

                                    let base = inbound.message.get_base_message();
                                    let message_id = inbound.info.id.to_string();
                                    let author = if from_me {
                                        own.first().cloned().unwrap_or_else(|| sender.clone())
                                    } else {
                                        inbound.info.source.sender.to_non_ad().to_string()
                                    };
                                    remember_structures(&store, &chat, &message_id, &author, &inbound.message);

                                    if let Some(update) = base.poll_update_message.as_option() {
                                        use whatsapp_rust::wacore::poll::{compute_option_hash, PollVoteCiphertext};
                                        let poll_id = update.poll_creation_message_key.as_option().and_then(|k| k.id.clone());
                                        let def = poll_id.as_deref().and_then(|id| store.poll_secret(&chat, id).ok().flatten());
                                        if let (Some(poll_id), Some(def), Some(vote), Some(client)) = (
                                            poll_id,
                                            def,
                                            update.vote.as_option(),
                                            client.as_deref(),
                                        ) {
                                            // The key is derived from the creator's and voter's
                                            // addresses, and an LID-addressed group uses the LID
                                            // form, so try every form either side is known by.
                                            let mut creators = vec![def.creator.clone()];
                                            if own.contains(&def.creator) {
                                                creators.extend(own.iter().filter(|j| **j != def.creator).cloned());
                                            }
                                            let mut voters = vec![inbound.info.source.sender.to_non_ad()];
                                            if let Some(alt) = inbound.info.source.sender_alt.as_ref() {
                                                voters.push(alt.to_non_ad());
                                            }
                                            if from_me {
                                                voters.extend(own.iter().filter_map(|j| j.parse::<Jid>().ok()));
                                            }
                                            let voter = voters[0].clone();
                                            let mut opened = Err(anyhow::anyhow!("no address pair opened it"));
                                            'pairs: for creator in creators.iter().filter_map(|c| c.parse::<Jid>().ok()) {
                                                for voter in &voters {
                                                    let cipher = PollVoteCiphertext {
                                                        enc_payload: vote.enc_payload.as_deref().unwrap_or_default(),
                                                        enc_iv: vote.enc_iv.as_deref().unwrap_or_default(),
                                                    };
                                                    if let Ok(hashes) = client
                                                        .polls()
                                                        .decrypt_vote(cipher, &def.secret, &poll_id, &creator, voter)
                                                        .await
                                                    {
                                                        opened = Ok(hashes);
                                                        break 'pairs;
                                                    }
                                                }
                                            }
                                            match opened {
                                                Ok(hashes) => {
                                                    let chosen: Vec<String> = def
                                                        .options
                                                        .iter()
                                                        .filter(|o| hashes.iter().any(|h| h.as_slice() == compute_option_hash(o)))
                                                        .cloned()
                                                        .collect();
                                                    let who = if from_me { "@me".to_string() } else { voter.to_string() };
                                                    let _ = store.set_poll_vote(&chat, &poll_id, &who, &chosen);
                                                    let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                                                }
                                                Err(e) => log::warn!("could not open a vote on poll {poll_id}: {e}"),
                                            }
                                        }
                                        continue;
                                    }

                                    if let Some(response) = base.enc_event_response_message.as_option() {
                                        let event_id = response.event_creation_message_key.as_option().and_then(|k| k.id.clone());
                                        let def = event_id.as_deref().and_then(|id| store.event_secret(&chat, id).ok().flatten());
                                        if let (Some(event_id), Some(def)) = (event_id, def) {
                                            let responder = inbound.info.source.sender.to_non_ad().to_string();
                                            // Our own events may have been answered under our other address.
                                            let mut creators = vec![def.creator.clone()];
                                            if own.contains(&def.creator) {
                                                creators.extend(own.iter().filter(|j| **j != def.creator).cloned());
                                            }
                                            let opened = creators.iter().find_map(|creator| {
                                                whatsapp_rust::wacore::event::decrypt_event_response_with_secret(
                                                    response.enc_payload.as_deref().unwrap_or_default(),
                                                    response.enc_iv.as_deref().unwrap_or_default(),
                                                    &def.secret,
                                                    &event_id,
                                                    creator,
                                                    &responder,
                                                )
                                                .ok()
                                            });
                                            match opened {
                                                Some(answer) => {
                                                    let who = if from_me { "@me".to_string() } else { responder };
                                                    let _ = store.set_event_response(&chat, &event_id, &who, response_name(answer.response));
                                                    let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                                                }
                                                None => log::warn!("could not open an RSVP to event {event_id}"),
                                            }
                                        }
                                        continue;
                                    }

                                    if let Some(reaction) = base.reaction_message.as_option() {
                                        if let Some(target) =
                                            reaction.key.as_option().and_then(|k| k.id.clone())
                                        {
                                            let who = if from_me {
                                                "@me".to_string()
                                            } else {
                                                inbound.info.source.sender.to_non_ad().to_string()
                                            };
                                            let emoji = reaction.text.clone().unwrap_or_default();
                                            let _ = store.set_reaction(&chat, &target, &who, &emoji);
                                            let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                                        }
                                        continue;
                                    }
                                    if let Some(pin) = base.pin_in_chat_message.as_option() {
                                        use wa::message::pin_in_chat_message::Type;
                                        let target = pin.key.as_option().and_then(|k| k.id.clone());
                                        let pinned = pin.r#type == Some(Type::PIN_FOR_ALL);
                                        let _ = store.set_message_pin(
                                            &chat,
                                            target.as_deref().filter(|_| pinned),
                                        );
                                        let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                                        continue;
                                    }

                                    if let Some(label) = member_label_change(&inbound.message) {
                                        let member =
                                            inbound.info.source.sender.to_non_ad().to_string();
                                        if let Some(info) = group_cache.lock().unwrap().get_mut(&chat) {
                                            if let Some(p) =
                                                info.participants.iter_mut().find(|p| p.jid == member)
                                            {
                                                p.label = (!label.is_empty()).then(|| label.clone());
                                            }
                                        }
                                        let _ = events.send(ServiceEvent::MemberLabel {
                                            chat: chat.clone(),
                                            jid: member,
                                            label,
                                        });
                                        continue;
                                    }

                                    // A revoke is a protocol message naming the
                                    // original; mark it deleted rather than
                                    // dropping the notice, so the chat shows
                                    // that something was removed.
                                    if let Some(target) = revoke_target(&inbound.message) {
                                        if let Ok(true) = store.revoke(&chat, &target) {
                                            if let Ok(updated) =
                                                store.message(&chat, &target)
                                            {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(updated),
                                                });
                                            }
                                        }
                                        continue;
                                    }

                                    if let Some((target, text)) = edit_of(&inbound.message) {
                                        if let Ok(true) = store.apply_edit(&chat, &target, &text) {
                                            if let Ok(updated) = store.message(&chat, &target) {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(updated),
                                                });
                                            }
                                            let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                                        }
                                        continue;
                                    }

                                    // Stickers and voice notes are small and read as part
                                    // of the conversation, so WhatsApp always fetches them.
                                    let base = inbound.message.get_base_message();
                                    let small = base.sticker_message.is_set()
                                        || base.audio_message.as_option().is_some_and(|a| a.ptt == Some(true));
                                    let auto_download = small
                                        || store
                                            .chat_auto_download(&chat)
                                            .ok()
                                            .flatten()
                                            .unwrap_or(auto_download_default);
                                    let Some(mut message) = incoming_message(
                                        inbound,
                                        client.as_deref(),
                                        media_dir.as_deref(),
                                        auto_download,
                                    )
                                    .await
                                    else {
                                        continue;
                                    };
                                    // Mentions stay `@<number>` as on the wire; the UI
                                    // resolves them when drawn, so later names apply.
                                    message.mentioned = mentions_me(&inbound.message, &own);
                                    if inbound.message.is_view_once() {
                                        let _ = store.set_view_once(&chat, &message.id, from_me);
                                    }
                                    if is_forwarded(&inbound.message) {
                                        let _ = store.set_forwarded(&chat, &message.id);
                                    }
                                    // Pairing only brings recent days; a reply to something
                                    // older pulls that chat's past so the quote can be opened.
                                    if let (Some(quoted), Some(client)) = (message.reply_to_id.clone(), client.clone()) {
                                        let quoted_chat = message.reply_to_chat.clone().unwrap_or_else(|| chat.clone());
                                        if store.message(&quoted_chat, &quoted).is_err() && recall_allowed(&quoted_chat) {
                                            let store = store.clone();
                                            tokio::spawn(async move {
                                                let _ = fetch_older(&client, &store, &quoted_chat, 50).await;
                                            });
                                        }
                                    }
                                    let _ = store.insert_message(&message);
                                    let _ = events.send(ServiceEvent::Message { message: Box::new(message) });
                                }
                                // Bound the store right after writes so the
                                // limit holds even if the process stops.
                                if let Ok(removed) = store.enforce_retention() {
                                    if removed > 0 {
                                        let _ = events
                                            .send(ServiceEvent::RetentionApplied { removed });
                                    }
                                }
                            }
                            Event::Disconnected(_) => {
                                connected.store(false, Ordering::SeqCst);
                                let _ = events.send(ServiceEvent::Disconnected);
                            }
                            Event::LoggedOut(_) => {
                                connected.store(false, Ordering::SeqCst);
                                let _ = events.send(ServiceEvent::LoggedOut);
                            }
                            // A receipt names the messages it refers to, so the
                            // outgoing row can move to delivered or read.
                            Event::Receipt(receipt) => {
                                let status = match receipt.r#type {
                                    ReceiptType::Read
                                    | ReceiptType::ReadSelf
                                    | ReceiptType::Played
                                    | ReceiptType::PlayedSelf => Some("read"),
                                    ReceiptType::Delivered | ReceiptType::Sender => {
                                        Some("delivered")
                                    }
                                    ReceiptType::Sent => Some("sent"),
                                    _ => None,
                                };
                                // Kept per recipient for the message info screen; our
                                // own devices' receipts say nothing about the others.
                                let kind = match receipt.r#type {
                                    ReceiptType::Delivered => Some("delivered"),
                                    ReceiptType::Read => Some("read"),
                                    ReceiptType::Played => Some("played"),
                                    _ => None,
                                };
                                if let Some(kind) = kind {
                                    let recipient = receipt.source.sender.to_non_ad().to_string();
                                    let at = receipt.timestamp.timestamp();
                                    for id in receipt.message_ids.iter() {
                                        let _ = store.record_receipt(id.as_str(), &recipient, kind, at);
                                    }
                                }
                                if let Some(status) = status {
                                    let chat = receipt.source.chat.to_string();
                                    for id in receipt.message_ids.iter() {
                                        if let Ok(true) =
                                            store.set_status(&chat, id.as_str(), status)
                                        {
                                            if let Ok(updated) = store.message(&chat, id.as_str()) {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(updated),
                                                });
                                            }
                                        } else if let Ok(updated) =
                                            store.set_status_by_id(id.as_str(), status)
                                        {
                                            for message in updated {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(message),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            // The server accepted our stanza, so it is at least sent.
                            // The ack only sometimes names the chat, and the named
                            // JID can differ in form from the stored one, so the
                            // id alone is the reliable correlator.
                            Event::ServerAck(ack) => {
                                let accepted = ack.error.is_none();
                                let is_message =
                                    matches!(ack.class.as_deref(), None | Some("message"));
                                if accepted && is_message {
                                    let mut done = false;
                                    if let Some(chat) = ack.from.as_ref() {
                                        let chat = chat.to_string();
                                        if let Ok(true) = store.set_status(&chat, &ack.id, "sent")
                                        {
                                            if let Ok(updated) = store.message(&chat, &ack.id) {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(updated),
                                                });
                                            }
                                            done = true;
                                        }
                                    }
                                    if !done {
                                        if let Ok(updated) =
                                            store.set_status_by_id(&ack.id, "sent")
                                        {
                                            for message in updated {
                                                let _ = events.send(ServiceEvent::Message {
                                                    message: Box::new(message),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            // The name the user saved for a contact comes from
                            // the address book and outranks the push name the
                            // contact set for themselves.
                            Event::ContactUpdate(update) => {
                                let name = update
                                    .action
                                    .full_name
                                    .as_deref()
                                    .or(update.action.first_name.as_deref());
                                if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
                                    let _ = store.set_saved_name(&update.jid.to_string(), name);
                                }
                            }
                            Event::ContactRemoved(removed) => {
                                let _ = store.clear_saved_name(&removed.jid.to_string());
                            }
                            // Progress for the initial catch-up, so the UI can
                            // show how much of the backlog is still arriving.
                            Event::OfflineSyncPreview(preview) => {
                                let pending = preview.messages.max(0) as usize;
                                if pending > 0 {
                                    let _ = events.send(ServiceEvent::Syncing { pending });
                                }
                            }
                            Event::OfflineSyncCompleted(_) => {
                                let _ = events.send(ServiceEvent::Synced);
                            }
                            // Pairing's recent window and "load older" answers
                            // arrive here, never as `Messages`.
                            Event::HistorySync(sync) => {
                                let Some(history) = sync.get() else {
                                    eprintln!("[hermodr] history sync type {} failed to decode", sync.sync_type());
                                    return;
                                };
                                eprintln!(
                                    "[hermodr] history sync type {} ({:?}): {} conversation(s), {} message(s), {} push name(s), {} LID mapping(s)",
                                    sync.sync_type(),
                                    sync.peer_data_request_session_id(),
                                    history.conversations.len(),
                                    history.conversations.iter().map(|c| c.messages.len()).sum::<usize>(),
                                    history.pushnames.len(),
                                    history.phone_number_to_lid_mappings.len(),
                                );
                                let _commit = store.batch();
                                let client = client_for_events.get().cloned();
                                let own = client
                                    .as_deref()
                                    .and_then(|c| c.pn())
                                    .map(|j| j.to_non_ad().to_string());
                                let mut names_learned = 0;
                                for push in &history.pushnames {
                                    if let (Some(id), Some(name)) = (&push.id, &push.pushname) {
                                        if !name.is_empty() && store.set_name(id, name).is_ok() {
                                            names_learned += 1;
                                        }
                                    }
                                }
                                let pair = |lid: Option<&str>, pn: Option<&str>| {
                                    let (Some(lid), Some(pn)) = (lid, pn) else { return false };
                                    let user = |j: &str| j.split(['@', ':']).next().unwrap_or(j).to_string();
                                    store.set_lid_pn(&user(lid), &user(pn)).is_ok()
                                };
                                for mapping in &history.phone_number_to_lid_mappings {
                                    if pair(mapping.lid_jid.as_deref(), mapping.pn_jid.as_deref()) {
                                        names_learned += 1;
                                    }
                                }
                                let mut chats = Vec::new();
                                for conversation in &history.conversations {
                                    let chat = conversation.id.clone();
                                    if chat == "status@broadcast" {
                                        continue;
                                    }
                                    pair(conversation.lid_jid.as_deref(), conversation.pn_jid.as_deref());
                                    if !chat.ends_with("@g.us") {
                                        let name = conversation
                                            .display_name
                                            .as_deref()
                                            .or(conversation.username.as_deref())
                                            .filter(|n| !n.trim().is_empty());
                                        if let Some(name) = name {
                                            let _ = store.set_name(&chat, name);
                                        }
                                    }
                                    if let Some(subject) =
                                        conversation.name.as_deref().filter(|n| !n.is_empty())
                                    {
                                        if chat.ends_with("@g.us") {
                                            let _ = store.set_name(&chat, subject);
                                        }
                                    }
                                    let mut added = false;
                                    for entry in &conversation.messages {
                                        let Some(web) = entry.message.as_option() else { continue };
                                        let Some(key) = web.key.as_option() else { continue };
                                        let (Some(id), Some(message)) =
                                            (key.id.clone(), web.message.as_option())
                                        else {
                                            continue;
                                        };
                                        let from_me = key.from_me.unwrap_or(false);
                                        let sender = if from_me {
                                            own.clone().unwrap_or_else(|| chat.clone())
                                        } else {
                                            web.participant
                                                .clone()
                                                .or_else(|| key.participant.clone())
                                                .unwrap_or_else(|| chat.clone())
                                        };
                                        // Learned even from rows we already have: a re-paired
                                        // device gets its names back from this history.
                                        if let Some(push) =
                                            web.push_name.as_deref().filter(|p| !p.is_empty())
                                        {
                                            if !from_me && store.set_name(&sender, push).is_ok() {
                                                names_learned += 1;
                                            }
                                        }
                                        // A stored row is already complete; rebuilding
                                        // it would only rewrite its thumbnails.
                                        if store.message(&chat, &id).is_ok() {
                                            continue;
                                        }
                                        if let Some(target) = revoke_target(message) {
                                            let _ = store.revoke(&chat, &target);
                                            continue;
                                        }
                                        remember_structures(&store, &chat, &id, &sender, message);
                                        let envelope = Envelope {
                                            chat: chat.clone(),
                                            id,
                                            sender,
                                            timestamp: web.message_timestamp.unwrap_or(0) as i64,
                                            from_me,
                                        };
                                        // History media is never bulk downloaded;
                                        // it is fetched on demand like any other.
                                        let Some(mut stored) = stored_message(
                                            message,
                                            envelope,
                                            client.as_deref(),
                                            media_dir.as_deref(),
                                            false,
                                        )
                                        .await
                                        else {
                                            continue;
                                        };
                                        // Old messages must not raise unread counts.
                                        stored.read = true;
                                        added |= store.insert_message(&stored).is_ok();
                                    }
                                    if added {
                                        chats.push(chat);
                                    }
                                }
                                if names_learned > 0 {
                                    let _ = events.send(ServiceEvent::NamesUpdated { count: names_learned });
                                }
                                // Retention is left to the next live write, so
                                // what was just loaded can be seen first.
                                if !chats.is_empty() {
                                    let _ = events.send(ServiceEvent::HistoryLoaded { chats });
                                }
                            }
                            Event::ChatPresence(update) => {
                                let state = match (update.state, update.media) {
                                    (ChatPresence::Composing, ChatPresenceMedia::Audio) => "recording",
                                    (ChatPresence::Composing, _) => "typing",
                                    _ => "paused",
                                };
                                let _ = events.send(ServiceEvent::Typing {
                                    chat: update.source.chat.to_non_ad().to_string(),
                                    sender: update.source.sender.to_non_ad().to_string(),
                                    state: state.to_string(),
                                });
                            }
                            Event::Presence(presence) => {
                                // A chat is keyed by phone number; presence may name the LID.
                                let mut jid = presence.from.to_non_ad();
                                if jid.is_lid() {
                                    if let Some(client) = client_for_events.get() {
                                        if let Ok(Some(entry)) = client.get_lid_pn_entry(&jid).await {
                                            jid = Jid::new(&*entry.phone_number, whatsapp_rust::wacore_binary::Server::Pn);
                                        }
                                    }
                                }
                                let _ = events.send(ServiceEvent::Presence {
                                    jid: jid.to_string(),
                                    online: !presence.unavailable,
                                    last_seen: presence.last_seen.map(|t| t.timestamp()),
                                });
                            }
                            Event::PictureUpdate(update) => {
                                let jid = update.jid.to_non_ad().to_string();
                                if let Some(dir) = media_dir.as_deref() {
                                    let path = avatar_path(dir, &jid);
                                    let _ = std::fs::remove_file(path.with_extension("none"));
                                    let _ = std::fs::remove_file(path);
                                }
                                let _ = events.send(ServiceEvent::AvatarChanged { jid });
                            }
                            // Linked devices never receive view-once media: the
                            // server sends a stub instead, kept as a placeholder
                            // that points at the phone.
                            Event::UndecryptableMessage(stub)
                                if stub.unavailable_type
                                    == whatsapp_rust::wacore::types::events::UnavailableType::ViewOnce =>
                            {
                                let info = &stub.info;
                                let chat = info.source.chat.to_string();
                                let id = info.id.to_string();
                                if store.message(&chat, &id).is_ok() {
                                    return;
                                }
                                let from_me = info.source.is_from_me;
                                let message = StoredMessage {
                                    chat: chat.clone(),
                                    id: id.clone(),
                                    sender: info.source.sender.to_string(),
                                    sender_name: None,
                                    timestamp: info.timestamp.timestamp(),
                                    from_me,
                                    text: String::new(),
                                    media_kind: Some("view_once".into()),
                                    media_path: None,
                                    media_thumb: None,
                                    media_ref: None,
                                    reply_to_id: None,
                                    reply_to_text: None,
                                    reply_to_sender: None,
                                    reply_to_chat: None,
                                    reply_to_kind: None,
                                    reply_to_thumb: None,
                                    read: from_me,
                                    revoked: false,
                                    mentioned: false,
                                    preview_url: None,
                                    preview_title: None,
                                    preview_desc: None,
                                    preview_thumb: None,
                                    status: None,
                                };
                                let _ = store.set_view_once(&chat, &id, from_me);
                                if store.insert_message(&message).is_ok() {
                                    let _ = events.send(ServiceEvent::Message { message: Box::new(message) });
                                }
                            }
                            // Chat pins are account state; mirror them so the
                            // list matches the phone.
                            Event::PinUpdate(pin) => {
                                let pinned = pin.action.pinned.unwrap_or(false);
                                let jid = pin.jid.to_non_ad().to_string();
                                let _ = store.set_pinned(&jid, pinned);
                            }
                            _ => {}
                        }
                    }
                },
            )
            .build()
            .await?;

        let client = bot.client();

        // Hand the client to the message handler, which needs it to download
        // media. Without this the slot stays empty and every attachment is
        // recorded with no file.
        let _ = client_slot.set(client.clone());

        // `run()` only returns on logout or shutdown, so it lives in its own task.
        tokio::spawn(async move {
            tokio::select! {
                _ = bot.run() => {}
                _ = shutdown_rx => {}
            }
        });

        Ok((
            Self {
                client,
                store,
                events,
                shutdown: Mutex::new(Some(shutdown_tx)),
                media_dir,
                qr: qr_state,
                connected: connected_state,
                subject_backoff: Mutex::default(),
                resolving: AtomicBool::new(false),
                group_cache,
                groups_cache: Mutex::new(Vec::new()),
            },
            initial_rx,
        ))
    }

    /// Subscribes to service events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    /// The current pairing code, if the account still needs pairing.
    ///
    /// Exposed separately from the event stream because the code is issued
    /// during startup, before a UI subscriber may have attached.
    pub fn current_qr(&self) -> Option<String> {
        self.qr.lock().unwrap().clone()
    }

    /// Whether the account is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Resolves display names for chats that do not have one yet.
    ///
    /// Only groups need a query: a one-to-one chat is named after its contact,
    /// whose name arrives with the message itself. Returns how many were
    /// resolved, so the caller can refresh only when something changed.
    /// A failed group waits before its next query, twice as long each time up to
    /// half an hour; overlapping calls return 0 at once.
    pub async fn resolve_missing_names(&self) -> Result<usize> {
        if self.resolving.swap(true, Ordering::SeqCst) {
            return Ok(0);
        }
        let result = self.resolve_untried_groups().await;
        self.resolving.store(false, Ordering::SeqCst);
        result
    }

    async fn resolve_untried_groups(&self) -> Result<usize> {
        let now = std::time::Instant::now();
        let due: Vec<String> = {
            let backoff = self.subject_backoff.lock().unwrap();
            self.store
                .chats()?
                .into_iter()
                .filter(|c| c.display_name.is_none() && c.chat.ends_with("@g.us"))
                .map(|c| c.chat)
                .filter(|chat| backoff.get(chat).map_or(true, |(retry_at, _)| *retry_at <= now))
                .collect()
        };
        let mut resolved = 0;
        for chat in due {
            if let Some(subject) = fetch_group_subject(&self.client, &chat).await {
                self.store.set_name(&chat, &subject)?;
                self.subject_backoff.lock().unwrap().remove(&chat);
                resolved += 1;
            } else {
                let mut backoff = self.subject_backoff.lock().unwrap();
                let wait = backoff
                    .get(&chat)
                    .map_or(Duration::from_secs(30), |(_, last)| (*last * 2).min(Duration::from_secs(30 * 60)));
                backoff.insert(chat, (std::time::Instant::now() + wait, wait));
            }
        }
        Ok(resolved)
    }

    /// Sends a text message to a chat.
    ///
    /// Members of a group chat, for mention autocomplete.
    pub async fn participants(&self, chat: &str) -> Result<Vec<Participant>> {
        Ok(self.group_info(chat).await?.participants)
    }

    /// Everything the group info sidebar needs.
    pub async fn group_info(&self, chat: &str) -> Result<GroupInfo> {
        if !chat.ends_with("@g.us") {
            return Ok(GroupInfo::default());
        }
        if let Some(info) = self.group_cache.lock().unwrap().get(chat).cloned() {
            return Ok(info);
        }
        let jid: Jid = chat.parse()?;
        let mut metadata = self
            .client
            .groups()
            .fetch_metadata(&jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        // The server often omits the phone number on LID participants, so fill
        // it from the client's LID/PN cache before naming them.
        self.client
            .groups()
            .resolve_participant_addresses(&mut metadata)
            .await;

        let mut seen = std::collections::HashSet::new();
        let mut participants = Vec::new();
        for member in &metadata.participants {
            let mention = member.jid.to_non_ad().to_string();
            if !seen.insert(mention.clone()) {
                continue;
            }
            remember_lid_pn(&self.store, &member.jid, member.phone_number.as_ref().or(member.lid.as_ref()));
            let candidates = [
                member.phone_number.as_ref(),
                member.lid.as_ref(),
                Some(&member.jid),
            ];
            // Phone number without the server, so it reads as a number.
            let number = member
                .phone_number
                .as_ref()
                .map(|j| j.to_non_ad().to_string())
                .or_else(|| mention.ends_with("@s.whatsapp.net").then(|| mention.clone()))
                .map(|j| j.split('@').next().unwrap_or(&j).to_string());
            let username = member.username.as_ref().map(|u| u.to_string());
            // WhatsApp's masked number for a member whose phone is hidden
            // ("+598∙∙∙∙∙27"). Only a label of last resort; never stored as a
            // name, or it would overwrite the member's real push name.
            let masked = member
                .details
                .as_ref()
                .and_then(|d| d.display_name.as_ref())
                .map(|n| n.to_string())
                .filter(|n| !n.trim().is_empty());
            // A name someone can read: saved/push name, then username, then the
            // phone number, and only last the masked number or the LID.
            // A placeholder under one form must not hide a real name stored
            // under the other.
            let name = candidates
                .into_iter()
                .flatten()
                .filter_map(|j| self.store.name_for(&j.to_string()).ok().flatten())
                .find(|n| !is_placeholder_name(n))
                .or_else(|| username.clone())
                .or_else(|| number.clone())
                .or(masked)
                .unwrap_or_else(|| mention.split('@').next().unwrap_or(&mention).to_string());
            let label = member
                .details
                .as_ref()
                .and_then(|d| d.participant_label.as_ref())
                .map(|l| l.to_string())
                .filter(|l| !l.is_empty());
            participants.push(Participant {
                jid: mention.clone(),
                name,
                admin: member.is_admin(),
                owner: member.is_super_admin(),
                number,
                username,
                label,
            });
        }
        // Group metadata rarely carries usernames; one usync query fills them in,
        // and gives members we only know by number a username or business name.
        let jids: Vec<Jid> = participants.iter().filter_map(|p| p.jid.parse().ok()).collect();
        if let Ok(infos) = self.client.contacts().get_user_info(&jids).await {
            let numeric = |n: &str| is_placeholder_name(n);
            for p in participants.iter_mut() {
                let user = p.jid.split('@').next().unwrap_or_default();
                let Some(info) = infos.values().find(|i| {
                    i.jid.user == user
                        || i.lid.as_ref().is_some_and(|l| l.user == user)
                        || p.number.as_deref() == Some(i.jid.user.as_str())
                }) else {
                    continue;
                };
                if p.username.is_none() {
                    p.username = info.username.as_ref().map(|u| u.to_string());
                }
                if numeric(&p.name) {
                    if let Some(better) = info
                        .verified_name
                        .as_ref()
                        .and_then(|v| v.name.clone())
                        .or_else(|| p.username.clone())
                    {
                        p.name = better;
                    }
                }
            }
        }
        participants.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let info = GroupInfo {
            subject: metadata.subject.clone(),
            description: metadata.description.clone(),
            created_at: metadata.creation_time,
            participants,
            allow_admin_reports: metadata.allow_admin_reports,
        };
        self.group_cache
            .lock()
            .unwrap()
            .insert(chat.to_string(), info.clone());
        Ok(info)
    }

    /// The sent message is stored and dispatched locally. WhatsApp does not echo
    /// a message back to the device that sent it, so without this the sender
    /// would not see their own message until the store was next reloaded.
    pub async fn send_text(
        &self,
        chat: &str,
        text: impl Into<String>,
        mentions: Vec<String>,
    ) -> Result<()> {
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let text = text.into();
        // `@all` is a group mention, carried separately from member mentions.
        let mention_all = mentions.iter().any(|m| m == "@all");
        let mentioned: Vec<String> = mentions.iter().filter(|m| *m != "@all").cloned().collect();

        // A link in the text gets an Open Graph preview, fetched off the runtime.
        let preview = if mentioned.is_empty() {
            match first_url(&text) {
                Some(url) => tokio::task::spawn_blocking(move || fetch_link_preview(&url))
                    .await
                    .ok()
                    .flatten(),
                None => None,
            }
        } else {
            None
        };

        let result = if mentioned.is_empty() && !mention_all && preview.is_none() {
            self.client.send_text(to, text.clone()).await?
        } else {
            let mut context = wa::ContextInfo {
                mentioned_jid: mentioned.clone(),
                ..Default::default()
            };
            if mention_all {
                context.group_mentions = vec![wa::GroupMention {
                    group_jid: Some(chat.to_string()),
                    group_subject: self.store.name_for(chat).ok().flatten(),
                }];
            }
            let extended = wa::message::ExtendedTextMessage {
                text: Some(text.clone()),
                matched_text: preview.as_ref().map(|p| p.url.clone()),
                title: preview.as_ref().and_then(|p| p.title.clone()),
                description: preview.as_ref().and_then(|p| p.description.clone()),
                jpeg_thumbnail: preview.as_ref().and_then(|p| p.thumbnail.clone()),
                context_info: MessageField::some(context),
                ..Default::default()
            };
            let message = wa::Message {
                extended_text_message: MessageField::some(extended),
                ..Default::default()
            };
            self.client.send_message(to, message).await?
        };

        // Keep the preview with our own copy, so the sender sees it too.
        let thumbnail = preview.as_ref().and_then(|p| {
            let bytes = p.thumbnail.as_ref()?;
            let dir = self.media_dir()?;
            std::fs::create_dir_all(&dir).ok()?;
            let path = dir.join(format!("{}_thumb.jpg", result.message_id));
            std::fs::write(&path, bytes).ok()?;
            Some(path.to_string_lossy().to_string())
        });

        let mut message = StoredMessage {
            chat: chat.to_string(),
            id: result.message_id.clone(),
            sender: self.own_jid(),
            sender_name: None,
            timestamp: unix_now(),
            from_me: true,
            text,
            media_kind: None,
            media_path: None,
            media_thumb: None,
            media_ref: None,
            reply_to_id: None,
            reply_to_text: None,
            reply_to_sender: None,
            reply_to_chat: None,
            reply_to_kind: None,
            reply_to_thumb: None,
            // Not `true`: we cannot know whether the recipient has read it, and
            // claiming so shows a read marker that is not true.
            read: false,
            revoked: false,
            mentioned: false,
            preview_url: None,
            preview_title: None,
            preview_desc: None,
            preview_thumb: None,
            // A message to ourselves is already where it needs to be; leaving
            // it pending would wait for a receipt that never arrives.
            status: Some(if to_self { "delivered".into() } else { "pending".into() }),
        };
        if let Some(p) = &preview {
            message.preview_url = Some(p.url.clone());
            message.preview_title = p.title.clone();
            message.preview_desc = p.description.clone();
            message.preview_thumb = thumbnail;
        }
        self.store.insert_message(&message)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(message) });
        Ok(())
    }

    /// Our own JID, used as the sender of messages we send.
    /// Our own JID without a device suffix, or empty before pairing.
    pub fn own_jid(&self) -> String {
        self.client
            .pn()
            .or_else(|| self.client.lid())
            .map(|j| j.to_non_ad().to_string())
            .unwrap_or_default()
    }

    /// Whether a destination is our own account, in either addressing form.
    ///
    /// A message to ourselves needs no network receipt to be delivered, so it
    /// is stored as delivered rather than left pending.
    fn is_self_jid(&self, jid: &Jid) -> bool {
        match (self.client.pn(), self.client.lid()) {
            (Some(pn), Some(lid)) => jid.matches_user_or_lid(&pn, Some(&lid)),
            (Some(pn), None) => jid.matches_user_or_lid(&pn, None),
            (None, Some(lid)) => jid.matches_user_or_lid(&lid, None),
            (None, None) => false,
        }
    }

    /// Best known name per JID, looked up under both its LID and phone form;
    /// our own addresses read as our push name. Falls back to the phone number
    /// digits, and leaves out JIDs nothing is known about.
    pub async fn names_for(&self, jids: &[String]) -> std::collections::HashMap<String, String> {
        let numeric = |n: &str| is_placeholder_name(n);
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        let push_name = self.client.push_name();
        let mut out = std::collections::HashMap::new();
        let mut unknown: Vec<(String, Jid)> = Vec::new();
        for jid in jids {
            let Ok(parsed) = jid.parse::<Jid>() else { continue };
            let bare = parsed.to_non_ad();
            let key = bare.to_string();
            if own.contains(&key) && !push_name.is_empty() {
                out.insert(jid.clone(), push_name.clone());
                continue;
            }
            let mut name = self.store.name_for(&key).ok().flatten();
            let mut number = bare.is_pn().then(|| bare.user.to_string());
            if name.as_deref().is_none_or(numeric) {
                if let Some((lid, pn)) = other_form(&self.client, &self.store, &bare).await {
                    let other = if bare.is_lid() {
                        format!("{pn}@s.whatsapp.net")
                    } else {
                        format!("{lid}@lid")
                    };
                    number = Some(pn);
                    if let Some(found) = self.store.name_for(&other).ok().flatten() {
                        if !numeric(&found) {
                            name = Some(found);
                        }
                    }
                }
            }
            match name.filter(|n| !numeric(n)) {
                Some(name) => {
                    out.insert(jid.clone(), name);
                }
                None => {
                    if let Some(number) = number {
                        out.insert(jid.clone(), number);
                    }
                    unknown.push((jid.clone(), bare));
                }
            }
        }
        // Push names only travel with messages; for anyone we have not heard
        // from, the username or verified business name is the next best thing.
        if !unknown.is_empty() {
            let query: Vec<Jid> = unknown.iter().map(|(_, j)| j.clone()).collect();
            if let Ok(infos) = self.client.contacts().get_user_info(&query).await {
                for (asked, jid) in unknown {
                    let info = infos.values().find(|i| {
                        i.jid.user == jid.user || i.lid.as_ref().is_some_and(|l| l.user == jid.user)
                    });
                    let found = info.and_then(|i| {
                        i.verified_name
                            .as_ref()
                            .and_then(|v| v.name.clone())
                            .or_else(|| i.username.as_ref().map(|u| u.to_string()))
                    });
                    if let Some(found) = found.filter(|n| !n.trim().is_empty()) {
                        let _ = self.store.set_name(&jid.to_string(), &found);
                        out.insert(asked, found);
                    } else {
                        eprintln!("[hermodr] no name known for {jid} (shown as {:?})", out.get(&asked));
                    }
                }
            }
        }
        out
    }

    /// A group invite link's group, without joining it.
    pub async fn invite_info(&self, link: &str) -> Result<InviteInfo> {
        let group = self
            .client
            .groups()
            .get_invite_info(link)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let jid = group.id.to_string();
        // Already a member when the chat is here.
        let joined = self.store.chats()?.iter().any(|c| c.chat == jid);
        let picture = self.avatar(&jid).await.ok().flatten();
        Ok(InviteInfo {
            size: group.size.unwrap_or(group.participants.len() as u32),
            subject: group.subject,
            description: group.description.filter(|d| !d.trim().is_empty()),
            created_at: group.creation_time,
            approval: group.membership_approval,
            community: group.is_parent_group,
            joined,
            picture,
            jid,
        })
    }

    /// Joins through an invite link; `pending` when an admin has to approve.
    pub async fn join_invite(&self, link: &str) -> Result<(String, bool)> {
        use whatsapp_rust::JoinGroupResult;
        let joined = self
            .client
            .groups()
            .join_with_invite_code(link)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let pending = matches!(joined, JoinGroupResult::PendingApproval(_));
        Ok((joined.group_jid().to_string(), pending))
    }

    /// Everything a profile card shows about someone, fetched fresh.
    pub async fn user_profile(&self, jid: &str) -> Result<UserProfile> {
        let bare = jid.parse::<Jid>()?.to_non_ad();
        let key = bare.to_string();
        let mut profile = UserProfile { jid: key.clone(), ..Default::default() };
        profile.number = if bare.is_pn() {
            Some(bare.user.to_string())
        } else {
            other_form(&self.client, &self.store, &bare).await.map(|(_, pn)| pn)
        };
        if let Ok(infos) = self.client.contacts().get_user_info(std::slice::from_ref(&bare)).await {
            if let Some(info) = infos.into_values().next() {
                profile.about = info.status.filter(|s| !s.is_empty());
                profile.username = info.username.map(|u| u.to_string());
                profile.business = info.verified_name.and_then(|v| v.name);
            }
        }
        profile.name = self
            .names_for(std::slice::from_ref(&key))
            .await
            .remove(&key)
            .filter(|n| !is_placeholder_name(n));
        Ok(profile)
    }

    /// Chats, contacts and groups matching a query.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        use std::collections::HashSet;

        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }

        let local = self.store.chats()?;
        let local_jids: HashSet<String> = local.iter().map(|c| c.chat.clone()).collect();
        let mut results: Vec<SearchResult> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Local chats first: they have history and are what a search usually
        // means.
        for chat in &local {
            let number = user_part(&chat.chat);
            let name = chat.display_name.clone().unwrap_or_else(|| number.clone());
            if name.to_lowercase().contains(&needle) || number.contains(&needle) {
                seen.insert(chat.chat.clone());
                results.push(SearchResult {
                    kind: if chat.chat.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                    saved: self.store.name_is_saved(&chat.chat),
                    jid: chat.chat.clone(),
                    name,
                    number,
                    has_messages: true,
                });
            }
        }

        // Address book and learned names.
        for (jid, name, saved) in self.store.search_names(&needle, 50)? {
            if !seen.insert(jid.clone()) {
                continue;
            }
            results.push(SearchResult {
                kind: if jid.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                saved,
                jid: jid.clone(),
                name,
                number: user_part(&jid),
                has_messages: local_jids.contains(&jid),
            });
        }

        // Groups from the account, including ones with no local history.
        for (jid, subject) in self.group_overviews().await {
            if subject.to_lowercase().contains(&needle) && seen.insert(jid.clone()) {
                results.push(SearchResult {
                    jid: jid.clone(),
                    name: subject,
                    number: String::new(),
                    kind: "group".into(),
                    saved: false,
                    has_messages: local_jids.contains(&jid),
                });
            }
        }

        results.truncate(50);
        Ok(results)
    }

    /// Every group the account is in, fetched once and cached.
    async fn group_overviews(&self) -> Vec<(String, String)> {
        {
            let cache = self.groups_cache.lock().unwrap();
            if !cache.is_empty() {
                return cache.clone();
            }
        }
        match self.client.groups().list_participating().await {
            Ok(groups) => {
                let pairs: Vec<(String, String)> = groups
                    .into_iter()
                    .filter_map(|g| g.subject.map(|s| (g.id.to_string(), s)))
                    .collect();
                *self.groups_cache.lock().unwrap() = pairs.clone();
                pairs
            }
            Err(_) => Vec::new(),
        }
    }

    /// Pins or unpins a chat, mirroring it to the account.
    pub async fn set_pinned(&self, chat: &str, pinned: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.store.set_pinned(&jid.to_non_ad().to_string(), pinned)?;
        let actions = self.client.chat_actions();
        let result = if pinned {
            actions.pin_chat(&jid).await
        } else {
            actions.unpin_chat(&jid).await
        };
        result.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Downloads a message's media on demand, when automatic downloads were
    /// off or the earlier attempt failed.
    pub async fn download_media(&self, chat: &str, id: &str) -> Result<()> {
        let Some(bytes) = self.store.media_ref_for(chat, id)? else {
            return Err(anyhow::anyhow!("no stored media reference"));
        };
        let message = <wa::Message as buffa::Message>::decode(&mut bytes.as_slice())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let Some(media) = detect_media(&message) else {
            return Err(anyhow::anyhow!("message carries no media"));
        };
        let client = self.client.clone();
        let data = client
            .download(media.downloadable.as_ref())
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let dir = self
            .media_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no media folder configured"))?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.{}", id, media.extension()));
        std::fs::write(&path, &data)?;
        self.store
            .set_media_path(chat, id, &path.to_string_lossy())?;
        if let Ok(updated) = self.store.message(chat, id) {
            let _ = self.events.send(ServiceEvent::Message { message: Box::new(updated) });
        }
        Ok(())
    }

    /// The chat a stored message id belongs to.
    pub fn chat_for_message(&self, id: &str) -> Result<Option<String>> {
        self.store.chat_of_message(id)
    }

    /// Sets a chat's auto download override.
    pub fn set_chat_auto_download(&self, chat: &str, enabled: bool) -> Result<()> {
        self.store.set_chat_auto_download(chat, enabled)
    }

    /// Deletes downloaded media and forgets the paths, keeping the messages.
    pub fn flush_media(&self) -> Result<usize> {
        let cleared = self.store.clear_media_paths()?;
        if let Some(dir) = &self.media_dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let _ = std::fs::remove_dir_all(&path);
                    } else {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
        Ok(cleared)
    }

    /// Asks the phone for older messages in a chat.
    ///
    /// The request goes to our own primary device, and the messages arrive
    /// asynchronously through the normal event stream.
    pub async fn load_older(&self, chat: &str, count: i32) -> Result<()> {
        fetch_older(&self.client, &self.store, chat, count).await
    }

    /// The key that names a message to the server: groups need its sender.
    fn message_key(chat: &str, id: &str, sender: &str, from_me: bool) -> wa::MessageKey {
        let participant = (chat.ends_with("@g.us") && !from_me)
            .then(|| sender.parse::<Jid>().map(|j| j.to_non_ad().to_string()).unwrap_or_default());
        wa::MessageKey {
            remote_jid: Some(chat.to_string()),
            from_me: Some(from_me),
            id: Some(id.to_string()),
            participant,
            ..Default::default()
        }
    }

    /// The sender as the app-state actions want it: set only for others in groups.
    fn participant(chat: &str, sender: &str, from_me: bool) -> Option<Jid> {
        (chat.ends_with("@g.us") && !from_me)
            .then(|| sender.parse::<Jid>().ok().map(|j| j.to_non_ad()))
            .flatten()
    }

    /// Reacts to a message; an empty emoji takes our reaction back.
    pub async fn react(&self, chat: &str, id: &str, sender: &str, from_me: bool, emoji: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .send_reaction(jid, Self::message_key(chat, id, sender, from_me), emoji)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_reaction(chat, id, "@me", emoji)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    pub async fn star(&self, chat: &str, id: &str, sender: &str, from_me: bool, starred: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let participant = Self::participant(chat, sender, from_me);
        let actions = self.client.chat_actions();
        let done = if starred {
            actions.star_message(&jid, participant.as_ref(), id, from_me).await
        } else {
            actions.unstar_message(&jid, participant.as_ref(), id, from_me).await
        };
        done.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_starred(chat, id, starred)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Pins a message for everyone in the chat for a week, or unpins it.
    pub async fn pin_message(&self, chat: &str, id: &str, sender: &str, from_me: bool, pinned: bool) -> Result<()> {
        use whatsapp_rust::send::PinDuration;
        let jid: Jid = chat.parse()?;
        let key = Self::message_key(chat, id, sender, from_me);
        let sent = if pinned {
            self.client.pin_message(jid, key, PinDuration::Days7).await
        } else {
            self.client.unpin_message(jid, key).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_message_pin(chat, pinned.then_some(id))?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Deletes a message for everyone: ours as the sender, anyone's as an admin.
    pub async fn delete_for_everyone(&self, chat: &str, id: &str, sender: &str, from_me: bool) -> Result<()> {
        use whatsapp_rust::send::RevokeType;
        let jid: Jid = chat.parse()?;
        let kind = if from_me {
            RevokeType::Sender
        } else {
            RevokeType::Admin { original_sender: sender.parse::<Jid>()?.to_non_ad() }
        };
        self.client
            .revoke_message(jid, id, kind)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.revoke(chat, id)?;
        if let Ok(updated) = self.store.message(chat, id) {
            let _ = self.events.send(ServiceEvent::Message { message: Box::new(updated) });
        }
        Ok(())
    }

    /// Deletes a message from our devices only.
    pub async fn delete_for_me(&self, chat: &str, id: &str, sender: &str, from_me: bool, timestamp: i64) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let participant = Self::participant(chat, sender, from_me);
        self.client
            .chat_actions()
            .delete_message_for_me(&jid, participant.as_ref(), id, from_me, true, Some(timestamp))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        // The stored copy is gone, so its media file has no other referent.
        if let Ok(message) = self.store.message(chat, id) {
            if let Some(path) = message.media_path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
        }
        self.store.delete_message(chat, id)
    }

    /// Reports a group message to the group's admins.
    pub async fn report_to_admins(&self, chat: &str, id: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .groups()
            .report_messages_to_admins(jid, &[id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Sends a copy of a stored message to another chat.
    pub async fn forward(&self, from_chat: &str, id: &str, to_chat: &str) -> Result<()> {
        let message = self.store.message(from_chat, id)?;
        // Uncaptioned media is stored as `[kind]`, which must not become a caption.
        let placeholder = message.media_kind.as_ref().map(|kind| format!("[{kind}]"));
        let text = message.text.trim();
        let caption = (!text.is_empty() && placeholder.as_deref() != Some(text))
            .then(|| message.text.clone());
        match message.media_path.as_deref().filter(|p| Path::new(p).is_file()) {
            Some(path) => {
                let bytes = std::fs::read(path)?;
                let name = Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                let gif = message.media_kind.as_deref() == Some("gif");
                if message.media_kind.as_deref() == Some("sticker") {
                    self.send_sticker_as(to_chat, bytes, true).await?;
                } else {
                    let options = SendOptions { gif, forwarded: true, ..Default::default() };
                    self.send_media(to_chat, &name, bytes, caption, None, options).await?;
                }
            }
            None if message.media_kind.is_some() => {
                anyhow::bail!("download the media before forwarding it")
            }
            None => {
                let to: Jid = to_chat.parse()?;
                let to_self = self.is_self_jid(&to);
                let content = wa::Message {
                    extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
                        text: Some(message.text.clone()),
                        context_info: MessageField::some(*forwarded_context(None)),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let result = self.client.send_message(to, content).await?;
                self.store.set_forwarded(to_chat, &result.message_id)?;
                let mut stored = self.own_message(to_chat, &result.message_id, message.text, "", to_self);
                stored.media_kind = None;
                self.store.insert_message(&stored)?;
                let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
            }
        }
        Ok(())
    }

    pub fn marks(&self, chat: &str) -> Result<crate::store::ChatMarks> {
        self.store.marks(chat)
    }

    /// Marks a view-once message opened and deletes its media.
    pub fn open_view_once(&self, chat: &str, id: &str) -> Result<()> {
        if let Some(path) = self.store.open_view_once(chat, id)? {
            let _ = std::fs::remove_file(path);
        }
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// A message we just sent, as the store keeps it until the server confirms.
    fn own_message(&self, chat: &str, id: &str, text: String, kind: &str, to_self: bool) -> StoredMessage {
        StoredMessage {
            chat: chat.to_string(),
            id: id.to_string(),
            sender: self.own_jid(),
            sender_name: None,
            timestamp: unix_now(),
            from_me: true,
            text,
            media_kind: Some(kind.to_string()),
            media_path: None,
            media_thumb: None,
            media_ref: None,
            reply_to_id: None,
            reply_to_text: None,
            reply_to_sender: None,
            reply_to_chat: None,
            reply_to_kind: None,
            reply_to_thumb: None,
            read: false,
            revoked: false,
            mentioned: false,
            preview_url: None,
            preview_title: None,
            preview_desc: None,
            preview_thumb: None,
            // A message to ourselves needs no receipt to count as delivered.
            status: Some(if to_self { "delivered".into() } else { "pending".into() }),
        }
    }

    pub async fn create_poll(&self, chat: &str, question: &str, options: Vec<String>, multi: bool) -> Result<()> {
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let selectable = if multi { options.len() as u32 } else { 1 };
        let (result, secret) = self
            .client
            .polls()
            .create(to, question, &options, selectable)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let id = result.message_id.clone();
        self.store
            .save_poll(chat, &id, &self.own_jid(), question, &options, multi, Some(&secret))?;
        let stored = self.own_message(chat, &id, question.to_string(), "poll", to_self);
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
        Ok(())
    }

    /// Casts or changes our vote; no options withdraws it.
    pub async fn vote_poll(&self, chat: &str, id: &str, options: Vec<String>) -> Result<()> {
        let def = self
            .store
            .poll_secret(chat, id)?
            .ok_or_else(|| anyhow::anyhow!("this poll arrived without its key, so it cannot be voted on here"))?;
        let jid: Jid = chat.parse()?;
        let creator: Jid = def.creator.parse()?;
        self.client
            .polls()
            .vote(jid, id, &creator, &def.secret, &options)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_poll_vote(chat, id, "@me", &options)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    pub async fn create_event(&self, chat: &str, event: crate::store::NewEvent) -> Result<()> {
        use whatsapp_rust::EventCreationParams;
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let params = EventCreationParams {
            name: event.name.clone(),
            description: event.description.clone(),
            start_time: event.start,
            end_time: event.end,
            join_link: event.link.clone(),
            location: event.location.clone().map(|name| wa::message::LocationMessage {
                name: Some(name),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (result, secret) = self
            .client
            .events()
            .create(to, params)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let id = result.message_id.clone();
        self.store.save_event(chat, &id, &self.own_jid(), &event, Some(&secret))?;
        let stored = self.own_message(chat, &id, event.name, "event", to_self);
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
        Ok(())
    }

    /// Edits or cancels one of our events. The edit is encrypted with the
    /// event's secret, which is how WhatsApp sends event edits.
    pub async fn edit_event(&self, chat: &str, id: &str, event: crate::store::NewEvent) -> Result<()> {
        let def = self
            .store
            .event_secret(chat, id)?
            .ok_or_else(|| anyhow::anyhow!("this event's key never reached this device"))?;
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        if !own.contains(&def.creator) {
            anyhow::bail!("only the event's creator can change it");
        }
        let content = wa::Message {
            event_message: MessageField::some(wa::message::EventMessage {
                name: Some(event.name.clone()),
                description: event.description.clone(),
                start_time: event.start,
                end_time: event.end,
                join_link: event.link.clone(),
                is_canceled: Some(event.canceled),
                location: event
                    .location
                    .clone()
                    .map(|name| wa::message::LocationMessage { name: Some(name), ..Default::default() })
                    .into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let to: Jid = chat.parse()?;
        self.client
            .edit_message_encrypted(to, id, &def.secret, content)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.save_event(chat, id, &def.creator, &event, None)?;
        self.store.apply_edit(chat, id, &event.name)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Who got, read and played one of our messages.
    pub fn message_info(&self, id: &str) -> Result<Vec<crate::store::MessageReceipt>> {
        self.store.receipts(id)
    }

    /// Starred messages across every chat, newest first.
    pub fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        self.store.starred_messages()
    }

    /// Messages that mention us, in one chat or all of them, newest first.
    pub fn pings(&self, chat: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.store.pings(chat, 500)
    }

    /// Up to `limit` messages in a chat containing `query`, newest first.
    pub fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.store.search_messages(chat, query.trim(), limit)
    }

    pub fn chat_retention(&self, chat: &str) -> Result<crate::store::ChatRetention> {
        self.store.chat_retention(chat)
    }

    /// Sets a chat's own retention and applies it at once.
    pub fn set_chat_retention(&self, chat: &str, retention: &crate::store::ChatRetention) -> Result<()> {
        self.store.set_chat_retention(chat, retention)?;
        self.store.enforce_retention()?;
        Ok(())
    }

    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, chat: &str) -> Result<Option<bool>> {
        self.store.chat_auto_download(chat)
    }

    /// Messages members reported to this group's admins. Only admins may ask.
    pub async fn admin_reports(&self, chat: &str) -> Result<Vec<AdminReport>> {
        let jid: Jid = chat.parse()?;
        let reported = self
            .client
            .groups()
            .get_reported_messages(jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(reported
            .reports
            .into_iter()
            .map(|report| AdminReport {
                message: self.store.message(chat, &report.message_id).ok(),
                reporters: report
                    .reporters
                    .into_iter()
                    .map(|r| (r.phone_number.unwrap_or(r.jid).to_non_ad().to_string(), r.timestamp))
                    .collect(),
                id: report.message_id,
            })
            .collect())
    }

    /// Lets members report messages to the admins, or stops them.
    pub async fn set_allow_admin_reports(&self, chat: &str, allow: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .groups()
            .set_allow_admin_reports(jid, allow)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(info) = self.group_cache.lock().unwrap().get_mut(chat) {
            info.allow_admin_reports = allow;
        }
        Ok(())
    }

    /// Answers an event: `going`, `not_going` or `maybe`.
    pub async fn respond_event(&self, chat: &str, id: &str, response: &str) -> Result<()> {
        use wa::message::event_response_message::EventResponseType;
        let answer = match response {
            "going" => EventResponseType::GOING,
            "not_going" => EventResponseType::NOT_GOING,
            "maybe" => EventResponseType::MAYBE,
            other => anyhow::bail!("unknown response {other}"),
        };
        let def = self
            .store
            .event_secret(chat, id)?
            .ok_or_else(|| anyhow::anyhow!("this event arrived without its key, so it cannot be answered here"))?;
        let jid: Jid = chat.parse()?;
        let creator: Jid = def.creator.parse()?;
        self.client
            .events()
            .respond(jid, id, &creator, &def.secret, answer, None)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_event_response(chat, id, "@me", response)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Sets our own tag in a group; empty clears it.
    pub async fn set_member_label(&self, chat: &str, label: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client
            .groups()
            .update_member_label(jid, label)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        if let Some(info) = self.group_cache.lock().unwrap().get_mut(chat) {
            if let Some(p) = info.participants.iter_mut().find(|p| own.contains(&p.jid)) {
                p.label = (!label.is_empty()).then(|| label.to_string());
            }
        }
        Ok(())
    }

    /// Tells the chat we are typing, or that we stopped.
    pub async fn send_typing(&self, chat: &str, typing: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let chatstate = self.client.chatstate();
        let sent = if typing {
            chatstate.send_composing(&jid).await
        } else {
            chatstate.send_paused(&jid).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Marks us online or away, as WhatsApp Web does on window focus. Typing
    /// indicators only arrive while we are online.
    pub async fn set_online(&self, online: bool) -> Result<()> {
        let presence = self.client.presence();
        let set = if online {
            presence.set_available().await
        } else {
            presence.set_unavailable().await
        };
        set.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Subscribes to a contact's presence, which one-to-one typing needs.
    pub async fn watch_presence(&self, jid: &str) -> Result<()> {
        let jid: Jid = jid.parse()?;
        self.client
            .presence()
            .subscribe(jid)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Our own name, about text and privacy settings.
    pub async fn profile(&self) -> Result<Profile> {
        let own: Jid = self.own_jid().parse()?;
        let about = self
            .client
            .contacts()
            .get_user_info(std::slice::from_ref(&own))
            .await
            .ok()
            .and_then(|mut info| info.drain().next())
            .and_then(|(_, info)| info.status);
        let privacy = self
            .client
            .fetch_privacy_settings()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .settings
            .into_iter()
            .map(|s| (s.category.to_string(), s.value.to_string()))
            .collect();
        let own_username = self.client.mex().get_username().await.ok().flatten();
        let username_reserved = own_username
            .as_ref()
            .and_then(|u| u.state.as_deref())
            .is_some_and(|state| state.eq_ignore_ascii_case("reserved"));
        Ok(Profile {
            name: self.client.push_name(),
            about,
            username: own_username.and_then(|u| u.username),
            username_reserved,
            privacy,
        })
    }

    /// Replaces our profile picture with any image, cropped square and sized
    /// the way WhatsApp expects. Empty bytes remove the picture.
    pub async fn set_own_picture(&self, bytes: Vec<u8>) -> Result<()> {
        let profile = self.client.profile();
        let sent = if bytes.is_empty() {
            profile.remove_profile_picture().await
        } else {
            let jpeg = square_jpeg(&bytes, 640)
                .ok_or_else(|| anyhow::anyhow!("that file is not an image we can read"))?;
            profile.set_profile_picture(jpeg).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(dir) = &self.media_dir {
            let path = avatar_path(dir, &self.own_jid());
            let _ = std::fs::remove_file(path.with_extension("none"));
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    pub async fn set_about(&self, text: &str) -> Result<()> {
        self.client
            .profile()
            .set_status_text(text)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    pub async fn set_push_name(&self, name: &str) -> Result<()> {
        self.client
            .profile()
            .set_push_name(name)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    pub async fn set_privacy(&self, category: &str, value: &str) -> Result<()> {
        let (category, value) = (PrivacyCategory::from(category), PrivacyValue::from(value));
        if !category.is_valid_value(&value) {
            anyhow::bail!("{value} is not a valid setting for {category}");
        }
        self.client
            .set_privacy_setting(category, value)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Path to a chat's profile picture, cached for a day in the media folder.
    ///
    /// `None` when the chat has no picture, hides it from us, or media is off.
    pub async fn avatar(&self, jid: &str) -> Result<Option<String>> {
        const TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
        let Some(dir) = self.media_dir.clone() else {
            return Ok(None);
        };
        let path = avatar_path(&dir, jid);
        // Marks a chat known to have no picture, so it is not asked again.
        let none = path.with_extension("none");
        let fresh = |p: &Path| {
            p.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < TTL)
        };
        if fresh(path.as_path()) {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
        if fresh(none.as_path()) {
            return Ok(None);
        }

        let target: Jid = jid.parse()?;
        let picture = self
            .client
            .contacts()
            .get_profile_picture(&target, true)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let Some(picture) = picture else {
            let _ = std::fs::remove_file(&path);
            std::fs::write(&none, b"")?;
            return Ok(None);
        };
        let url = picture.url;
        let bytes = tokio::task::spawn_blocking(move || {
            let mut response = ureq::get(&url).call().ok()?;
            response.body_mut().read_to_vec().ok()
        })
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("could not download the profile picture"))?;
        std::fs::write(&path, bytes)?;
        let _ = std::fs::remove_file(&none);
        Ok(Some(path.to_string_lossy().into_owned()))
    }

    /// Unread messages that mention us, oldest first.
    pub fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        self.store.unread_mentions(chat)
    }

    /// Where media is stored, if enabled.
    pub fn media_dir(&self) -> Option<PathBuf> {
        self.media_dir.clone()
    }

    /// Whether the account has read receipts turned off in its privacy
    /// settings. The protocol client keeps this in sync with the server; when
    /// true, read and played receipts must not be sent.
    pub fn read_receipts_disabled(&self) -> bool {
        self.client
            .persistence_manager()
            .get_device_snapshot()
            .read_receipts_disabled
    }

    /// Marks a chat's incoming messages as read, and with `receipts` tells
    /// their senders. Returns how many changed.
    pub async fn mark_read(&self, chat: &str, receipts: bool) -> Result<usize> {
        let unread = if receipts { self.store.unread_ids(chat)? } else { Vec::new() };
        let changed = self.store.mark_chat_read(chat)?;
        if unread.is_empty() {
            return Ok(changed);
        }
        let to: Jid = chat.parse()?;
        // A group receipt names the author, one receipt per author; a direct
        // chat needs none.
        let mut by_sender: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (id, sender) in unread {
            let key = if to.is_group() { sender } else { String::new() };
            by_sender.entry(key).or_default().push(id);
        }
        for (sender, ids) in by_sender {
            let sender = sender.parse::<Jid>().ok().map(|j| j.to_non_ad());
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            if let Err(e) = self.client.mark_as_read(&to, sender.as_ref(), &ids).await {
                log::warn!("could not send read receipts: {e}");
            }
        }
        Ok(changed)
    }

    /// Tells the sender that a voice note was played or view-once media opened.
    pub async fn mark_played(&self, chat: &str, id: &str, sender: &str) -> Result<()> {
        let to: Jid = chat.parse()?;
        let sender = if to.is_group() { sender.parse::<Jid>().ok().map(|j| j.to_non_ad()) } else { None };
        self.client
            .mark_as_played(&to, sender.as_ref(), &[id])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Sends a text message quoting an earlier one.
    ///
    /// The quote is rebuilt from the stored message rather than the original
    /// protobuf, which we do not keep; the recipient renders the quoted text.
    pub async fn send_reply(
        &self,
        chat: &str,
        text: impl Into<String>,
        reply_to_id: &str,
        reply_to_sender: &str,
        reply_to_text: &str,
        mentions: Vec<String>,
        // The chat the quoted message is in, when it is not this one (a group
        // message answered privately).
        quote_chat: Option<&str>,
    ) -> Result<()> {
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let quoted_chat: Jid = match quote_chat {
            Some(other) => other.parse()?,
            None => to.clone(),
        };
        // The quoted author must be the address without a device suffix: a
        // participant like `123:98@lid` is not resolvable by recipients, who
        // then attribute the quoted message to the sender of the reply.
        let sender: Jid = reply_to_sender.parse::<Jid>()?.to_non_ad();
        let text = text.into();

        use whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info;
        let quoted = wa::Message::text(reply_to_text);
        let mention_all = mentions.iter().any(|m| m == "@all");
        let mentioned: Vec<String> = mentions.iter().filter(|m| *m != "@all").cloned().collect();
        let mut context =
            build_quote_context_with_info(reply_to_id, &sender, &quoted_chat, &to, &quoted);
        if !mentioned.is_empty() {
            context.mentioned_jid = mentioned.clone();
        }
        if mention_all {
            context.group_mentions = vec![wa::GroupMention {
                group_jid: Some(chat.to_string()),
                group_subject: self.store.name_for(chat).ok().flatten(),
            }];
        }

        use whatsapp_rust::wacore::proto_helpers::MessageBuilderExt;
        let message = wa::Message::text_with_context(text.clone(), context);
        let result = self.client.send_message(to, message).await?;

        let stored = StoredMessage {
            chat: chat.to_string(),
            id: result.message_id.clone(),
            sender: self.own_jid(),
            sender_name: None,
            timestamp: unix_now(),
            from_me: true,
            text,
            media_kind: None,
            media_path: None,
            media_thumb: None,
            media_ref: None,
            reply_to_id: Some(reply_to_id.to_string()),
            reply_to_text: Some(reply_to_text.to_string()),
            reply_to_sender: Some(if sender.to_string() == self.own_jid() {
                "@me".to_string()
            } else {
                reply_to_sender.to_string()
            }),
            reply_to_chat: quote_chat.map(str::to_string),
            reply_to_kind: None,
            reply_to_thumb: None,
            read: false,
            revoked: false,
            mentioned: false,
            preview_url: None,
            preview_title: None,
            preview_desc: None,
            preview_thumb: None,
            status: Some(if to_self { "delivered".into() } else { "pending".into() }),
        };
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
        Ok(())
    }

    /// Sends a file as an image or document, chosen from its extension.
    ///
    /// Images are sent as images so they render inline; everything else goes as
    /// a document, which is what a file picker is usually for.
    pub async fn send_media(
        &self,
        chat: &str,
        file_name: &str,
        bytes: Vec<u8>,
        caption: Option<String>,
        reply: Option<(String, String, String)>,
        options: SendOptions,
    ) -> Result<Option<String>> {
        let SendOptions { gif, view_once, voice, forwarded, mentions, progress } = options;
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let file_name = file_name.to_string();
        let extension = std::path::Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();

        // The extension decides how the receiver renders the file, so a video
        // only arrives as a video (not a document) if it is sent as one.
        let (media_type, kind) = match extension.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" => (MediaType::Image, "image"),
            "mp4" | "mov" | "m4v" | "webm" | "mkv" => (MediaType::Video, "video"),
            "ogg" | "opus" | "mp3" | "m4a" | "aac" | "wav" => (MediaType::Audio, "audio"),
            _ => (MediaType::Document, "document"),
        };

        let upload = match progress {
            Some(token) => self.upload_reporting(bytes.clone(), media_type, token).await?,
            None => self.client.upload(bytes.clone(), media_type, Default::default()).await?,
        };

        let mimetype = mime_for(&extension).map(str::to_string);

        // A thumbnail lets the recipient see a preview before the file lands.
        let thumb = media_thumbnail(kind, &bytes);
        let warning = (kind == "video" || kind == "gif")
            .then(|| thumb.is_none())
            .filter(|missing| *missing)
            .map(|_| {
                if cfg!(windows) {
                    "The video was sent without a preview because Windows could not decode it."
                } else {
                    "The video was sent without a preview because ffmpeg is not installed."
                }
                .to_string()
            });

        // An attachment can carry a quote, the same as a text reply.
        let context = match &reply {
            Some((id, sender, text)) => {
                use whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info;
                let sender: Jid = sender.parse::<Jid>()?.to_non_ad();
                let quoted = wa::Message::text(text.clone());
                Some(Box::new(build_quote_context_with_info(
                    id, &sender, &to, &to, &quoted,
                )))
            }
            None => None,
        };
        let context = if forwarded { Some(forwarded_context(context)) } else { context };
        let context = if mentions.is_empty() {
            context
        } else {
            let mut context = context.unwrap_or_default();
            context.mentioned_jid = mentions;
            Some(context)
        };

        let mut message = match kind {
            "image" => media::image_message(
                upload,
                ImageOptions {
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    context_info: context,
                    ..Default::default()
                },
            ),
            "video" => media::video_message(
                upload,
                VideoOptions {
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    gif_playback: gif.then_some(true),
                    context_info: context,
                    ..Default::default()
                },
            ),
            "audio" => media::audio_message(
                upload,
                AudioOptions {
                    mimetype: if voice.is_some() { Some("audio/ogg; codecs=opus".into()) } else { mimetype },
                    // An ogg/opus attachment is a voice note, which is how
                    // WhatsApp records and replays them.
                    ptt: Some(extension == "ogg"),
                    duration_seconds: voice.as_ref().map(|v| v.seconds),
                    waveform: voice.map(|v| v.waveform),
                    context_info: context,
                },
            ),
            _ => media::document_message(
                upload,
                DocumentOptions {
                    file_name: Some(file_name.clone()),
                    caption: caption.clone(),
                    mimetype,
                    jpeg_thumbnail: thumb.clone(),
                    context_info: context,
                    ..Default::default()
                },
            ),
        };
        if view_once {
            if let Some(m) = message.image_message.as_option_mut() {
                m.view_once = Some(true);
            }
            if let Some(m) = message.video_message.as_option_mut() {
                m.view_once = Some(true);
            }
            if let Some(m) = message.audio_message.as_option_mut() {
                m.view_once = Some(true);
            }
        }

        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id)?;
        }
        // The sender cannot reopen view-once media either, so no copy is kept.
        if view_once {
            self.store.set_view_once(chat, &result.message_id, true)?;
        }

        // Keep our own copy so the sender sees what they sent.
        let mut stored_path = None;
        if let Some(dir) = self.media_dir().filter(|_| !view_once) {
            let dir = &dir;
            if std::fs::create_dir_all(dir).is_ok() {
                let name = if extension.is_empty() { "bin".to_string() } else { extension.clone() };
                let dest = dir.join(format!("{}.{}", result.message_id, name));
                if std::fs::write(&dest, &bytes).is_ok() {
                    stored_path = Some(dest.to_string_lossy().to_string());
                }
            }
        }

        let stored = StoredMessage {
            chat: chat.to_string(),
            id: result.message_id.clone(),
            sender: self.own_jid(),
            sender_name: None,
            timestamp: unix_now(),
            from_me: true,
            text: caption.unwrap_or_default(),
            media_kind: Some(if gif && kind == "video" { "gif" } else { kind }.to_string()),
            media_path: stored_path,
            media_thumb: None,
            media_ref: None,
            reply_to_id: reply.as_ref().map(|(id, _, _)| id.clone()),
            reply_to_text: reply.as_ref().map(|(_, _, text)| text.clone()),
            reply_to_sender: reply.as_ref().map(|(_, sender, _)| sender.clone()),
            reply_to_chat: None,
            reply_to_kind: None,
            reply_to_thumb: None,
            read: false,
            revoked: false,
            mentioned: false,
            preview_url: None,
            preview_title: None,
            preview_desc: None,
            preview_thumb: None,
            status: Some(if to_self { "delivered".into() } else { "pending".into() }),
        };
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
        Ok(warning)
    }

    /// Uploads media while reporting its progress as [`ServiceEvent::UploadProgress`], about once per percent.
    async fn upload_reporting(
        &self,
        bytes: Vec<u8>,
        media_type: MediaType,
        token: String,
    ) -> Result<whatsapp_rust::upload::UploadResponse> {
        use whatsapp_rust::wacore::upload::{encrypt_media_with_key_and_sidecar, EncryptedMediaInfo};
        let file_length = bytes.len() as u64;
        let enc = tokio::task::spawn_blocking(move || {
            encrypt_media_with_key_and_sidecar(&bytes, media_type, None, None)
        })
        .await??;
        let total = enc.data_to_upload.len() as u64;
        let events = self.events.clone();
        let last = Arc::new(AtomicU64::new(u64::MAX));
        let report: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |sent: u64| {
            let percent = sent.saturating_mul(100) / total.max(1);
            if last.swap(percent, Ordering::Relaxed) != percent {
                let _ = events.send(ServiceEvent::UploadProgress { token: token.clone(), sent, total });
            }
        });
        let source = ProgressSource { data: Arc::from(enc.data_to_upload), report };
        let info = EncryptedMediaInfo {
            media_key: enc.media_key,
            file_sha256: enc.file_sha256,
            file_enc_sha256: enc.file_enc_sha256,
            file_length,
            streaming_sidecar: enc.streaming_sidecar,
        };
        Ok(self.client.upload_stream(source, info, media_type).await?)
    }

    /// Sends a picture as a sticker (see [`sticker_webp`]).
    pub async fn send_sticker(&self, chat: &str, bytes: Vec<u8>) -> Result<()> {
        self.send_sticker_as(chat, bytes, false).await
    }

    /// Turns a picture into a sticker in the media folder without sending it.
    pub fn save_sticker(&self, bytes: &[u8]) -> Result<String> {
        let webp = sticker_webp(bytes)
            .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
        let dir = self
            .media_dir()
            .ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?
            .join("stickers");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("saved-{}.webp", unix_now()));
        std::fs::write(&path, webp)?;
        Ok(path.to_string_lossy().into_owned())
    }

    async fn send_sticker_as(&self, chat: &str, bytes: Vec<u8>, forwarded: bool) -> Result<()> {
        let to: Jid = chat.parse()?;
        let to_self = self.is_self_jid(&to);
        let webp = sticker_webp(&bytes)
            .ok_or_else(|| anyhow::anyhow!("that file is not an image we can turn into a sticker"))?;
        let upload = self
            .client
            .upload(webp.clone(), MediaType::Sticker, Default::default())
            .await?;
        let message = wa::Message {
            sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key.to_vec()),
                file_sha256: Some(upload.file_sha256.to_vec()),
                file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                file_length: Some(upload.file_length),
                media_key_timestamp: Some(upload.media_key_timestamp),
                mimetype: Some("image/webp".into()),
                width: Some(512),
                height: Some(512),
                context_info: if forwarded {
                    MessageField::some(*forwarded_context(None))
                } else {
                    MessageField::none()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = self.client.send_message(to, message).await?;
        if forwarded {
            self.store.set_forwarded(chat, &result.message_id)?;
        }

        let media_path = self.media_dir().and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            let dest = dir.join(format!("{}.webp", result.message_id));
            std::fs::write(&dest, &webp).ok()?;
            Some(dest.to_string_lossy().into_owned())
        });
        let stored = StoredMessage {
            chat: chat.to_string(),
            id: result.message_id.clone(),
            sender: self.own_jid(),
            sender_name: None,
            timestamp: unix_now(),
            from_me: true,
            text: "[sticker]".into(),
            media_kind: Some("sticker".into()),
            media_path,
            media_thumb: None,
            media_ref: None,
            reply_to_id: None,
            reply_to_text: None,
            reply_to_sender: None,
            reply_to_chat: None,
            reply_to_kind: None,
            reply_to_thumb: None,
            read: false,
            revoked: false,
            mentioned: false,
            preview_url: None,
            preview_title: None,
            preview_desc: None,
            preview_thumb: None,
            status: Some(if to_self { "delivered".into() } else { "pending".into() }),
        };
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::Message { message: Box::new(stored) });
        Ok(())
    }

    /// Stickers or GIFs already on this device, newest first, for the picker.
    /// Recent stickers or GIFs, newest first, one per distinct file. Copies of a
    /// path in `prefer` (the favourites) are dropped in its favour.
    pub fn media_library(&self, kind: &str, prefer: &[String]) -> Result<Vec<String>> {
        use std::hash::{Hash, Hasher};
        use std::io::Read;
        let recent = self.store.recent_media(kind, 200)?;
        let dir = self.media_dir().and_then(|d| std::fs::canonicalize(d).ok());
        // Favourites come from the UI, so only files in the media folder count.
        let preferred = prefer.iter().filter(|p| {
            dir.as_ref()
                .is_some_and(|d| std::fs::canonicalize(p).is_ok_and(|f| f.starts_with(d)))
        });
        // The same sticker sent or received again is a new file; show it once.
        // Length plus the first 64 KiB identifies it without reading whole videos.
        let mut seen = std::collections::HashSet::new();
        let mut listed = std::collections::HashSet::new();
        Ok(preferred
            .cloned()
            .chain(recent)
            .filter(|p| listed.insert(p.clone()))
            .filter(|p| {
                let Ok(file) = std::fs::File::open(p) else { return false };
                let length = file.metadata().map(|m| m.len()).unwrap_or(0);
                let mut head = Vec::new();
                if file.take(64 * 1024).read_to_end(&mut head).is_err() {
                    return false;
                }
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (length, head).hash(&mut hasher);
                seen.insert(hasher.finish())
            })
            .collect())
    }

    /// Re-sends a sticker or GIF from the media folder.
    pub async fn send_from_library(&self, chat: &str, path: &str, kind: &str) -> Result<()> {
        let dir = self.media_dir().ok_or_else(|| anyhow::anyhow!("no media folder is configured"))?;
        let file = std::fs::canonicalize(path)?;
        if !file.starts_with(std::fs::canonicalize(&dir)?) {
            anyhow::bail!("refusing to send a file from outside the media folder");
        }
        let bytes = std::fs::read(&file)?;
        match kind {
            "sticker" => self.send_sticker(chat, bytes).await,
            "gif" => {
                let options = SendOptions { gif: true, ..Default::default() };
                self.send_media(chat, "gif.mp4", bytes, None, None, options).await.map(|_| ())
            }
            _ => anyhow::bail!("only stickers and GIFs are sent from the library"),
        }
    }

    /// Stored messages for a chat, newest first.
    pub fn messages(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        self.store.messages_for(chat, limit)
    }

    /// Chat summaries, most recently active first.
    pub fn chats(&self) -> Result<Vec<crate::store::ChatSummary>> {
        self.store.chats()
    }

    /// Stops the background task.
    pub fn shutdown(&self) {
        if let Some(tx) = self.shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

/// A poll's question, its options, and whether more than one may be chosen.
fn poll_of(message: &wa::Message) -> Option<(String, Vec<String>, bool)> {
    let base = message.get_base_message();
    let poll = base
        .poll_creation_message
        .as_option()
        .or_else(|| base.poll_creation_message_v2.as_option())
        .or_else(|| base.poll_creation_message_v3.as_option())?;
    let options = poll.options.iter().filter_map(|o| o.option_name.clone()).collect();
    Some((
        poll.name.clone().unwrap_or_default(),
        options,
        poll.selectable_options_count.unwrap_or(0) != 1,
    ))
}

fn event_of(message: &wa::Message) -> Option<crate::store::NewEvent> {
    let event = message.get_base_message().event_message.as_option()?;
    Some(crate::store::NewEvent {
        name: event.name.clone().unwrap_or_default(),
        description: event.description.clone(),
        start: event.start_time,
        end: event.end_time,
        location: event
            .location
            .as_option()
            .and_then(|l| l.name.clone().or_else(|| l.address.clone())),
        link: event.join_link.clone(),
        canceled: event.is_canceled.unwrap_or(false),
    })
}

/// The per-message secret polls and events key their votes and RSVPs with.
fn message_secret(message: &wa::Message) -> Option<Vec<u8>> {
    let secret = |m: &wa::Message| {
        m.message_context_info
            .as_option()
            .and_then(|c| c.message_secret.clone())
    };
    secret(message).or_else(|| secret(message.get_base_message()))
}

/// Keeps a poll's or event's definition, which its later votes and RSVPs need.
fn remember_structures(store: &MessageStore, chat: &str, id: &str, creator: &str, message: &wa::Message) {
    let secret = message_secret(message);
    if let Some((name, options, multi)) = poll_of(message) {
        let _ = store.save_poll(chat, id, creator, &name, &options, multi, secret.as_deref());
    }
    if let Some(event) = event_of(message) {
        let _ = store.save_event(chat, id, creator, &event, secret.as_deref());
    }
}

fn response_name(response: Option<wa::message::event_response_message::EventResponseType>) -> &'static str {
    use wa::message::event_response_message::EventResponseType;
    match response {
        Some(EventResponseType::GOING) => "going",
        Some(EventResponseType::NOT_GOING) => "not_going",
        Some(EventResponseType::MAYBE) => "maybe",
        _ => "",
    }
}

/// The new group member tag, if this message sets one; empty clears it.
fn member_label_change(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::GROUP_MEMBER_LABEL_CHANGE) {
        return None;
    }
    Some(protocol.member_label.as_option()?.label.clone().unwrap_or_default())
}

/// The edited message's id and its new text (or caption), if this message is an edit.
fn edit_of(message: &wa::Message) -> Option<(String, String)> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::MESSAGE_EDIT) {
        return None;
    }
    let target = protocol.key.as_option()?.id.clone().filter(|id| !id.is_empty())?;
    let edited = protocol.edited_message.as_option()?;
    let text = edited.text_content().or_else(|| edited.get_caption())?.to_string();
    Some((target, text))
}

/// The id of the message a revoke refers to, if this message is a revoke.
fn revoke_target(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::REVOKE) {
        return None;
    }
    let key = protocol.key.as_option()?;
    key.id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

/// Extracts the quoted message id and text from an incoming message.
///
/// `context_info` lives on each inner message type rather than on `Message`
/// itself, so the carriers a reply can arrive on are checked in turn.
fn quote_of(message: &wa::Message) -> Option<(String, String, String, String, Option<Vec<u8>>, Option<String>)> {
    let base = message.get_base_message();
    let context = base
        .extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            base.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })?;

    let id = context.stanza_id.as_ref()?.to_string();
    let author = context
        .participant
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let quoted = context.quoted_message.as_option()?;
    // A quoted media message has no text, so name its type instead of saying
    // "media". The caption, when there is one, wins.
    let (kind, name) = if quoted.image_message.as_option().is_some() {
        ("image", None)
    } else if quoted.video_message.as_option().is_some() {
        ("video", None)
    } else if quoted.audio_message.as_option().is_some() {
        ("audio", None)
    } else if let Some(document) = quoted.document_message.as_option() {
        ("document", document.file_name.clone())
    } else {
        ("", None)
    };
    let text = quoted
        .text_content()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .unwrap_or_else(|| match kind {
            "image" => "Photo".to_string(),
            "video" => "Video".to_string(),
            "audio" => "Voice message".to_string(),
            "document" => name.unwrap_or_else(|| "Document".to_string()),
            _ => "[media]".to_string(),
        });
    let thumb = quoted
        .image_message
        .as_option()
        .and_then(|m| m.jpeg_thumbnail.clone())
        .or_else(|| {
            quoted
                .video_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        })
        .or_else(|| {
            quoted
                .document_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        });
    Some((id, author, text, kind.to_string(), thumb, context.remote_jid.clone()))
}

/// The user part of a JID, without the device suffix or server.
fn user_part(jid: &str) -> String {
    jid.split('@')
        .next()
        .unwrap_or(jid)
        .split(':')
        .next()
        .unwrap_or(jid)
        .to_string()
}

/// Seconds since the Unix epoch.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The media carried by a message, if any.
struct MediaInfo {
    /// `image`, `video`, `gif`, `audio` or `document`. A video sent with
    /// `gifPlayback` is a GIF, which WhatsApp keeps as a muted looping clip.
    kind: &'static str,
    media_type: MediaType,
    /// The four media protos are distinct types that each implement
    /// `Downloadable`.
    downloadable: Box<dyn Downloadable + Send + Sync>,
    /// The small JPEG the message carries, available without downloading.
    thumb: Option<Vec<u8>>,
    /// A document's own extension, so the file opens as what it is.
    ext: Option<String>,
}

impl MediaInfo {
    fn extension(&self) -> String {
        self.ext.clone().unwrap_or_else(|| extension_for(self.kind, self.media_type).to_string())
    }
}

/// A safe file extension from a document's name, or its MIME type for an SVG.
fn document_extension(document: &wa::message::DocumentMessage) -> Option<String> {
    let from_name = document
        .file_name
        .as_deref()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    from_name.or_else(|| (document.mimetype.as_deref() == Some("image/svg+xml")).then(|| "svg".to_string()))
}

fn detect_media(message: &wa::Message) -> Option<MediaInfo> {
    if let Some(image) = message.image_message.as_option() {
        return Some(MediaInfo {
            kind: "image",
            media_type: MediaType::Image,
            downloadable: Box::new(image.clone()),
            thumb: image.jpeg_thumbnail.clone(),
            ext: None,
        });
    }
    if let Some(video) = message.video_message.as_option() {
        let kind = if video.gif_playback.unwrap_or(false) {
            "gif"
        } else {
            "video"
        };
        return Some(MediaInfo {
            kind,
            media_type: MediaType::Video,
            downloadable: Box::new(video.clone()),
            thumb: video.jpeg_thumbnail.clone(),
            ext: None,
        });
    }
    if let Some(audio) = message.audio_message.as_option() {
        return Some(MediaInfo {
            kind: "audio",
            media_type: MediaType::Audio,
            downloadable: Box::new(audio.clone()),
            thumb: None,
            ext: None,
        });
    }
    if let Some(document) = message.document_message.as_option() {
        return Some(MediaInfo {
            kind: "document",
            media_type: MediaType::Document,
            downloadable: Box::new(document.clone()),
            thumb: document.jpeg_thumbnail.clone(),
            ext: document_extension(document),
        });
    }
    if let Some(sticker) = message.sticker_message.as_option() {
        return Some(MediaInfo {
            kind: "sticker",
            media_type: MediaType::Sticker,
            downloadable: Box::new(sticker.clone()),
            thumb: None,
            ext: None,
        });
    }
    None
}

/// Any picture as a WhatsApp sticker: fitted into 512×512 on transparency, as
/// WebP. A WebP is sent unchanged, so an animated sticker stays animated.
fn sticker_webp(bytes: &[u8]) -> Option<Vec<u8>> {
    if image::guess_format(bytes).ok()? == image::ImageFormat::WebP {
        return Some(bytes.to_vec());
    }
    let fitted = image::load_from_memory(bytes).ok()?.thumbnail(512, 512).to_rgba8();
    let mut canvas = image::RgbaImage::new(512, 512);
    let (x, y) = ((512 - fitted.width()) / 2, (512 - fitted.height()) / 2);
    image::imageops::overlay(&mut canvas, &fitted, x.into(), y.into());
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::WebP)
        .ok()?;
    Some(out)
}

/// Converts a protocol message into a storable one, downloading any media.
///
/// Returns `None` for messages that carry neither text nor media, so protocol
/// traffic does not fill the store with empty rows.
async fn incoming_message(
    inbound: &InboundMessage,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let info = &inbound.info;
    let envelope = Envelope {
        chat: info.source.chat.to_string(),
        id: info.id.to_string(),
        sender: info.source.sender.to_string(),
        timestamp: info.timestamp.timestamp(),
        from_me: info.source.is_from_me,
    };
    stored_message(&inbound.message, envelope, client, media_dir, auto_download).await
}

/// Where a chat's cached profile picture lives.
fn avatar_path(media_dir: &Path, jid: &str) -> PathBuf {
    let name: String = jid
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    media_dir.join("avatars").join(format!("{name}.jpg"))
}

/// Where a message came from, whether it arrived live or through history sync.
struct Envelope {
    chat: String,
    id: String,
    sender: String,
    timestamp: i64,
    from_me: bool,
}

async fn stored_message(
    message: &wa::Message,
    envelope: Envelope,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let id = envelope.id.as_str();
    let mut text = message.text_content().unwrap_or_default().to_string();

    let mut media_kind = None;
    let mut media_path = None;
    let mut media_thumb = None;
    let mut media_ref = None;

    if let Some(media) = detect_media(message) {
        media_kind = Some(media.kind.to_string());

        // The thumbnail rides in the message, so it is kept even when the file
        // itself is not downloaded.
        if let (Some(dir), Some(bytes)) = (media_dir, media.thumb.as_ref()) {
            if std::fs::create_dir_all(dir).is_ok() {
                let path = dir.join(format!("{id}_thumb.jpg"));
                if std::fs::write(&path, bytes).is_ok() {
                    media_thumb = Some(path.to_string_lossy().to_string());
                }
            }
        }

        if auto_download {
            // Download when a destination and a client are available. A failure
            // still records the message, so the text and metadata are not lost.
            if let (Some(client), Some(dir)) = (client, media_dir) {
                match client.download(media.downloadable.as_ref()).await {
                    Ok(bytes) => {
                        if std::fs::create_dir_all(dir).is_ok() {
                            let path = dir.join(format!("{}.{}", id, media.extension()));
                            if std::fs::write(&path, &bytes).is_ok() {
                                media_path = Some(path.to_string_lossy().to_string());
                            }
                        }
                    }
                    Err(e) => log::warn!("failed to download {id} media: {e}"),
                }
            }
        } else {
            // Keep the message so the file can be fetched on demand later.
            media_ref = Some(buffa::Message::encode_to_vec(message));
        }

        if text.is_empty() {
            text = format!("[{}]", media.kind);
        }
    }

    // Polls and events render as cards; the row carries their title.
    if media_kind.is_none() {
        if let Some((question, _, _)) = poll_of(message) {
            text = question;
            media_kind = Some("poll".to_string());
        } else if let Some(event) = event_of(message) {
            text = event.name;
            media_kind = Some("event".to_string());
        }
    }

    if text.is_empty() && media_kind.is_none() {
        return None;
    }

    // A reply carries the quote in the message context. We do not keep the
    // original protobuf, so the text is copied out for display.
    let (
        reply_to_id,
        reply_to_text,
        reply_to_sender,
        reply_to_kind,
        reply_to_thumb,
        reply_to_chat,
    ) = quote_of(message)
            .map(|(id, sender, text, kind, thumb, chat)| {
                // Quoting our own message should read "You", not our phone number.
                let mine = client
                    .map(|c| {
                        [c.pn(), c.lid()]
                            .into_iter()
                            .flatten()
                            .any(|j| j.to_non_ad().to_string() == sender)
                    })
                    .unwrap_or(false);
                let sender = if mine { "@me".to_string() } else { sender };
                // Keep the quoted thumbnail so the quote shows a preview.
                let thumb_path = thumb.and_then(|bytes| {
                    let dir = media_dir?;
                    std::fs::create_dir_all(dir).ok()?;
                    let path = dir.join(format!("{id}_quote.jpg"));
                    std::fs::write(&path, bytes).ok()?;
                    Some(path.to_string_lossy().to_string())
                });
                (
                    Some(id),
                    Some(text),
                    Some(sender),
                    if kind.is_empty() { None } else { Some(kind) },
                    thumb_path,
                    chat,
                )
            })
            .unwrap_or((None, None, None, None, None, None));

    // A link preview rides on the extended text message.
    let (preview_url, preview_title, preview_desc, preview_thumb) =
        link_preview(message, media_dir, id);

    Some(StoredMessage {
        chat: envelope.chat,
        id: envelope.id.clone(),
        sender: envelope.sender,
        // Names are resolved separately and joined by the store on read.
        sender_name: None,
        timestamp: envelope.timestamp,
        from_me: envelope.from_me,
        text,
        media_kind,
        media_path,
        media_thumb,
        media_ref,
        reply_to_id,
        reply_to_text,
        reply_to_sender,
        reply_to_chat,
        reply_to_kind,
        reply_to_thumb,
        // Newly arrived, so unseen until the chat is opened.
        read: false,
        revoked: false,
        mentioned: false,
        preview_url,
        preview_title,
        preview_desc,
        preview_thumb,
        status: None,
    })
}

/// The link preview a message carries, with its thumbnail written next to the
/// other media so the UI can show it.
fn link_preview(
    message: &wa::Message,
    media_dir: Option<&std::path::Path>,
    id: &str,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let Some(text) = message
        .get_base_message()
        .extended_text_message
        .as_option()
    else {
        return (None, None, None, None);
    };
    // `matched_text` is the URL as it appeared in the message.
    let Some(url) = text.matched_text.clone() else {
        return (None, None, None, None);
    };
    let thumb = text.jpeg_thumbnail.as_ref().and_then(|bytes| {
        let dir = media_dir?;
        std::fs::create_dir_all(dir).ok()?;
        let path = dir.join(format!("{id}_thumb.jpg"));
        std::fs::write(&path, bytes).ok()?;
        Some(path.to_string_lossy().to_string())
    });
    (Some(url), text.title.clone(), text.description.clone(), thumb)
}

/// File extension for a downloaded media item.
fn extension_for(kind: &str, media_type: MediaType) -> &'static str {
    match kind {
        "image" => "jpg",
        "video" | "gif" => "mp4",
        "audio" => "ogg",
        "sticker" => "webp",
        "document" => "bin",
        _ => match media_type {
            MediaType::Image => "jpg",
            _ => "bin",
        },
    }
}

/// Copies address-book names onto the LID form of the same address.
///
/// The address book is keyed by phone number, while messages in an
/// LID-addressed chat carry the LID. The library's own mapping table bridges
/// the two, so names already learned apply to existing history instead of only
/// to messages that arrive after this point.
fn backfill_lid_names(session_path: &std::path::Path, store: &MessageStore) {
    use std::collections::HashMap;

    let Ok(saved) = store.saved_names() else {
        return;
    };
    let by_phone: HashMap<&str, &str> = saved
        .iter()
        .filter_map(|(jid, name)| {
            jid.strip_suffix("@s.whatsapp.net")
                .map(|phone| (phone, name.as_str()))
        })
        .collect();
    if by_phone.is_empty() {
        return;
    }

    let Ok(conn) = rusqlite::Connection::open_with_flags(
        session_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return;
    };
    let mut by_lid: HashMap<String, String> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT lid, phone_number FROM lid_pn_mapping") {
        if let Ok(rows) = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        {
            for (lid, phone) in rows.flatten() {
                by_lid.insert(lid, phone);
            }
        }
    }

    for address in store.known_addresses().unwrap_or_default() {
        let Some((user, server)) = address.split_once('@') else {
            continue;
        };
        if server != "lid" {
            continue;
        }
        let bare = user.split(':').next().unwrap_or(user);
        let Some(phone) = by_lid.get(bare) else {
            continue;
        };
        match by_phone.get(phone.as_str()) {
            Some(name) => {
                let _ = store.set_saved_name(&address, name);
                let _ = store.set_saved_name(&format!("{bare}@lid"), name);
            }
            // Without a saved name, show the phone number instead of the LID,
            // which nobody can read.
            None => {
                let _ = store.set_name(&address, phone);
                let _ = store.set_name(&format!("{bare}@lid"), phone);
            }
        }
    }
}

/// The JIDs a message mentions, from whichever message type carries them.
fn message_context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    base.extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            base.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
}

/// Whether a message mentions us: directly, or everyone through @all.
fn mentions_me(message: &wa::Message, own: &[String]) -> bool {
    let Some(context) = message_context(message) else {
        return false;
    };
    if !context.group_mentions.is_empty() {
        return true;
    }
    context.mentioned_jid.iter().any(|mention| {
        let bare = mention.split(':').next().unwrap_or(mention);
        own.iter().any(|me| me == mention || me == bare)
    })
}

/// A link preview fetched from a URL.
struct LinkPreview {
    url: String,
    title: Option<String>,
    description: Option<String>,
    thumbnail: Option<Vec<u8>>,
}

/// The first http(s) URL in a piece of text.
fn first_url(text: &str) -> Option<String> {
    let start = text.find("http://").or_else(|| text.find("https://"))?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Fetches the Open Graph metadata for a URL, and its image when it is a JPEG.
///
/// Blocking; call it from `spawn_blocking`. The image is only attached when it
/// is already a JPEG and reasonably small, since WhatsApp's thumbnail field
/// takes JPEG bytes and we do not re-encode here.
fn fetch_link_preview(url: &str) -> Option<LinkPreview> {
    let mut response = ureq::get(url).call().ok()?;
    let html = response.body_mut().read_to_string().ok()?;

    let meta = |property: &str| -> Option<String> {
        let needle = format!("property=\"{property}\"");
        let rest = &html[html.find(&needle)?..];
        let content_at = rest.find("content=\"")? + "content=\"".len();
        let value = &rest[content_at..];
        Some(value[..value.find('"')?].to_string())
    };

    let image = meta("og:image").filter(|src| src.starts_with("http"));
    let thumbnail = image.and_then(|src| {
        let mut response = ureq::get(&src).call().ok()?;
        let bytes = response.body_mut().read_to_vec().ok()?;
        let is_jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
        (is_jpeg && bytes.len() < 300_000).then_some(bytes)
    });

    Some(LinkPreview {
        url: url.to_string(),
        title: meta("og:title"),
        description: meta("og:description"),
        thumbnail,
    })
}

/// A small JPEG preview for an outgoing attachment.
///
/// Images are downscaled locally. Video needs a decoder, so it is best effort:
/// Media Foundation on Windows, ffmpeg elsewhere when present, and `None`
/// means the file goes without a preview.
fn media_thumbnail(kind: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    match kind {
        "image" => image_thumbnail(bytes),
        "video" | "gif" => video_thumbnail(bytes),
        _ => None,
    }
}

fn image_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    jpeg_thumbnail(image::load_from_memory(bytes).ok()?)
}

/// A centred square crop, at most `size` pixels a side, as JPEG.
fn square_jpeg(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let side = image.width().min(image.height());
    let square = image
        .crop_imm((image.width() - side) / 2, (image.height() - side) / 2, side, side)
        .resize_exact(side.min(size), side.min(size), image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(square.to_rgb8())
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

fn jpeg_thumbnail(image: image::DynamicImage) -> Option<Vec<u8>> {
    let thumb = image.thumbnail(256, 256);
    let mut out = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}

#[cfg(windows)]
fn video_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::{
        Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_NOSOCKET, MF_VERSION},
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
    };

    // COM is initialised per thread, so decode on a thread of our own rather
    // than on a runtime worker.
    let frame = std::thread::scope(|scope| {
        scope
            .spawn(|| unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
                let frame = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).ok().and_then(|()| {
                    let frame = first_frame(bytes);
                    let _ = MFShutdown();
                    frame
                });
                CoUninitialize();
                frame
            })
            .join()
            .ok()
            .flatten()
    })?;
    jpeg_thumbnail(image::DynamicImage::ImageRgb8(frame))
}

/// The first decodable video frame, cropped to its visible area.
///
/// Must run between `MFStartup` and `MFShutdown` on a COM thread.
#[cfg(windows)]
unsafe fn first_frame(bytes: &[u8]) -> Option<image::RgbImage> {
    use windows::Win32::{Media::MediaFoundation::*, UI::Shell::SHCreateMemStream};

    let stream = MFCreateMFByteStreamOnStream(&SHCreateMemStream(Some(bytes))?).ok()?;
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 1).ok()?;
    let attributes = attributes?;
    // Lets the reader convert whatever the decoder emits to RGB32.
    attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
    let reader = MFCreateSourceReaderFromByteStream(&stream, &attributes).ok()?;

    let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).ok()?;
    reader.SetStreamSelection(video, true).ok()?;
    let wanted = MFCreateMediaType().ok()?;
    wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
    wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
    reader.SetCurrentMediaType(video, None, &wanted).ok()?;

    let mut sample: Option<IMFSample> = None;
    for _ in 0..64 {
        let mut flags = 0u32;
        reader
            .ReadSample(video, 0, None, Some(&mut flags as *mut _), None, Some(&mut sample as *mut _))
            .ok()?;
        if sample.is_some() || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
    }
    let buffer = sample?.ConvertToContiguousBuffer().ok()?;

    // Read after the first sample: the decoder only settles the frame size then.
    let format = reader.GetCurrentMediaType(video).ok()?;
    let size = format.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
    let (width, height) = ((size >> 32) as u32, size as u32);
    let stride = format
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|s| s as i32)
        .unwrap_or(width as i32 * 4);
    // Decoders pad to whole macroblocks (1080 rows become 1088); the aperture is
    // the picture. MFVideoArea: two MFOffset { fract: u16, value: i16 }, then SIZE.
    let mut area = [0u8; 16];
    let (left, top, visible_w, visible_h) = format
        .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None)
        .ok()
        .map(|()| {
            (
                i16::from_le_bytes([area[2], area[3]]).max(0) as u32,
                i16::from_le_bytes([area[6], area[7]]).max(0) as u32,
                i32::from_le_bytes([area[8], area[9], area[10], area[11]]).max(0) as u32,
                i32::from_le_bytes([area[12], area[13], area[14], area[15]]).max(0) as u32,
            )
        })
        .filter(|&(x, y, w, h)| w > 0 && h > 0 && x + w <= width && y + h <= height)
        .unwrap_or((0, 0, width, height));

    let mut data: *mut u8 = std::ptr::null_mut();
    let mut len = 0u32;
    buffer.Lock(&mut data, None, Some(&mut len as *mut _)).ok()?;
    let pixels = std::slice::from_raw_parts(data, len as usize);
    let row = stride.unsigned_abs() as usize;
    let frame = (width > 0 && row >= width as usize * 4 && pixels.len() >= row * height as usize)
        .then(|| {
            image::RgbImage::from_fn(visible_w, visible_h, |x, y| {
                let y = top + y;
                // A negative stride means the rows are stored bottom-up.
                let y = (if stride < 0 { height - 1 - y } else { y }) as usize;
                let i = y * row + (left + x) as usize * 4;
                // RGB32 is BGRX in memory.
                image::Rgb([pixels[i + 2], pixels[i + 1], pixels[i]])
            })
        });
    let _ = buffer.Unlock();
    frame
}

#[cfg(not(windows))]
fn video_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("ffmpeg")
        .args([
            "-loglevel", "error",
            "-i", "pipe:0",
            "-frames:v", "1",
            "-vf", "scale=256:-2",
            "-f", "mjpeg",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Fed from another thread while stdout drains: ffmpeg stops reading after
    // the first frame, so a write can fail with a broken pipe after the frame
    // is out, or block while ffmpeg waits on a full stdout pipe.
    let mut stdin = child.stdin.take()?;
    let output = std::thread::scope(|scope| {
        scope.spawn(move || {
            let _ = stdin.write_all(bytes);
        });
        child.wait_with_output()
    })
    .ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}

/// MIME type for an outgoing attachment, from its file extension.
fn mime_for(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" | "mov" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vacuum_waits_for_a_large_freelist_and_a_week() {
        let week = 7 * 86_400;
        assert!(should_vacuum(5_000, 10_000, week));
        assert!(!should_vacuum(5_000, 10_000, week - 1));
        assert!(!should_vacuum(900, 1_000, week));
        assert!(!should_vacuum(1_500, 100_000, week));
    }

    #[test]
    fn events_serialize_for_the_ui() {
        // Regression guard: these are emitted with `app.emit`, which fails
        // silently for a shape serde cannot represent.
        for event in [
            ServiceEvent::QrCode { code: "2@abc".into() },
            ServiceEvent::Connected,
            ServiceEvent::Disconnected,
            ServiceEvent::RetentionApplied { removed: 3 },
            ServiceEvent::NamesUpdated { count: 2 },
            ServiceEvent::Syncing { pending: 5 },
            ServiceEvent::Synced,
        ] {
            let json = serde_json::to_string(&event).expect("event must serialize");
            assert!(json.contains("\"kind\""), "missing tag: {json}");
        }

        let message = ServiceEvent::Message {
            message: Box::new(StoredMessage {
                chat: "a@s".into(),
                id: "1".into(),
                sender: "b@s".into(),
                sender_name: None,
                timestamp: 0,
                from_me: false,
                text: "hi".into(),
                media_kind: None,
                media_path: None,
                media_thumb: None,
                media_ref: None,
                reply_to_id: None,
                reply_to_text: None,
                reply_to_sender: None,
                reply_to_chat: None,
                reply_to_kind: None,
                reply_to_thumb: None,
                read: false,
                revoked: false,
                mentioned: false,
                preview_url: None,
                preview_title: None,
                preview_desc: None,
                preview_thumb: None,
                status: None,
            }),
        };
        let json = serde_json::to_string(&message).expect("message event must serialize");
        assert!(json.contains("\"message\""), "missing payload: {json}");
    }

    #[test]
    fn default_config_targets_its_data_dir() {
        let c = ServiceConfig::under("/tmp/example");
        assert_eq!(c.session_path, Path::new("/tmp/example").join("session.db"));
        assert_eq!(c.messages_path, Path::new("/tmp/example").join("messages.db"));
        assert_eq!(c.retention, Retention::default());
        assert!(!c.accept_full_history);
    }

    #[test]
    fn default_retention_is_bounded() {
        // The whole point of the rewrite: the default must not be unbounded.
        let r = Retention::default();
        assert!(r.max_age_hours.is_some());
        assert!(r.max_messages_per_chat.is_some());
    }

    #[test]
    fn secret_horizon_tracks_the_message_window() {
        // Keys must not outlive the messages they belong to, or the session
        // database grows far beyond the history we actually keep.
        let config = cache_config_for(&Retention {
            max_age_hours: Some(24),
            max_messages_per_chat: None,
        });
        let day = Duration::from_secs(24 * 3600);
        assert!(config.msg_secret_retention.text < day * 2);
        assert!(config.msg_secret_retention.poll_event < day * 2);
    }

    #[test]
    fn secret_horizon_has_a_floor() {
        // An edit can arrive shortly after its parent, so the horizon must not
        // collapse to zero for a very short retention window.
        let config = cache_config_for(&Retention {
            max_age_hours: Some(0),
            max_messages_per_chat: None,
        });
        assert!(config.msg_secret_retention.text >= Duration::from_secs(3600));
    }

    #[test]
    fn unlimited_retention_falls_back_to_the_library_default() {
        let config = cache_config_for(&Retention::unlimited());
        assert_eq!(
            config.msg_secret_retention.text,
            Duration::from_secs(30 * 86_400)
        );
    }
}
