//! Retention: per-chat overrides, pruning and clearing history.

use super::*;

impl MessageStore {
    pub fn chat_retention(&self, jid: &str) -> Result<ChatRetention> {
        let conn = self.conn.lock().unwrap();
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

    /// Replaces the global policy; the next prune uses it.
    pub fn set_retention(&self, retention: Retention) {
        *self.retention.lock().unwrap() = retention;
    }

    fn prune(&self, chats: Option<&[String]>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        // With no policy anywhere, every delete below matches nothing by
        // construction, yet each still scans: on a large store that holds the
        // store lock for seconds per live message, stalling sends, reads and
        // the UI behind it. So each scan only runs when a policy that could
        // match exists, globally or on some chat.
        let oldest = self.retention.lock().unwrap().oldest_allowed();
        let per_chat_age = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chat_retention WHERE age_mode = 'limited')",
                [],
                |r| r.get::<_, i32>(0),
            )?
            != 0;
        let cap = self.retention.lock().unwrap().max_messages_per_chat.value();
        let per_chat_cap = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chat_retention WHERE count_mode = 'limited')",
                [],
                |r| r.get::<_, i32>(0),
            )?
            != 0;
        if oldest.is_none() && !per_chat_age && cap.is_none() && !per_chat_cap {
            return Ok(0);
        }
        let json = chats.map(serde_json::to_string).transpose()?;
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

        let mut removed = 0;

        // A chat with its own window or cap is only bound by that one.
        if let Some(oldest) = oldest {
            removed += run(
                &format!(
                    "DELETE FROM messages WHERE {scope} AND timestamp < :oldest AND chat NOT IN
                         (SELECT jid FROM chat_retention WHERE age_mode != 'inherit')"
                ),
                &[(":oldest", &oldest)],
            )?;
        }

        // The per-chat window only matches when some chat has one; the global
        // oldest above already covers the shared window.
        if per_chat_age {
            removed += run(
                &format!(
                    "DELETE FROM messages WHERE {scope} AND EXISTS (
                         SELECT 1 FROM chat_retention r WHERE r.jid = messages.chat
                         AND r.age_mode = 'limited' AND messages.timestamp < :now - r.max_age_hours * 3600)"
                ),
                &[(":now", &unix_now())],
            )?;
        }

        // Rank within each chat and drop everything past its cap.
        if cap.is_some() || per_chat_cap {
            removed += run(
                &format!(
                    "DELETE FROM messages WHERE (chat, id) IN (
                         SELECT chat, id FROM (
                             SELECT m.chat, m.id,
                                    ROW_NUMBER() OVER (PARTITION BY m.chat ORDER BY m.timestamp DESC) AS rank,
                                    CASE WHEN r.jid IS NULL OR r.count_mode = 'inherit' THEN :cap
                                         WHEN r.count_mode = 'limited' THEN r.max_messages ELSE NULL END AS cap
                             FROM messages m LEFT JOIN chat_retention r ON r.jid = m.chat
                             WHERE {scope_m}
                         ) WHERE cap IS NOT NULL AND rank > cap
                     )"
                ),
                &[(":cap", &cap)],
            )?;
        }
        // State attached to messages only goes stale when messages go.
        if removed > 0 {
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
