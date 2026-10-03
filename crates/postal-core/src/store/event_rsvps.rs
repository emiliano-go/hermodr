use super::*;
use crate::message_ref::MessageRef;
use whatsapp_rust::wacore_binary::Jid;

pub(super) const PUBLIC_EVENT: &str = "m.media_kind='event' AND m.deleted=0 AND m.revoked=0 AND m.spoiler=0
    AND m.system_kind IS NULL AND m.media_once_kind IS NULL
    AND NOT EXISTS(SELECT 1 FROM hidden_chats h WHERE h.jid=m.chat)
    AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id)";

#[derive(Debug, Clone)]
pub(crate) struct EventRsvpUpdate {
    pub response: String,
    pub timestamp_ms: Option<i64>,
    pub extra_guest_count: Option<i32>,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventRsvpToken(RowState);

#[derive(Debug, Clone)]
pub(crate) struct EventRsvpContext {
    pub secret: Secretive,
    pub invitation_id: Option<String>,
    pub invitation: bool,
    pub canceled: bool,
    pub event: NewEvent,
    pub extra_guests_allowed: bool,
    pub prior: Option<EventRsvpToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RowState {
    response: String,
    timestamp_ms: Option<i64>,
    extra_guest_count: Option<i32>,
    source_id: Option<String>,
    watermark_ms: Option<i64>,
    revision: i64,
    ack_lower_bound_ms: Option<i64>,
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    for (table, column, definition) in [
        ("events", "extra_guests_allowed", "INTEGER CHECK(extra_guests_allowed IN (0,1))"),
        ("events", "is_scheduled_call", "INTEGER CHECK(is_scheduled_call IN (0,1))"),
        ("events", "has_reminder", "INTEGER CHECK(has_reminder IN (0,1))"),
        ("events", "invitation", "INTEGER NOT NULL DEFAULT 0 CHECK(invitation IN (0,1))"),
        ("events", "reminder_offset_sec", "INTEGER"), ("events", "invitation_id", "TEXT"),
        ("event_responses", "timestamp_ms", "INTEGER"),
        ("event_responses", "extra_guest_count", "INTEGER CHECK(extra_guest_count>=0)"),
        ("event_responses", "source_id", "TEXT"), ("event_responses", "watermark_ms", "INTEGER"),
        ("event_responses", "revision", "INTEGER NOT NULL DEFAULT 0"),
        ("event_responses", "ack_lower_bound_ms", "INTEGER"),
    ] {
        let exists: bool = conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"),
            [column], |row| row.get(0))?;
        if !exists { conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?; }
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS event_rsvp_clock (
        id INTEGER PRIMARY KEY CHECK(id=1), value INTEGER NOT NULL CHECK(typeof(value)='integer' AND value>=0));
        INSERT OR IGNORE INTO event_rsvp_clock(id,value) VALUES(1,0);
        UPDATE event_responses SET watermark_ms=timestamp_ms WHERE watermark_ms IS NULL AND timestamp_ms IS NOT NULL;")?;
    Ok(())
}

fn responder(conn: &Connection, value: &str) -> Result<String> {
    if value == "@me" { return Ok(value.to_owned()); }
    let jid: Jid = value.parse::<Jid>().map_err(|error|
        anyhow::Error::new(MessageRef::new("error.event_responder")).context(error.to_string()))?;
    anyhow::ensure!(!jid.user.is_empty() && (jid.is_pn() || jid.is_lid()), MessageRef::new("error.event_responder"));
    Ok(names::canonical_chat(conn, &jid.to_non_ad().to_string())?.into_owned())
}

fn token_id(value: &str) -> Result<()> {
    anyhow::ensure!(!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control), MessageRef::new("error.event_source_id"));
    Ok(())
}

fn row_state(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<RowState> {
    Ok(RowState { response: row.get(offset)?, timestamp_ms: row.get(offset + 1)?, extra_guest_count: row.get(offset + 2)?,
        source_id: row.get(offset + 3)?, watermark_ms: row.get(offset + 4)?, revision: row.get(offset + 5)?,
        ack_lower_bound_ms: row.get(offset + 6)? })
}

fn read_state(conn: &Connection, chat: &str, event: &str, who: &str) -> Result<Option<RowState>> {
    Ok(conn.query_row("SELECT response,timestamp_ms,extra_guest_count,source_id,watermark_ms,revision,ack_lower_bound_ms
        FROM event_responses WHERE chat=?1 AND event=?2 AND responder=?3", params![chat, event, who], |row| row_state(row, 0)).optional()?)
}

fn write_state(conn: &Connection, chat: &str, event: &str, who: &str, state: &RowState) -> Result<()> {
    conn.execute("INSERT INTO event_responses(chat,event,responder,response,timestamp_ms,extra_guest_count,source_id,watermark_ms,revision,ack_lower_bound_ms)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(chat,event,responder) DO UPDATE SET
        response=excluded.response,timestamp_ms=excluded.timestamp_ms,extra_guest_count=excluded.extra_guest_count,
        source_id=excluded.source_id,watermark_ms=excluded.watermark_ms,revision=excluded.revision,ack_lower_bound_ms=excluded.ack_lower_bound_ms",
        params![chat, event, who, state.response, state.timestamp_ms, state.extra_guest_count,
            state.source_id, state.watermark_ms, state.revision, state.ack_lower_bound_ms])?;
    Ok(())
}

fn revision(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("UPDATE event_rsvp_clock SET value=MAX(value,
        (SELECT COALESCE(MAX(revision),0) FROM event_responses))+1 WHERE id=1 RETURNING value", [], |row| row.get(0))?)
}

fn reconcile_event(conn: &Connection, chat: &str, event: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT responder,response,timestamp_ms,extra_guest_count,source_id,watermark_ms,revision,ack_lower_bound_ms
        FROM event_responses WHERE chat=?1 AND event=?2 ORDER BY revision,rowid")?;
    let rows = stmt.query_map(params![chat, event], |row| Ok((row.get::<_, String>(0)?, row_state(row, 1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut winners = std::collections::BTreeMap::new();
    let mut changed = false;
    for (original, state) in &rows {
        let who = responder(conn, original).unwrap_or_else(|_| original.clone());
        changed |= who != *original || winners.contains_key(&who);
        let winner = winners.entry(who).or_insert_with(|| state.clone());
        let behind_ack = winner.ack_lower_bound_ms.is_some_and(|bound| state.watermark_ms.is_some_and(|time| time < bound));
        let newer_ack = state.ack_lower_bound_ms.is_some_and(|bound| winner.watermark_ms.is_some_and(|time| time < bound));
        if !behind_ack && (newer_ack || state.watermark_ms > winner.watermark_ms) { *winner = state.clone(); }
    }
    if changed {
        conn.execute("DELETE FROM event_responses WHERE chat=?1 AND event=?2", params![chat, event])?;
        for (who, state) in winners { write_state(conn, chat, event, &who, &state)?; }
    }
    Ok(())
}

pub(crate) fn reconcile_event_responders(conn: &Connection) -> Result<()> {
    let columns: u32 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('event_responses') WHERE name IN
        ('timestamp_ms','extra_guest_count','source_id','watermark_ms','revision','ack_lower_bound_ms')", [], |row| row.get(0))?;
    if columns != 6 { return Ok(()); }
    let mut stmt = conn.prepare("SELECT chat,event,responder,response,timestamp_ms,extra_guest_count,source_id,
        watermark_ms,revision,ack_lower_bound_ms FROM event_responses ORDER BY revision,rowid")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
        row.get::<_, String>(2)?, row_state(row, 3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut winners = std::collections::BTreeMap::new(); let mut changed = false;
    for (chat, event, who, state) in rows {
        let target = names::canonical_chat(conn, &chat)?.into_owned();
        let identity = responder(conn, &who).unwrap_or_else(|_| who.clone());
        let key = (target.clone(), event, identity.clone());
        changed |= target != chat || identity != who || winners.contains_key(&key);
        let winner = winners.entry(key).or_insert_with(|| state.clone());
        let behind_ack = winner.ack_lower_bound_ms.is_some_and(|bound| state.watermark_ms.is_some_and(|time| time < bound));
        let newer_ack = state.ack_lower_bound_ms.is_some_and(|bound| winner.watermark_ms.is_some_and(|time| time < bound));
        if !behind_ack && (newer_ack || state.watermark_ms > winner.watermark_ms) { *winner = state; }
    }
    if !changed { return Ok(()); }
    conn.execute_batch("SAVEPOINT postal_event_aliases")?;
    let result = (|| -> Result<()> {
        conn.execute("DELETE FROM event_responses", [])?;
        for ((chat, event, who), state) in winners { write_state(conn, &chat, &event, &who, &state)?; }
        Ok(())
    })();
    if result.is_err() { conn.execute_batch("ROLLBACK TO postal_event_aliases")?; }
    conn.execute_batch("RELEASE postal_event_aliases")?;
    result
}

pub(super) fn public_parent(conn: &Connection, chat: &str, event: &str) -> Result<bool> {
    Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM events e JOIN messages m ON m.chat=e.chat AND m.id=e.id
        WHERE e.chat=?1 AND e.id=?2 AND ({PUBLIC_EVENT}))"), params![chat, event], |row| row.get(0))?)
}

fn known_response(value: &str) -> &str {
    match value { "going" | "not_going" | "maybe" => value, _ => "" }
}

fn fresher(previous: &RowState, update: &EventRsvpUpdate) -> bool {
    if previous.ack_lower_bound_ms.is_some_and(|bound| update.timestamp_ms.is_some_and(|time| time < bound))
        && previous.source_id.as_deref() != Some(update.source_id.as_str()) { return false; }
    match (previous.watermark_ms, update.timestamp_ms) {
        (Some(old), Some(new)) => new > old || new == old && previous.timestamp_ms.is_none()
            && previous.source_id.as_deref() == Some(update.source_id.as_str())
            && previous.response == known_response(&update.response) && previous.extra_guest_count == update.extra_guest_count,
        (Some(_), None) => false,
        (None, Some(_)) => true,
        (None, None) => previous.source_id.is_none(),
    }
}

impl MessageStore {
    pub(crate) fn fill_event_secret(&self, chat: &str, id: &str, approved_creators: &[String], secret: &[u8]) -> Result<bool> {
        anyhow::ensure!(secret.len() == 32, MessageRef::new("error.event_secret_length").with_param("expected_bytes", serde_json::Number::from(32)));
        anyhow::ensure!(!approved_creators.is_empty() && approved_creators.len() <= 32
            && approved_creators.iter().all(|creator| !creator.is_empty() && creator.len() <= 256
                && !creator.chars().any(char::is_control)), MessageRef::new("error.event_approved_creators"));
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        let creators = serde_json::to_string(approved_creators)?;
        let changed = conn.execute(&format!("UPDATE events SET secret=?3 WHERE chat=?1 AND id=?2 AND invitation=0
            AND (secret IS NULL OR length(secret)<>32) AND creator IN (SELECT value FROM json_each(?4))
            AND EXISTS(SELECT 1 FROM messages m WHERE m.chat=events.chat AND m.id=events.id AND ({PUBLIC_EVENT}))"),
            params![chat.as_ref(), id, secret, creators])?;
        Ok(changed > 0)
    }

    pub(crate) fn apply_event_rsvp(&self, chat: &str, event: &str, who: &str, update: &EventRsvpUpdate) -> Result<bool> {
        token_id(&update.source_id)?;
        anyhow::ensure!(update.timestamp_ms.is_none_or(|time| time >= 0) && update.extra_guest_count.is_none_or(|guests| guests >= 0),
            MessageRef::new("error.event_timestamp_guests"));
        let mut conn = self.conn.lock().unwrap(); let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned(); let who = responder(&tx, who)?;
        if !public_parent(&tx, &chat, event)? { return Ok(false); }
        reconcile_event(&tx, &chat, event)?;
        let prior = read_state(&tx, &chat, event, &who)?;
        if prior.as_ref().is_some_and(|prior| !fresher(prior, update)) { return Ok(false); }
        let state = RowState { response: known_response(&update.response).into(), timestamp_ms: update.timestamp_ms,
            extra_guest_count: update.extra_guest_count, source_id: Some(update.source_id.clone()),
            watermark_ms: update.timestamp_ms, revision: revision(&tx)?, ack_lower_bound_ms: None };
        write_state(&tx, &chat, event, &who, &state)?; tx.commit()?;
        Ok(true)
    }

    pub(super) fn legacy_event_response(&self, chat: &str, event: &str, who: &str, response: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap(); let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned();
        let who = responder(&tx, who).unwrap_or_else(|_| who.to_owned());
        if !public_parent(&tx, &chat, event)? { return Ok(()); }
        reconcile_event(&tx, &chat, event)?;
        if read_state(&tx, &chat, event, &who)?.is_some_and(|state| state.watermark_ms.is_some() || state.source_id.is_some()
            || state.response == response) { return Ok(()); }
        write_state(&tx, &chat, event, &who, &RowState { response: response.into(), timestamp_ms: None,
            extra_guest_count: None, source_id: None, watermark_ms: None, revision: revision(&tx)?, ack_lower_bound_ms: None })?;
        tx.commit()?; Ok(())
    }

    pub(crate) fn event_rsvp_context(&self, chat: &str, event: &str, who: &str) -> Result<Option<EventRsvpContext>> {
        let mut conn = self.conn.lock().unwrap(); let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned(); let who = responder(&tx, who)?;
        if !public_parent(&tx, &chat, event)? { return Ok(None); }
        let row: Option<(String, Option<Vec<u8>>)> = tx.query_row("SELECT creator,secret FROM events WHERE chat=?1 AND id=?2",
            params![chat, event], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let Some((creator, Some(secret))) = row else { return Ok(None) };
        if secret.len() != 32 { return Ok(None); }
        let metadata = metadata(&tx, &chat, event)?.ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.event_metadata_missing")))?;
        reconcile_event(&tx, &chat, event)?;
        let prior = read_state(&tx, &chat, event, &who)?.map(EventRsvpToken);
        tx.commit()?;
        Ok(Some(EventRsvpContext { secret: Secretive { creator, secret, options: Vec::new() },
            invitation_id: metadata.invitation_id.clone(), invitation: metadata.invitation, canceled: metadata.canceled,
            extra_guests_allowed: metadata.extra_guests_allowed == Some(true), event: metadata, prior }))
    }

    #[cfg(test)]
    pub(crate) fn commit_event_rsvp_ack(&self, chat: &str, event: &str, who: &str, prior: &Option<EventRsvpToken>,
        response: &str, guests: Option<i32>, source_id: &str) -> Result<bool> {
        self.commit_event_rsvp_ack_with_bound(chat, event, who, prior, response, guests, source_id, None)
    }

    pub(crate) fn commit_event_rsvp_ack_with_bound(&self, chat: &str, event: &str, who: &str, prior: &Option<EventRsvpToken>,
        response: &str, guests: Option<i32>, source_id: &str, request_started_ms: Option<i64>) -> Result<bool> {
        token_id(source_id)?;
        anyhow::ensure!(!known_response(response).is_empty() && guests.is_none_or(|guests| guests >= 0)
            && request_started_ms.is_none_or(|time| time >= 0), MessageRef::new("error.event_own_response"));
        let mut conn = self.conn.lock().unwrap(); let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?.into_owned(); let who = responder(&tx, who)?;
        if !public_parent(&tx, &chat, event)? { return Ok(false); }
        let allowed: bool = tx.query_row("SELECT length(secret)=32 AND canceled=0 AND invitation=0
            AND (?3 IS NULL OR ?3=0 OR (extra_guests_allowed=1 AND ?4 IN ('going','maybe'))) FROM events WHERE chat=?1 AND id=?2",
            params![chat, event, guests, response], |row| row.get::<_, Option<bool>>(0))?.unwrap_or(false);
        if !allowed { return Ok(false); }
        reconcile_event(&tx, &chat, event)?;
        let current = read_state(&tx, &chat, event, &who)?;
        if current.as_ref() != prior.as_ref().map(|token| &token.0) { return Ok(false); }
        let watermark_ms = current.and_then(|state| state.watermark_ms);
        write_state(&tx, &chat, event, &who, &RowState { response: response.into(), timestamp_ms: None,
            extra_guest_count: guests, source_id: Some(source_id.into()), watermark_ms, revision: revision(&tx)?,
            ack_lower_bound_ms: request_started_ms })?;
        tx.commit()?; Ok(true)
    }
}

impl StoreWorker {
    pub(crate) async fn apply_event_rsvp(&self, chat: &str, event: &str, who: &str, update: &EventRsvpUpdate) -> Result<bool> {
        let (chat, event, who, update) = (chat.to_owned(), event.to_owned(), who.to_owned(), update.clone());
        self.run(move |store| store.apply_event_rsvp(&chat, &event, &who, &update)).await
    }

    pub(crate) async fn event_rsvp_context(&self, chat: &str, event: &str, who: &str) -> Result<Option<EventRsvpContext>> {
        let (chat, event, who) = (chat.to_owned(), event.to_owned(), who.to_owned());
        self.run(move |store| store.event_rsvp_context(&chat, &event, &who)).await
    }

    pub(crate) async fn commit_event_rsvp_ack_with_bound(&self, chat: &str, event: &str, who: &str, prior: &Option<EventRsvpToken>,
        response: &str, guests: Option<i32>, source_id: &str, request_started_ms: Option<i64>) -> Result<bool> {
        let (chat, event, who, prior) = (chat.to_owned(), event.to_owned(), who.to_owned(), prior.clone());
        let (response, source_id) = (response.to_owned(), source_id.to_owned());
        self.run(move |store| store.commit_event_rsvp_ack_with_bound(&chat, &event, &who, &prior,
            &response, guests, &source_id, request_started_ms)).await
    }

    pub(crate) async fn fill_event_secret(&self, chat: &str, id: &str, approved_creators: &[String], secret: &[u8]) -> Result<bool> {
        let chat = chat.to_owned(); let id = id.to_owned();
        let creators = approved_creators.to_vec(); let secret = secret.to_vec();
        self.run(move |store| store.fill_event_secret(&chat, &id, &creators, &secret)).await
    }
}

fn metadata(conn: &Connection, chat: &str, id: &str) -> Result<Option<NewEvent>> {
    Ok(conn.query_row("SELECT name,description,start_at,end_at,location,link,canceled,extra_guests_allowed,
        is_scheduled_call,has_reminder,reminder_offset_sec,invitation_id,invitation FROM events WHERE chat=?1 AND id=?2",
        params![chat, id], |row| Ok(NewEvent { name: row.get(0)?, description: row.get(1)?, start: row.get(2)?, end: row.get(3)?,
            location: row.get(4)?, link: row.get(5)?, canceled: row.get(6)?, extra_guests_allowed: row.get(7)?,
            is_scheduled_call: row.get(8)?, has_reminder: row.get(9)?, reminder_offset_sec: row.get(10)?,
            invitation_id: row.get(11)?, invitation: row.get(12)? })).optional()?)
}

#[cfg(test)]
#[path = "event_rsvps_tests.rs"]
mod tests;
