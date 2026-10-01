//! Message and chat storage.
//!
//! The repository owns persisted rows; disk pruning is a separate policy.

use std::{path::Path, sync::Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

mod chats;
mod schema;
mod marks;
mod media;
mod messages;
mod quick_switcher;
mod unavailable;
use unavailable::VISIBLE_MESSAGE_SQL;
pub mod scheduled;
mod group_history;
pub mod contact_identity;
pub(crate) mod favorites;
pub mod gallery;
pub mod links;
pub(crate) mod pins;
pub(crate) mod history_pins;
mod secret_edits;
pub(crate) use secret_edits::{PollOption, PollEdit, EventEdit, SecretEdit, EditRevision};
pub mod transcription;
pub mod media_policy;
mod notification_prefs;
mod chat_unarchive;
mod group_create;
mod paging;
pub use paging::{MessageCursor, MessagePage, MessagePageDirection, MAX_MESSAGE_PAGE};
mod names;
mod receipts;
mod retention;
pub use retention::{DiskRetention, DiskRetentionManager};
mod limits;
mod storage;
pub mod archive;
mod stickers;
pub use stickers::{Sticker, StickerPack};
mod worker;
pub(crate) use worker::{StoreWorker, AliasWorker};
pub use limits::RetentionLimit;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod history_floor_tests;

/// A number standing in for a name: bare digits, or a `+`-prefixed phone label
/// such as WhatsApp's masked `+598∙∙∙∙∙27`. Never a real contact or push name.
pub fn is_placeholder_name(name: &str) -> bool {
    let name = name.trim();
    name.trim_start_matches('+').chars().all(|c| c.is_ascii_digit())
        || (name.starts_with('+') && !name.chars().any(char::is_alphabetic))
}

/// A stored message, as rows are read and as the UI receives it.
///
/// Each concern is its own type; they serialize flattened, so the IPC shape
/// stays one flat object with the column names as keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StoredMessage {
    #[serde(default)]
    pub spoiler: bool,
    #[serde(skip)]
    pub history_shareable: bool,
    #[serde(flatten)]
    pub header: MessageHeader,
    /// Resolved from `names` when read; never stored on the row.
    pub sender_name: Option<String>,
    pub text: String,
    #[serde(flatten)]
    pub media: Media,
    #[serde(flatten)]
    pub quote: Quote,
    #[serde(flatten)]
    pub link: LinkCard,
    #[serde(flatten)]
    pub local: LocalState,
    #[serde(flatten)]
    pub system: SystemNotice,
    /// The last position of a live location, updated in place as edits arrive.
    pub live_location: Option<LiveLocation>,
}

/// A live location share as last seen: the position, its accuracy and the
/// update order, so late or replayed updates never move it backwards.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct LiveLocation {
    pub lat: f64,
    pub lng: f64,
    /// The sender's own accuracy estimate, in metres.
    pub accuracy: Option<u32>,
    /// Movement speed in metres per second.
    pub speed: Option<f32>,
    /// Travel direction, degrees clockwise from magnetic north.
    pub heading: Option<u32>,
    /// The sender's update counter; higher is newer.
    pub sequence: Option<i64>,
    /// When the share started (the first message's timestamp).
    pub started_at: i64,
    /// When the last position was received.
    pub updated_at: i64,
    /// When the share is expected to end, when the message carried one.
    pub expires_at: Option<i64>,
    /// Stopped by the sender or expired; the last position is kept.
    pub ended: bool,
}

/// A system line (group change, security notice, …) instead of a message; empty for messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct SystemNotice {
    /// The protocol's stub type name, such as `E2E_IDENTITY_CHANGED`.
    #[serde(rename = "system_kind")]
    pub kind: Option<String>,
    /// The stub's parameters, usually the JIDs it is about.
    #[serde(rename = "system_params")]
    pub params: Vec<String>,
}

/// Where a message lives, who sent it and when.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageHeader {
    pub chat: String,
    pub id: String,
    pub sender: String,
    pub timestamp: i64,
    pub from_me: bool,
}

/// The media a message carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Media {
    /// `image`, `video`, `audio`, `document`, `sticker`, `gif`, `poll`, `event`…
    #[serde(rename = "media_kind")]
    pub kind: Option<String>,
    /// Absolute path to the downloaded media, if it was kept.
    #[serde(rename = "media_path")]
    pub path: Option<String>,
    /// The media's thumbnail, embedded in the message and available without
    /// downloading the full file: a `data:` URI for received media (a few KB
    /// in the row), a file path for older rows.
    #[serde(rename = "media_thumb")]
    pub thumb: Option<String>,
    /// Audio/voice-note length in seconds, when the message carries it. Lets
    /// the bubble show the time before the file is decoded or played.
    #[serde(rename = "media_duration")]
    pub duration: Option<u32>,
    /// The media submessage, kept so the file can be downloaded on demand when
    /// automatic downloads are off: keys, hashes and URL, without thumbnail or
    /// quote, typically a few hundred bytes. Internal: not handed to the UI.
    #[serde(skip)]
    pub locator: Option<Vec<u8>>,
    /// What this media was before `kind` was rewritten to `view_once`: the kind
    /// is what decides which player a recovered photo, video or voice note needs.
    #[serde(rename = "media_once_kind")]
    pub once_kind: Option<String>,
}

/// The message a reply quotes, copied so the quote renders without a lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Quote {
    #[serde(rename = "reply_to_id")]
    pub id: Option<String>,
    #[serde(rename = "reply_to_text")]
    pub text: Option<String>,
    #[serde(rename = "reply_to_sender")]
    pub sender: Option<String>,
    /// Chat the quoted message lives in. Different from this chat for a private
    /// reply, which is a direct message quoting a group message.
    #[serde(rename = "reply_to_chat")]
    pub chat: Option<String>,
    /// Media kind of the quoted message, when it carried media.
    #[serde(rename = "reply_to_kind")]
    pub kind: Option<String>,
    /// The quoted media's thumbnail, when one was available.
    #[serde(rename = "reply_to_thumb")]
    pub thumb: Option<String>,
    /// The quoted message was view-once. A linked device never gets that media
    /// any other way, so a reply quoting one is the only copy it will see.
    #[serde(rename = "reply_to_view_once")]
    pub view_once: bool,
    /// Whether this account may take the copy a reply quotes. Not a view-once:
    /// anyone may, as its media is an ordinary message of its own. A view-once:
    /// only its author may, since the media is in the reply but was not sent to
    /// anyone else.
    #[serde(rename = "reply_to_recoverable")]
    pub recoverable: bool,
    /// Where a recovered copy was written. Named after the quoted message, so
    /// every reply quoting the same view-once shares one file.
    #[serde(rename = "reply_to_path")]
    pub path: Option<String>,
    /// The quoted media submessage, so the copy can be fetched on demand.
    /// Internal: not handed to the UI.
    #[serde(skip)]
    pub locator: Option<Vec<u8>>,
}

/// A link preview: canonical URL, title, description and thumbnail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct LinkCard {
    #[serde(rename = "preview_url")]
    pub url: Option<String>,
    #[serde(rename = "preview_title")]
    pub title: Option<String>,
    #[serde(rename = "preview_desc")]
    pub desc: Option<String>,
    #[serde(rename = "preview_thumb")]
    pub thumb: Option<String>,
    /// Site name; empty for received links, whose preview does not carry it.
    #[serde(rename = "preview_site")]
    pub site: Option<String>,
    /// The page's theme colour, for the embed's side bar.
    #[serde(rename = "preview_color")]
    pub color: Option<String>,
}

/// What this device knows about a message beyond its content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct LocalState {
    /// Persistent first-seen order for messages sharing a wire timestamp.
    #[serde(default)]
    pub sort_order: i64,
    /// Whether the user has seen this message.
    pub read: bool,
    /// Whether the sender deleted the message for everyone.
    pub revoked: bool,
    /// Deleted by the user on this device only. The row is kept and shown
    /// greyed out; nothing about it leaves this computer.
    #[serde(default)]
    pub deleted: bool,
    /// Whether the message mentions us (directly or via @all).
    pub mentioned: bool,
    /// Delivery state of a message we sent: `pending`, `sent`, `delivered` or
    /// `read`. `None` for incoming messages.
    pub status: Option<String>,
}

/// A chat summary derived from stored messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatSummary {
    pub chat: String,
    /// Resolved display name, when one has been learned.
    pub display_name: Option<String>,
    pub last_message_at: i64,
    pub last_text: String,
    /// Whether the last message was sent by the account owner.
    pub last_from_me: bool,
    /// Resolved name of the last message's sender, when known.
    pub last_sender_name: Option<String>,
    /// The last message's sender, for when no name is known.
    pub last_sender: String,
    /// What the last message carried (`image`, `video`, …), if not only text.
    pub last_media_kind: Option<String>,
    pub message_count: i64,
    /// Incoming messages the user has not seen yet.
    pub unread_count: i64,
    /// Unread messages that mention us.
    pub mention_count: i64,
    /// Whether the chat is pinned, mirrored from the account.
    pub pinned: bool,
    /// Archived, mirrored from the account.
    pub archived: bool,
    /// Muted until this Unix time in seconds; -1 is indefinitely, 0 not muted.
    pub muted_until: i64,
    /// Marked unread by hand, mirrored from the account.
    pub marked_unread: bool,
}

/// The columns [`message_row`] reads, from `messages m` joined to `names n` on the sender.
const MESSAGE_COLUMNS: &str = "m.chat, m.id, m.sender, m.timestamp, m.from_me, m.text,
    n.name, m.media_kind, m.media_path, m.reply_to_id, m.reply_to_text,
    m.read, m.revoked, m.status, m.reply_to_sender, m.mentioned,
    m.preview_url, m.preview_title, m.preview_desc, m.preview_thumb,
    m.reply_to_kind, m.reply_to_thumb, m.media_thumb, m.media_ref, m.reply_to_chat,
    m.preview_site, m.preview_color, m.media_duration, m.system_kind, m.system_params,
    m.reply_to_view_once, m.reply_to_recoverable, m.reply_to_path, m.reply_to_locator,
  m.media_once_kind, m.sort_order, m.deleted, m.live_location, m.history_shareable, m.spoiler";

fn message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        spoiler: row.get(39)?,
        history_shareable: row.get(38)?,
        header: MessageHeader {
            chat: row.get(0)?,
            id: row.get(1)?,
            sender: row.get(2)?,
            timestamp: row.get(3)?,
            from_me: row.get::<_, i32>(4)? != 0,
        },
        sender_name: row.get(6)?,
        text: row.get(5)?,
        media: Media {
            kind: row.get(7)?,
            path: row.get(8)?,
            thumb: row.get(22)?,
  duration: row.get(27)?,
  locator: row.get(23)?,
  once_kind: row.get(34)?,
 },
        quote: Quote {
            id: row.get(9)?,
            text: row.get(10)?,
            sender: row.get(14)?,
            chat: row.get(24)?,
            kind: row.get(20)?,
            thumb: row.get(21)?,
            view_once: row.get::<_, i32>(30)? != 0,
            recoverable: row.get::<_, i32>(31)? != 0,
            path: row.get(32)?,
            locator: row.get(33)?,
        },
        link: LinkCard {
            url: row.get(16)?,
            title: row.get(17)?,
            desc: row.get(18)?,
            thumb: row.get(19)?,
            site: row.get(25)?,
            color: row.get(26)?,
        },
        local: LocalState {
            sort_order: row.get(35)?,
            read: row.get::<_, i32>(11)? != 0,
            revoked: row.get::<_, i32>(12)? != 0,
            deleted: row.get::<_, i32>(36)? != 0,
            mentioned: row.get::<_, i32>(15)? != 0,
            status: row.get(13)?,
        },
        system: SystemNotice {
            kind: row.get(28)?,
            params: row
                .get::<_, Option<String>>(29)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default(),
        },
        live_location: row
            .get::<_, Option<String>>(37)?
            .and_then(|json| serde_json::from_str(&json).ok()),
    })
}

/// One recipient's receipts for a message we sent, as Unix times.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageReceipt {
    pub recipient: String,
    pub name: Option<String>,
    pub delivered_at: Option<i64>,
    pub read_at: Option<i64>,
    pub played_at: Option<i64>,
}

/// A chat's own retention, overriding the global policy where set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatRetention {
    #[serde(deserialize_with = "limits::chat_limit")]
    pub max_age_hours: RetentionLimit,
    #[serde(deserialize_with = "limits::chat_limit")]
    pub max_messages: RetentionLimit,
    /// Whether scrolling to the top asks the phone for older messages.
    pub on_demand: bool,
}

impl Default for ChatRetention {
    fn default() -> Self {
        Self { max_age_hours: RetentionLimit::Inherit, max_messages: RetentionLimit::Inherit, on_demand: true }
    }
}

/// Ordering for outgoing delivery states. Higher means further along; unknown
/// states rank below everything so any known state replaces them.
fn status_rank(s: &str) -> i32 {
    match s {
        "pending" => 0,
        "sent" => 1,
        "delivered" => 2,
        "read" => 3,
        _ => -1,
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Reaction {
    pub target: String,
    pub sender: String,
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct PollVote {
    pub voter: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Poll {
    pub id: String,
    pub name: String,
    pub options: Vec<String>,
    /// More than one option may be chosen.
    pub multi: bool,
    pub votes: Vec<PollVote>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct EventResponse {
    pub responder: String,
    /// `going`, `not_going` or `maybe`.
    pub response: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Event {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub location: Option<String>,
    pub link: Option<String>,
    pub canceled: bool,
    pub responses: Vec<EventResponse>,
}

/// What a poll or event needs to encrypt or open its votes and RSVPs.
#[derive(Debug, Clone)]
pub struct Secretive {
    pub creator: String,
    pub secret: Vec<u8>,
    pub options: Vec<String>,
}

/// An event as it arrives or is created, before any responses.
#[derive(Debug, Clone, Default)]
pub struct NewEvent {
    pub name: String,
    pub description: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub location: Option<String>,
    pub link: Option<String>,
    pub canceled: bool,
}

/// Per-message state kept beside the messages of one chat.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatMarks {
    pub reactions: Vec<Reaction>,
    pub starred: Vec<String>,
    pub pinned: Option<String>,
    pub polls: Vec<Poll>,
    pub events: Vec<Event>,
    /// View-once messages and whether each was opened (or sent by us, which counts).
    pub view_once: Vec<ViewOnce>,
    /// Ids of messages that arrived marked as forwarded.
    pub forwarded: Vec<String>,
    /// Ids of messages their sender edited.
    pub edited: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ViewOnce {
    pub id: String,
    pub opened: bool,
    /// Whether this view-once can be shown: the file is already on disk, or a
    /// reply quotes it carrying a copy.
    pub available: bool,
}

/// SQLite-backed message store.
pub struct MessageStore {
    conn: Mutex<Connection>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Returns up to `pages` free pages (0: all of them) to the filesystem. The
/// pragma frees one page per step, so it has to be stepped to the end.
fn reclaim(conn: &Connection, pages: u32) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA incremental_vacuum({pages})"))?;
    let mut rows = stmt.query([])?;
    while rows.next()?.is_some() {}
    Ok(())
}

/// An open [`MessageStore::batch`]; commits on drop.
pub struct Batch<'a> {
    store: &'a MessageStore,
    open: bool,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        if self.open {
            if let Err(e) = self.store.conn.lock().unwrap().execute_batch("RELEASE batch") {
                log::warn!("could not commit a store batch: {e}");
            }
        }
    }
}

impl MessageStore {
    /// Opens (or creates) the store at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening message store at {}", path.display()))?;

        // WAL keeps reads from blocking the writer, which matters because
        // messages arrive while the UI is querying.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // With WAL, NORMAL skips the fsync per commit and still survives an app
        // crash; a power loss can drop the last commits but not corrupt the file.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Pruned pages go back to the filesystem a step at a time (`reclaim`)
        // instead of through a full VACUUM. Switching an existing file over
        // takes one VACUUM, done here before anything else touches the store.
        let auto_vacuum: i64 = conn.pragma_query_value(None, "auto_vacuum", |row| row.get(0))?;
        if auto_vacuum != 2 {
            let started = std::time::Instant::now();
            conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
            conn.execute_batch("VACUUM")?;
            log::info!("message store switched to incremental vacuum in {:?}", started.elapsed());
        }
        let started = std::time::Instant::now();
        schema::migrate(&conn)?;
        let schema_elapsed = started.elapsed();
        let started = std::time::Instant::now();
        chats::reconcile_addresses(&conn)?;
        log::info!("message store ready: schema {:?}, address reconciliation {:?}",
            schema_elapsed, started.elapsed());

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Groups every write made until the guard drops into one commit. Guards
    /// nest and may overlap across tasks; the last one to drop commits. Nothing
    /// is rolled back: a failed write fails alone, as it would outside a batch.
    pub fn batch(&self) -> Batch<'_> {
        let open = match self.conn.lock().unwrap().execute_batch("SAVEPOINT batch") {
            Ok(()) => true,
            Err(error) => {
                log::error!("could not start storage batch: {error}");
                false
            }
        };
        Batch { store: self, open }
    }

    /// A bookkeeping value, such as when maintenance last ran.
    pub fn meta(&self, key: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
