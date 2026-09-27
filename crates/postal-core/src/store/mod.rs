//! Message and chat storage.
//!
//! Postal keeps its own history rather than relying on the protocol library,
//! which stores none. That makes retention ours to enforce: the [`Retention`]
//! policy bounds what is kept, so the store cannot grow without limit the way a
//! synced WhatsApp Web profile does.

use std::{path::Path, sync::Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

mod chats;
mod marks;
mod media;
mod messages;
mod names;
mod receipts;
mod retention;
#[cfg(test)]
mod tests;

/// A number standing in for a name: bare digits, or a `+`-prefixed phone label
/// such as WhatsApp's masked `+598∙∙∙∙∙27`. Never a real contact or push name.
pub fn is_placeholder_name(name: &str) -> bool {
    let name = name.trim();
    name.trim_start_matches('+').chars().all(|c| c.is_ascii_digit())
        || (name.starts_with('+') && !name.chars().any(char::is_alphabetic))
}

/// [`is_placeholder_name`] for the `name` column, as far as GLOB can tell (Latin letters only).
const PLACEHOLDER_SQL: &str = "(name NOT GLOB '*[^0-9+]*' OR (name GLOB '+*' AND name NOT GLOB '*[A-Za-z]*'))";

/// How much history to keep locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    /// Drop messages older than this many hours. `None` keeps everything.
    pub max_age_hours: Option<u32>,
    /// Cap on stored messages per chat. `None` means no cap.
    pub max_messages_per_chat: Option<u32>,
}

impl Default for Retention {
    fn default() -> Self {
        // A small window by default: enough for current conversations without
        // re-creating the multi-gigabyte history the web client pulled in.
        Self {
            max_age_hours: Some(24),
            max_messages_per_chat: Some(500),
        }
    }
}

impl Retention {
    /// Keep everything, matching the default WhatsApp client behaviour.
    pub fn unlimited() -> Self {
        Self {
            max_age_hours: None,
            max_messages_per_chat: None,
        }
    }

    /// The oldest timestamp still inside the window, if one is set.
    fn oldest_allowed(&self) -> Option<i64> {
        self.max_age_hours.map(|hours| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            now - i64::from(hours) * 3600
        })
    }
}

/// A stored message, as rows are read and as the UI receives it.
///
/// Each concern is its own type; they serialize flattened, so the IPC shape
/// stays one flat object with the column names as keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
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
}

/// A system line (group change, security notice, …) instead of a message; empty for messages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct MessageHeader {
    pub chat: String,
    pub id: String,
    pub sender: String,
    pub timestamp: i64,
    pub from_me: bool,
}

/// The media a message carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct LocalState {
    /// Whether the user has seen this message.
    pub read: bool,
    /// Whether the sender deleted the message for everyone.
    pub revoked: bool,
    /// Whether the message mentions us (directly or via @all).
    pub mentioned: bool,
    /// Delivery state of a message we sent: `pending`, `sent`, `delivered` or
    /// `read`. `None` for incoming messages.
    pub status: Option<String>,
}

/// A chat summary derived from stored messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
  m.media_once_kind";

fn message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
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
            read: row.get::<_, i32>(11)? != 0,
            revoked: row.get::<_, i32>(12)? != 0,
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
    })
}

/// One recipient's receipts for a message we sent, as Unix times.
#[derive(Debug, Clone, Serialize)]
pub struct MessageReceipt {
    pub recipient: String,
    pub name: Option<String>,
    pub delivered_at: Option<i64>,
    pub read_at: Option<i64>,
    pub played_at: Option<i64>,
}

/// A chat's own retention, overriding the global policy where set.
/// `Some(0)` keeps without limit; `None` defers to the global setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRetention {
    pub max_age_hours: Option<i64>,
    pub max_messages: Option<i64>,
    /// Whether scrolling to the top asks the phone for older messages.
    pub on_demand: bool,
}

impl Default for ChatRetention {
    fn default() -> Self {
        Self { max_age_hours: None, max_messages: None, on_demand: true }
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
pub struct Reaction {
    pub target: String,
    pub sender: String,
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollVote {
    pub voter: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Poll {
    pub id: String,
    pub name: String,
    pub options: Vec<String>,
    /// More than one option may be chosen.
    pub multi: bool,
    pub votes: Vec<PollVote>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventResponse {
    pub responder: String,
    /// `going`, `not_going` or `maybe`.
    pub response: String,
}

#[derive(Debug, Clone, Serialize)]
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
    retention: Mutex<Retention>,
    /// When retention last covered every chat (unix seconds).
    last_full_prune: std::sync::atomic::AtomicI64,
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
    pub fn open(path: &Path, retention: Retention) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
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
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                 chat         TEXT NOT NULL,
                 id           TEXT NOT NULL,
                 sender       TEXT NOT NULL,
                 timestamp    INTEGER NOT NULL,
                 from_me      INTEGER NOT NULL,
                 text         TEXT NOT NULL,
                 media_kind   TEXT,
                 media_path   TEXT,
                 reply_to_id  TEXT,
                 reply_to_text TEXT,
                 reply_to_sender TEXT,
                 read         INTEGER NOT NULL DEFAULT 0,
                 revoked      INTEGER NOT NULL DEFAULT 0,
                 status       TEXT,
                 PRIMARY KEY (chat, id)
             );
             CREATE INDEX IF NOT EXISTS idx_messages_chat_time
                 ON messages (chat, timestamp DESC);
             -- Receipts and server acks name a message by id alone.
             CREATE INDEX IF NOT EXISTS idx_messages_id ON messages (id);
             -- Display names, learned from message push names and group queries.
             -- Kept separately from messages because one JID has one name and
             -- it should survive pruning of the messages that revealed it.
             -- `saved` marks a name that came from the account's address book,
             -- which outranks the push name a contact sets for themselves.
             CREATE TABLE IF NOT EXISTS names (
                 jid   TEXT PRIMARY KEY,
                 name  TEXT NOT NULL,
                 saved INTEGER NOT NULL DEFAULT 0
             );",
        )?;

        // Columns added after the first release. SQLite has no "ADD COLUMN IF
        // NOT EXISTS", so the existing set is inspected first.
        let existing: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(messages)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for column in [
            "media_kind",
            "media_path",
            "media_thumb",
            "reply_to_id",
            "reply_to_text",
            "reply_to_sender",
            "reply_to_chat",
            "reply_to_kind",
            "reply_to_thumb",
            "reply_to_path",
            "preview_url",
            "preview_title",
            "preview_desc",
            "preview_thumb",
            "preview_site",
            "preview_color",
            "system_kind",
            "system_params",
        ] {
            if !existing.iter().any(|c| c == column) {
                conn.execute(&format!("ALTER TABLE messages ADD COLUMN {column} TEXT"), [])?;
            }
        }

        if !existing.iter().any(|c| c == "mentioned") {
            conn.execute(
                "ALTER TABLE messages ADD COLUMN mentioned INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }

        if !existing.iter().any(|c| c == "media_duration") {
            conn.execute("ALTER TABLE messages ADD COLUMN media_duration INTEGER", [])?;
        }

        // The view-once a reply quotes, and the only copy of it a linked device
        // is ever sent. `reply_to_locator` is the quoted message as it arrived
        // (older rows: the bare media submessage), so the copy can be fetched
        // and quoted again in the same form.
        for (column, decl) in [
            ("reply_to_view_once", "INTEGER NOT NULL DEFAULT 0"),
            ("reply_to_recoverable", "INTEGER NOT NULL DEFAULT 0"),
            ("reply_to_locator", "BLOB"),
            // The kind a view-once had before it was marked as one, so a
            // recovered copy is shown by the player that fits it.
            ("media_once_kind", "TEXT"),
        ] {
            if !existing.iter().any(|c| c == column) {
                conn.execute(&format!("ALTER TABLE messages ADD COLUMN {column} {decl}"), [])?;
            }
        }

        // Chat pins, mirrored from the account so they match the phone.
        conn.execute("CREATE TABLE IF NOT EXISTS pins (jid TEXT PRIMARY KEY)", [])?;
        // Archive, mute and mark-unread, mirrored from the account like pins.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chat_state (
                jid TEXT PRIMARY KEY,
                archived INTEGER NOT NULL DEFAULT 0,
                muted_until INTEGER NOT NULL DEFAULT 0,
                marked_unread INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        // Per-message state that is not part of the message itself.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS reactions (
                 chat TEXT NOT NULL, target TEXT NOT NULL, sender TEXT NOT NULL,
                 emoji TEXT NOT NULL, PRIMARY KEY (chat, target, sender));
             CREATE TABLE IF NOT EXISTS stars (
                 chat TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS message_pins (chat TEXT PRIMARY KEY, id TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS polls (
                 chat TEXT NOT NULL, id TEXT NOT NULL, creator TEXT NOT NULL,
                 name TEXT NOT NULL, options TEXT NOT NULL, multi INTEGER NOT NULL,
                 secret BLOB, PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS poll_votes (
                 chat TEXT NOT NULL, poll TEXT NOT NULL, voter TEXT NOT NULL,
                 options TEXT NOT NULL, PRIMARY KEY (chat, poll, voter));
             CREATE TABLE IF NOT EXISTS events (
                 chat TEXT NOT NULL, id TEXT NOT NULL, creator TEXT NOT NULL,
                 name TEXT NOT NULL, description TEXT, start_at INTEGER, end_at INTEGER,
                 location TEXT, link TEXT, canceled INTEGER NOT NULL DEFAULT 0,
                 secret BLOB, PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS event_responses (
                 chat TEXT NOT NULL, event TEXT NOT NULL, responder TEXT NOT NULL,
                 response TEXT NOT NULL, PRIMARY KEY (chat, event, responder));
             CREATE TABLE IF NOT EXISTS view_once (
                 chat TEXT NOT NULL, id TEXT NOT NULL, opened INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS forwarded (
                 chat TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS edited (
                 chat TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY (chat, id));
             CREATE TABLE IF NOT EXISTS receipts (
                 id TEXT NOT NULL, recipient TEXT NOT NULL, delivered_at INTEGER,
                 read_at INTEGER, played_at INTEGER, PRIMARY KEY (id, recipient));
             CREATE TABLE IF NOT EXISTS chat_retention (
                 jid TEXT PRIMARY KEY, max_age_hours INTEGER, max_messages INTEGER,
                 on_demand INTEGER NOT NULL DEFAULT 1);
             CREATE TABLE IF NOT EXISTS lid_pn (
                 lid TEXT PRIMARY KEY, pn TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS lid_pn_by_pn ON lid_pn (pn);
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);",
        )?;

        // Per chat overrides. Absent means the global setting applies.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chat_settings (
                 jid TEXT PRIMARY KEY,
                 auto_download INTEGER NOT NULL DEFAULT 1
             )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chat_privacy (
                 jid TEXT PRIMARY KEY,
                 send_typing INTEGER,
                 send_receipts INTEGER
             )",
            [],
        )?;
        // Local-only chat list state. Clearing a chat drops its messages but
        // keeps an empty row in the list; deleting one hides it until a new
        // message arrives. Neither touches the phone or the other side.
        conn.execute("CREATE TABLE IF NOT EXISTS hidden_chats (jid TEXT PRIMARY KEY)", [])?;
        conn.execute("CREATE TABLE IF NOT EXISTS cleared_chats (jid TEXT PRIMARY KEY)", [])?;

        // The media reference is a blob, so it cannot go through the TEXT
        // migration loop above.
        let message_columns: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(messages)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !message_columns.iter().any(|c| c == "media_ref") {
            conn.execute("ALTER TABLE messages ADD COLUMN media_ref BLOB", [])?;
        }

        let existing_names: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(names)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if !existing_names.iter().any(|c| c == "saved") {
            conn.execute("ALTER TABLE names ADD COLUMN saved INTEGER NOT NULL DEFAULT 0", [])?;
        }

        // Status updates were once stored as a chat. Drop them so the list stops
        // showing a "status" conversation.
        conn.execute("DELETE FROM messages WHERE chat = 'status@broadcast'", [])?;

        // Masked group labels (`+598∙∙∙∙∙27`) were once stored as names, over the
        // real push names. Dropping them lets the push names come back.
        conn.execute(
            "DELETE FROM names WHERE name GLOB '+*' AND name GLOB '*[^0-9+]*' AND name NOT GLOB '*[A-Za-z]*' AND saved = 0",
            [],
        )?;

        // Names learned from messages are keyed with the sender's device suffix
        // (`123:98@lid`), but participants are listed without one. Mirror every
        // such name onto the bare form so lookups find it.
        conn.execute(
            "INSERT OR IGNORE INTO names (jid, name, saved)
             SELECT substr(jid, 1, instr(jid, ':') - 1) || substr(jid, instr(jid, '@')),
                    name, saved
             FROM names
             WHERE jid LIKE '%:%@%'",
            [],
        )?;
        if !existing.iter().any(|c| c == "read") {
            conn.execute(
                "ALTER TABLE messages ADD COLUMN read INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !existing.iter().any(|c| c == "revoked") {
            conn.execute(
                "ALTER TABLE messages ADD COLUMN revoked INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !existing.iter().any(|c| c == "status") {
            conn.execute("ALTER TABLE messages ADD COLUMN status TEXT", [])?;
        }

        // One direct chat can be stored under both its LID and phone-number
        // forms, which shows the same contact twice. Fold the LID copy onto the
        // phone-number one; the write path now keys direct chats by number.
        let lid_chats: Vec<String> = {
            let mut found = std::collections::BTreeSet::new();
            for (table, column) in [
                ("messages", "chat"),
                ("chat_state", "jid"),
                ("pins", "jid"),
                ("cleared_chats", "jid"),
                ("hidden_chats", "jid"),
            ] {
                let mut stmt = conn.prepare(&format!(
                    "SELECT DISTINCT {column} FROM {table} WHERE {column} LIKE '%@lid'"
                ))?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                for row in rows {
                    found.insert(row?);
                }
            }
            found.into_iter().collect()
        };
        if !lid_chats.is_empty() {
            let tx = conn.unchecked_transaction()?;
            for lid_chat in &lid_chats {
                let Some(user) = lid_chat.split('@').next().and_then(|u| u.split(':').next()) else {
                    continue;
                };
                let pn: Option<String> = tx
                    .query_row("SELECT pn FROM lid_pn WHERE lid = ?1", params![user], |r| r.get(0))
                    .optional()?;
                if let Some(pn) = pn {
                    chats::fold_chat(&tx, lid_chat, &format!("{pn}@s.whatsapp.net"))?;
                }
            }
            tx.commit()?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            retention: Mutex::new(retention),
            last_full_prune: std::sync::atomic::AtomicI64::new(0),
        })
    }

    /// Groups every write made until the guard drops into one commit. Guards
    /// nest and may overlap across tasks; the last one to drop commits. Nothing
    /// is rolled back: a failed write fails alone, as it would outside a batch.
    pub fn batch(&self) -> Batch<'_> {
        let open = self.conn.lock().unwrap().execute_batch("SAVEPOINT batch").is_ok();
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
