//! DiskRetention: per-chat overrides, pruning and clearing history.

use super::*;

/// Explicit limits on persisted messages, independent of the RAM window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskRetention {
    #[serde(deserialize_with = "limits::global_age")]
    pub max_age_hours: RetentionLimit,
    #[serde(deserialize_with = "limits::global_count")]
    pub max_messages_per_chat: RetentionLimit,
}

impl Default for DiskRetention {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl DiskRetention {
    pub fn unlimited() -> Self {
        Self { max_age_hours: RetentionLimit::Unlimited, max_messages_per_chat: RetentionLimit::Unlimited }
    }

    fn oldest_allowed(&self) -> Option<i64> {
        self.max_age_hours.value().map(|hours| unix_now() - i64::from(hours) * 3600)
    }
}

pub struct DiskRetentionManager {
    policy: Mutex<DiskRetention>,
    last_full_prune: std::sync::atomic::AtomicI64,
}

/// Whether any global or per-chat limit could match a row. With no policy
/// anywhere, every delete below matches nothing by construction, yet each
/// still scans: on a large store that holds the store lock for seconds per
/// live message, stalling sends, reads and the UI behind it. So the scans only
/// run when something could match, globally or on some chat.
fn policy_could_match(conn: &Connection, policy: DiskRetention) -> Result<bool> {
    Ok(policy.oldest_allowed().is_some()
        || policy.max_messages_per_chat.value().is_some()
        || per_chat_limited(conn, "age_mode")?
        || per_chat_limited(conn, "count_mode")?)
}

fn per_chat_limited(conn: &Connection, mode: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM chat_retention WHERE {mode} = 'limited')"),
            [],
            |r| r.get::<_, i32>(0),
        )?
        != 0)
}

/// The optional chat list a prune is scoped to, encoded once as JSON so the
/// scoped statements filter on the (chat, timestamp) index instead of reading
/// every row.
struct PruneScope<'a> {
    conn: &'a Connection,
    json: Option<String>,
}

impl PruneScope<'_> {
    fn new<'a>(conn: &'a Connection, chats: Option<&[String]>) -> Result<PruneScope<'a>> {
        Ok(PruneScope { conn, json: chats.map(serde_json::to_string).transpose()? })
    }

    fn scope(&self) -> &'static str {
        if self.json.is_some() { "chat IN (SELECT value FROM json_each(:scope))" } else { "1" }
    }

    fn scope_m(&self) -> &'static str {
        if self.json.is_some() { "m.chat IN (SELECT value FROM json_each(:scope))" } else { "1" }
    }

    fn run(&self, sql: &str, extra: &[(&str, &dyn rusqlite::ToSql)]) -> rusqlite::Result<usize> {
        let mut params = extra.to_vec();
        if let Some(json) = &self.json {
            params.push((":scope", json));
        }
        self.conn.execute(sql, params.as_slice())
    }

    /// The shared window; a chat with its own window or cap is only bound by
    /// that one.
    fn purge_global_age(&self, oldest: Option<i64>) -> Result<usize> {
        let Some(oldest) = oldest else { return Ok(0) };
        Ok(self.run(
            &format!(
                "DELETE FROM messages WHERE {} AND timestamp < :oldest AND chat NOT IN
                     (SELECT jid FROM chat_retention WHERE age_mode != 'inherit')",
                self.scope()
            ),
            &[(":oldest", &oldest)],
        )?)
    }

    /// The per-chat window only matches when some chat has one; the global
    /// oldest above already covers the shared window.
    fn purge_per_chat_age(&self) -> Result<usize> {
        if !per_chat_limited(self.conn, "age_mode")? {
            return Ok(0);
        }
        let now = unix_now();
        Ok(self.run(
            &format!(
                "DELETE FROM messages WHERE {} AND EXISTS (
                     SELECT 1 FROM chat_retention r WHERE r.jid = messages.chat
                     AND r.age_mode = 'limited' AND messages.timestamp < :now - r.max_age_hours * 3600)",
                self.scope()
            ),
            &[(":now", &now)],
        )?)
    }

    /// Rank within each chat and drop everything past its cap.
    fn purge_caps(&self, cap: Option<u32>) -> Result<usize> {
        if cap.is_none() && !per_chat_limited(self.conn, "count_mode")? {
            return Ok(0);
        }
        Ok(self.run(
            &format!(
                "DELETE FROM messages WHERE (chat, id) IN (
                     SELECT chat, id FROM (
                         SELECT m.chat, m.id,
                                ROW_NUMBER() OVER (PARTITION BY m.chat ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC) AS rank,
                                CASE WHEN r.jid IS NULL OR r.count_mode = 'inherit' THEN :cap
                                     WHEN r.count_mode = 'limited' THEN r.max_messages ELSE NULL END AS cap
                         FROM messages m LEFT JOIN chat_retention r ON r.jid = m.chat
                         WHERE {}
                     ) WHERE cap IS NOT NULL AND rank > cap
                 )",
                self.scope_m()
            ),
            &[(":cap", &cap)],
        )?)
    }
}

/// Drops the state rows whose message has just been pruned.
fn purge_orphan_state(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM forwarded WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = forwarded.chat AND m.id = forwarded.id);
         DELETE FROM edited WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = edited.chat AND m.id = edited.id);
         DELETE FROM view_once WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = view_once.chat AND m.id = view_once.id);
         DELETE FROM receipts WHERE NOT EXISTS (SELECT 1 FROM messages m WHERE m.id = receipts.id);
         DELETE FROM reactions WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = reactions.chat AND m.id = reactions.target);
         DELETE FROM stars WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = stars.chat AND m.id = stars.id);
         DELETE FROM message_pins WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = message_pins.chat AND m.id = message_pins.id);
         DELETE FROM poll_votes WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = poll_votes.chat AND m.id = poll_votes.poll);
         DELETE FROM polls WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = polls.chat AND m.id = polls.id);
         DELETE FROM event_responses WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = event_responses.chat AND m.id = event_responses.event);
         DELETE FROM events WHERE NOT EXISTS (
             SELECT 1 FROM messages m WHERE m.chat = events.chat AND m.id = events.id);",
    )?;
    Ok(())
}

impl MessageStore {
    pub fn chat_retention(&self, jid: &str) -> Result<ChatRetention> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        Ok(conn
            .query_row(
                "SELECT age_mode, max_age_hours, count_mode, max_messages, on_demand FROM chat_retention WHERE jid = ?1",
                params![jid],
                |r| {
                    Ok(ChatRetention {
                        max_age_hours: RetentionLimit::from_row(r, 0, 1)?,
                        max_messages: RetentionLimit::from_row(r, 2, 3)?,
                        on_demand: r.get::<_, i32>(4)? != 0,
                    })
                },
            )
            .optional()?.unwrap_or_default())
    }

    pub fn set_chat_retention(&self, jid: &str, retention: &ChatRetention) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        if *retention == ChatRetention::default() {
            conn.execute("DELETE FROM chat_retention WHERE jid = ?1", params![jid])?;
        } else {
            conn.execute(
                "INSERT INTO chat_retention (jid, max_age_hours, max_messages, on_demand, age_mode, count_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(jid) DO UPDATE SET max_age_hours = excluded.max_age_hours,
                     max_messages = excluded.max_messages, on_demand = excluded.on_demand,
                     age_mode = excluded.age_mode, count_mode = excluded.count_mode",
                params![jid, retention.max_age_hours.value(), retention.max_messages.value(), retention.on_demand as i32,
                    retention.max_age_hours.mode(), retention.max_messages.mode()],
            )?;
        }
        Ok(())
    }

}

impl DiskRetentionManager {
    pub fn new(policy: DiskRetention) -> Self {
        Self { policy: Mutex::new(policy), last_full_prune: std::sync::atomic::AtomicI64::new(0) }
    }

    /// Applies the retention policy to every chat, returning how many messages
    /// were dropped.
    pub fn enforce(&self, store: &MessageStore) -> Result<usize> {
        let policy = *self.policy.lock().unwrap();
        self.prune(store, None, policy)
    }

    /// DiskRetention after a live batch: only the chats it wrote to, which stays
    /// cheap on a large store, plus a pass over everything at most hourly so
    /// quiet chats still age out. Called after writes rather than on a timer
    /// so the bound holds even if the process is interrupted.
    pub fn enforce_for(&self, store: &MessageStore, chats: &[String]) -> Result<usize> {
        let policy = *self.policy.lock().unwrap();
        let now = unix_now();
        if now - self.last_full_prune.load(std::sync::atomic::Ordering::Relaxed) >= 3600 {
            let removed = self.prune(store, None, policy)?;
            let current = self.policy.lock().unwrap();
            if *current == policy {
                self.last_full_prune.store(now, std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(removed);
        }
        self.prune(store, Some(chats), policy)
    }

    /// Replaces the global policy; the next prune uses it.
    pub fn set_policy(&self, retention: DiskRetention) {
        let mut policy = self.policy.lock().unwrap();
        if *policy != retention {
            *policy = retention;
            self.last_full_prune.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn prune(&self, store: &MessageStore, chats: Option<&[String]>, policy: DiskRetention) -> Result<usize> {
        let conn = store.conn.lock().unwrap();
        if !policy_could_match(&conn, policy)? {
            return Ok(0);
        }
        let scope = PruneScope::new(&conn, chats)?;
        let mut removed = 0;
        removed += scope.purge_global_age(policy.oldest_allowed())?;
        removed += scope.purge_per_chat_age()?;
        removed += scope.purge_caps(policy.max_messages_per_chat.value())?;
        if removed > 0 {
            // State attached to messages only goes stale when messages go.
            purge_orphan_state(&conn)?;
            reclaim(&conn, 2_000)?;
        }
        Ok(removed)
    }

}

impl MessageStore {
    /// Deletes every stored message and the state attached to them. Names,
    /// chat pins and per-chat settings stay. Returns how many messages went.
    pub fn clear_history(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let removed = conn.execute("DELETE FROM messages", [])?;
        conn.execute_batch(
            "DELETE FROM reactions; DELETE FROM stars; DELETE FROM message_pins;
             DELETE FROM polls; DELETE FROM poll_votes; DELETE FROM events;
             DELETE FROM event_responses; DELETE FROM view_once; DELETE FROM forwarded;
             DELETE FROM edited; DELETE FROM receipts; DELETE FROM hidden_chats;
             DELETE FROM cleared_chats;",
        )?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// Total stored messages, used by tests and diagnostics.
    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?)
    }
}

impl StoreWorker {
    pub(crate) async fn chat_retention(&self, jid: &str) -> Result<ChatRetention> {
        let jid = jid.to_owned();
        self.run(move |store| store.chat_retention(&jid)).await
    }

    pub(crate) async fn set_chat_retention(&self, jid: &str, retention: &ChatRetention) -> Result<()> {
        let jid = jid.to_owned();
        let retention = retention.clone();
        self.run(move |store| store.set_chat_retention(&jid, &retention)).await
    }

    pub(crate) async fn clear_history(&self) -> Result<usize> {
        self.run(move |store| store.clear_history()).await
    }

    #[cfg(test)]
    pub(crate) async fn count(&self) -> Result<i64> {
        self.run(move |store| store.count()).await
    }
}
