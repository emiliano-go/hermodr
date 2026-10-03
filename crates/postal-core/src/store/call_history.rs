use super::*;
use crate::message_ref::MessageRef;
use std::collections::BTreeSet;
use whatsapp_rust::wacore_binary::{Jid, JidExt};

pub const MAX_CALL_HISTORY_PAGE: u32 = 200;
const MAX_ID_BYTES: usize = 256;
pub const MAX_CALL_PARTICIPANTS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum CallOutcome {
    Connected, Rejected, Cancelled, AcceptedElsewhere, Missed, Invalid,
    Unavailable, Upcoming, Failed, Abandoned, Ongoing, Unknown,
}

impl CallOutcome {
    fn from_raw(value: Option<i32>) -> Self {
        match value {
            Some(0) => Self::Connected, Some(1) => Self::Rejected, Some(2) => Self::Cancelled,
            Some(3) => Self::AcceptedElsewhere, Some(4) => Self::Missed, Some(5) => Self::Invalid,
            Some(6) => Self::Unavailable, Some(7) => Self::Upcoming, Some(8) => Self::Failed,
            Some(9) => Self::Abandoned, Some(10) => Self::Ongoing, _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct CallRecord {
    pub call_id: String,
    pub creator_jid: String,
    pub peer_jid: Option<String>,
    pub group_jid: Option<String>,
    pub chat: Option<String>,
    pub from_me: bool,
    pub is_video: Option<bool>,
    pub outcome: CallOutcome,
    pub outcome_raw: Option<i32>,
    pub call_type_raw: Option<i32>,
    pub start_time_raw: Option<String>,
    pub duration_raw: Option<String>,
    pub mutation_at_ms: i64,
    pub from_full_sync: bool,
}

#[derive(Debug, Clone)]
pub struct CallHistoryUpdate {
    pub call_id: String,
    pub creator_jid: String,
    pub group_jid: Option<String>,
    pub peer_jids: Vec<String>,
    pub from_me: bool,
    pub is_video: Option<bool>,
    pub outcome_raw: Option<i32>,
    pub call_type_raw: Option<i32>,
    pub start_time_raw: Option<i64>,
    pub duration_raw: Option<i64>,
    pub mutation_at_ms: i64,
    pub from_full_sync: bool,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS call_history (
        call_id TEXT PRIMARY KEY CHECK(length(call_id) > 0), creator_jid TEXT NOT NULL,
        peer_jid TEXT, group_jid TEXT, chat TEXT, from_me INTEGER NOT NULL CHECK(from_me IN (0,1)),
        is_video INTEGER CHECK(is_video IN (0,1)), outcome_raw INTEGER, call_type_raw INTEGER,
        start_time_raw INTEGER, duration_raw INTEGER, mutation_at_ms INTEGER NOT NULL,
        from_full_sync INTEGER NOT NULL CHECK(from_full_sync IN (0,1)));
        CREATE INDEX IF NOT EXISTS call_history_order ON call_history(mutation_at_ms DESC,call_id DESC);
        CREATE INDEX IF NOT EXISTS call_history_chat ON call_history(chat,mutation_at_ms DESC,call_id DESC);")?;
    Ok(())
}

fn bounded(value: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty() && value.len() <= MAX_ID_BYTES && !value.chars().any(char::is_control),
        MessageRef::new("error.calls_identifier_invalid"));
    Ok(())
}

fn user(value: &str) -> Result<Jid> {
    bounded(value)?;
    let jid: Jid = value.parse().map_err(|error| anyhow::Error::new(MessageRef::new("error.calls_address_invalid")).context(error))?;
    anyhow::ensure!(!jid.user.is_empty() && (jid.is_pn() || jid.is_lid()), MessageRef::new("error.calls_address_invalid"));
    Ok(jid.to_non_ad())
}

fn canonical_user(conn: &Connection, value: &str) -> Result<String> {
    let jid = user(value)?.to_string();
    Ok(names::canonical_chat(conn, &jid)?.into_owned())
}

struct Target {
    peer: Option<String>,
    group: Option<String>,
    chat: Option<String>,
    explicit: bool,
}

fn target(conn: &Connection, update: &CallHistoryUpdate, creator: &str) -> Result<Target> {
    let mut peers = BTreeSet::new();
    for peer in &update.peer_jids {
        let peer = canonical_user(conn, peer)?;
        if peer != creator { peers.insert(peer); }
    }
    let peer = if update.from_me {
        if peers.len() == 1 { peers.into_iter().next() } else { None }
    } else { Some(creator.to_owned()) };
    let group = update.group_jid.as_ref().and_then(|value| value.parse::<Jid>().ok())
        .filter(|jid| !jid.user.is_empty() && jid.is_group()).map(|jid| jid.to_non_ad().to_string());
    let chat = if update.group_jid.is_some() { group.clone() } else { peer.clone() };
    Ok(Target { peer, group, chat, explicit: update.group_jid.is_some() || !update.from_me || !update.peer_jids.is_empty() })
}

fn previous(conn: &Connection, update: &CallHistoryUpdate, creator: &str) -> Result<Option<i64>> {
    let old = conn.query_row("SELECT creator_jid,from_me,mutation_at_ms FROM call_history WHERE call_id=?1",
        [&update.call_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?))).optional()?;
    if let Some((old_creator, old_from_me, revision)) = old {
        let same_creator = canonical_user(conn, &old_creator)? == creator;
        anyhow::ensure!(old_from_me == update.from_me && (same_creator || update.from_me),
            MessageRef::new("error.calls_identity_conflict"));
        return Ok(Some(revision));
    }
    Ok(None)
}

const UPSERT: &str = "INSERT INTO call_history (
    call_id,creator_jid,peer_jid,group_jid,chat,from_me,is_video,outcome_raw,call_type_raw,
    start_time_raw,duration_raw,mutation_at_ms,from_full_sync)
    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
    ON CONFLICT(call_id) DO UPDATE SET creator_jid=excluded.creator_jid,
    peer_jid=CASE WHEN ?14 THEN excluded.peer_jid ELSE call_history.peer_jid END,
    group_jid=CASE WHEN ?14 THEN excluded.group_jid ELSE call_history.group_jid END,
    chat=CASE WHEN ?14 THEN excluded.chat ELSE call_history.chat END,
    is_video=COALESCE(excluded.is_video,call_history.is_video),
    outcome_raw=COALESCE(excluded.outcome_raw,call_history.outcome_raw),
    call_type_raw=COALESCE(excluded.call_type_raw,call_history.call_type_raw),
    start_time_raw=COALESCE(excluded.start_time_raw,call_history.start_time_raw),
    duration_raw=COALESCE(excluded.duration_raw,call_history.duration_raw),
    mutation_at_ms=excluded.mutation_at_ms,from_full_sync=excluded.from_full_sync
    WHERE excluded.mutation_at_ms>call_history.mutation_at_ms";

impl MessageStore {
    pub fn capture_call(&self, update: &CallHistoryUpdate) -> Result<bool> {
        bounded(&update.call_id)?;
        anyhow::ensure!(update.mutation_at_ms >= 0, MessageRef::new("error.calls_revision_invalid"));
        anyhow::ensure!(update.peer_jids.len() <= MAX_CALL_PARTICIPANTS, MessageRef::new("error.calls_participants_limit"));
        if let Some(group) = &update.group_jid { bounded(group)?; }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let creator = canonical_user(&tx, &update.creator_jid)?;
        let prior = previous(&tx, update, &creator)?;
        if prior.is_some_and(|revision| update.mutation_at_ms <= revision) { return Ok(false); }
        let target = target(&tx, update, &creator)?;
        let changed = tx.execute(UPSERT, params![update.call_id, creator, target.peer, target.group, target.chat,
            update.from_me, update.is_video, update.outcome_raw, update.call_type_raw, update.start_time_raw,
            update.duration_raw, update.mutation_at_ms, update.from_full_sync, target.explicit])? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn call_history(&self, limit: u32) -> Result<Vec<CallRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT call_id,creator_jid,peer_jid,group_jid,chat,from_me,is_video,
            outcome_raw,call_type_raw,start_time_raw,duration_raw,mutation_at_ms,from_full_sync
            FROM call_history ORDER BY mutation_at_ms DESC,call_id DESC LIMIT ?1")?;
        let mut calls = statement.query_map([limit.clamp(1, MAX_CALL_HISTORY_PAGE)], read_call)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for call in &mut calls {
            call.creator_jid = names::canonical_chat(&conn, &call.creator_jid)?.into_owned();
            for address in [&mut call.peer_jid, &mut call.group_jid, &mut call.chat] {
                if let Some(jid) = address { *jid = names::canonical_chat(&conn, jid)?.into_owned(); }
            }
        }
        Ok(calls)
    }
}

fn read_call(row: &rusqlite::Row<'_>) -> rusqlite::Result<CallRecord> {
    let outcome_raw = row.get(7)?;
    Ok(CallRecord {
        call_id: row.get(0)?, creator_jid: row.get(1)?, peer_jid: row.get(2)?, group_jid: row.get(3)?,
        chat: row.get(4)?, from_me: row.get(5)?, is_video: row.get(6)?,
        outcome: CallOutcome::from_raw(outcome_raw), outcome_raw, call_type_raw: row.get(8)?,
        start_time_raw: row.get::<_, Option<i64>>(9)?.map(|value| value.to_string()),
        duration_raw: row.get::<_, Option<i64>>(10)?.map(|value| value.to_string()),
        mutation_at_ms: row.get(11)?, from_full_sync: row.get(12)?,
    })
}

#[cfg(test)]
#[path = "call_history_tests.rs"]
mod tests;
