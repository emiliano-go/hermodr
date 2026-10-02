use super::*;
use whatsapp_rust::wacore_binary::{Jid, JidExt};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct BroadcastList {
    pub chat: String,
    pub recipients: Vec<String>,
    pub source_timestamp: i64,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS broadcast_lists (
        chat TEXT PRIMARY KEY, source_id TEXT NOT NULL,
        source_timestamp INTEGER NOT NULL, recipients TEXT NOT NULL);",
    )?;
    Ok(())
}

fn broadcast_chat(chat: &str) -> Option<String> {
    let jid: Jid = chat.parse().ok()?;
    (jid.is_broadcast_list() && !jid.user.is_empty()).then(|| jid.to_non_ad().to_string())
}

fn normalize_recipients(recipients: &[String]) -> Option<Vec<String>> {
    if recipients.is_empty() {
        return None;
    }
    let mut normalized = Vec::with_capacity(recipients.len());
    for recipient in recipients {
        let jid: Jid = recipient.parse().ok()?;
        if !(jid.is_pn() || jid.is_lid()) || jid.user.is_empty() {
            return None;
        }
        normalized.push(jid.to_non_ad().to_string());
    }
    normalized.sort();
    normalized.dedup();
    Some(normalized)
}

fn public_source_timestamp(conn: &Connection, chat: &str, id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            &format!("SELECT m.timestamp FROM messages m WHERE m.chat=?1 AND m.id=?2 AND ({PUBLIC_SOURCE_SQL})"),
            params![chat, id],
            |row| row.get(0),
        )
        .optional()?)
}

const PUBLIC_SOURCE_SQL: &str =
    "m.deleted=0 AND m.revoked=0 AND m.spoiler=0 AND m.media_once_kind IS NULL
    AND COALESCE(m.media_kind,'')<>'view_once' AND m.system_kind IS NULL
    AND NOT EXISTS(SELECT 1 FROM hidden_chats WHERE jid=m.chat)";

pub(super) fn purge_unavailable_sources(conn: &Connection) -> Result<()> {
    conn.execute(&format!("DELETE FROM broadcast_lists WHERE NOT EXISTS(
        SELECT 1 FROM messages m WHERE m.chat=broadcast_lists.chat AND m.id=broadcast_lists.source_id
        AND ({PUBLIC_SOURCE_SQL}))"), [])?;
    Ok(())
}

impl MessageStore {
    pub(crate) fn remember_broadcast_list(
        &self,
        chat: &str,
        source_id: &str,
        recipients: &[String],
    ) -> Result<bool> {
        let (Some(chat), Some(recipients)) =
            (broadcast_chat(chat), normalize_recipients(recipients))
        else {
            return Ok(false);
        };
        if source_id.is_empty() || source_id.len() > 256 {
            return Ok(false);
        }
        let conn = self.conn.lock().unwrap();
        let Some(timestamp) = public_source_timestamp(&conn, &chat, source_id)?.filter(|t| *t > 0)
        else {
            return Ok(false);
        };
        Ok(conn.execute(
            "INSERT INTO broadcast_lists VALUES(?1,?2,?3,?4)
            ON CONFLICT(chat) DO UPDATE SET source_id=excluded.source_id,
            source_timestamp=excluded.source_timestamp, recipients=excluded.recipients
            WHERE excluded.source_timestamp>broadcast_lists.source_timestamp",
            params![
                chat,
                source_id,
                timestamp,
                serde_json::to_string(&recipients)?
            ],
        )? > 0)
    }

    pub(crate) fn broadcast_list(&self, chat: &str) -> Result<Option<BroadcastList>> {
        let Some(chat) = broadcast_chat(chat) else {
            return Ok(None);
        };
        let conn = self.conn.lock().unwrap();
        let snapshot = conn
            .query_row(
                "SELECT source_id,source_timestamp,recipients FROM broadcast_lists WHERE chat=?1",
                [&chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((source_id, source_timestamp, recipients)) = snapshot else {
            return Ok(None);
        };
        if public_source_timestamp(&conn, &chat, &source_id)?.is_none() {
            return Ok(None);
        }
        Ok(Some(BroadcastList {
            chat,
            source_timestamp,
            recipients: serde_json::from_str(&recipients)?,
        }))
    }
}

impl StoreWorker {
    pub(crate) async fn remember_broadcast_list(
        &self,
        chat: &str,
        source_id: &str,
        recipients: Vec<String>,
    ) -> Result<bool> {
        let (chat, source_id) = (chat.to_owned(), source_id.to_owned());
        self.run(move |store| store.remember_broadcast_list(&chat, &source_id, &recipients))
            .await
    }

    pub(crate) async fn broadcast_list(&self, chat: &str) -> Result<Option<BroadcastList>> {
        let chat = chat.to_owned();
        self.run(move |store| store.broadcast_list(&chat)).await
    }
}

#[cfg(test)]
#[path = "broadcast_lists_tests.rs"]
mod tests;
