use super::*;

pub const MAX_MESSAGE_PAGE: u32 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageCursor {
    pub timestamp: i64,
    pub id: String,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum MessagePageDirection { #[default] Before, After, Through }

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessagePage {
    pub messages: Vec<StoredMessage>,
    pub has_more: bool,
}

impl MessageStore {
    pub fn message_page(&self, chat: &str, limit: u32, cursor: Option<&MessageCursor>, direction: MessagePageDirection) -> Result<MessagePage> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let limit = limit.clamp(1, MAX_MESSAGE_PAGE) as usize;
        let fetch = (limit + 1) as i64;
        let (comparison, order) = match direction {
            MessagePageDirection::Before => ("<", "DESC"),
            MessagePageDirection::After => (">", "ASC"),
            MessagePageDirection::Through => ("<=", "DESC"),
        };
        let sort_order = if let Some(cursor) = cursor {
            if cursor.sort_order > 0 { cursor.sort_order } else {
                conn.query_row("SELECT sort_order FROM messages WHERE chat = ?1 AND id = ?2", params![chat, cursor.id], |r| r.get(0)).optional()?
                    .ok_or_else(|| anyhow::anyhow!("message cursor expired; reload the chat"))?
            }
        } else { 0 };
        let filter = if cursor.is_some() { format!("AND (m.timestamp, m.sort_order, m.id) {comparison} (?3, ?4, ?5)") } else { String::new() };
        let mut statement = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1 {filter} ORDER BY m.timestamp {order}, m.sort_order {order}, m.id {order} LIMIT ?2"
        ))?;
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&chat, &fetch];
        if let Some(cursor) = cursor { values.extend([&cursor.timestamp as &dyn rusqlite::ToSql, &sort_order, &cursor.id]); }
        let mut messages = statement.query_map(values.as_slice(), message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = messages.len() > limit;
        messages.truncate(limit);
        if matches!(direction, MessagePageDirection::After) { messages.reverse(); }
        Ok(MessagePage { messages, has_more })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_preview_preserves_unread_mentions_and_stored_state() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for n in 0..8 {
            store.insert_message(&StoredMessage {
                header: MessageHeader { chat: "preview@s".into(), id: n.to_string(), timestamp: n,
                    sender: "peer@s".into(), from_me: false },
                local: LocalState { read: n == 0, mentioned: n == 7, ..Default::default() },
                media: Media { kind: Some("image".into()), ..Default::default() },
                ..Default::default()
            }).unwrap();
        }
        store.set_marked_unread("preview@s", true).unwrap();
        let before = store.chats().unwrap();
        let stored = store.messages_for("preview@s", 20).unwrap();
        let page = store.message_page("preview@s", 5, None, MessagePageDirection::Before).unwrap();
        assert_eq!(page.messages.iter().map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["7", "6", "5", "4", "3"]);
        assert_eq!(store.chats().unwrap(), before);
        assert_eq!(store.messages_for("preview@s", 20).unwrap(), stored);
        let conn = store.conn.lock().unwrap();
        let mut plan = conn.prepare("EXPLAIN QUERY PLAN SELECT id FROM messages WHERE chat = ?1 ORDER BY timestamp DESC, sort_order DESC, id DESC LIMIT 5").unwrap();
        let details = plan.query_map(["preview@s"], |row| row.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert!(details.iter().any(|detail| detail.contains("idx_messages_chat_time")), "{details:?}");
        assert!(!details.iter().any(|detail| detail.contains("TEMP B-TREE")), "{details:?}");
    }

    #[test]
    fn equal_timestamp_messages_keep_arrival_order_instead_of_id_order() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        for id in ["z", "a", "m"] {
            store.insert_message(&StoredMessage { header: MessageHeader {
                chat: "order@s".into(), id: id.into(), timestamp: 100, ..Default::default()
            }, ..Default::default() }).unwrap();
        }
        let page = store.message_page("order@s", 2, None, MessagePageDirection::Before).unwrap();
        assert_eq!(page.messages.iter().map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["m", "a"]);
        assert_eq!(store.unread_until("order@s", "a").unwrap().iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(store.mark_read_until("order@s", "a").unwrap(), 2);
        assert!(!store.message("order@s", "m").unwrap().local.read);
        assert_eq!(store.messages_for("order@s", 3).unwrap().iter().map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["m", "a", "z"]);
        let anchor = store.message("order@s", "a").unwrap();
        let cursor = MessageCursor { id: "a".into(), timestamp: 100, sort_order: anchor.local.sort_order };
        let mut replay = anchor.clone();
        replay.local.sort_order = 0;
        store.insert_message(&replay).unwrap();
        assert_eq!(store.message("order@s", "a").unwrap().local.sort_order, cursor.sort_order);
        store.conn.lock().unwrap().execute("DELETE FROM messages WHERE id = 'a'", []).unwrap();
        store.conn.lock().unwrap().execute_batch("VACUUM").unwrap();
        let before = store.message_page("order@s", 3, Some(&cursor), MessagePageDirection::Before).unwrap();
        let after = store.message_page("order@s", 3, Some(&cursor), MessagePageDirection::After).unwrap();
        assert_eq!(before.messages[0].header.id, "z");
        assert_eq!(after.messages[0].header.id, "m");
        assert_eq!(store.chats().unwrap()[0].last_message_at, 100);
    }

    #[test]
    fn legacy_order_survives_reopen_and_counter_never_reuses_deleted_order() {
        let path = std::env::temp_dir().join(format!("postal-order-{}-{}.db", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let conn = Connection::open(&path).unwrap();
        super::super::schema::migrate_to(&conn, 9).unwrap();
        for id in ["z", "a", "m"] {
            conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text) VALUES ('legacy@s',?1,'peer@s',100,0,?1)", [id]).unwrap();
        }
        drop(conn);
        let store = MessageStore::open(&path).unwrap();
        let order = store.message("legacy@s", "m").unwrap().local.sort_order;
        assert_eq!(store.chats().unwrap()[0].last_text, "m");
        store.conn.lock().unwrap().execute("DELETE FROM messages WHERE id = 'm'", []).unwrap();
        drop(store);
        let store = MessageStore::open(&path).unwrap();
        store.insert_message(&StoredMessage { header: MessageHeader {
            chat: "legacy@s".into(), id: "0".into(), timestamp: 100, ..Default::default()
        }, text: "latest".into(), ..Default::default() }).unwrap();
        assert!(store.message("legacy@s", "0").unwrap().local.sort_order > order);
        assert_eq!(store.chats().unwrap()[0].last_text, "latest");
        assert_eq!(store.message_page("legacy@s", 3, None, MessagePageDirection::Before).unwrap().messages.iter()
            .map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["0", "a", "z"]);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pages_cover_timestamp_ties_in_both_directions_without_deleting_rows() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let batch = store.batch();
        for n in 0..2_005 {
            store.insert_message(&StoredMessage {
                header: MessageHeader { chat: "test@s".into(), id: format!("{n:04}"), timestamp: 100,
                    sender: "peer@s".into(), from_me: false }, ..Default::default()
            }).unwrap();
        }
        drop(batch);
        let mut cursor = None;
        let mut seen = Vec::new();
        loop {
            let page = store.message_page("test@s", 113, cursor.as_ref(), MessagePageDirection::Before).unwrap();
            cursor = page.messages.last().map(|m| MessageCursor { id: m.header.id.clone(), timestamp: m.header.timestamp, sort_order: m.local.sort_order });
            seen.extend(page.messages.iter().map(|m| m.header.id.clone()));
            if !page.has_more { break; }
        }
        assert_eq!(seen, (0..2_005).rev().map(|n| format!("{n:04}")).collect::<Vec<_>>());
        let cursor = MessageCursor { id: "0500".into(), timestamp: 100, sort_order: 0 };
        let after = store.message_page("test@s", 2, Some(&cursor), MessagePageDirection::After).unwrap();
        assert_eq!(after.messages.iter().map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["0502", "0501"]);
        assert!(after.has_more);
        let through = store.message_page("test@s", 1, Some(&cursor), MessagePageDirection::Through).unwrap();
        assert_eq!(through.messages[0].header.id, "0500");
        assert_eq!(store.message_page("test@s", u32::MAX, None, MessagePageDirection::Before).unwrap().messages.len(), MAX_MESSAGE_PAGE as usize);
        assert_eq!(store.count().unwrap(), 2_005);
        for id in ["0050", "0060"] {
            store.set_starred("test@s", id, true).unwrap();
            store.set_reaction("test@s", id, "peer@s", "x").unwrap();
            store.save_poll("test@s", id, "peer@s", "Question", &["A".into()], false, None).unwrap();
            store.set_poll_vote("test@s", id, "peer@s", &["A".into()]).unwrap();
        }
        let marks = store.marks_for("test@s", Some(&["0050".into()])).unwrap();
        assert_eq!(marks.starred, ["0050"]);
        assert_eq!(marks.reactions.len(), 1);
        assert_eq!(marks.polls.len(), 1);
        assert_eq!(marks.polls[0].votes.len(), 1);
        assert!(store.marks_for("test@s", Some(&[])).unwrap().starred.is_empty());
        for id in ["c", "a", "b"] {
            store.insert_message(&StoredMessage { header: MessageHeader {
                chat: "ties@s".into(), id: id.into(), timestamp: 200, sender: "peer@s".into(), ..Default::default()
            }, ..Default::default() }).unwrap();
        }
        assert_eq!(store.unread_until("ties@s", "a").unwrap().iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["c", "a"]);
        assert_eq!(store.mark_read_until("ties@s", "a").unwrap(), 2);
        assert!(!store.message("ties@s", "b").unwrap().local.read);
    }
}

impl StoreWorker {
    pub(crate) async fn message_page(&self, chat: &str, limit: u32, cursor: Option<&MessageCursor>, direction: MessagePageDirection) -> Result<MessagePage> {
        let chat = chat.to_owned();
        let cursor = cursor.cloned();
        self.run(move |store| store.message_page(&chat, limit, cursor.as_ref(), direction)).await
    }
}
