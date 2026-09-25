//! Message and chat storage.
//!
//! Hermóðr keeps its own history rather than relying on the protocol library,
//! which stores none. That makes retention ours to enforce: the [`Retention`]
//! policy bounds what is kept, so the store cannot grow without limit the way a
//! synced WhatsApp Web profile does.

use std::{path::Path, sync::Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// How much history to keep locally.
/// A number standing in for a name: bare digits, or a `+`-prefixed phone label
/// such as WhatsApp's masked `+598∙∙∙∙∙27`. Never a real contact or push name.
pub fn is_placeholder_name(name: &str) -> bool {
    let name = name.trim();
    name.trim_start_matches('+').chars().all(|c| c.is_ascii_digit())
        || (name.starts_with('+') && !name.chars().any(char::is_alphabetic))
}

/// [`is_placeholder_name`] for the `name` column, as far as GLOB can tell (Latin letters only).
const PLACEHOLDER_SQL: &str = "(name NOT GLOB '*[^0-9+]*' OR (name GLOB '+*' AND name NOT GLOB '*[A-Za-z]*'))";

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
    /// The media submessage, kept so the file can be downloaded on demand when
    /// automatic downloads are off: keys, hashes and URL, without thumbnail or
    /// quote, typically a few hundred bytes. Internal: not handed to the UI.
    #[serde(skip)]
    pub locator: Option<Vec<u8>>,
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
}

/// The columns [`message_row`] reads, from `messages m` joined to `names n` on the sender.
const MESSAGE_COLUMNS: &str = "m.chat, m.id, m.sender, m.timestamp, m.from_me, m.text,
    n.name, m.media_kind, m.media_path, m.reply_to_id, m.reply_to_text,
    m.read, m.revoked, m.status, m.reply_to_sender, m.mentioned,
    m.preview_url, m.preview_title, m.preview_desc, m.preview_thumb,
    m.reply_to_kind, m.reply_to_thumb, m.media_thumb, m.media_ref, m.reply_to_chat,
    m.preview_site, m.preview_color";

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
            locator: row.get(23)?,
        },
        quote: Quote {
            id: row.get(9)?,
            text: row.get(10)?,
            sender: row.get(14)?,
            chat: row.get(24)?,
            kind: row.get(20)?,
            thumb: row.get(21)?,
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
}

/// SQLite-backed message store.
pub struct MessageStore {
    conn: Mutex<Connection>,
    retention: Retention,
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
            "preview_url",
            "preview_title",
            "preview_desc",
            "preview_thumb",
            "preview_site",
            "preview_color",
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

        // Chat pins, mirrored from the account so they match the phone.
        conn.execute("CREATE TABLE IF NOT EXISTS pins (jid TEXT PRIMARY KEY)", [])?;
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

        Ok(Self {
            conn: Mutex::new(conn),
            retention,
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

    /// Records a message. A repeat of a stored one (a replayed or duplicate
    /// event) refreshes its content but never moves local state backwards:
    /// delivery status only advances, read/mentioned stay set, a known media
    /// file or edited text is kept, and a revoked message is left as it is.
    /// State changes have their own methods (`set_status`, `mark_chat_read`,
    /// `revoke`, `apply_edit`, `set_media_path`).
    pub fn insert_message(&self, message: &StoredMessage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages
                 (chat, id, sender, timestamp, from_me, text,
                  media_kind, media_path, reply_to_id, reply_to_text, reply_to_sender,
                  read, revoked, mentioned, status,
                  preview_url, preview_title, preview_desc, preview_thumb,
                  reply_to_kind, reply_to_thumb, media_thumb, media_ref, reply_to_chat,
                  preview_site, preview_color)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                     ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
             ON CONFLICT(chat, id) DO UPDATE SET
                 sender = excluded.sender,
                 timestamp = excluded.timestamp,
                 from_me = excluded.from_me,
                 text = CASE WHEN EXISTS (SELECT 1 FROM edited e
                                          WHERE e.chat = messages.chat AND e.id = messages.id)
                        THEN text ELSE excluded.text END,
                 media_kind = excluded.media_kind,
                 media_path = COALESCE(media_path, excluded.media_path),
                 reply_to_id = excluded.reply_to_id,
                 reply_to_text = excluded.reply_to_text,
                 reply_to_sender = excluded.reply_to_sender,
                 read = MAX(read, excluded.read),
                 revoked = excluded.revoked,
                 mentioned = MAX(mentioned, excluded.mentioned),
                 status = CASE WHEN ?27 >(CASE status WHEN 'pending' THEN 0 WHEN 'sent' THEN 1
                                           WHEN 'delivered' THEN 2 WHEN 'read' THEN 3 ELSE -1 END)
                          THEN excluded.status ELSE status END,
                 preview_url = excluded.preview_url,
                 preview_title = excluded.preview_title,
                 preview_desc = excluded.preview_desc,
                 preview_thumb = excluded.preview_thumb,
                 reply_to_kind = excluded.reply_to_kind,
                 reply_to_thumb = excluded.reply_to_thumb,
                 media_thumb = COALESCE(media_thumb, excluded.media_thumb),
                 media_ref = COALESCE(excluded.media_ref, media_ref),
                 reply_to_chat = excluded.reply_to_chat,
                 preview_site = excluded.preview_site,
                 preview_color = excluded.preview_color
             WHERE revoked = 0",
            params![
                message.header.chat,
                message.header.id,
                message.header.sender,
                message.header.timestamp,
                message.header.from_me as i32,
                message.text,
                message.media.kind,
                message.media.path,
                message.quote.id,
                message.quote.text,
                message.quote.sender,
                message.local.read as i32,
                message.local.revoked as i32,
                message.local.mentioned as i32,
                message.local.status,
                message.link.url,
                message.link.title,
                message.link.desc,
                message.link.thumb,
                message.quote.kind,
                message.quote.thumb,
                message.media.thumb,
                message.media.locator,
                message.quote.chat,
                message.link.site,
                message.link.color,
                message.local.status.as_deref().map(status_rank).unwrap_or(-1),
            ],
        )?;
        Ok(())
    }

    /// Records a display name for a JID, from a push name or group query.
    ///
    /// Empty names are ignored: a message with no push name should not erase a
    /// name learned earlier. A name from the address book is never overwritten
    /// by one the contact chose for themselves.
    pub fn set_name(&self, jid: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || jid.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        if is_placeholder_name(name) {
            conn.execute("INSERT OR IGNORE INTO names (jid, name, saved) VALUES (?1, ?2, 0)", params![jid, name])?;
            return Ok(());
        }
        conn.execute(
            &format!(
                "INSERT INTO names (jid, name, saved) VALUES (?1, ?2, 0)
                 ON CONFLICT(jid) DO UPDATE SET name = excluded.name, saved = 0
                 WHERE saved = 0 OR {PLACEHOLDER_SQL}"
            ),
            params![jid, name],
        )?;
        Ok(())
    }

    /// Records a name from the account's address book, which takes priority
    /// over any push name already stored.
    pub fn set_saved_name(&self, jid: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || jid.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO names (jid, name, saved) VALUES (?1, ?2, 1)
             ON CONFLICT(jid) DO UPDATE SET name = excluded.name, saved = 1",
            params![jid, name],
        )?;
        Ok(())
    }

    /// Drops the address-book flag when a contact is removed, so the name can
    /// later be replaced by a push name.
    pub fn clear_saved_name(&self, jid: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE names SET saved = 0 WHERE jid = ?1", params![jid])?;
        Ok(())
    }

    /// How many address-book names are stored.
    pub fn saved_name_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count = conn.query_row("SELECT COUNT(*) FROM names WHERE saved = 1", [], |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(count as usize)
    }

    /// Every JID that appears as a message sender or a chat.
    pub fn known_addresses(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT DISTINCT sender FROM messages UNION SELECT DISTINCT chat FROM messages")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Address-book names, as `(jid, name)` pairs.
    pub fn saved_names(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid, name FROM names WHERE saved = 1")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Whether the stored name for a JID came from the address book.
    pub fn name_is_saved(&self, jid: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT saved FROM names WHERE jid = ?1",
            params![jid],
            |r| r.get::<_, i32>(0),
        )
        .map(|v| v != 0)
        .unwrap_or(false)
    }

    /// Names matching a query, for the search box.
    pub fn search_names(&self, query: &str, limit: u32) -> Result<Vec<(String, String, bool)>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query.to_lowercase());
        let mut stmt = conn.prepare(
            "SELECT jid, name, saved FROM names
             WHERE lower(name) LIKE ?1 OR lower(jid) LIKE ?1
             ORDER BY saved DESC, name LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)? != 0,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// The display name for a JID, if known.
    pub fn name_for(&self, jid: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let name = conn
            .query_row("SELECT name FROM names WHERE jid = ?1", params![jid], |r| {
                r.get::<_, String>(0)
            })
            .ok();
        Ok(name)
    }

    /// Remembers that a LID user and a phone number are the same person.
    ///
    /// Kept here because the session's own mapping is lost when a device re-pairs.
    pub fn set_lid_pn(&self, lid: &str, pn: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO lid_pn (lid, pn) VALUES (?1, ?2) ON CONFLICT(lid) DO UPDATE SET pn = excluded.pn",
            params![lid, pn],
        )?;
        Ok(())
    }

    /// `(lid, pn)` user parts for either form of a user part.
    pub fn lid_pn(&self, user: &str) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT lid, pn FROM lid_pn WHERE lid = ?1 OR pn = ?1 LIMIT 1",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok())
    }

    /// Messages in a chat, newest first.
    pub fn messages_for(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1
             ORDER BY m.timestamp DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![chat, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Messages that mention us, in one chat or all of them, newest first.
    pub fn pings(&self, chat: Option<&str>, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.mentioned = 1 AND m.from_me = 0 AND (?1 IS NULL OR m.chat = ?1)
             ORDER BY m.timestamp DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![chat, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Messages in a chat whose text contains `query`, ignoring case, newest first.
    pub fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{}%", escaped.to_lowercase());
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1 AND lower(m.text) LIKE ?2 ESCAPE '\\'
             ORDER BY m.timestamp DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![chat, pattern, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Starred messages across every chat, newest first.
    pub fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM stars s
             JOIN messages m ON m.chat = s.chat AND m.id = s.id
             LEFT JOIN names n ON n.jid = m.sender
             ORDER BY m.timestamp DESC"
        ))?;
        let rows = stmt.query_map([], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// The oldest stored message in a chat, as (id, from_me, timestamp).
    pub fn oldest_message(&self, chat: &str) -> Result<Option<(String, bool, i64)>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, from_me, timestamp FROM messages
                 WHERE chat = ?1 ORDER BY timestamp ASC LIMIT 1",
                params![chat],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i32>(1)? != 0,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .ok();
        Ok(row)
    }

    /// The chat a stored message id belongs to, when it is known locally. A
    /// quoted message in another chat can be located with this.
    pub fn chat_of_message(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let chat = conn
            .query_row(
                "SELECT chat FROM messages WHERE id = ?1 LIMIT 1",
                params![id],
                |r| r.get::<_, String>(0),
            )
            .ok();
        Ok(chat)
    }

    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT auto_download FROM chat_settings WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i32>(0),
            )
            .ok();
        Ok(value.map(|v| v != 0))
    }

    /// Sets the per chat auto download override.
    pub fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_settings (jid, auto_download) VALUES (?1, ?2)
             ON CONFLICT(jid) DO UPDATE SET auto_download = excluded.auto_download",
            params![jid, enabled as i32],
        )?;
        Ok(())
    }

    /// The stored media reference for a message.
    pub fn media_ref_for(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT media_ref FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get::<_, Option<Vec<u8>>>(0),
            )
            .ok()
            .flatten();
        Ok(value)
    }

    /// Records where a downloaded file was written.
    pub fn set_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET media_path = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, path],
        )?;
        Ok(())
    }

    /// Forgets every stored media path, returning how many rows changed.
    pub fn clear_media_paths(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE messages
             SET media_path = NULL,
                 media_thumb = CASE WHEN media_thumb LIKE 'data:%' THEN media_thumb END
             WHERE media_path IS NOT NULL OR media_thumb NOT LIKE 'data:%'",
            [],
        )?)
    }

    /// Records a reaction; an empty emoji removes the sender's reaction.
    pub fn set_reaction(&self, chat: &str, target: &str, sender: &str, emoji: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if emoji.is_empty() {
            conn.execute(
                "DELETE FROM reactions WHERE chat = ?1 AND target = ?2 AND sender = ?3",
                params![chat, target, sender],
            )?;
        } else {
            conn.execute(
                "INSERT INTO reactions (chat, target, sender, emoji) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(chat, target, sender) DO UPDATE SET emoji = excluded.emoji",
                params![chat, target, sender, emoji],
            )?;
        }
        Ok(())
    }

    pub fn set_starred(&self, chat: &str, id: &str, starred: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let sql = if starred {
            "INSERT OR IGNORE INTO stars (chat, id) VALUES (?1, ?2)"
        } else {
            "DELETE FROM stars WHERE chat = ?1 AND id = ?2"
        };
        conn.execute(sql, params![chat, id])?;
        Ok(())
    }

    /// The chat's pinned message, or none.
    // One pin per chat; WhatsApp allows three, add a rank column if needed.
    pub fn set_message_pin(&self, chat: &str, id: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        match id {
            Some(id) => conn.execute(
                "INSERT INTO message_pins (chat, id) VALUES (?1, ?2)
                 ON CONFLICT(chat) DO UPDATE SET id = excluded.id",
                params![chat, id],
            )?,
            None => conn.execute("DELETE FROM message_pins WHERE chat = ?1", params![chat])?,
        };
        Ok(())
    }

    /// Reactions, stars and the pin for one chat.
    pub fn marks(&self, chat: &str) -> Result<ChatMarks> {
        let conn = self.conn.lock().unwrap();
        let reactions = conn
            .prepare("SELECT target, sender, emoji FROM reactions WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok(Reaction { target: r.get(0)?, sender: r.get(1)?, emoji: r.get(2)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let starred = conn
            .prepare("SELECT id FROM stars WHERE chat = ?1")?
            .query_map(params![chat], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let pinned = conn
            .query_row("SELECT id FROM message_pins WHERE chat = ?1", params![chat], |r| r.get(0))
            .ok();
        let json = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();

        let mut polls: Vec<Poll> = conn
            .prepare("SELECT id, name, options, multi FROM polls WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok(Poll {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    options: json(r.get(2)?),
                    multi: r.get::<_, i32>(3)? != 0,
                    votes: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let votes: Vec<(String, PollVote)> = conn
            .prepare("SELECT poll, voter, options FROM poll_votes WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok((r.get(0)?, PollVote { voter: r.get(1)?, options: json(r.get(2)?) }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (poll, vote) in votes {
            if let Some(p) = polls.iter_mut().find(|p| p.id == poll) {
                p.votes.push(vote);
            }
        }

        let mut events: Vec<Event> = conn
            .prepare(
                "SELECT id, name, description, start_at, end_at, location, link, canceled
                 FROM events WHERE chat = ?1",
            )?
            .query_map(params![chat], |r| {
                Ok(Event {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    start: r.get(3)?,
                    end: r.get(4)?,
                    location: r.get(5)?,
                    link: r.get(6)?,
                    canceled: r.get::<_, i32>(7)? != 0,
                    responses: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let responses: Vec<(String, EventResponse)> = conn
            .prepare("SELECT event, responder, response FROM event_responses WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok((r.get(0)?, EventResponse { responder: r.get(1)?, response: r.get(2)? }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (event, response) in responses {
            if let Some(e) = events.iter_mut().find(|e| e.id == event) {
                e.responses.push(response);
            }
        }

        let view_once = conn
            .prepare("SELECT id, opened FROM view_once WHERE chat = ?1")?
            .query_map(params![chat], |r| Ok(ViewOnce { id: r.get(0)?, opened: r.get::<_, i32>(1)? != 0 }))?
            .collect::<rusqlite::Result<_>>()?;

        let ids = |table: &str| -> Result<Vec<String>> {
            conn.prepare(&format!("SELECT id FROM {table} WHERE chat = ?1"))?
                .query_map(params![chat], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()
                .map_err(Into::into)
        };
        let forwarded = ids("forwarded")?;
        let edited = ids("edited")?;

        Ok(ChatMarks { reactions, starred, pinned, polls, events, view_once, forwarded, edited })
    }

    /// Records when one recipient got, read or played one of our messages.
    /// Reading implies delivery, and playing implies reading.
    pub fn record_receipt(&self, id: &str, recipient: &str, kind: &str, at: i64) -> Result<()> {
        let (delivered, read, played) = match kind {
            "played" => (Some(at), Some(at), Some(at)),
            "read" => (Some(at), Some(at), None),
            _ => (Some(at), None, None),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO receipts (id, recipient, delivered_at, read_at, played_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id, recipient) DO UPDATE SET
                 delivered_at = COALESCE(receipts.delivered_at, excluded.delivered_at),
                 read_at = COALESCE(receipts.read_at, excluded.read_at),
                 played_at = COALESCE(receipts.played_at, excluded.played_at)",
            params![id, recipient, delivered, read, played],
        )?;
        Ok(())
    }

    /// Who got, read and played one of our messages, and when.
    pub fn receipts(&self, id: &str) -> Result<Vec<MessageReceipt>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT r.recipient, n.name, r.delivered_at, r.read_at, r.played_at
             FROM receipts r LEFT JOIN names n ON n.jid = r.recipient
             WHERE r.id = ?1 ORDER BY COALESCE(r.read_at, r.delivered_at)",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(MessageReceipt {
                recipient: r.get(0)?,
                name: r.get(1)?,
                delivered_at: r.get(2)?,
                read_at: r.get(3)?,
                played_at: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn set_forwarded(&self, chat: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO forwarded (chat, id) VALUES (?1, ?2)", params![chat, id])?;
        Ok(())
    }

    /// Replaces a message's text (its caption, for media) after its sender edited it.
    pub fn apply_edit(&self, chat: &str, id: &str, text: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE messages SET text = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, text],
        )?;
        if changed > 0 {
            conn.execute("INSERT OR IGNORE INTO edited (chat, id) VALUES (?1, ?2)", params![chat, id])?;
        }
        Ok(changed > 0)
    }

    pub fn chat_retention(&self, jid: &str) -> Result<ChatRetention> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT max_age_hours, max_messages, on_demand FROM chat_retention WHERE jid = ?1",
                params![jid],
                |r| {
                    Ok(ChatRetention {
                        max_age_hours: r.get(0)?,
                        max_messages: r.get(1)?,
                        on_demand: r.get::<_, i32>(2)? != 0,
                    })
                },
            )
            .unwrap_or_default())
    }

    pub fn set_chat_retention(&self, jid: &str, retention: &ChatRetention) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if *retention == ChatRetention::default() {
            conn.execute("DELETE FROM chat_retention WHERE jid = ?1", params![jid])?;
        } else {
            conn.execute(
                "INSERT INTO chat_retention (jid, max_age_hours, max_messages, on_demand)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(jid) DO UPDATE SET max_age_hours = excluded.max_age_hours,
                     max_messages = excluded.max_messages, on_demand = excluded.on_demand",
                params![jid, retention.max_age_hours, retention.max_messages, retention.on_demand as i32],
            )?;
        }
        Ok(())
    }

    /// Records a view-once message; `opened` only ever moves from false to true.
    pub fn set_view_once(&self, chat: &str, id: &str, opened: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO view_once (chat, id, opened) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, id) DO UPDATE SET opened = MAX(opened, excluded.opened)",
            params![chat, id, opened as i32],
        )?;
        Ok(())
    }

    /// Opens a view-once message: marks it and forgets its file, returning the path to delete.
    pub fn open_view_once(&self, chat: &str, id: &str) -> Result<Option<String>> {
        self.set_view_once(chat, id, true)?;
        let conn = self.conn.lock().unwrap();
        let path: Option<String> = conn
            .query_row(
                "SELECT media_path FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        conn.execute(
            "UPDATE messages SET media_path = NULL, media_thumb = NULL, media_ref = NULL
             WHERE chat = ?1 AND id = ?2",
            params![chat, id],
        )?;
        Ok(path)
    }

    /// Records a poll the first time it is seen; later copies change nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn save_poll(
        &self,
        chat: &str,
        id: &str,
        creator: &str,
        name: &str,
        options: &[String],
        multi: bool,
        secret: Option<&[u8]>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO polls (chat, id, creator, name, options, multi, secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![chat, id, creator, name, serde_json::to_string(options)?, multi as i32, secret],
        )?;
        Ok(())
    }

    pub fn poll_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT creator, secret, options FROM polls WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| {
                    Ok(r.get::<_, Option<Vec<u8>>>(1)?.map(|secret| Secretive {
                        creator: r.get(0).unwrap_or_default(),
                        secret,
                        options: serde_json::from_str(&r.get::<_, String>(2).unwrap_or_default())
                            .unwrap_or_default(),
                    }))
                },
            )
            .ok()
            .flatten())
    }

    /// A voter's current choice; an empty list withdraws their vote.
    pub fn set_poll_vote(&self, chat: &str, poll: &str, voter: &str, options: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO poll_votes (chat, poll, voter, options) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat, poll, voter) DO UPDATE SET options = excluded.options",
            params![chat, poll, voter, serde_json::to_string(options)?],
        )?;
        Ok(())
    }

    /// Records an event, or updates it when its creator edits or cancels it.
    pub fn save_event(
        &self,
        chat: &str,
        id: &str,
        creator: &str,
        event: &NewEvent,
        secret: Option<&[u8]>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO events
                 (chat, id, creator, name, description, start_at, end_at, location, link, canceled, secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(chat, id) DO UPDATE SET
                 name = excluded.name, description = excluded.description,
                 start_at = excluded.start_at, end_at = excluded.end_at, location = excluded.location,
                 link = excluded.link, canceled = excluded.canceled,
                 secret = COALESCE(events.secret, excluded.secret)",
            params![
                chat,
                id,
                creator,
                event.name,
                event.description,
                event.start,
                event.end,
                event.location,
                event.link,
                event.canceled as i32,
                secret
            ],
        )?;
        Ok(())
    }

    pub fn event_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT creator, secret FROM events WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| {
                    Ok(r.get::<_, Option<Vec<u8>>>(1)?.map(|secret| Secretive {
                        creator: r.get(0).unwrap_or_default(),
                        secret,
                        options: Vec::new(),
                    }))
                },
            )
            .ok()
            .flatten())
    }

    pub fn set_event_response(&self, chat: &str, event: &str, responder: &str, response: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO event_responses (chat, event, responder, response) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat, event, responder) DO UPDATE SET response = excluded.response",
            params![chat, event, responder, response],
        )?;
        Ok(())
    }

    /// Downloaded files of one media kind, newest first, each file once.
    pub fn recent_media(&self, kind: &str, limit: u32) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .prepare(
                "SELECT media_path FROM messages
                 WHERE media_kind = ?1 AND media_path IS NOT NULL AND revoked = 0
                 GROUP BY media_path ORDER BY MAX(timestamp) DESC LIMIT ?2",
            )?
            .query_map(params![kind, limit], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Removes one message, as "delete for me" does.
    pub fn delete_message(&self, chat: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        conn.execute("DELETE FROM reactions WHERE chat = ?1 AND target = ?2", params![chat, id])?;
        conn.execute("DELETE FROM stars WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        conn.execute("DELETE FROM poll_votes WHERE chat = ?1 AND poll = ?2", params![chat, id])?;
        conn.execute("DELETE FROM event_responses WHERE chat = ?1 AND event = ?2", params![chat, id])?;
        conn.execute("DELETE FROM view_once WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        conn.execute("DELETE FROM forwarded WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        conn.execute("DELETE FROM edited WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        conn.execute("DELETE FROM receipts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Moves the media files this store references out of `from` into `to`,
    /// rewriting the stored paths, and returns how many paths were rewritten.
    ///
    /// Only referenced files move, so `from` may be a folder shared with other
    /// programs. A reference whose file is gone is also relinked when a file of
    /// the same name is found in `from` or `to`.
    pub fn relocate_media(&self, from: &[&Path], to: &Path) -> Result<usize> {
        const COLUMNS: [&str; 4] = ["media_path", "media_thumb", "reply_to_thumb", "preview_thumb"];
        let conn = self.conn.lock().unwrap();
        let mut rewritten = 0;
        for column in COLUMNS {
            let paths: Vec<String> = conn
                .prepare(&format!("SELECT DISTINCT {column} FROM messages WHERE {column} IS NOT NULL AND {column} NOT LIKE 'data:%'"))?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for old in paths {
                let old_path = Path::new(&old);
                let Some(name) = old_path.file_name() else { continue };
                let in_from = from.iter().any(|dir| old_path.starts_with(dir));
                if old_path.starts_with(to) || (!in_from && old_path.exists()) {
                    continue;
                }
                let dest = to.join(name);
                if !dest.exists() {
                    let source = std::iter::once(old_path.to_path_buf())
                        .filter(|_| in_from)
                        .chain(from.iter().map(|dir| dir.join(name)))
                        .find(|p| p.is_file());
                    let Some(source) = source else { continue };
                    std::fs::create_dir_all(to)?;
                    if std::fs::rename(&source, &dest).is_err() {
                        // A different filesystem cannot be renamed across.
                        std::fs::copy(&source, &dest)?;
                        let _ = std::fs::remove_file(&source);
                    }
                }
                rewritten += conn.execute(
                    &format!("UPDATE messages SET {column} = ?2 WHERE {column} = ?1"),
                    params![old, dest.to_string_lossy()],
                )?;
            }
        }
        Ok(rewritten)
    }

    /// Unread messages in `chat` that mention us, oldest first.
    pub fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM messages
             WHERE chat = ?1 AND read = 0 AND from_me = 0 AND mentioned = 1
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![chat], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Mirrors a chat's pin state from the account.
    pub fn set_pinned(&self, jid: &str, pinned: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if pinned {
            conn.execute("INSERT OR IGNORE INTO pins (jid) VALUES (?1)", params![jid])?;
        } else {
            conn.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        }
        Ok(())
    }

    /// The pinned chats.
    pub fn pinned_chats(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid FROM pins")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// A single stored message.
    pub fn message(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        let message = conn.query_row(
            &format!(
                "SELECT {MESSAGE_COLUMNS}
                 FROM messages m
                 LEFT JOIN names n ON n.jid = m.sender
                 WHERE m.chat = ?1 AND m.id = ?2"
            ),
            params![chat, id],
            message_row,
        )?;
        Ok(message)
    }

    /// One summary per chat, most recently active first.
    pub fn chats(&self) -> Result<Vec<ChatSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            // The preview row is one index seek per chat; a window over every
            // message would copy the whole table into a temporary sort.
            "SELECT g.chat, g.last_message_at, g.message_count, n.name, g.unread_count,
                    g.mention_count, p.jid IS NOT NULL AS pinned,
                    m.text, m.from_me, s.name, m.sender, m.media_kind
             FROM (SELECT chat,
                          MAX(timestamp) AS last_message_at,
                          COUNT(*) AS message_count,
                          SUM(read = 0 AND from_me = 0) AS unread_count,
                          SUM(read = 0 AND from_me = 0 AND mentioned = 1) AS mention_count
                   FROM messages GROUP BY chat) g
             JOIN messages m ON m.rowid =
                  (SELECT rowid FROM messages WHERE chat = g.chat ORDER BY timestamp DESC LIMIT 1)
             LEFT JOIN names n ON n.jid = g.chat
             LEFT JOIN names s ON s.jid = m.sender
             LEFT JOIN pins p ON p.jid = g.chat
             ORDER BY pinned DESC, g.last_message_at DESC",
        )?;
        let summaries = stmt
            .query_map([], |row| {
                Ok(ChatSummary {
                    chat: row.get(0)?,
                    last_message_at: row.get(1)?,
                    message_count: row.get(2)?,
                    display_name: row.get(3)?,
                    unread_count: row.get(4)?,
                    mention_count: row.get(5)?,
                    pinned: row.get::<_, i64>(6)? != 0,
                    last_text: row.get(7)?,
                    last_from_me: row.get::<_, i64>(8)? != 0,
                    last_sender_name: row.get(9)?,
                    last_sender: row.get(10)?,
                    last_media_kind: row.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(summaries)
    }

    /// Advances an outgoing message's delivery state.
    ///
    /// Only moves forward: a late `delivered` receipt must not undo a `read`.
    /// Returns whether anything changed so the caller can skip a refresh.
    pub fn set_status(&self, chat: &str, id: &str, status: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let current: Option<Option<String>> = conn
            .query_row(
                "SELECT status FROM messages WHERE chat = ?1 AND id = ?2 AND from_me = 1",
                params![chat, id],
                |row| row.get(0),
            )
            .ok();
        let Some(current) = current else {
            return Ok(false);
        };
        let current_rank = current.as_deref().map(status_rank).unwrap_or(-1);
        if status_rank(status) <= current_rank {
            return Ok(false);
        }
        conn.execute(
            "UPDATE messages SET status = ?1 WHERE chat = ?2 AND id = ?3",
            params![status, chat, id],
        )?;
        Ok(true)
    }

    /// Advances an outgoing message matched by id alone, whatever chat it was
    /// stored under.
    ///
    /// Server acks carry the message id but only sometimes name the chat, and
    /// the named JID can differ in form from the stored one (LID vs phone
    /// number, device suffix, address mode). Matching on the id is what lets a
    /// `pending` message still reach `sent` in those cases, including a message
    /// sent to our own number. Returns the updated messages so the caller can
    /// forward them without re-querying.
    pub fn set_status_by_id(&self, id: &str, status: &str) -> Result<Vec<StoredMessage>> {
        let chats: Vec<(String, Option<String>)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT chat, status FROM messages WHERE id = ?1 AND from_me = 1",
            )?;
            let rows = stmt.query_map(params![id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut updated = Vec::new();
        for (chat, current) in chats {
            let current_rank = current.as_deref().map(status_rank).unwrap_or(-1);
            if status_rank(status) <= current_rank {
                continue;
            }
            {
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "UPDATE messages SET status = ?1 WHERE chat = ?2 AND id = ?3",
                    params![status, chat, id],
                )?;
            }
            if let Ok(message) = self.message(&chat, id) {
                updated.push(message);
            }
        }
        Ok(updated)
    }

    /// Marks a message as deleted by its sender, clearing its content.
    ///
    /// The row is kept so the chat shows that something was removed rather than
    /// silently losing a message. Returns whether a row was updated.
    pub fn revoke(&self, chat: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE messages
             SET revoked = 1, text = '', media_kind = NULL, media_path = NULL,
                 reply_to_id = NULL, reply_to_text = NULL
             WHERE chat = ?1 AND id = ?2 AND revoked = 0",
            params![chat, id],
        )?;
        Ok(changed > 0)
    }

    /// Marks every incoming message in a chat as read.
    ///
    /// Returns how many rows changed, so the caller can skip a refresh when
    /// nothing was unread.
    /// Unread incoming messages in `chat` as `(id, sender)`, oldest first.
    pub fn unread_ids(&self, chat: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, sender FROM messages
             WHERE chat = ?1 AND read = 0 AND from_me = 0 ORDER BY timestamp",
        )?;
        let rows = stmt.query_map(params![chat], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn mark_chat_read(&self, chat: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE messages SET read = 1 WHERE chat = ?1 AND read = 0 AND from_me = 0",
            params![chat],
        )?;
        Ok(changed)
    }

    /// Applies the retention policy, returning how many messages were dropped.
    ///
    /// Called after writes rather than on a timer so the bound holds even if the
    /// process is interrupted.
    pub fn enforce_retention(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut removed = 0;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Every chat keeps its newest message, whatever its age: a quiet chat
        // must stay in the list with its last preview, not vanish.
        const NOT_NEWEST: &str = "(chat, id) NOT IN (
             SELECT chat, id FROM (
                 SELECT chat, id, ROW_NUMBER() OVER (PARTITION BY chat ORDER BY timestamp DESC) AS rank
                 FROM messages
             ) WHERE rank = 1)";

        // A chat with its own window or cap is only bound by that one.
        if let Some(oldest) = self.retention.oldest_allowed() {
            removed += conn.execute(
                &format!(
                    "DELETE FROM messages WHERE timestamp < ?1 AND chat NOT IN
                         (SELECT jid FROM chat_retention WHERE max_age_hours IS NOT NULL)
                     AND {NOT_NEWEST}"
                ),
                params![oldest],
            )?;
        }
        removed += conn.execute(
            &format!(
                "DELETE FROM messages WHERE EXISTS (
                     SELECT 1 FROM chat_retention r WHERE r.jid = messages.chat
                     AND r.max_age_hours > 0 AND messages.timestamp < ?1 - r.max_age_hours * 3600)
                 AND {NOT_NEWEST}"
            ),
            params![now],
        )?;

        // Rank within each chat and drop everything past its cap.
        removed += conn.execute(
            "DELETE FROM messages WHERE (chat, id) IN (
                 SELECT chat, id FROM (
                     SELECT m.chat, m.id,
                            ROW_NUMBER() OVER (PARTITION BY m.chat ORDER BY m.timestamp DESC) AS rank,
                            COALESCE(r.max_messages, ?1) AS cap
                     FROM messages m LEFT JOIN chat_retention r ON r.jid = m.chat
                 ) WHERE cap > 0 AND rank > cap
             )",
            params![self.retention.max_messages_per_chat.map(|c| c as i64).unwrap_or(0)],
        )?;

        conn.execute(
            "DELETE FROM forwarded WHERE NOT EXISTS (
                 SELECT 1 FROM messages m WHERE m.chat = forwarded.chat AND m.id = forwarded.id)",
            [],
        )?;
        conn.execute(
            "DELETE FROM edited WHERE NOT EXISTS (
                 SELECT 1 FROM messages m WHERE m.chat = edited.chat AND m.id = edited.id)",
            [],
        )?;
        conn.execute(
            "DELETE FROM view_once WHERE NOT EXISTS (
                 SELECT 1 FROM messages m WHERE m.chat = view_once.chat AND m.id = view_once.id)",
            [],
        )?;
        conn.execute(
            "DELETE FROM receipts WHERE NOT EXISTS (
                 SELECT 1 FROM messages m WHERE m.id = receipts.id)",
            [],
        )?;

        if removed > 0 {
            reclaim(&conn, 2_000)?;
        }
        Ok(removed)
    }

    /// Deletes every stored message and the state attached to them. Names,
    /// chat pins and per-chat settings stay. Returns how many messages went.
    pub fn clear_history(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let removed = conn.execute("DELETE FROM messages", [])?;
        conn.execute_batch(
            "DELETE FROM reactions; DELETE FROM stars; DELETE FROM message_pins;
             DELETE FROM polls; DELETE FROM poll_votes; DELETE FROM events;
             DELETE FROM event_responses; DELETE FROM view_once; DELETE FROM forwarded;
             DELETE FROM edited; DELETE FROM receipts;",
        )?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// Total stored messages, used by tests and diagnostics.
    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn store(retention: Retention) -> MessageStore {
        // In-memory keeps tests independent and fast. The real schema is used,
        // so adding a column never breaks the tests.
        MessageStore::open(Path::new(":memory:"), retention).unwrap()
    }

    fn msg(chat: &str, id: &str, age_hours: i64, text: &str) -> StoredMessage {
        StoredMessage {
            header: MessageHeader {
                chat: chat.into(),
                id: id.into(),
                sender: "them".into(),
                timestamp: now() - age_hours * 3600,
                from_me: false,
            },
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn clearing_history_keeps_names_and_settings() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
        s.set_name("a@s", "Ann").unwrap();
        s.set_chat_auto_download("a@s", false).unwrap();
        assert_eq!(s.clear_history().unwrap(), 1);
        assert_eq!(s.count().unwrap(), 0);
        assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Ann"));
        assert_eq!(s.chat_auto_download("a@s").unwrap(), Some(false));
    }

    #[test]
    fn overlapping_batches_commit_when_the_last_drops() {
        let s = store(Retention::default());
        let autocommit = |s: &MessageStore| s.conn.lock().unwrap().is_autocommit();
        let first = s.batch();
        let second = s.batch();
        s.insert_message(&msg("a", "1", 0, "x")).unwrap();
        drop(first);
        assert!(!autocommit(&s), "still inside the second batch");
        drop(second);
        assert!(autocommit(&s), "committed");
        assert_eq!(s.messages_for("a", 10).unwrap().len(), 1);
    }

    #[test]
    fn chat_retention_overrides_the_global_policy() {
        let s = store(Retention { max_age_hours: Some(24), max_messages_per_chat: Some(1) });
        for (chat, id, age) in [("a", "1", 1), ("a", "2", 2), ("a", "3", 48), ("b", "1", 1), ("b", "2", 48), ("c", "1", 1), ("c", "2", 3)] {
            s.insert_message(&msg(chat, id, age, "x")).unwrap();
        }
        let keep_all = ChatRetention { max_age_hours: Some(0), max_messages: Some(0), on_demand: true };
        s.set_chat_retention("a", &keep_all).unwrap();
        let two_hours = ChatRetention { max_age_hours: Some(2), max_messages: None, on_demand: true };
        s.set_chat_retention("c", &two_hours).unwrap();
        s.enforce_retention().unwrap();
        assert_eq!(s.messages_for("a", 10).unwrap().len(), 3, "unlimited override");
        assert_eq!(s.messages_for("b", 10).unwrap().len(), 1, "global policy");
        // "c" keeps its own 2 h window but still falls back to the global cap.
        assert_eq!(s.messages_for("c", 10).unwrap().len(), 1);
        assert_eq!(s.chat_retention("a").unwrap(), keep_all);
        s.set_chat_retention("a", &ChatRetention::default()).unwrap();
        assert_eq!(s.chat_retention("a").unwrap(), ChatRetention::default());
    }

    #[test]
    fn receipts_only_move_forward() {
        let s = store(Retention::unlimited());
        s.record_receipt("m", "a", "delivered", 10).unwrap();
        s.record_receipt("m", "a", "read", 20).unwrap();
        s.record_receipt("m", "a", "delivered", 30).unwrap();
        s.record_receipt("m", "b", "played", 40).unwrap();
        let got = s.receipts("m").unwrap();
        let a = got.iter().find(|r| r.recipient == "a").unwrap();
        assert_eq!((a.delivered_at, a.read_at, a.played_at), (Some(10), Some(20), None));
        let b = got.iter().find(|r| r.recipient == "b").unwrap();
        assert_eq!((b.delivered_at, b.read_at, b.played_at), (Some(40), Some(40), Some(40)));
    }

    #[test]
    fn delete_message_clears_its_related_rows() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a", "1", 0, "hi")).unwrap();
        s.set_forwarded("a", "1").unwrap();
        s.apply_edit("a", "1", "edited").unwrap();
        s.set_view_once("a", "1", true).unwrap();
        s.record_receipt("1", "them", "read", 10).unwrap();

        s.delete_message("a", "1").unwrap();

        let marks = s.marks("a").unwrap();
        assert!(marks.forwarded.is_empty());
        assert!(marks.edited.is_empty());
        assert!(marks.view_once.is_empty());
        assert!(s.receipts("1").unwrap().is_empty());
    }

    #[test]
    fn edits_replace_text_and_mark_the_message() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a", "1", 0, "old")).unwrap();
        assert!(s.apply_edit("a", "1", "new").unwrap());
        assert!(!s.apply_edit("a", "missing", "new").unwrap());
        assert_eq!(s.messages_for("a", 1).unwrap()[0].text, "new");
        assert_eq!(s.marks("a").unwrap().edited, vec!["1".to_string()]);
    }

    #[test]
    fn pings_and_message_search() {
        let s = store(Retention::unlimited());
        let mut ping = msg("g", "1", 1, "hey @123 look");
        ping.local.mentioned = true;
        s.insert_message(&ping).unwrap();
        s.insert_message(&msg("g", "2", 0, "100% done_ok")).unwrap();
        let mut elsewhere = msg("h", "3", 0, "@123");
        elsewhere.local.mentioned = true;
        s.insert_message(&elsewhere).unwrap();
        assert_eq!(s.pings(Some("g"), 10).unwrap().len(), 1);
        assert_eq!(s.pings(None, 10).unwrap().len(), 2);
        assert_eq!(s.search_messages("g", "LOOK", 10).unwrap()[0].header.id, "1");
        assert_eq!(s.search_messages("g", "0% d", 10).unwrap()[0].header.id, "2");
        assert!(s.search_messages("g", "_", 10).unwrap().iter().all(|m| m.text.contains('_')));
    }

    #[test]
    fn stores_and_reads_messages() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "hello")).unwrap();
        let got = s.messages_for("a@s", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "hello");
    }

    #[test]
    fn insert_replaces_same_id() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "first")).unwrap();
        s.insert_message(&msg("a@s", "1", 0, "edited")).unwrap();
        let got = s.messages_for("a@s", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "edited");
    }

    #[test]
    fn replayed_messages_never_regress_local_state() {
        let s = store(Retention::unlimited());
        let mut sent = msg("a@s", "1", 0, "hi");
        sent.header.from_me = true;
        sent.local.status =Some("pending".into());
        s.insert_message(&sent).unwrap();
        assert!(s.set_status("a@s", "1", "delivered").unwrap());
        s.set_media_path("a@s", "1", "/tmp/1.jpg").unwrap();
        sent.local.status =None;
        s.insert_message(&sent).unwrap();
        let got = s.message("a@s", "1").unwrap();
        assert_eq!(got.local.status.as_deref(), Some("delivered"));
        assert_eq!(got.media.path.as_deref(), Some("/tmp/1.jpg"));

        s.insert_message(&msg("a@s", "2", 0, "original")).unwrap();
        s.mark_chat_read("a@s").unwrap();
        assert!(s.apply_edit("a@s", "2", "fixed").unwrap());
        s.insert_message(&msg("a@s", "2", 0, "original")).unwrap();
        let got = s.message("a@s", "2").unwrap();
        assert!(got.local.read);
        assert_eq!(got.text, "fixed");

        s.insert_message(&msg("a@s", "3", 0, "oops")).unwrap();
        assert!(s.revoke("a@s", "3").unwrap());
        s.insert_message(&msg("a@s", "3", 0, "oops")).unwrap();
        let got = s.message("a@s", "3").unwrap();
        assert!(got.local.revoked);
        assert_eq!(got.text, "");
    }

    #[test]
    fn drops_messages_older_than_the_window() {
        let s = store(Retention {
            max_age_hours: Some(24),
            max_messages_per_chat: None,
        });
        s.insert_message(&msg("a@s", "old", 48, "ancient")).unwrap();
        s.insert_message(&msg("a@s", "new", 1, "recent")).unwrap();
        assert_eq!(s.enforce_retention().unwrap(), 1);
        let got = s.messages_for("a@s", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "recent");
    }

    #[test]
    fn caps_messages_per_chat() {
        let s = store(Retention {
            max_age_hours: None,
            max_messages_per_chat: Some(3),
        });
        for i in 0..10 {
            s.insert_message(&msg("a@s", &i.to_string(), i, &format!("m{i}")))
                .unwrap();
        }
        s.enforce_retention().unwrap();
        assert_eq!(s.count().unwrap(), 3);
        // The newest survive.
        let got = s.messages_for("a@s", 10).unwrap();
        assert_eq!(got[0].text, "m0");
    }

    #[test]
    fn cap_applies_per_chat() {
        let s = store(Retention {
            max_age_hours: None,
            max_messages_per_chat: Some(2),
        });
        for i in 0..5 {
            s.insert_message(&msg("a@s", &i.to_string(), i, "x")).unwrap();
            s.insert_message(&msg("b@s", &i.to_string(), i, "y")).unwrap();
        }
        s.enforce_retention().unwrap();
        assert_eq!(s.messages_for("a@s", 99).unwrap().len(), 2);
        assert_eq!(s.messages_for("b@s", 99).unwrap().len(), 2);
    }

    #[test]
    fn summaries_are_newest_first() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("old@s", "1", 10, "older")).unwrap();
        s.insert_message(&msg("new@s", "1", 1, "newer")).unwrap();
        let chats = s.chats().unwrap();
        assert_eq!(chats[0].chat, "new@s");
        assert_eq!(chats[0].last_text, "newer");
        assert_eq!(chats[1].chat, "old@s");
    }

    #[test]
    fn names_resolve_in_reads() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("group@g.us", "1", 0, "hi")).unwrap();
        s.set_name("group@g.us", "Team Chat").unwrap();
        s.set_name("them", "Alice").unwrap();

        let got = s.messages_for("group@g.us", 10).unwrap();
        assert_eq!(got[0].sender_name.as_deref(), Some("Alice"));

        let chats = s.chats().unwrap();
        assert_eq!(chats[0].display_name.as_deref(), Some("Team Chat"));
    }

    #[test]
    fn empty_name_does_not_erase_a_known_name() {
        let s = store(Retention::unlimited());
        s.set_name("a@s", "Alice").unwrap();
        s.set_name("a@s", "   ").unwrap();
        assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
    }

    #[test]
    fn push_names_replace_a_saved_number() {
        let s = store(Retention::default());
        s.set_saved_name("1@lid", "59899022028").unwrap();
        s.set_name("1@lid", "Ana").unwrap();
        assert_eq!(s.name_for("1@lid").unwrap().as_deref(), Some("Ana"));
        s.set_saved_name("2@lid", "Bea").unwrap();
        s.set_name("2@lid", "Other").unwrap();
        assert_eq!(s.name_for("2@lid").unwrap().as_deref(), Some("Bea"));
        // A masked group label never replaces a push name, and a push name replaces it.
        s.set_name("3@lid", "Cata").unwrap();
        s.set_name("3@lid", "+598∙∙∙∙∙27").unwrap();
        assert_eq!(s.name_for("3@lid").unwrap().as_deref(), Some("Cata"));
        s.set_name("4@lid", "+598∙∙∙∙∙41").unwrap();
        s.set_name("4@lid", "Dani").unwrap();
        assert_eq!(s.name_for("4@lid").unwrap().as_deref(), Some("Dani"));
        assert!(is_placeholder_name("+598∙∙∙∙∙27") && is_placeholder_name("59899") && !is_placeholder_name("Ana"));
    }

    #[test]
    fn names_survive_message_pruning() {
        // A name is learned from a message but must outlive it, otherwise the
        // chat list falls back to a raw number once history ages out.
        let s = store(Retention {
            max_age_hours: Some(1),
            max_messages_per_chat: None,
        });
        s.insert_message(&msg("a@s", "older", 72, "hello")).unwrap();
        s.insert_message(&msg("a@s", "old", 48, "hi")).unwrap();
        s.set_name("a@s", "Alice").unwrap();
        s.enforce_retention().unwrap();
        assert_eq!(s.count().unwrap(), 1);
        assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
    }

    #[test]
    fn quiet_chats_keep_their_newest_message() {
        let s = store(Retention { max_age_hours: Some(24), max_messages_per_chat: None });
        s.insert_message(&msg("quiet@s", "1", 100, "first")).unwrap();
        s.insert_message(&msg("quiet@s", "2", 50, "last word")).unwrap();
        s.insert_message(&msg("busy@s", "1", 50, "old")).unwrap();
        s.insert_message(&msg("busy@s", "2", 1, "new")).unwrap();
        s.enforce_retention().unwrap();
        let chats = s.chats().unwrap();
        assert_eq!(chats.len(), 2, "no chat vanishes from the list");
        assert_eq!(s.messages_for("quiet@s", 9).unwrap()[0].text, "last word");
        assert_eq!(s.messages_for("busy@s", 9).unwrap().len(), 1);
    }

    #[test]
    fn unread_counts_only_incoming_unread() {
        let s = store(Retention::unlimited());
        let mut incoming = msg("a@s", "1", 0, "hi");
        incoming.local.read = false;
        s.insert_message(&incoming).unwrap();

        let mut outgoing = msg("a@s", "2", 0, "hello");
        outgoing.header.from_me = true;
        outgoing.local.read = true;
        s.insert_message(&outgoing).unwrap();

        let chats = s.chats().unwrap();
        assert_eq!(chats[0].unread_count, 1);

        assert_eq!(s.mark_chat_read("a@s").unwrap(), 1);
        assert_eq!(s.chats().unwrap()[0].unread_count, 0);
    }

    #[test]
    fn marking_read_is_idempotent() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
        assert_eq!(s.mark_chat_read("a@s").unwrap(), 1);
        // Nothing left to change the second time.
        assert_eq!(s.mark_chat_read("a@s").unwrap(), 0);
    }

    #[test]
    fn media_and_reply_fields_round_trip() {
        let s = store(Retention::unlimited());
        let mut m = msg("a@s", "1", 0, "look");
        m.media.kind = Some("image".into());
        m.media.path = Some("/tmp/pic.jpg".into());
        m.quote.id = Some("0".into());
        m.quote.text = Some("earlier".into());
        s.insert_message(&m).unwrap();

        let got = &s.messages_for("a@s", 1).unwrap()[0];
        assert_eq!(got.media.kind.as_deref(), Some("image"));
        assert_eq!(got.quote.text.as_deref(), Some("earlier"));
    }

    #[test]
    fn relocate_media_moves_only_referenced_files() {
        let root = std::env::temp_dir().join(format!("hermodr-relocate-{}", std::process::id()));
        let (shared, to) = (root.join("shared"), root.join("app"));
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("1.jpg"), b"ours").unwrap();
        std::fs::write(shared.join("other.jpg"), b"not ours").unwrap();

        let s = store(Retention::unlimited());
        let mut m = msg("a@s", "1", 0, "");
        m.media.path = Some(shared.join("1.jpg").to_string_lossy().into());
        m.media.thumb = Some(shared.join("1.jpg").to_string_lossy().into());
        s.insert_message(&m).unwrap();

        assert_eq!(s.relocate_media(&[&shared], &to).unwrap(), 2);
        let got = s.message("a@s", "1").unwrap();
        assert_eq!(got.media.path.as_deref(), Some(&*to.join("1.jpg").to_string_lossy()));
        assert_eq!(got.media.thumb, got.media.path);
        assert_eq!(std::fs::read(to.join("1.jpg")).unwrap(), b"ours");
        assert!(shared.join("other.jpg").exists());
        // Idempotent: a second run finds nothing to do.
        assert_eq!(s.relocate_media(&[&shared], &to).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn status_advances_but_never_regresses() {
        let s = store(Retention::unlimited());
        let mut m = msg("a@s", "1", 0, "hi");
        m.header.from_me = true;
        m.local.status = Some("pending".into());
        s.insert_message(&m).unwrap();

        assert!(s.set_status("a@s", "1", "sent").unwrap());
        assert!(s.set_status("a@s", "1", "delivered").unwrap());
        assert!(s.set_status("a@s", "1", "read").unwrap());
        // A late duplicate must not undo the read state.
        assert!(!s.set_status("a@s", "1", "delivered").unwrap());
        assert_eq!(s.message("a@s", "1").unwrap().local.status.as_deref(), Some("read"));
    }

    #[test]
    fn status_ignores_incoming_messages() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
        assert!(!s.set_status("a@s", "1", "read").unwrap());
    }

    #[test]
    fn status_by_id_advances_without_the_chat() {
        // Server acks name the message id but only sometimes the chat, and the
        // named JID can differ in form from the stored one. The id alone must
        // still move a pending message to sent.
        let s = store(Retention::unlimited());
        let mut m = msg("a@s.whatsapp.net", "1", 0, "hi");
        m.header.from_me = true;
        m.local.status = Some("pending".into());
        s.insert_message(&m).unwrap();

        // Wrong chat: the addressed update misses.
        assert!(!s.set_status("b@s.whatsapp.net", "1", "sent").unwrap());
        // Id-only update still advances it.
        let updated = s.set_status_by_id("1", "sent").unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].local.status.as_deref(), Some("sent"));
        assert_eq!(
            s.message("a@s.whatsapp.net", "1").unwrap().local.status.as_deref(),
            Some("sent")
        );
        // Forward-only still holds through the id path.
        assert!(s.set_status_by_id("1", "delivered").unwrap().len() == 1);
        assert!(s.set_status_by_id("1", "sent").unwrap().is_empty());
    }

    #[test]
    fn revoking_keeps_the_row_but_clears_content() {
        let s = store(Retention::unlimited());
        s.insert_message(&msg("a@s", "1", 0, "oops")).unwrap();
        assert!(s.revoke("a@s", "1").unwrap());

        let got = &s.messages_for("a@s", 1).unwrap()[0];
        assert!(got.local.revoked);
        assert_eq!(got.text, "");
        // Revoking twice changes nothing the second time.
        assert!(!s.revoke("a@s", "1").unwrap());
    }

    #[test]
    fn unlimited_retention_keeps_everything() {
        let s = store(Retention::unlimited());
        for i in 0..50 {
            s.insert_message(&msg("a@s", &i.to_string(), i * 100, "x")).unwrap();
        }
        assert_eq!(s.enforce_retention().unwrap(), 0);
        assert_eq!(s.count().unwrap(), 50);
    }
}
