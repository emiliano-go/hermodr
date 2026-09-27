use super::*;

pub const MAX_MESSAGE_PAGE: u32 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageCursor {
    pub timestamp: i64,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePageDirection { #[default] Before, After, Through }

#[derive(Debug, Serialize)]
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
        let filter = if cursor.is_some() { format!("AND (m.timestamp, m.id) {comparison} (?3, ?4)") } else { String::new() };
        let mut statement = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1 {filter} ORDER BY m.timestamp {order}, m.id {order} LIMIT ?2"
        ))?;
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&chat, &fetch];
        if let Some(cursor) = cursor { values.extend([&cursor.timestamp as &dyn rusqlite::ToSql, &cursor.id]); }
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
            cursor = page.messages.last().map(|m| MessageCursor { id: m.header.id.clone(), timestamp: m.header.timestamp });
            seen.extend(page.messages.iter().map(|m| m.header.id.clone()));
            if !page.has_more { break; }
        }
        assert_eq!(seen, (0..2_005).rev().map(|n| format!("{n:04}")).collect::<Vec<_>>());
        let cursor = MessageCursor { id: "0500".into(), timestamp: 100 };
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
    }
}
