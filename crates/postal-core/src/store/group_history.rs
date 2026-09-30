use super::*;

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('messages') WHERE name = 'history_shareable')", [], |row| row.get(0))?;
    if !exists {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN history_shareable INTEGER NOT NULL DEFAULT 0 CHECK(history_shareable IN (0, 1));")?;
    }
    Ok(())
}

impl MessageStore {
    pub(crate) fn group_history_text(&self, chat: &str, now: i64, window: u64, limit: usize) -> Result<Vec<StoredMessage>> {
        anyhow::ensure!(chat.ends_with("@g.us"), "history sharing requires a group");
        let conn = self.conn.lock().unwrap();
        let start = now.saturating_sub(window.min(7 * 86400) as i64);
        let mut stmt = conn.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m
            LEFT JOIN names n ON n.jid = m.sender
            WHERE m.chat = ?1 AND m.timestamp BETWEEN ?2 AND ?3 AND m.history_shareable = 1
              AND m.revoked = 0 AND m.deleted = 0 AND m.media_kind IS NULL AND m.system_kind IS NULL
              AND LENGTH(CAST(m.text AS BLOB)) BETWEEN 1 AND 65536
              AND (m.from_me = 0 OR m.status IN ('sent', 'delivered', 'read'))
              AND NOT EXISTS (SELECT 1 FROM edited e WHERE e.chat = m.chat AND e.id = m.id)
            ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC LIMIT ?4"))?;
        let rows = stmt.query_map(params![chat, start, now, limit.min(100) as i64], message_row)?;
        let mut total = 0;
        let mut selected = Vec::new();
        for row in rows {
            let row = row?;
            if total + row.text.len() > 256 * 1024 { break; }
            total += row.text.len();
            selected.push(row);
        }
        selected.reverse();
        Ok(selected)
    }
}

impl StoreWorker {
    pub(crate) async fn group_history_text(&self, chat: &str, now: i64, window: u64, limit: usize) -> Result<Vec<StoredMessage>> {
        let chat = chat.to_owned();
        self.run(move |store| store.group_history_text(&chat, now, window, limit)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_provenance_is_internal_and_history_selection_stays_bounded() {
        let mut row = StoredMessage { history_shareable: true, text: "x".repeat(4096),
            header: MessageHeader { chat: "1@g.us".into(), timestamp: 100, ..Default::default() }, ..Default::default() };
        let mut json = serde_json::to_value(&row).unwrap();
        assert!(json.get("history_shareable").is_none());
        json["history_shareable"] = serde_json::json!(true);
        assert!(!serde_json::from_value::<StoredMessage>(json).unwrap().history_shareable);
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for index in 0..110 {
            row.header.id = format!("{index:03}");
            store.insert_message(&row).unwrap();
        }
        let rows = store.group_history_text("1@g.us", 100, u64::MAX, usize::MAX).unwrap();
        assert_eq!(rows.len(), 64);
        assert_eq!(rows.iter().map(|row| row.text.len()).sum::<usize>(), 256 * 1024);
        assert_eq!(rows.last().unwrap().header.id, "109");
        row.text = "x".into();
        for index in 0..110 {
            row.header.id = format!("{index:03}");
            store.insert_message(&row).unwrap();
        }
        assert_eq!(store.group_history_text("1@g.us", 100, u64::MAX, usize::MAX).unwrap().len(), 100);
    }

    #[test]
    fn group_history_excludes_unverified_private_expired_unsent_and_changed_rows() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for (id, chat, at, known, from_me, status) in [
            ("good", "1@g.us", 99, true, false, None), ("own", "1@g.us", 100, true, true, Some("sent")),
            ("legacy", "1@g.us", 100, false, false, None), ("private", "2@s.whatsapp.net", 100, true, false, None),
            ("other-group", "2@g.us", 100, true, false, None), ("old", "1@g.us", 1, true, false, None),
            ("future", "1@g.us", 101, true, false, None), ("pending", "1@g.us", 100, true, true, Some("pending")),
            ("revoked", "1@g.us", 100, true, false, None), ("deleted", "1@g.us", 100, true, false, None),
            ("edited", "1@g.us", 100, true, false, None),
        ] {
            store.insert_message(&StoredMessage { history_shareable: known, text: id.into(),
                header: MessageHeader { chat: chat.into(), id: id.into(), timestamp: at, from_me, ..Default::default() },
                local: LocalState { status: status.map(str::to_owned), ..Default::default() }, ..Default::default() }).unwrap();
        }
        store.revoke_message("1@g.us", "revoked").unwrap();
        store.conn.lock().unwrap().execute("UPDATE messages SET deleted = 1 WHERE id = 'deleted'", []).unwrap();
        store.update_message_content("1@g.us", "edited", "changed").unwrap();
        let rows = store.group_history_text("1@g.us", 100, 10, 100).unwrap();
        assert_eq!(rows.iter().map(|row| row.header.id.as_str()).collect::<Vec<_>>(), ["good", "own"]);
        assert_eq!(store.group_history_text("1@g.us", 100, 10, 1).unwrap()[0].header.id, "own");
        assert!(store.group_history_text("2@s.whatsapp.net", 100, 10, 100).is_err());
        let mut duplicate = rows[0].clone();
        duplicate.history_shareable = false;
        store.insert_message(&duplicate).unwrap();
        duplicate.history_shareable = true;
        store.insert_message(&duplicate).unwrap();
        assert!(!store.message("1@g.us", "good").unwrap().history_shareable);
    }
}
