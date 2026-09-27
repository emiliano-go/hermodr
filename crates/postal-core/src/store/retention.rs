//! Retention: per-chat overrides, pruning and clearing history.

use super::*;

impl MessageStore {
    pub fn chat_retention(&self, jid: &str) -> Result<ChatRetention> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT max_age_hours, max_messages, on_demand FROM chat_retention WHERE jid = ?1",
                params![jid],
                |r| {
                    Ok(ChatRetention {
                        max_age_hours: r.get(0)?,
                        max_messages: r.get(1)?,
                        on_demand: r.get::<_, i32>(2)? != 0,
                    })
                },
            )
            .unwrap_or_default())
    }

    pub fn set_chat_retention(&self, jid: &str, retention: &ChatRetention) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if *retention == ChatRetention::default() {
            conn.execute("DELETE FROM chat_retention WHERE jid = ?1", params![jid])?;
        } else {
            conn.execute(
                "INSERT INTO chat_retention (jid, max_age_hours, max_messages, on_demand)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(jid) DO UPDATE SET max_age_hours = excluded.max_age_hours,
                     max_messages = excluded.max_messages, on_demand = excluded.on_demand",
                params![jid, retention.max_age_hours, retention.max_messages, retention.on_demand as i32],
            )?;
        }
        Ok(())
    }

    /// Applies the retention policy to every chat, returning how many messages
    /// were dropped.
    pub fn enforce_retention(&self) -> Result<usize> {
        self.prune(None)
    }

    /// Retention after a live batch: only the chats it wrote to, which stays
    /// cheap on a large store, plus a pass over everything at most hourly so
    /// quiet chats still age out. Called after writes rather than on a timer
    /// so the bound holds even if the process is interrupted.
    pub fn enforce_retention_for(&self, chats: &[String]) -> Result<usize> {
        let now = unix_now();
        if now - self.last_full_prune.load(std::sync::atomic::Ordering::Relaxed) >= 3600 {
            self.last_full_prune.store(now, std::sync::atomic::Ordering::Relaxed);
            return self.prune(None);
        }
        self.prune(Some(chats))
    }

    fn prune(&self, chats: Option<&[String]>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let json = chats.map(|c| serde_json::to_string(c).unwrap_or_default());
        // Scoped statements filter on the (chat, timestamp) index instead of reading every row.
        let (scope, scope_m) = if json.is_some() {
            ("chat IN (SELECT value FROM json_each(:scope))", "m.chat IN (SELECT value FROM json_each(:scope))")
        } else {
            ("1", "1")
        };
        let run = |sql: &str, extra: &[(&str, &dyn rusqlite::ToSql)]| -> rusqlite::Result<usize> {
            let mut params = extra.to_vec();
            if let Some(json) = &json {
                params.push((":scope", json));
            }
            conn.execute(sql, params.as_slice())
        };

        // Every chat keeps its newest message, whatever its age: a quiet chat
        // must stay in the list with its last preview, not vanish.
        let not_newest = format!(
            "(chat, id) NOT IN (
                 SELECT chat, id FROM (
                     SELECT chat, id, ROW_NUMBER() OVER (PARTITION BY chat ORDER BY timestamp DESC) AS rank
                     FROM messages WHERE {scope}
                 ) WHERE rank = 1)"
        );
        let mut removed = 0;

        // A chat with its own window or cap is only bound by that one.
        if let Some(oldest) = self.retention.oldest_allowed() {
            removed += run(
                &format!(
                    "DELETE FROM messages WHERE {scope} AND timestamp < :oldest AND chat NOT IN
                         (SELECT jid FROM chat_retention WHERE max_age_hours IS NOT NULL)
                     AND {not_newest}"
                ),
                &[(":oldest", &oldest)],
            )?;
        }
        removed += run(
            &format!(
                "DELETE FROM messages WHERE {scope} AND EXISTS (
                     SELECT 1 FROM chat_retention r WHERE r.jid = messages.chat
                     AND r.max_age_hours > 0 AND messages.timestamp < :now - r.max_age_hours * 3600)
                 AND {not_newest}"
            ),
            &[(":now", &unix_now())],
        )?;

        // Rank within each chat and drop everything past its cap.
        removed += run(
            &format!(
                "DELETE FROM messages WHERE (chat, id) IN (
                     SELECT chat, id FROM (
                         SELECT m.chat, m.id,
                                ROW_NUMBER() OVER (PARTITION BY m.chat ORDER BY m.timestamp DESC) AS rank,
                                COALESCE(r.max_messages, :cap) AS cap
                         FROM messages m LEFT JOIN chat_retention r ON r.jid = m.chat
                         WHERE {scope_m}
                     ) WHERE cap > 0 AND rank > cap
                 )"
            ),
            &[(":cap", &self.retention.max_messages_per_chat.map(|c| c as i64).unwrap_or(0))],
        )?;

        // State attached to messages only goes stale when messages go.
        if removed > 0 {
            conn.execute_batch(
                "DELETE FROM forwarded WHERE NOT EXISTS (
                     SELECT 1 FROM messages m WHERE m.chat = forwarded.chat AND m.id = forwarded.id);
                 DELETE FROM edited WHERE NOT EXISTS (
                     SELECT 1 FROM messages m WHERE m.chat = edited.chat AND m.id = edited.id);
                 DELETE FROM view_once WHERE NOT EXISTS (
                     SELECT 1 FROM messages m WHERE m.chat = view_once.chat AND m.id = view_once.id);
                 DELETE FROM receipts WHERE NOT EXISTS (SELECT 1 FROM messages m WHERE m.id = receipts.id);",
            )?;
            reclaim(&conn, 2_000)?;
        }
        Ok(removed)
    }

    /// Deletes every stored message and the state attached to them. Names,
    /// chat pins and per-chat settings stay. Returns how many messages went.
    pub fn clear_history(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let removed = conn.execute("DELETE FROM messages", [])?;
        conn.execute_batch(
            "DELETE FROM reactions; DELETE FROM stars; DELETE FROM message_pins;
             DELETE FROM polls; DELETE FROM poll_votes; DELETE FROM events;
             DELETE FROM event_responses; DELETE FROM view_once; DELETE FROM forwarded;
             DELETE FROM edited; DELETE FROM receipts;",
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
