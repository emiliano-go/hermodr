//! Delivery and read state: receipts, outgoing status and unread marks.

use super::*;

impl MessageStore {
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

    /// Advances an outgoing message's delivery state.
    ///
    /// Only moves forward: a late `delivered` receipt must not undo a `read`.
    /// Returns whether anything changed so the caller can skip a refresh.
    pub fn set_delivery_state(&self, chat: &str, id: &str, status: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let current: Option<Option<String>> = conn
            .query_row(
                "SELECT status FROM messages WHERE chat = ?1 AND id = ?2 AND from_me = 1",
                params![chat, id],
                |row| row.get(0),
            )
            .optional()?;
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
    pub fn set_delivery_state_by_id(&self, id: &str, status: &str) -> Result<Vec<StoredMessage>> {
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

    /// Unread incoming messages in `chat` as `(id, sender)`, oldest first.
    pub fn unread_ids(&self, chat: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let mut stmt = conn.prepare(
            "SELECT id, sender FROM messages
             WHERE chat = ?1 AND read = 0 AND from_me = 0 ORDER BY timestamp",
        )?;
        let rows = stmt.query_map(params![chat], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Marks every incoming message in a chat as read.
    ///
    /// Returns how many rows changed, so the caller can skip a refresh when
    /// nothing was unread.
    pub fn mark_read(&self, chat: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let changed = conn.execute(
            "UPDATE messages SET read = 1 WHERE chat = ?1 AND read = 0 AND from_me = 0",
            params![chat],
        )?;
        Ok(changed)
    }

    /// Marks incoming messages at or before `timestamp` as read, for a read
    /// state another device synced with a message range.
    pub fn mark_read_through(&self, chat: &str, timestamp: i64) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let changed = conn.execute(
            "UPDATE messages SET read = 1
             WHERE chat = ?1 AND read = 0 AND from_me = 0 AND timestamp <= ?2",
            params![chat, timestamp],
        )?;
        Ok(changed)
    }

    /// Unread incoming messages up to and including `id`, oldest first.
    ///
    /// The timestamp/id boundary matches message paging.
    pub fn unread_until(&self, chat: &str, id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let mut stmt = conn.prepare(
            "SELECT id, sender FROM messages
             WHERE chat = ?1 AND read = 0 AND from_me = 0
               AND (timestamp, id) <= (SELECT timestamp, id FROM messages WHERE chat = ?1 AND id = ?2)
             ORDER BY timestamp, id",
        )?;
        let rows = stmt.query_map(params![chat, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Marks incoming messages up to and including `id` as read.
    pub fn mark_read_until(&self, chat: &str, id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let changed = conn.execute(
            "UPDATE messages SET read = 1
             WHERE chat = ?1 AND read = 0 AND from_me = 0
               AND (timestamp, id) <= (SELECT timestamp, id FROM messages WHERE chat = ?1 AND id = ?2)",
            params![chat, id],
        )?;
        Ok(changed)
    }
}

impl StoreWorker {
    pub(crate) async fn record_receipt(&self, id: &str, recipient: &str, kind: &str, at: i64) -> Result<()> {
        let id = id.to_owned();
        let recipient = recipient.to_owned();
        let kind = kind.to_owned();
        self.run(move |store| store.record_receipt(&id, &recipient, &kind, at)).await
    }

    pub(crate) async fn receipts(&self, id: &str) -> Result<Vec<MessageReceipt>> {
        let id = id.to_owned();
        self.run(move |store| store.receipts(&id)).await
    }

    pub(crate) async fn set_delivery_state(&self, chat: &str, id: &str, status: &str) -> Result<bool> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let status = status.to_owned();
        self.run(move |store| store.set_delivery_state(&chat, &id, &status)).await
    }

    pub(crate) async fn set_delivery_state_by_id(&self, id: &str, status: &str) -> Result<Vec<StoredMessage>> {
        let id = id.to_owned();
        let status = status.to_owned();
        self.run(move |store| store.set_delivery_state_by_id(&id, &status)).await
    }

    pub(crate) async fn unread_ids(&self, chat: &str) -> Result<Vec<(String, String)>> {
        let chat = chat.to_owned();
        self.run(move |store| store.unread_ids(&chat)).await
    }

    pub(crate) async fn mark_read(&self, chat: &str) -> Result<usize> {
        let chat = chat.to_owned();
        self.run(move |store| store.mark_read(&chat)).await
    }

    pub(crate) async fn mark_read_through(&self, chat: &str, timestamp: i64) -> Result<usize> {
        let chat = chat.to_owned();
        self.run(move |store| store.mark_read_through(&chat, timestamp)).await
    }

    pub(crate) async fn unread_until(&self, chat: &str, id: &str) -> Result<Vec<(String, String)>> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.unread_until(&chat, &id)).await
    }

    pub(crate) async fn mark_read_until(&self, chat: &str, id: &str) -> Result<usize> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.mark_read_until(&chat, &id)).await
    }
}
