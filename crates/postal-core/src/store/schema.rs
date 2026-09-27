use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use super::chats;

pub(super) fn migrate(conn: &Connection) -> Result<()> {
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
    Ok(())
}
