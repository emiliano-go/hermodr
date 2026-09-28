#[cfg(test)]
use super::chats;
use anyhow::{Context, Result};
use rusqlite::Connection;

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn chat_metadata_migration_rolls_back_and_backfills_existing_and_cleared_chats() {
        let conn = legacy();
        for step in &MIGRATIONS[..4] { step(&conn).unwrap(); }
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute_batch(
            "INSERT INTO cleared_chats VALUES ('empty@s.whatsapp.net');
             CREATE TABLE chats (jid TEXT PRIMARY KEY, last_message_at INTEGER NOT NULL DEFAULT 0);
             CREATE TRIGGER fail_metadata BEFORE INSERT ON chats BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;",
        ).unwrap();
        assert!(migrate(&conn).is_err());
        assert_eq!(version(&conn), 4);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, u32>(0)).unwrap(), 0);
        conn.execute_batch("DROP TRIGGER fail_metadata").unwrap();
        migrate(&conn).unwrap();
        let rows = conn.prepare("SELECT jid, last_message_at FROM chats ORDER BY jid").unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(rows, vec![("1@s.whatsapp.net".into(), 123), ("empty@s.whatsapp.net".into(), 0)]);
        conn.execute_batch("DELETE FROM messages").unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, u32>(0)).unwrap(), 2);
    }

    #[test]
    fn explicit_limits_migrate_legacy_overrides_without_changing_their_policy() {
        let conn = Connection::open_in_memory().unwrap();
        for step in &MIGRATIONS[..5] { step(&conn).unwrap(); }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch(
            "INSERT INTO chat_retention (jid, max_age_hours, max_messages, on_demand) VALUES
                 ('inherit', NULL, NULL, 1), ('unlimited', 0, 0, 0), ('bounded', 24, 500, 1);
             CREATE VIEW legacy_chat_retention AS SELECT 1;",
        ).unwrap();
        assert!(migrate(&conn).is_err());
        assert_eq!(version(&conn), 5);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM chat_retention", [], |r| r.get::<_, u32>(0)).unwrap(), 3);
        conn.execute_batch("DROP VIEW legacy_chat_retention").unwrap();
        migrate(&conn).unwrap();
        let rows = conn.prepare("SELECT jid, age_mode, max_age_hours, count_mode, max_messages, on_demand FROM chat_retention ORDER BY jid").unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?,
                crate::store::RetentionLimit::from_row(r, 1, 2)?,
                crate::store::RetentionLimit::from_row(r, 3, 4)?,
                r.get::<_, bool>(5)?))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        use crate::store::RetentionLimit::*;
        assert_eq!(rows, vec![
            ("bounded".into(), Limited(24), Limited(500), true),
            ("inherit".into(), Inherit, Inherit, true),
            ("unlimited".into(), Unlimited, Unlimited, false),
        ]);
    }

    #[test]
    fn caption_migration_recovers_batches_without_overwriting_message_state() {
        use buffa::{Message, MessageField};
        use whatsapp_rust::prelude::wa;
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        let caption = "Look @12345\nsecond line";
        let locator = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some(caption.into()), ..Default::default()
            }), ..Default::default()
        }.encode_to_vec();
        for id in 0..135 {
            conn.execute(
                "INSERT INTO messages (chat, id, sender, timestamp, from_me, text, media_kind, media_ref)
                 VALUES ('chat', ?1, 'them', 1, 0, '[image]', 'image', ?2)",
                (id.to_string(), &locator),
            ).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO edited VALUES ('chat', '128');
             UPDATE messages SET revoked = 1 WHERE id = '129';
             INSERT INTO view_once VALUES ('chat', '130', 0);
             UPDATE messages SET text = 'custom' WHERE id = '131';
             UPDATE messages SET media_ref = X'FF' WHERE id = '132';",
        ).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_caption BEFORE UPDATE OF text ON messages
            WHEN OLD.id = '133' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(migrate(&conn).is_err());
        assert_eq!(version(&conn), 2);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages WHERE text = ?1", [caption],
            |r| r.get::<_, i64>(0)).unwrap(), 0);
        conn.execute_batch("DROP TRIGGER fail_caption").unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages WHERE text = ?1", [caption],
            |r| r.get::<_, i64>(0)).unwrap(), 130);
        for (id, expected) in [(128, "[image]"), (129, "[image]"), (130, "[image]"), (131, "custom"), (132, "[image]")] {
            assert_eq!(conn.query_row("SELECT text FROM messages WHERE id = ?1", [id.to_string()],
                |r| r.get::<_, String>(0)).unwrap(), expected);
        }
    }

    fn legacy() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                chat TEXT NOT NULL, id TEXT NOT NULL, sender TEXT NOT NULL,
                timestamp INTEGER NOT NULL, from_me INTEGER NOT NULL, text TEXT NOT NULL,
                PRIMARY KEY (chat, id));
             CREATE TABLE names (jid TEXT PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO messages VALUES ('1@s.whatsapp.net', 'm', '1@s.whatsapp.net', 123, 0, 'kept');
             INSERT INTO names VALUES ('1:2@lid', 'Ada');",
        ).unwrap();
        conn
    }

    #[test]
    fn fresh_and_legacy_databases_reach_current_version() {
        for (conn, expected) in [(Connection::open_in_memory().unwrap(), 0), (legacy(), 1)] {
            migrate(&conn).unwrap();
            assert_eq!(version(&conn), MIGRATIONS.len() as i64);
            conn.prepare(
                "SELECT media_ref, reply_to_locator, media_duration, status, deleted FROM messages",
            )
            .unwrap();
            conn.prepare("SELECT secret FROM polls").unwrap();
            conn.prepare("SELECT secret FROM events").unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE text = 'kept'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                count,
                conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0))
                    .unwrap()
            );
            assert_eq!(count, expected);
        }
    }

    #[test]
    fn a_v10_database_gains_the_soft_delete_column_on_open() {
        use crate::store::MessageStore;
        let path = std::env::temp_dir().join(format!(
            "postal-softdelete-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        {
            // The schema as released before soft delete: one version behind head.
            let conn = Connection::open(&path).unwrap();
            migrate_to(&conn, MIGRATIONS.len() - 1).unwrap();
            conn.execute_batch(
                "INSERT INTO messages (chat, id, sender, timestamp, from_me, text)
                     VALUES ('a@s', '1', 'them', 1, 0, 'hi');",
            ).unwrap();
        }
        let store = MessageStore::open(&path).unwrap();
        // Both of these select `m.deleted`; they must work and read false.
        assert!(store.chats().is_ok());
        assert!(!store.message("a@s", "1").unwrap().local.deleted);
        drop(store);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn unversioned_current_database_preserves_secrets_and_folds_address_forms() {
        let conn = legacy();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 0;
             INSERT INTO polls VALUES ('1@s.whatsapp.net', 'poll', 'me', 'Question', '[]', 0, X'010203');
             INSERT INTO events VALUES ('1@s.whatsapp.net', 'event', 'me', 'Meeting', NULL, NULL, NULL, NULL, NULL, 0, X'040506');
             INSERT INTO lid_pn VALUES ('9', '1');
             INSERT INTO messages (chat, id, sender, timestamp, from_me, text)
                 VALUES ('9@lid', 'older', '9@lid', 100, 0, 'old');
             INSERT INTO chat_state VALUES ('9@lid', 1, -1, 1);",
        ).unwrap();
        migrate(&conn).unwrap();
        chats::reconcile_addresses(&conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(
            conn.query_row("SELECT secret FROM polls", [], |r| r.get::<_, Vec<u8>>(0))
                .unwrap(),
            [1, 2, 3]
        );
        assert_eq!(
            conn.query_row("SELECT secret FROM events", [], |r| r.get::<_, Vec<u8>>(0))
                .unwrap(),
            [4, 5, 6]
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE chat = '1@s.whatsapp.net'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT archived FROM chat_state WHERE jid = '1@s.whatsapp.net'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            true
        );
    }

    #[test]
    fn completed_migrations_are_not_replayed() {
        let conn = legacy();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO names (jid, name) VALUES ('later:3@lid', 'Later')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM names WHERE jid = 'later@lid'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn failed_cleanup_rolls_back_its_data_and_version_then_retries() {
        let conn = legacy();
        conn.execute_batch(
            "INSERT INTO messages VALUES ('status@broadcast', 'status', 'them', 10, 0, 'status');
             INSERT INTO names VALUES ('masked@lid', '+598∙∙27');
             CREATE TRIGGER fail_cleanup BEFORE DELETE ON names BEGIN SELECT RAISE(ABORT, 'test failure'); END;",
        ).unwrap();
        assert!(migrate(&conn).is_err());
        assert_eq!(version(&conn), 1);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE chat = 'status@broadcast'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM names WHERE jid = 'masked@lid'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        conn.execute_batch("DROP TRIGGER fail_cleanup").unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE chat = 'status@broadcast'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT name FROM names WHERE jid = '1@lid'", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "Ada"
        );
    }

    #[test]
    fn failed_schema_step_leaves_no_partial_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIEW names AS SELECT '1@lid' AS jid, 'Ada' AS name")
            .unwrap();
        assert!(migrate(&conn).is_err());
        assert_eq!(version(&conn), 0);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn unsupported_version_is_rejected_without_schema_changes() {
        for version in [-1, MIGRATIONS.len() as i64 + 1] {
            let conn = Connection::open_in_memory().unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            assert!(migrate(&conn).is_err());
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}

const MIGRATIONS: &[fn(&Connection) -> Result<()>] = &[
    migrate_v1_schema,
    migrate_v2_legacy_data,
    migrate_v3_media_captions,
    migrate_v4_quote_chat_index,
    migrate_v5_chat_metadata,
    migrate_v6_explicit_limits,
    migrate_v7_media_paths,
    migrate_v8_page_cursor,
    migrate_v9_stickers,
    migrate_v10_message_order,
    migrate_v11_soft_delete,
];

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    migrate_to(conn, MIGRATIONS.len())
}

pub(super) fn migrate_to(conn: &Connection, target: usize) -> Result<()> {
    anyhow::ensure!(target <= MIGRATIONS.len(), "unsupported message schema version {target}");
    loop {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        anyhow::ensure!(
            (0..=MIGRATIONS.len() as i64).contains(&version),
            "unsupported message schema version {version}"
        );
        if version as usize == target {
            tx.commit()?;
            return Ok(());
        }
        anyhow::ensure!((version as usize) < target, "cannot downgrade message schema");
        let step = MIGRATIONS[version as usize];
        let next = version + 1;
        step(&tx).with_context(|| format!("applying message schema migration {next}"))?;
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
    }
}

// Version 0 covered several released schemas; only this adoption step probes columns.
fn migrate_v1_schema(conn: &Connection) -> Result<()> {
    create_base_tables(conn)?;
    let existing = table_columns(conn, "messages")?;
    add_missing_columns(conn, &existing, ADDED_MESSAGE_COLUMNS)?;
    create_state_tables(conn)?;
    // The media reference is a blob, so it cannot go through the TEXT
    // migration list above.
    let message_columns = table_columns(conn, "messages")?;
    if !message_columns.iter().any(|c| c == "media_ref") {
        conn.execute("ALTER TABLE messages ADD COLUMN media_ref BLOB", [])?;
    }
    let name_columns = table_columns(conn, "names")?;
    if !name_columns.iter().any(|c| c == "saved") {
        conn.execute("ALTER TABLE names ADD COLUMN saved INTEGER NOT NULL DEFAULT 0", [])?;
    }
    add_missing_columns(
        conn,
        &existing,
        &[
            ("read", "INTEGER NOT NULL DEFAULT 0"),
            ("revoked", "INTEGER NOT NULL DEFAULT 0"),
            ("status", "TEXT"),
        ],
    )?;
    Ok(())
}

/// The two tables every released schema has.
fn create_base_tables(conn: &Connection) -> Result<()> {
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
    Ok(())
}

/// Everything that hangs off a message or a chat.
fn create_state_tables(conn: &Connection) -> Result<()> {
    create_mark_tables(conn)?;
    create_chat_tables(conn)?;
    Ok(())
}

/// The tables holding per-message state.
fn create_mark_tables(conn: &Connection) -> Result<()> {
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
    Ok(())
}

/// The tables holding per-chat settings and list state.
fn create_chat_tables(conn: &Connection) -> Result<()> {
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
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hidden_chats (jid TEXT PRIMARY KEY)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cleared_chats (jid TEXT PRIMARY KEY)",
        [],
    )?;
    Ok(())
}

/// Columns added after the first release. SQLite has no "ADD COLUMN IF NOT
/// EXISTS", so the existing set is inspected first.
const ADDED_MESSAGE_COLUMNS: &[(&str, &str)] = &[
    ("media_kind", "TEXT"),
    ("media_path", "TEXT"),
    ("media_thumb", "TEXT"),
    ("reply_to_id", "TEXT"),
    ("reply_to_text", "TEXT"),
    ("reply_to_sender", "TEXT"),
    ("reply_to_chat", "TEXT"),
    ("reply_to_kind", "TEXT"),
    ("reply_to_thumb", "TEXT"),
    ("reply_to_path", "TEXT"),
    ("preview_url", "TEXT"),
    ("preview_title", "TEXT"),
    ("preview_desc", "TEXT"),
    ("preview_thumb", "TEXT"),
    ("preview_site", "TEXT"),
    ("preview_color", "TEXT"),
    ("system_kind", "TEXT"),
    ("system_params", "TEXT"),
    ("mentioned", "INTEGER NOT NULL DEFAULT 0"),
    ("media_duration", "INTEGER"),
    // The view-once a reply quotes, and the only copy of it a linked device
    // is ever sent. `reply_to_locator` is the quoted message as it arrived
    // (older rows: the bare media submessage), so the copy can be fetched
    // and quoted again in the same form.
    ("reply_to_view_once", "INTEGER NOT NULL DEFAULT 0"),
    ("reply_to_recoverable", "INTEGER NOT NULL DEFAULT 0"),
    ("reply_to_locator", "BLOB"),
    // The kind a view-once had before it was marked as one, so a recovered
    // copy is shown by the player that fits it.
    ("media_once_kind", "TEXT"),
];

/// Adds the columns of `columns` the table does not have yet.
fn add_missing_columns(conn: &Connection, existing: &[String], columns: &[(&str, &str)]) -> Result<()> {
    for (column, decl) in columns {
        if !existing.iter().any(|c| c == column) {
            conn.execute(&format!("ALTER TABLE messages ADD COLUMN {column} {decl}"), [])?;
        }
    }
    Ok(())
}

/// A table's column names, for the probes SQLite cannot express as DDL.
fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn migrate_v2_legacy_data(conn: &Connection) -> Result<()> {
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
    Ok(())
}

fn migrate_v3_media_captions(conn: &Connection) -> Result<()> {
    use buffa::Message;
    use whatsapp_rust::{prelude::wa, wacore::proto_helpers::MessageExt};

    let mut after = i64::MIN;
    loop {
        let batch = conn.prepare(
            "SELECT rowid, media_ref FROM messages m
             WHERE rowid > ?1 AND media_kind IN ('image', 'video', 'gif', 'document')
               AND text = '[' || media_kind || ']' AND media_ref IS NOT NULL AND revoked = 0
               AND NOT EXISTS (SELECT 1 FROM edited e WHERE e.chat = m.chat AND e.id = m.id)
               AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat = m.chat AND v.id = m.id)
             ORDER BY rowid LIMIT 128",
        )?.query_map([after], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if batch.is_empty() { break; }
        for (row, locator) in batch {
            after = row;
            let Ok(message) = wa::Message::decode(&mut locator.as_slice()) else { continue };
            if let Some(caption) = message.get_caption().filter(|text| !text.is_empty()) {
                conn.execute("UPDATE messages SET text = ?1 WHERE rowid = ?2", (caption, row))?;
            }
        }
    }
    Ok(())
}

fn migrate_v4_quote_chat_index(conn: &Connection) -> Result<()> {
    conn.execute("CREATE INDEX IF NOT EXISTS idx_messages_reply_chat ON messages (reply_to_chat)
        WHERE reply_to_chat IS NOT NULL", [])?;
    Ok(())
}

fn migrate_v5_chat_metadata(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chats (jid TEXT PRIMARY KEY, last_message_at INTEGER NOT NULL DEFAULT 0);
         INSERT OR IGNORE INTO chats (jid, last_message_at)
             SELECT chat, MAX(timestamp) FROM messages
             WHERE system_kind IS NULL OR system_kind LIKE 'CALL_MISSED%'
                OR system_kind LIKE 'SILENCED_UNKNOWN_CALLER%' OR chat LIKE '%@g.us'
             GROUP BY chat;
         INSERT OR IGNORE INTO chats (jid) SELECT jid FROM cleared_chats;
         CREATE TRIGGER IF NOT EXISTS remember_message_chat AFTER INSERT ON messages
         WHEN NEW.system_kind IS NULL OR NEW.system_kind LIKE 'CALL_MISSED%'
           OR NEW.system_kind LIKE 'SILENCED_UNKNOWN_CALLER%' OR NEW.chat LIKE '%@g.us'
         BEGIN
             INSERT INTO chats (jid, last_message_at) VALUES (NEW.chat, NEW.timestamp)
             ON CONFLICT(jid) DO UPDATE SET last_message_at = MAX(chats.last_message_at, excluded.last_message_at);
         END;",
    )?;
    Ok(())
}

fn migrate_v6_explicit_limits(conn: &Connection) -> Result<()> {
    let columns = conn.prepare("PRAGMA table_info(chat_retention)")?
        .query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if columns.iter().any(|c| c == "age_mode" || c == "count_mode") {
        anyhow::ensure!(columns.iter().any(|c| c == "age_mode") && columns.iter().any(|c| c == "count_mode"),
            "incomplete explicit retention schema");
        return Ok(());
    }
    conn.execute_batch(
        "ALTER TABLE chat_retention RENAME TO legacy_chat_retention;
         CREATE TABLE chat_retention (
             jid TEXT PRIMARY KEY,
             age_mode TEXT NOT NULL CHECK(age_mode IN ('inherit', 'unlimited', 'limited')),
             max_age_hours INTEGER CHECK(max_age_hours >= 0),
             count_mode TEXT NOT NULL CHECK(count_mode IN ('inherit', 'unlimited', 'limited')),
             max_messages INTEGER CHECK(max_messages >= 0),
             on_demand INTEGER NOT NULL DEFAULT 1,
             CHECK((age_mode = 'limited') = (max_age_hours IS NOT NULL)),
             CHECK((count_mode = 'limited') = (max_messages IS NOT NULL))
         );
         INSERT INTO chat_retention
             SELECT jid,
                 CASE WHEN max_age_hours IS NULL THEN 'inherit' WHEN max_age_hours <= 0 THEN 'unlimited' ELSE 'limited' END,
                 CASE WHEN max_age_hours > 0 THEN max_age_hours ELSE NULL END,
                 CASE WHEN max_messages IS NULL THEN 'inherit' WHEN max_messages <= 0 THEN 'unlimited' ELSE 'limited' END,
                 CASE WHEN max_messages > 0 THEN max_messages ELSE NULL END,
                 on_demand
             FROM legacy_chat_retention;
         DROP TABLE legacy_chat_retention;",
    )?;
    Ok(())
}

fn migrate_v7_media_paths(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_messages_media_path ON messages(media_path) WHERE media_path IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_messages_quote_path ON messages(reply_to_path) WHERE reply_to_path IS NOT NULL;",
    )?;
    Ok(())
}

fn migrate_v8_page_cursor(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_messages_chat_time;
        CREATE INDEX idx_messages_chat_time ON messages(chat, timestamp DESC, id DESC);")?;
    Ok(())
}

/// Sticker packs and their stickers, keyed the way app-state sync addresses them:
/// pack id and the base64 file hash. Strictly upsert-only; a remote removal
/// clears a flag rather than dropping a row, so nothing is ever deleted here.
fn migrate_v9_stickers(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sticker_packs (
             pack_id TEXT PRIMARY KEY,
             name TEXT,
             publisher TEXT,
             tray_path TEXT,
             origin TEXT,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS stickers (
             filehash TEXT PRIMARY KEY,
             pack_id TEXT,
             path TEXT,
             animated INTEGER NOT NULL DEFAULT 0,
             lottie INTEGER NOT NULL DEFAULT 0,
             emojis TEXT,
             favorite INTEGER NOT NULL DEFAULT 0,
             recent_at INTEGER,
             updated_at INTEGER NOT NULL,
             media_ref BLOB
         );
         CREATE INDEX IF NOT EXISTS idx_stickers_pack ON stickers(pack_id);
         CREATE INDEX IF NOT EXISTS idx_stickers_recent ON stickers(recent_at DESC);",
    )?;
    Ok(())
}

fn migrate_v10_message_order(conn: &Connection) -> Result<()> {
    let columns = table_columns(conn, "messages")?;
    if !columns.iter().any(|column| column == "sort_order") {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK(sort_order >= 0);")?;
    }
    conn.execute_batch("UPDATE messages SET sort_order = rowid WHERE sort_order = 0;
        CREATE TABLE IF NOT EXISTS message_order_counter (id INTEGER PRIMARY KEY CHECK(id = 1), value INTEGER NOT NULL CHECK(value >= 0));
        INSERT INTO message_order_counter VALUES (1, COALESCE((SELECT MAX(sort_order) FROM messages), 0))
            ON CONFLICT(id) DO UPDATE SET value = MAX(value, excluded.value);
        CREATE TRIGGER IF NOT EXISTS assign_message_order AFTER INSERT ON messages BEGIN
            UPDATE message_order_counter SET value = CASE WHEN NEW.sort_order > 0 THEN MAX(value, NEW.sort_order) ELSE value + 1 END WHERE id = 1;
            UPDATE messages SET sort_order = (SELECT value FROM message_order_counter WHERE id = 1)
                WHERE rowid = NEW.rowid AND sort_order = 0;
        END;
        DROP INDEX IF EXISTS idx_messages_chat_time;
        CREATE INDEX idx_messages_chat_time ON messages(chat, timestamp DESC, sort_order DESC, id DESC);")?;
    Ok(())
}

/// Soft delete: the row stays, flagged on this device only. A column added
/// after v10, so it must probe rather than assume the adoption step ran.
fn migrate_v11_soft_delete(conn: &Connection) -> Result<()> {
    let columns = table_columns(conn, "messages")?;
    if !columns.iter().any(|column| column == "deleted") {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;")?;
    }
    Ok(())
}
