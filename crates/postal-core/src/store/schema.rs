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
            assert_eq!(version(&conn), 2);
            conn.prepare(
                "SELECT media_ref, reply_to_locator, media_duration, status FROM messages",
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
        assert_eq!(version(&conn), 2);
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
        assert_eq!(version(&conn), 2);
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
        assert_eq!(version(&conn), 2);
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
        for version in [-1, 3] {
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
];

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    loop {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        anyhow::ensure!(
            (0..=MIGRATIONS.len() as i64).contains(&version),
            "unsupported message schema version {version}"
        );
        let Some(step) = MIGRATIONS.get(version as usize) else {
            tx.commit()?;
            return Ok(());
        };
        let next = version + 1;
        step(&tx).with_context(|| format!("applying message schema migration {next}"))?;
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
    }
}

// Version 0 covered several released schemas; only this adoption step probes columns.
fn migrate_v1_schema(conn: &Connection) -> Result<()> {
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
            conn.execute(
                &format!("ALTER TABLE messages ADD COLUMN {column} TEXT"),
                [],
            )?;
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
            conn.execute(
                &format!("ALTER TABLE messages ADD COLUMN {column} {decl}"),
                [],
            )?;
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
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hidden_chats (jid TEXT PRIMARY KEY)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cleared_chats (jid TEXT PRIMARY KEY)",
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
        conn.execute(
            "ALTER TABLE names ADD COLUMN saved INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

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
    Ok(())
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
