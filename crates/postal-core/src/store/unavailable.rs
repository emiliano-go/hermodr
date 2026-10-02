use super::*;

pub(crate) const UNAVAILABLE_MESSAGE: &str = "UNAVAILABLE_MESSAGE";
pub(super) const VISIBLE_MESSAGE_SQL: &str = "NOT (deleted <> 0 AND text = '' AND media_kind IS NULL AND system_kind IS NULL)";

impl StoredMessage {
    pub(crate) fn is_unavailable(&self) -> bool {
        self.system.kind.as_deref() == Some(UNAVAILABLE_MESSAGE)
    }

    pub(crate) fn is_hidden_tombstone(&self) -> bool {
        self.local.deleted && self.text.is_empty() && self.media.kind.is_none() && self.system.kind.is_none()
    }
}

impl MessageStore {
    pub(crate) fn unavailable_unread(&self, chat: &str, up_to: Option<&str>) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        conn.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE chat = ?1
            AND system_kind = 'UNAVAILABLE_MESSAGE' AND read = 0 AND from_me = 0
            AND (?2 IS NULL OR (timestamp, sort_order, id) <=
                (SELECT timestamp, sort_order, id FROM messages WHERE chat = ?1 AND id = ?2)))",
            params![chat.as_ref(), up_to], |row| row.get(0)).map_err(Into::into)
    }

    pub(crate) fn insert_unavailable(&self, header: &MessageHeader) -> Result<Option<StoredMessage>> {
        if header.id.is_empty() || header.chat.is_empty() || header.timestamp <= 0 || header.chat == "status@broadcast" { return Ok(None); }
        let stored = {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.savepoint()?;
            let mut header = header.clone();
            header.chat = names::canonical_chat(&tx, &header.chat)?.into_owned();
            let blocked: bool = tx.query_row("SELECT
                EXISTS(SELECT 1 FROM messages WHERE chat = ?1 AND id = ?2)
                OR EXISTS(SELECT 1 FROM view_once WHERE chat = ?1 AND id = ?2)
                OR EXISTS(SELECT 1 FROM hidden_chats WHERE jid = ?1)
                OR EXISTS(SELECT 1 FROM cleared_chats WHERE jid = ?1)",
                params![header.chat, header.id], |row| row.get(0))?;
            if blocked { return Ok(None); }
            let stored = StoredMessage {
                local: LocalState { read: header.from_me, ..Default::default() },
                header,
                system: SystemNotice { kind: Some(UNAVAILABLE_MESSAGE.into()), params: Vec::new() },
                // The marker cannot be shared; recovered content supplies the actual eligibility.
                history_shareable: true,
                ..Default::default()
            };
            Self::insert_row(&tx, &stored)?;
            tx.execute("INSERT INTO chats (jid, last_message_at) VALUES (?1, ?2)
                ON CONFLICT(jid) DO UPDATE SET last_message_at = MAX(chats.last_message_at, excluded.last_message_at)",
                params![stored.header.chat, stored.header.timestamp])?;
            tx.commit()?;
            stored
        };
        self.message(&stored.header.chat, &stored.header.id).map(Some)
    }

    pub(crate) fn insert_history_row(&self, message: &StoredMessage) -> Result<Option<StoredMessage>> {
        let mut message = message.clone();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.savepoint()?;
            message.header.chat = names::canonical_chat(&tx, &message.header.chat)?.into_owned();
            let existing = tx.query_row("SELECT system_kind, read, revoked, deleted FROM messages WHERE chat = ?1 AND id = ?2",
                params![message.header.chat, message.header.id], |row| Ok((row.get::<_, Option<String>>(0)?,
                    row.get::<_, bool>(1)?, row.get::<_, bool>(2)?, row.get::<_, bool>(3)?))).optional()?;
            if let Some((kind, read, revoked, deleted)) = existing {
                if kind.as_deref() != Some(UNAVAILABLE_MESSAGE) || revoked || deleted { return Ok(None); }
                message.local.read = read;
            }
            Self::insert_row(&tx, &message)?;
            self.revive_chat(&tx, &message.header.chat)?;
            tx.commit()?;
        }
        self.message(&message.header.chat, &message.header.id).map(Some)
    }

    pub(crate) fn retire_unavailable(&self, chat: &str, id: &str) -> Result<Option<MessageHeader>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned();
        let header = tx.query_row("SELECT sender, timestamp, from_me FROM messages
            WHERE chat = ?1 AND id = ?2 AND system_kind = ?3",
            params![chat, id, UNAVAILABLE_MESSAGE], |row| Ok(MessageHeader {
                chat: chat.clone(), id: id.to_owned(), sender: row.get(0)?, timestamp: row.get(1)?, from_me: row.get(2)?,
            })).optional()?;
        if header.is_some() {
            tx.execute("UPDATE messages SET system_kind = NULL, system_params = NULL, deleted = 1, read = 1
                WHERE chat = ?1 AND id = ?2 AND system_kind = ?3", params![chat, id, UNAVAILABLE_MESSAGE])?;
        }
        tx.commit()?;
        Ok(header)
    }

    pub(crate) fn retire_quiz_source(&self, chat: &str, poll: &str, id: &str) -> Result<Option<MessageHeader>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned();
        let header = tx.query_row("SELECT sender,timestamp,from_me FROM messages s
            WHERE chat=?1 AND id=?2 AND system_kind=?3 AND deleted=0 AND revoked=0 AND spoiler=0
                AND media_once_kind IS NULL AND COALESCE(media_kind,'')<>'view_once'
                AND EXISTS(SELECT 1 FROM quiz_source_retirements r WHERE r.chat=s.chat AND r.update_id=s.id AND r.poll=?4)",
            params![chat,id,UNAVAILABLE_MESSAGE,poll], |row| Ok(MessageHeader {
                chat: chat.clone(), id: id.into(), sender: row.get(0)?, timestamp: row.get(1)?, from_me: row.get(2)?,
            })).optional()?;
        if header.is_some() {
            tx.execute("UPDATE messages SET system_kind=NULL,system_params=NULL,deleted=1,read=1 WHERE chat=?1 AND id=?2",
                params![chat,id])?;
        }
        tx.commit()?;
        Ok(header)
    }

    pub(crate) fn insert_view_once_stub(&self, message: &StoredMessage) -> Result<Option<StoredMessage>> {
        let mut message = message.clone();
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.savepoint()?;
            message.header.chat = names::canonical_chat(&tx, &message.header.chat)?.into_owned();
            let existing = tx.query_row("SELECT system_kind, revoked, deleted FROM messages WHERE chat = ?1 AND id = ?2",
                params![message.header.chat, message.header.id], |row| Ok((row.get::<_, Option<String>>(0)?,
                    row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))).optional()?;
            if existing.is_some_and(|(kind, revoked, deleted)| kind.as_deref() != Some(UNAVAILABLE_MESSAGE) || revoked || deleted) { return Ok(None); }
            tx.execute("INSERT OR IGNORE INTO view_once (chat, id, opened) VALUES (?1, ?2, ?3)",
                params![message.header.chat, message.header.id, message.header.from_me])?;
            Self::insert_row(&tx, &message)?;
            tx.commit()?;
        }
        self.message(&message.header.chat, &message.header.id).map(Some)
    }
}

impl StoreWorker {
    pub(crate) async fn retire_quiz_source(&self, chat: &str, poll: &str, id: &str) -> Result<Option<MessageHeader>> {
        let (chat,poll,id) = (chat.to_owned(),poll.to_owned(),id.to_owned());
        self.run(move |store| store.retire_quiz_source(&chat,&poll,&id)).await
    }
    pub(crate) async fn retire_unavailable(&self, chat: &str, id: &str) -> Result<Option<MessageHeader>> {
        let chat = chat.to_owned(); let id = id.to_owned();
        self.run(move |store| store.retire_unavailable(&chat, &id)).await
    }

    pub(crate) async fn insert_view_once_stub(&self, message: &StoredMessage) -> Result<Option<StoredMessage>> {
        let message = message.clone();
        self.run(move |store| store.insert_view_once_stub(&message)).await
    }
    pub(crate) async fn unavailable_unread(&self, chat: &str, up_to: Option<&str>) -> Result<bool> {
        let chat = chat.to_owned();
        let up_to = up_to.map(str::to_owned);
        self.run(move |store| store.unavailable_unread(&chat, up_to.as_deref())).await
    }

    pub(crate) async fn insert_unavailable(&self, header: &MessageHeader) -> Result<Option<StoredMessage>> {
        let header = header.clone();
        self.run(move |store| store.insert_unavailable(&header)).await
    }

    pub(crate) async fn insert_history_row(&self, message: &StoredMessage) -> Result<Option<StoredMessage>> {
        let message = message.clone();
        self.run(move |store| store.insert_history_row(&message)).await
    }
}

#[cfg(test)]
#[path = "unavailable_tests.rs"]
mod tests;
