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
    // ponytail: one message pin per chat; add ranks for multiple pins.
    pub fn set_message_pin(&self, chat: &str, id: Option<&str>) -> Result<()> {
        self.mirror_message_pin(chat, id)
    }

    /// Reactions, stars and the pin for one chat.
    pub fn marks(&self, chat: &str) -> Result<ChatMarks> {
        self.marks_for(chat, None)
    }

    pub fn marks_for(&self, chat: &str, message_ids: Option<&[String]>) -> Result<ChatMarks> {
        Self::marks_on(&self.conn.lock().unwrap(), chat, message_ids)
    }

    pub(super) fn marks_on(conn: &Connection, chat: &str, message_ids: Option<&[String]>) -> Result<ChatMarks> {
        let scope = MarksScope::new(conn, chat, message_ids)?;
        Ok(ChatMarks {
            reactions: scope.reactions()?,
            starred: scope.starred()?,
            pinned: scope.pinned()?,
            polls: scope.polls_with_votes()?,
            events: scope.events_with_responses()?,
            view_once: scope.view_once()?,
            forwarded: scope.simple_marks("forwarded")?,
            edited: scope.simple_marks("edited")?,
        })
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
                 (chat, id, creator, name, description, start_at, end_at, location, link, canceled, secret,
                  extra_guests_allowed,is_scheduled_call,has_reminder,reminder_offset_sec,invitation_id,invitation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(chat, id) DO UPDATE SET
                 name = excluded.name, description = excluded.description,
                 start_at = excluded.start_at, end_at = excluded.end_at, location = excluded.location,
                 link = excluded.link, canceled = excluded.canceled,
                 secret = COALESCE(events.secret, excluded.secret),
                 extra_guests_allowed=COALESCE(excluded.extra_guests_allowed,events.extra_guests_allowed),
                 is_scheduled_call=COALESCE(excluded.is_scheduled_call,events.is_scheduled_call),
                 has_reminder=COALESCE(excluded.has_reminder,events.has_reminder),
                 reminder_offset_sec=COALESCE(excluded.reminder_offset_sec,events.reminder_offset_sec),
                 invitation_id=COALESCE(excluded.invitation_id,events.invitation_id),
                 invitation=MAX(events.invitation,excluded.invitation)",
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
                secret, event.extra_guests_allowed,event.is_scheduled_call,event.has_reminder,
                event.reminder_offset_sec,event.invitation_id,event.invitation
            ],
        )?;
        Ok(())
    }

    pub fn event_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                &format!("SELECT e.creator,e.secret FROM events e JOIN messages m ON m.chat=e.chat AND m.id=e.id
                    WHERE e.chat=?1 AND e.id=?2 AND ({})", super::event_rsvps::PUBLIC_EVENT),
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
        self.legacy_event_response(chat, event, responder, response)
    }
}

impl StoreWorker {
    pub(crate) async fn set_reaction(&self, chat: &str, target: &str, sender: &str, emoji: &str) -> Result<()> {
        let chat = chat.to_owned();
        let target = target.to_owned();
        let sender = sender.to_owned();
        let emoji = emoji.to_owned();
        self.run(move |store| store.set_reaction(&chat, &target, &sender, &emoji)).await
    }

    pub(crate) async fn set_starred(&self, chat: &str, id: &str, starred: bool) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.set_starred(&chat, &id, starred)).await
    }

    pub(crate) async fn marks(&self, chat: &str) -> Result<ChatMarks> {
        let chat = chat.to_owned();
        self.run(move |store| store.marks(&chat)).await
    }

    pub(crate) async fn marks_for(&self, chat: &str, message_ids: Option<&[String]>) -> Result<ChatMarks> {
        let chat = chat.to_owned();
        let message_ids = message_ids.map(|value| value.to_vec());
        self.run(move |store| store.marks_for(&chat, message_ids.as_deref())).await
    }

    pub(crate) async fn set_forwarded(&self, chat: &str, id: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.set_forwarded(&chat, &id)).await
    }

    pub(crate) async fn save_poll(&self, chat: &str, id: &str, creator: &str, name: &str, options: &[String], multi: bool, secret: Option<&[u8]>) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let creator = creator.to_owned();
        let name = name.to_owned();
        let options = options.to_vec();
        let secret = secret.map(|value| value.to_vec());
        self.run(move |store| store.save_poll(&chat, &id, &creator, &name, &options, multi, secret.as_deref())).await
    }

    pub(crate) async fn poll_secret(&self, chat: &str, id: &str) -> Result<Option<Secretive>> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.poll_secret(&chat, &id)).await
    }

    pub(crate) async fn set_poll_vote(&self, chat: &str, poll: &str, voter: &str, options: &[String]) -> Result<()> {
        let chat = chat.to_owned();
        let poll = poll.to_owned();
        let voter = voter.to_owned();
        let options = options.to_vec();
        self.run(move |store| store.set_poll_vote(&chat, &poll, &voter, &options)).await
    }

    pub(crate) async fn save_event(&self, chat: &str, id: &str, creator: &str, event: &NewEvent, secret: Option<&[u8]>) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let creator = creator.to_owned();
        let event = event.clone();
        let secret = secret.map(|value| value.to_vec());
        self.run(move |store| store.save_event(&chat, &id, &creator, &event, secret.as_deref())).await
    }

}


/// One chat's marks, optionally limited to a window of message ids. The id
/// list is encoded once as the JSON every query filters on.
struct MarksScope<'a> {
    conn: &'a Connection,
    chat: String,
    window: Option<String>,
}

impl MarksScope<'_> {
    fn new<'a>(conn: &'a Connection, chat: &str, message_ids: Option<&[String]>) -> Result<MarksScope<'a>> {
        anyhow::ensure!(
            message_ids.is_none_or(|ids| ids.len() <= MAX_MESSAGE_PAGE as usize),
            "too many message IDs"
        );
        Ok(MarksScope {
            conn,
            chat: names::canonical_chat(conn, chat)?.into_owned(),
            window: message_ids.map(serde_json::to_string).transpose()?,
        })
    }

    fn reactions(&self) -> Result<Vec<Reaction>> {
        let rows = self
            .conn
            .prepare("SELECT target, sender, emoji FROM reactions WHERE chat = ?1 AND (?2 IS NULL OR target IN (SELECT value FROM json_each(?2)))")?
            .query_map(params![self.chat, self.window], |r| {
                Ok(Reaction { target: r.get(0)?, sender: r.get(1)?, emoji: r.get(2)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    fn starred(&self) -> Result<Vec<String>> {
        let rows = self
            .conn
            .prepare("SELECT id FROM stars WHERE chat = ?1 AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))")?
            .query_map(params![self.chat, self.window], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    fn pinned(&self) -> Result<Option<String>> {
        super::history_pins::pinned(self.conn, &self.chat)
    }

    fn polls_with_votes(&self) -> Result<Vec<Poll>> {
        let mut polls: Vec<Poll> = self
            .conn
            .prepare("SELECT id, name, options, multi FROM polls WHERE chat = ?1 AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))")?
            .query_map(params![self.chat, self.window], |r| {
                Ok(Poll {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    options: json_list(r.get(2)?),
                    multi: r.get::<_, i32>(3)? != 0,
                    votes: Vec::new(),
                    quiz: None,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let votes: Vec<(String, PollVote)> = self
            .conn
            .prepare("SELECT poll, voter, options FROM poll_votes WHERE chat = ?1 AND (?2 IS NULL OR poll IN (SELECT value FROM json_each(?2)))")?
            .query_map(params![self.chat, self.window], |r| {
                Ok((r.get(0)?, PollVote { voter: r.get(1)?, options: json_list(r.get(2)?) }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (poll, vote) in votes {
            if let Some(p) = polls.iter_mut().find(|p| p.id == poll) {
                p.votes.push(vote);
            }
        }
        Ok(polls)
    }

    fn events_with_responses(&self) -> Result<Vec<Event>> {
        let mut events: Vec<Event> = self
            .conn
            .prepare(
                &format!("SELECT e.id,e.name,e.description,e.start_at,e.end_at,e.location,e.link,e.canceled,
                 e.extra_guests_allowed,e.is_scheduled_call,e.has_reminder,e.reminder_offset_sec,e.invitation_id,e.invitation,
                 COALESCE(length(e.secret)=32 AND e.canceled=0 AND e.invitation=0 AND EXISTS(SELECT 1 FROM messages m
                     WHERE m.chat=e.chat AND m.id=e.id AND ({})),0),
                 EXISTS(SELECT 1 FROM messages m WHERE m.chat=e.chat AND m.id=e.id AND ({}) AND (
                     EXISTS(SELECT 1 FROM message_pin_sync s WHERE s.chat=e.chat AND s.target=e.id AND s.pinned=1
                         AND (s.expires_at IS NULL OR s.expires_at>?3)) OR
                     EXISTS(SELECT 1 FROM message_pins p WHERE p.chat=e.chat AND p.id=e.id
                         AND NOT EXISTS(SELECT 1 FROM message_pin_sync s WHERE s.chat=e.chat))))
                 FROM events e WHERE e.chat=?1 AND (?2 IS NULL OR e.id IN (SELECT value FROM json_each(?2)))",
                    super::event_rsvps::PUBLIC_EVENT, super::event_rsvps::PUBLIC_EVENT),
            )?
            .query_map(params![self.chat, self.window, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_millis().min(i64::MAX as u128) as i64], |r| {
                Ok(Event {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    start: r.get(3)?,
                    end: r.get(4)?,
                    location: r.get(5)?,
                    link: r.get(6)?,
                    canceled: r.get::<_, i32>(7)? != 0,
                    extra_guests_allowed:r.get(8)?,is_scheduled_call:r.get(9)?,has_reminder:r.get(10)?,
                    reminder_offset_sec:r.get(11)?,invitation_id:r.get(12)?,invitation:r.get(13)?,can_respond:r.get(14)?,pinned:r.get(15)?,
                    responses: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let responses: Vec<(String, EventResponse)> = self
            .conn
            .prepare("SELECT event,responder,response,extra_guest_count,timestamp_ms FROM event_responses WHERE chat=?1
                AND (response IN ('going','not_going','maybe') OR (source_id IS NULL AND response<>''))
                AND (?2 IS NULL OR event IN (SELECT value FROM json_each(?2)))")?
            .query_map(params![self.chat, self.window], |r| {
                Ok((r.get(0)?, EventResponse { responder:r.get(1)?,response:r.get(2)?,extra_guest_count:r.get(3)?,timestamp_ms:r.get(4)? }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (event, response) in responses {
            if let Some(e) = events.iter_mut().find(|e| e.id == event) {
                e.responses.push(response);
            }
        }
        Ok(events)
    }

    fn view_once(&self) -> Result<Vec<ViewOnce>> {
        let rows = self
            .conn
            .prepare(
                "SELECT id, opened,
                        EXISTS(SELECT 1 FROM messages m
                                WHERE m.chat = v.chat AND m.id = v.id
                                  AND m.media_path IS NOT NULL AND m.media_path != '')
                     OR EXISTS(SELECT 1 FROM messages q
                                WHERE q.reply_to_id = v.id
                                  AND q.reply_to_locator IS NOT NULL AND q.reply_to_locator != '')
                 FROM view_once v WHERE chat = ?1 AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))",
            )?
            .query_map(params![self.chat, self.window], |r| {
                Ok(ViewOnce { id: r.get(0)?, opened: r.get::<_, i32>(1)? != 0, available: r.get::<_, i32>(2)? != 0 })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    fn simple_marks(&self, table: &str) -> Result<Vec<String>> {
        let rows = self
            .conn
            .prepare(&format!("SELECT id FROM {table} WHERE chat = ?1 AND (?2 IS NULL OR id IN (SELECT value FROM json_each(?2)))"))?
            .query_map(params![self.chat, self.window], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

fn json_list(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}
