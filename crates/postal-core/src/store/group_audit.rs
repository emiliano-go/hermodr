use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum GroupAuditKind {
    Join, Leave, Remove, Promote, Demote, Subject, Description, Locked, Announce,
    Ephemeral, JoinApproval, MemberAddMode, Forwarding, InviteChange, Create, Delete,
    Picture, MessageEdit, MessageDelete, MessagePin, MessageUnpin, MemberTag,
    MemberLinkMode, MemberShareHistoryMode, HistorySharing, OwnerChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum GroupAuditSource { Notification, Message, History, Local }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum GroupAuditOldSource { Protocol, Cached }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupAuditEntry {
    pub id: i64,
    pub chat: String,
    pub kind: GroupAuditKind,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub old_source: Option<GroupAuditOldSource>,
    pub timestamp: Option<i64>,
    pub observed_at: i64,
    pub source: GroupAuditSource,
    pub message_id: Option<String>,
    pub jump_available: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupAuditRecord {
    pub chat: String,
    pub source_id: Option<String>,
    pub kind: GroupAuditKind,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub old_source: Option<GroupAuditOldSource>,
    pub timestamp: Option<i64>,
    pub observed_at: i64,
    pub source: GroupAuditSource,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupAuditCursor { pub timestamp: i64, pub id: i64 }

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupAuditFilter {
    pub kind: Option<GroupAuditKind>,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub member: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub before: Option<GroupAuditCursor>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupAuditPage {
    pub entries: Vec<GroupAuditEntry>,
    pub has_more: bool,
    pub next_cursor: Option<GroupAuditCursor>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS group_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT, chat TEXT NOT NULL, event_key TEXT NOT NULL,
            kind TEXT NOT NULL, actor TEXT, target TEXT NOT NULL DEFAULT '',
            old_value TEXT, new_value TEXT, old_source TEXT, timestamp INTEGER,
            observed_at INTEGER NOT NULL, source TEXT NOT NULL, message_id TEXT,
            UNIQUE(chat, event_key, kind, target));
         CREATE INDEX IF NOT EXISTS idx_group_audit_chat ON group_audit(chat, timestamp DESC, id DESC);
         CREATE INDEX IF NOT EXISTS idx_group_audit_target ON group_audit(target, timestamp DESC, id DESC);",
    )?;
    Ok(())
}

fn enum_name(value: impl Serialize) -> Result<String> {
    serde_json::to_value(value)?.as_str().map(str::to_owned).ok_or_else(|| anyhow::anyhow!("invalid audit enum"))
}

fn enum_row<T: serde::de::DeserializeOwned>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(row.get(index)?)).map_err(|error|
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error)))
}

fn address(conn: &Connection, jid: Option<&str>) -> Result<Option<String>> {
    jid.filter(|jid| !jid.is_empty()).map(|jid| names::canonical_chat(conn, jid).map(|jid| jid.to_string())).transpose()
}

fn group_chat(chat: &str) -> Result<()> {
    anyhow::ensure!(chat.ends_with("@g.us") && chat.len() > 5, "choose a group chat");
    Ok(())
}

fn record(conn: &Connection, raw: &GroupAuditRecord) -> Result<bool> {
    group_chat(&raw.chat)?;
    anyhow::ensure!(raw.observed_at >= 0 && raw.timestamp.is_none_or(|at| at >= 0), "invalid audit timestamp");
    let blocked: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM view_once WHERE chat = ?1 AND id = ?2)
           OR EXISTS(SELECT 1 FROM messages WHERE chat = ?1 AND id = ?2
             AND (spoiler <> 0 OR media_once_kind IS NOT NULL OR media_kind IN ('view_once', 'unknown')
               OR system_kind = 'UNAVAILABLE_MESSAGE' OR (?3 AND media_kind = 'live_location')))",
        params![raw.chat, raw.message_id, raw.kind == GroupAuditKind::MessageDelete], |row| row.get(0),
    )?;
    if blocked { return Ok(false); }
    let actor = address(conn, raw.actor.as_deref())?;
    let target = address(conn, raw.target.as_deref())?.unwrap_or_default();
    let kind = enum_name(raw.kind)?;
    let key = match raw.source_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) if matches!(raw.kind, GroupAuditKind::MessageEdit | GroupAuditKind::MessageDelete | GroupAuditKind::MessagePin | GroupAuditKind::MessageUnpin) =>
            format!("message-id:{}", serde_json::to_string(&(id, &raw.message_id))?),
        Some(id) => format!("id:{id}"),
        None if raw.source == GroupAuditSource::Local => format!("local:{}", conn.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM group_audit", [], |row| row.get::<_, i64>(0))?),
        None => {
            let digest = Sha256::digest(serde_json::to_vec(&(&raw.chat, &kind, raw.timestamp, &raw.new_value, &raw.message_id))?);
            format!("observation:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
        }
    };
    let family = match raw.kind {
        GroupAuditKind::Promote | GroupAuditKind::Demote => vec!["promote", "demote"],
        GroupAuditKind::Join | GroupAuditKind::Leave | GroupAuditKind::Remove => vec!["join", "leave", "remove"],
        _ => vec![kind.as_str()],
    };
    let later: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM group_audit WHERE chat = ?1 AND kind IN (SELECT value FROM json_each(?2))
           AND target = ?3 AND timestamp > ?4 AND (?5 IS NULL OR message_id = ?5))",
        params![raw.chat, serde_json::to_string(&family)?, target, raw.timestamp, raw.message_id], |row| row.get(0),
    )?;
    let cached_stale = raw.old_source == Some(GroupAuditOldSource::Cached) && later;
    let owner_old = if raw.kind == GroupAuditKind::OwnerChange { address(conn, raw.old_value.as_deref())? } else { None };
    let owner_new = if raw.kind == GroupAuditKind::OwnerChange { address(conn, raw.new_value.as_deref())? } else { None };
    let old_value = if cached_stale { None } else { owner_old.as_deref().or(raw.old_value.as_deref()) };
    let new_value = owner_new.as_deref().or(raw.new_value.as_deref());
    let old_source = if cached_stale { None } else { raw.old_source.map(enum_name).transpose()? };
    Ok(conn.execute(
        "INSERT OR IGNORE INTO group_audit
         (chat, event_key, kind, actor, target, old_value, new_value, old_source, timestamp, observed_at, source, message_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![raw.chat, key, kind, actor, target, old_value, new_value, old_source,
            raw.timestamp, raw.observed_at, enum_name(raw.source)?, raw.message_id],
    )? > 0)
}

impl MessageStore {
    pub(crate) fn audit_message_context(&self, chat: &str, id: &str) -> Result<Option<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
            WHERE m.chat = ?1 AND m.id = ?2"), params![chat, id], message_row).optional()?)
    }

    pub(crate) fn record_group_audit(&self, records: &[GroupAuditRecord]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let mut changed = 0;
        for raw in records { changed += usize::from(record(&tx, raw)?); }
        tx.commit()?;
        Ok(changed)
    }

    pub fn group_audit_page(&self, chat: Option<&str>, filter: &GroupAuditFilter) -> Result<GroupAuditPage> {
        if let Some(chat) = chat { group_chat(chat)?; }
        anyhow::ensure!(filter.since.zip(filter.until).is_none_or(|(since, until)| since <= until), "invalid audit date range");
        let conn = self.conn.lock().unwrap();
        let actor = address(&conn, filter.actor.as_deref())?;
        let target = address(&conn, filter.target.as_deref())?;
        let member = address(&conn, filter.member.as_deref())?;
        let kind = filter.kind.map(enum_name).transpose()?;
        let limit = filter.limit.unwrap_or(100).clamp(1, 200) as usize;
        let mut stmt = conn.prepare(
            "SELECT a.id, a.chat, a.kind, a.actor, NULLIF(a.target, ''), a.old_value, a.new_value,
                a.old_source, a.timestamp, a.observed_at, a.source, a.message_id,
                EXISTS(SELECT 1 FROM messages m WHERE m.chat = a.chat AND m.id = a.message_id
                  AND NOT (m.deleted <> 0 AND m.text = '' AND m.media_kind IS NULL AND m.system_kind IS NULL))
             FROM group_audit a WHERE (?1 IS NULL OR a.chat = ?1) AND (?2 IS NULL OR a.kind = ?2)
               AND (?3 IS NULL OR a.actor = ?3) AND (?4 IS NULL OR a.target = ?4)
               AND (?5 IS NULL OR COALESCE(a.timestamp, a.observed_at) >= ?5)
               AND (?6 IS NULL OR COALESCE(a.timestamp, a.observed_at) <= ?6)
               AND (?7 IS NULL OR (COALESCE(a.timestamp, a.observed_at), a.id) < (?7, ?8))
               AND (?10 IS NULL OR a.actor = ?10 OR a.target = ?10)
               AND NOT EXISTS(SELECT 1 FROM hidden_chats h WHERE h.jid = a.chat)
               AND NOT EXISTS(SELECT 1 FROM view_once v WHERE v.chat = a.chat AND v.id = a.message_id)
               AND NOT EXISTS(SELECT 1 FROM messages m WHERE m.chat = a.chat AND m.id = a.message_id
                   AND (m.spoiler <> 0 OR m.media_once_kind IS NOT NULL OR m.media_kind IN ('view_once', 'unknown')))
             ORDER BY COALESCE(a.timestamp, a.observed_at) DESC, a.id DESC LIMIT ?9",
        )?;
        let mut entries = stmt.query_map(params![chat, kind, actor, target, filter.since, filter.until,
            filter.before.as_ref().map(|c| c.timestamp), filter.before.as_ref().map(|c| c.id), (limit + 1) as i64, member], |row| {
            let old_source: Option<String> = row.get(7)?;
            Ok(GroupAuditEntry {
                id: row.get(0)?, chat: row.get(1)?, kind: enum_row(row, 2)?, actor: row.get(3)?, target: row.get(4)?,
                old_value: row.get(5)?, new_value: row.get(6)?,
                old_source: old_source.map(|_| enum_row(row, 7)).transpose()?,
                timestamp: row.get(8)?, observed_at: row.get(9)?, source: enum_row(row, 10)?,
                message_id: row.get(11)?, jump_available: row.get(12)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = entries.len() > limit;
        entries.truncate(limit);
        let next_cursor = entries.last().filter(|_| has_more).map(|entry| GroupAuditCursor {
            timestamp: entry.timestamp.unwrap_or(entry.observed_at), id: entry.id,
        });
        Ok(GroupAuditPage { entries, has_more, next_cursor })
    }

}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from == to { return Ok(()); }
    conn.execute("UPDATE group_audit SET actor = ?1 WHERE actor = ?2", params![to, from])?;
    conn.execute(
        "DELETE FROM group_audit WHERE target = ?2 AND EXISTS(SELECT 1 FROM group_audit same
         WHERE same.chat = group_audit.chat AND same.event_key = group_audit.event_key
           AND same.kind = group_audit.kind AND same.target = ?1)", params![to, from],
    )?;
    conn.execute("UPDATE group_audit SET target = ?1 WHERE target = ?2", params![to, from])?;
    conn.execute("UPDATE group_audit SET old_value = ?1 WHERE kind = 'owner_change' AND old_value = ?2", params![to, from])?;
    conn.execute("UPDATE group_audit SET new_value = ?1 WHERE kind = 'owner_change' AND new_value = ?2", params![to, from])?;
    Ok(())
}

pub(super) fn clear(conn: &Connection, chat: &str) -> Result<()> {
    conn.execute("DELETE FROM group_audit WHERE chat = ?1", [chat])?;
    Ok(())
}

#[cfg(test)]
#[path = "group_audit_tests.rs"]
mod tests;
