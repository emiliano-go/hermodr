use super::*;
use whatsapp_rust::wacore_binary::Jid;

#[derive(Debug, Clone)]
pub(crate) struct PendingEventRsvp {
    pub event_id: String,
    pub source_id: String,
    pub responder: String,
    pub responder_alt: Option<String>,
    pub from_me: bool,
    pub key_chat: Option<String>,
    pub creator_hint: Option<String>,
    pub key_from_me: Option<bool>,
    pub payload: Vec<u8>,
    pub iv: Vec<u8>,
    pub received_at: i64,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS event_rsvp_pending (
        chat TEXT NOT NULL,event_id TEXT NOT NULL,source_id TEXT NOT NULL,responder TEXT NOT NULL,responder_alt TEXT,
        from_me INTEGER NOT NULL,key_chat TEXT,creator_hint TEXT,key_from_me INTEGER,payload BLOB NOT NULL,
        iv BLOB NOT NULL,received_at INTEGER NOT NULL,PRIMARY KEY(chat,event_id,source_id));")?;
    Ok(())
}

const PRIVATE_SOURCE_SQL: &str = "s.deleted<>0 OR s.revoked<>0 OR s.spoiler<>0 OR s.media_once_kind IS NOT NULL
    OR s.media_kind='view_once' OR (s.system_kind IS NOT NULL AND s.system_kind<>'UNAVAILABLE_MESSAGE')";
const PRIVATE_TARGET_SQL: &str = "s.deleted<>0 OR s.revoked<>0 OR s.spoiler<>0 OR s.media_once_kind IS NOT NULL
    OR s.media_kind='view_once' OR (s.system_kind IS NOT NULL AND s.system_kind<>'UNAVAILABLE_MESSAGE')
    OR (s.system_kind IS NULL AND COALESCE(s.media_kind,'')<>'event')";

pub(super) fn purge(conn: &Connection) -> Result<()> {
    conn.execute(&format!("DELETE FROM event_rsvp_pending AS p WHERE
        EXISTS(SELECT 1 FROM hidden_chats h WHERE h.jid=p.chat) OR EXISTS(SELECT 1 FROM cleared_chats h WHERE h.jid=p.chat)
        OR EXISTS(SELECT 1 FROM view_once v WHERE v.chat=p.chat AND (v.id=p.event_id OR v.id=p.source_id))
        OR EXISTS(SELECT 1 FROM messages s WHERE s.chat=p.chat AND s.id=p.event_id AND ({PRIVATE_TARGET_SQL}))
        OR EXISTS(SELECT 1 FROM messages s WHERE s.chat=p.chat AND s.id=p.source_id AND ({PRIVATE_SOURCE_SQL}))"), [])?;
    Ok(())
}

fn admitted(conn: &Connection, chat: &str, event: &str, source: &str) -> Result<bool> {
    Ok(!conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM hidden_chats WHERE jid=?1 UNION ALL
        SELECT 1 FROM cleared_chats WHERE jid=?1 UNION ALL
        SELECT 1 FROM view_once WHERE chat=?1 AND (id=?2 OR id=?3) UNION ALL
        SELECT 1 FROM messages s WHERE s.chat=?1 AND s.id=?2 AND ({PRIVATE_TARGET_SQL}) UNION ALL
        SELECT 1 FROM messages s WHERE s.chat=?1 AND s.id=?3 AND ({PRIVATE_SOURCE_SQL}))"
        ),
        params![chat, event, source],
        |row| row.get::<_, bool>(0),
    )?)
}

fn valid(record: &PendingEventRsvp) -> bool {
    let individual = |value: &str| {
        value
            .parse::<Jid>()
            .ok()
            .is_some_and(|jid| (jid.is_pn() || jid.is_lid()) && !jid.user.is_empty())
    };
    !record.event_id.is_empty()
        && record.event_id.len() <= 256
        && !record.source_id.is_empty()
        && record.source_id.len() <= 256
        && individual(&record.responder)
        && record.responder_alt.as_deref().is_none_or(individual)
        && record.creator_hint.as_deref().is_none_or(individual)
        && record.payload.len() >= 16
        && record.payload.len() <= 4096
        && record.iv.len() == 12
        && record.received_at > 0
        && record
            .key_chat
            .as_deref()
            .is_none_or(|value| value.len() <= 256 && value.parse::<Jid>().is_ok())
}

impl MessageStore {
    pub(crate) fn queue_event_rsvp(&self, chat: &str, record: &PendingEventRsvp) -> Result<bool> {
        if !valid(record) {
            return Ok(false);
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?;
        if !admitted(&tx, &chat, &record.event_id, &record.source_id)? {
            return Ok(false);
        }
        purge(&tx)?;
        let changed=tx.execute("INSERT OR IGNORE INTO event_rsvp_pending VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![chat.as_ref(),record.event_id,record.source_id,record.responder,record.responder_alt,record.from_me,
                record.key_chat,record.creator_hint,record.key_from_me,record.payload,record.iv,record.received_at])?>0;
        // ponytail: 512 pending ciphertexts; configurable budget if bursts exceed it.
        tx.execute("DELETE FROM event_rsvp_pending WHERE rowid NOT IN(SELECT rowid FROM event_rsvp_pending ORDER BY rowid DESC LIMIT 512)",[])?;
        tx.commit()?;
        Ok(changed)
    }

    pub(crate) fn pending_event_rsvps(
        &self,
        chat: &str,
        event: Option<&str>,
    ) -> Result<Vec<PendingEventRsvp>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        purge(&conn)?;
        let mut statement=conn.prepare("SELECT event_id,source_id,responder,responder_alt,from_me,key_chat,creator_hint,key_from_me,payload,iv,received_at
            FROM event_rsvp_pending WHERE chat=?1 AND (?2 IS NULL OR event_id=?2) ORDER BY rowid")?;
        let records = statement
            .query_map(params![chat.as_ref(), event], |row| {
                Ok(PendingEventRsvp {
                    event_id: row.get(0)?,
                    source_id: row.get(1)?,
                    responder: row.get(2)?,
                    responder_alt: row.get(3)?,
                    from_me: row.get(4)?,
                    key_chat: row.get(5)?,
                    creator_hint: row.get(6)?,
                    key_from_me: row.get(7)?,
                    payload: row.get(8)?,
                    iv: row.get(9)?,
                    received_at: row.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }

    pub(crate) fn remove_pending_event_rsvp(
        &self,
        chat: &str,
        event: &str,
        source: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        conn.execute(
            "DELETE FROM event_rsvp_pending WHERE chat=?1 AND event_id=?2 AND source_id=?3",
            params![chat.as_ref(), event, source],
        )?;
        Ok(())
    }
}

impl StoreWorker {
    pub(crate) async fn queue_event_rsvp(
        &self,
        chat: &str,
        record: PendingEventRsvp,
    ) -> Result<bool> {
        let chat = chat.to_owned();
        self.run(move |store| store.queue_event_rsvp(&chat, &record))
            .await
    }
    pub(crate) async fn pending_event_rsvps(
        &self,
        chat: &str,
        event: Option<&str>,
    ) -> Result<Vec<PendingEventRsvp>> {
        let (chat, event) = (chat.to_owned(), event.map(str::to_owned));
        self.run(move |store| store.pending_event_rsvps(&chat, event.as_deref()))
            .await
    }
    pub(crate) async fn remove_pending_event_rsvp(
        &self,
        chat: &str,
        event: &str,
        source: &str,
    ) -> Result<()> {
        let (chat, event, source) = (chat.to_owned(), event.to_owned(), source.to_owned());
        self.run(move |store| store.remove_pending_event_rsvp(&chat, &event, &source))
            .await
    }
}

#[cfg(test)]
#[path = "event_rsvp_pending_tests.rs"]
mod tests;
