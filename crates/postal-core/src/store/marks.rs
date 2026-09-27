//! Per-message state beside the messages: reactions, stars, pins, polls and events.

use super::*;

impl MessageStore {
    /// Records a reaction; an empty emoji removes the sender's reaction.
    pub fn set_reaction(&self, chat: &str, target: &str, sender: &str, emoji: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        if emoji.is_empty() {
            conn.execute(
                "DELETE FROM reactions WHERE chat = ?1 AND target = ?2 AND sender = ?3",
                params![chat, target, sender],
            )?;
        } else {
            conn.execute(
                "INSERT INTO reactions (chat, target, sender, emoji) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(chat, target, sender) DO UPDATE SET emoji = excluded.emoji",
                params![chat, target, sender, emoji],
            )?;
        }
        Ok(())
    }

    pub fn set_starred(&self, chat: &str, id: &str, starred: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let sql = if starred {
            "INSERT OR IGNORE INTO stars (chat, id) VALUES (?1, ?2)"
        } else {
            "DELETE FROM stars WHERE chat = ?1 AND id = ?2"
        };
        conn.execute(sql, params![chat, id])?;
        Ok(())
    }

    /// The chat's pinned message, or none.
    // One pin per chat; WhatsApp allows three, add a rank column if needed.
    pub fn set_message_pin(&self, chat: &str, id: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        match id {
            Some(id) => conn.execute(
                "INSERT INTO message_pins (chat, id) VALUES (?1, ?2)
                 ON CONFLICT(chat) DO UPDATE SET id = excluded.id",
                params![chat, id],
            )?,
            None => conn.execute("DELETE FROM message_pins WHERE chat = ?1", params![chat])?,
        };
        Ok(())
    }

    /// Reactions, stars and the pin for one chat.
    pub fn marks(&self, chat: &str) -> Result<ChatMarks> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let reactions = conn
            .prepare("SELECT target, sender, emoji FROM reactions WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok(Reaction { target: r.get(0)?, sender: r.get(1)?, emoji: r.get(2)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let starred = conn
            .prepare("SELECT id FROM stars WHERE chat = ?1")?
            .query_map(params![chat], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let pinned = conn
            .query_row("SELECT id FROM message_pins WHERE chat = ?1", params![chat], |r| r.get(0))
            .optional()?;
        let json = |s: String| serde_json::from_str::<Vec<String>>(&s).unwrap_or_default();

        let mut polls: Vec<Poll> = conn
            .prepare("SELECT id, name, options, multi FROM polls WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok(Poll {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    options: json(r.get(2)?),
                    multi: r.get::<_, i32>(3)? != 0,
                    votes: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let votes: Vec<(String, PollVote)> = conn
            .prepare("SELECT poll, voter, options FROM poll_votes WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok((r.get(0)?, PollVote { voter: r.get(1)?, options: json(r.get(2)?) }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (poll, vote) in votes {
            if let Some(p) = polls.iter_mut().find(|p| p.id == poll) {
                p.votes.push(vote);
            }
        }

        let mut events: Vec<Event> = conn
            .prepare(
                "SELECT id, name, description, start_at, end_at, location, link, canceled
                 FROM events WHERE chat = ?1",
            )?
            .query_map(params![chat], |r| {
                Ok(Event {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    start: r.get(3)?,
                    end: r.get(4)?,
                    location: r.get(5)?,
                    link: r.get(6)?,
                    canceled: r.get::<_, i32>(7)? != 0,
                    responses: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let responses: Vec<(String, EventResponse)> = conn
            .prepare("SELECT event, responder, response FROM event_responses WHERE chat = ?1")?
            .query_map(params![chat], |r| {
                Ok((r.get(0)?, EventResponse { responder: r.get(1)?, response: r.get(2)? }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (event, response) in responses {
            if let Some(e) = events.iter_mut().find(|e| e.id == event) {
                e.responses.push(response);
            }
        }

        let view_once = conn
            .prepare(
                "SELECT id, opened,
                        EXISTS(SELECT 1 FROM messages m
                                WHERE m.chat = v.chat AND m.id = v.id
                                  AND m.media_path IS NOT NULL AND m.media_path != '')
                     OR EXISTS(SELECT 1 FROM messages q
                                WHERE q.reply_to_id = v.id
                                  AND q.reply_to_locator IS NOT NULL AND q.reply_to_locator != '')
                 FROM view_once v WHERE chat = ?1",
            )?
            .query_map(params![chat], |r| {
                Ok(ViewOnce { id: r.get(0)?, opened: r.get::<_, i32>(1)? != 0, available: r.get::<_, i32>(2)? != 0 })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let ids = |table: &str| -> Result<Vec<String>> {
            conn.prepare(&format!("SELECT id FROM {table} WHERE chat = ?1"))?
                .query_map(params![chat], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()
                .map_err(Into::into)
        };
        let forwarded = ids("forwarded")?;
        let edited = ids("edited")?;

        Ok(ChatMarks { reactions, starred, pinned, polls, events, view_once, forwarded, edited })
    }

    pub fn set_forwarded(&self, chat: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute("INSERT OR IGNORE INTO forwarded (chat, id) VALUES (?1, ?2)", params![chat, id])?;
        Ok(())
    }

    /// Records a poll the first time it is seen; later copies change nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn save_poll(
        &self,
        chat: &str,
        id: &str,
        creator: &str,
        name: &str,
        options: &[String],
        multi: bool,
        secret: Option<&[u8]>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT OR IGNORE INTO polls (chat, id, creator, name, options, multi, secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![chat, id, creator, name, serde_json::to_string(options)?, multi as i32, secret],
        )?;
        Ok(())
    }

    pub fn poll_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT creator, secret, options FROM polls WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| {
                    let creator = r.get(0)?;
                    let options = r.get::<_, String>(2)?;
                    Ok(r.get::<_, Option<Vec<u8>>>(1)?.map(|secret| Secretive {
                        creator, secret, options: serde_json::from_str(&options).unwrap_or_default(),
                    }))
                },
            )
            .optional()?
            .flatten())
    }

    /// A voter's current choice; an empty list withdraws their vote.
    pub fn set_poll_vote(&self, chat: &str, poll: &str, voter: &str, options: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO poll_votes (chat, poll, voter, options) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat, poll, voter) DO UPDATE SET options = excluded.options",
            params![chat, poll, voter, serde_json::to_string(options)?],
        )?;
        Ok(())
    }

    /// Records an event, or updates it when its creator edits or cancels it.
    pub fn save_event(
        &self,
        chat: &str,
        id: &str,
        creator: &str,
        event: &NewEvent,
        secret: Option<&[u8]>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO events
                 (chat, id, creator, name, description, start_at, end_at, location, link, canceled, secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(chat, id) DO UPDATE SET
                 name = excluded.name, description = excluded.description,
                 start_at = excluded.start_at, end_at = excluded.end_at, location = excluded.location,
                 link = excluded.link, canceled = excluded.canceled,
                 secret = COALESCE(events.secret, excluded.secret)",
            params![
                chat,
                id,
                creator,
                event.name,
                event.description,
                event.start,
                event.end,
                event.location,
                event.link,
                event.canceled as i32,
                secret
            ],
        )?;
        Ok(())
    }

    pub fn event_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT creator, secret FROM events WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| {
                    let creator = r.get(0)?;
                    Ok(r.get::<_, Option<Vec<u8>>>(1)?.map(|secret| Secretive {
                        creator, secret, options: Vec::new(),
                    }))
                },
            )
            .optional()?
            .flatten())
    }

    pub fn set_event_response(&self, chat: &str, event: &str, responder: &str, response: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO event_responses (chat, event, responder, response) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat, event, responder) DO UPDATE SET response = excluded.response",
            params![chat, event, responder, response],
        )?;
        Ok(())
    }
}
