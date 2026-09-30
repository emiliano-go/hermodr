use super::*;
use anyhow::ensure;
use whatsapp_rust::wacore::poll::compute_option_hash;

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS poll_option_hashes (
    chat TEXT NOT NULL, id TEXT NOT NULL, options TEXT NOT NULL,
    allow_add_option INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (chat, id));
    CREATE TABLE IF NOT EXISTS secret_edit_revisions (
    chat TEXT NOT NULL, id TEXT NOT NULL, timestamp_ms INTEGER NOT NULL,
    message_id TEXT NOT NULL, PRIMARY KEY (chat, id));";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PollOption {
    pub name: String,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PollEdit {
    pub name: Option<String>,
    pub options: Vec<PollOption>,
    pub selectable: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EventEdit {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub location: Option<String>,
    pub link: Option<String>,
    pub canceled: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) enum SecretEdit {
    Poll(PollEdit),
    AddOption(PollOption),
    Event(EventEdit),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditRevision {
    pub timestamp_ms: i64,
    pub message_id: String,
}

fn revision_is_fresh(conn: &Connection, chat: &str, id: &str, created_at: i64, revision: &EditRevision) -> Result<bool> {
    ensure!(
        revision.timestamp_ms >= 0 && !revision.message_id.trim().is_empty(),
        "invalid edit revision"
    );
    let created_ms = created_at
        .checked_mul(1000)
        .ok_or_else(|| anyhow::anyhow!("creation timestamp overflow"))?;
    if revision.timestamp_ms < created_ms {
        return Ok(false);
    }
    let previous: Option<i64> = conn
        .query_row(
            "SELECT timestamp_ms FROM secret_edit_revisions WHERE chat = ?1 AND id = ?2",
            params![chat, id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(previous.map_or(true, |timestamp| revision.timestamp_ms > timestamp))
}

fn validate_options(options: &[PollOption]) -> Result<()> {
    ensure!(!options.is_empty(), "poll has no options");
    ensure!(options.len() <= 12, "poll has too many options");
    for (index, option) in options.iter().enumerate() {
        ensure!(!option.name.is_empty(), "poll option has empty name");
        ensure!(
            options[..index]
                .iter()
                .all(|other| other.name != option.name && other.hash != option.hash),
            "duplicate poll option name or hash"
        );
    }
    Ok(())
}

fn option_row(conn: &Connection, chat: &str, id: &str) -> Result<Option<(Vec<PollOption>, bool)>> {
    let row: Option<(String, bool)> = conn
        .query_row(
            "SELECT options, allow_add_option FROM poll_option_hashes WHERE chat = ?1 AND id = ?2",
            params![chat, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(json, allow)| {
        let options: Vec<PollOption> = serde_json::from_str(&json)?;
        validate_options(&options)?;
        Ok((options, allow))
    })
    .transpose()
}

fn poll_options(conn: &Connection, chat: &str, id: &str, stored: &[String]) -> Result<(Vec<PollOption>, bool)> {
    if let Some((options, allow)) = option_row(conn, chat, id)? {
        ensure!(
            options.iter().map(|option| &option.name).eq(stored.iter()),
            "poll option metadata differs from definition"
        );
        Ok((options, allow))
    } else {
        let options: Vec<_> = stored
            .iter()
            .map(|name| PollOption {
                name: name.clone(),
                hash: compute_option_hash(name),
            })
            .collect();
        validate_options(&options)?;
        Ok((options, false))
    }
}

fn matching(forms: &[String], actual: &str) -> bool {
    forms.iter().any(|form| form == actual)
}

fn expanded_forms(store: &MessageStore, forms: &[String]) -> Result<Vec<String>> {
    let mut expanded = Vec::new();
    for form in forms {
        expanded.extend(names::name_forms(store, form)?);
    }
    expanded.sort();
    expanded.dedup();
    Ok(expanded)
}

impl MessageStore {
    pub(crate) fn remember_poll_options(&self, chat: &str, id: &str, options: &[PollOption], allow_add_option: bool) -> Result<()> {
        validate_options(options)?;
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        if option_row(&conn, chat, id)?.is_some() {
            return Ok(());
        }
        let stored: Option<String> = conn
            .query_row("SELECT options FROM polls WHERE chat = ?1 AND id = ?2", params![chat, id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(stored) = stored else {
            anyhow::bail!("poll definition is missing");
        };
        let names: Vec<String> = serde_json::from_str(&stored)?;
        ensure!(
            options.iter().map(|option| &option.name).eq(names.iter()),
            "poll option hashes do not match definition"
        );
        conn.execute(
            "INSERT OR IGNORE INTO poll_option_hashes (chat, id, options, allow_add_option) VALUES (?1, ?2, ?3, ?4)",
            params![chat, id, serde_json::to_string(options)?, allow_add_option],
        )?;
        Ok(())
    }

    pub(crate) fn poll_option_hashes(&self, chat: &str, id: &str, chosen: &[String]) -> Result<Vec<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let stored: String = conn.query_row("SELECT options FROM polls WHERE chat = ?1 AND id = ?2", params![chat, id], |r| {
            r.get(0)
        })?;
        let names: Vec<String> = serde_json::from_str(&stored)?;
        let (options, _) = poll_options(&conn, chat, id, &names)?;
        let mut hashes = Vec::with_capacity(chosen.len());
        for name in chosen {
            let option = options
                .iter()
                .find(|option| option.name == *name)
                .ok_or_else(|| anyhow::anyhow!("unknown poll option"))?;
            ensure!(
                !hashes.iter().any(|hash: &Vec<u8>| hash.as_slice() == option.hash),
                "duplicate poll choice"
            );
            hashes.push(option.hash.to_vec());
        }
        Ok(hashes)
    }

    pub(crate) fn poll_option_names(&self, chat: &str, id: &str, hashes: &[Vec<u8>]) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let stored: String = conn.query_row("SELECT options FROM polls WHERE chat = ?1 AND id = ?2", params![chat, id], |r| {
            r.get(0)
        })?;
        let names: Vec<String> = serde_json::from_str(&stored)?;
        let (options, _) = poll_options(&conn, chat, id, &names)?;
        let mut result = Vec::with_capacity(hashes.len());
        for hash in hashes {
            ensure!(hash.len() == 32, "invalid poll option hash");
            let option = options
                .iter()
                .find(|option| option.hash.as_slice() == hash)
                .ok_or_else(|| anyhow::anyhow!("unknown poll option hash"))?;
            ensure!(!result.contains(&option.name), "duplicate poll choice");
            result.push(option.name.clone());
        }
        Ok(result)
    }

    pub(crate) fn replace_event_content(&self, chat: &str, id: &str, event: &NewEvent) -> Result<bool> {
        ensure!(!event.name.is_empty(), "event title is empty");
        let mut conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?.into_owned();
        let transaction = conn.savepoint()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages m JOIN events e ON e.chat = m.chat AND e.id = m.id
             WHERE m.chat = ?1 AND m.id = ?2 AND m.media_kind = 'event' AND m.revoked = 0 AND m.deleted = 0)",
            params![chat, id],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE events SET name = ?3, description = ?4, start_at = ?5, end_at = ?6,
             location = ?7, link = ?8, canceled = ?9 WHERE chat = ?1 AND id = ?2",
            params![
                chat,
                id,
                event.name,
                event.description,
                event.start,
                event.end,
                event.location,
                event.link,
                event.canceled
            ],
        )?;
        transaction.execute(
            "UPDATE messages SET text = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, event.name],
        )?;
        transaction.execute("INSERT OR IGNORE INTO edited (chat, id) VALUES (?1, ?2)", params![chat, id])?;
        super::links::refresh(&transaction, &chat, id)?;
        transaction.commit()?;
        Ok(true)
    }

    pub(crate) fn apply_secret_edit(
        &self,
        chat: &str,
        id: &str,
        editor_forms: &[String],
        target_author_forms: &[String],
        edit: &SecretEdit,
        revision: &EditRevision,
    ) -> Result<bool> {
        let editors = expanded_forms(self, editor_forms)?;
        let authors = expanded_forms(self, target_author_forms)?;
        if editors.is_empty() || authors.is_empty() {
            return Ok(false);
        }
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute_batch("SAVEPOINT postal_secret_edit")?;
        let result = (|| -> Result<bool> {
            let target: Option<(String, Option<String>, i64)> = conn
                .query_row(
                    "SELECT sender, media_kind, timestamp FROM messages WHERE chat = ?1 AND id = ?2 AND revoked = 0 AND deleted = 0",
                    params![chat, id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((sender, kind, created_at)) = target else {
                return Ok(false);
            };
            if !matching(&authors, &sender) {
                return Ok(false);
            }
            match edit {
                SecretEdit::Poll(change) => {
                    if kind.as_deref() != Some("poll") {
                        return Ok(false);
                    }
                    let row: Option<(String, String, Vec<String>, bool)> = conn
                        .query_row(
                            "SELECT creator, name, options, multi FROM polls WHERE chat = ?1 AND id = ?2",
                            params![chat, id],
                            |r| {
                                let json: String = r.get(2)?;
                                Ok((
                                    r.get(0)?,
                                    r.get(1)?,
                                    serde_json::from_str(&json).map_err(|e| {
                                        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
                                    })?,
                                    r.get(3)?,
                                ))
                            },
                        )
                        .optional()?;
                    let Some((creator, old_name, old_names, multi)) = row else {
                        return Ok(false);
                    };
                    if !matching(&authors, &creator) || !matching(&editors, &creator) {
                        return Ok(false);
                    }
                    if !revision_is_fresh(&conn, chat, id, created_at, revision)? {
                        return Ok(false);
                    }
                    let new_name = change.name.as_ref().unwrap_or(&old_name);
                    ensure!(!new_name.is_empty(), "poll title is empty");
                    let (old_options, allow) = poll_options(&conn, chat, id, &old_names)?;
                    let options = if change.options.is_empty() {
                        &old_options
                    } else {
                        validate_options(&change.options)?;
                        &change.options
                    };
                    let names: Vec<_> = options.iter().map(|option| option.name.clone()).collect();
                    let multi = change.selectable.map(|count| count != 1).unwrap_or(multi);
                    if let Some(count) = change.selectable {
                        ensure!(count == 0 || count as usize <= options.len(), "invalid selectable option count");
                    }
                    conn.execute(
                        "UPDATE polls SET name = ?3, options = ?4, multi = ?5 WHERE chat = ?1 AND id = ?2",
                        params![chat, id, new_name, serde_json::to_string(&names)?, multi],
                    )?;
                    if !change.options.is_empty() {
                        conn.execute(
                            "INSERT INTO poll_option_hashes (chat, id, options, allow_add_option) VALUES (?1, ?2, ?3, ?4)
                            ON CONFLICT(chat, id) DO UPDATE SET options = excluded.options, allow_add_option = excluded.allow_add_option",
                            params![chat, id, serde_json::to_string(options)?, allow],
                        )?;
                        migrate_votes(&conn, chat, id, &old_options, options)?;
                    }
                    conn.execute(
                        "UPDATE messages SET text = ?3 WHERE chat = ?1 AND id = ?2",
                        params![chat, id, new_name],
                    )?;
                }
                SecretEdit::AddOption(option) => {
                    if kind.as_deref() != Some("poll") {
                        return Ok(false);
                    }
                    let row: Option<(String, Vec<String>)> = conn
                        .query_row(
                            "SELECT creator, options FROM polls WHERE chat = ?1 AND id = ?2",
                            params![chat, id],
                            |r| {
                                let json: String = r.get(1)?;
                                Ok((
                                    r.get(0)?,
                                    serde_json::from_str(&json).map_err(|e| {
                                        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
                                    })?,
                                ))
                            },
                        )
                        .optional()?;
                    let Some((creator, old_names)) = row else {
                        return Ok(false);
                    };
                    if !matching(&authors, &creator) {
                        return Ok(false);
                    }
                    let (mut options, allow) = poll_options(&conn, chat, id, &old_names)?;
                    if !allow {
                        return Ok(false);
                    }
                    if !revision_is_fresh(&conn, chat, id, created_at, revision)? {
                        return Ok(false);
                    }
                    options.push(option.clone());
                    validate_options(&options)?;
                    let names: Vec<_> = options.iter().map(|option| option.name.clone()).collect();
                    conn.execute(
                        "UPDATE polls SET options = ?3 WHERE chat = ?1 AND id = ?2",
                        params![chat, id, serde_json::to_string(&names)?],
                    )?;
                    conn.execute(
                        "INSERT INTO poll_option_hashes (chat, id, options, allow_add_option) VALUES (?1, ?2, ?3, ?4)
                        ON CONFLICT(chat, id) DO UPDATE SET options = excluded.options, allow_add_option = excluded.allow_add_option",
                        params![chat, id, serde_json::to_string(&options)?, allow],
                    )?;
                }
                SecretEdit::Event(change) => {
                    if kind.as_deref() != Some("event") {
                        return Ok(false);
                    }
                    let row: Option<(String, String)> = conn
                        .query_row(
                            "SELECT creator, name FROM events WHERE chat = ?1 AND id = ?2",
                            params![chat, id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    let Some((creator, old_name)) = row else {
                        return Ok(false);
                    };
                    if !matching(&authors, &creator) || !matching(&editors, &creator) {
                        return Ok(false);
                    }
                    if !revision_is_fresh(&conn, chat, id, created_at, revision)? {
                        return Ok(false);
                    }
                    let new_name = change.name.as_ref().unwrap_or(&old_name);
                    ensure!(!new_name.is_empty(), "event title is empty");
                    conn.execute(
                        "UPDATE events SET name = ?3, description = COALESCE(?4, description),
                        start_at = COALESCE(?5, start_at), end_at = COALESCE(?6, end_at),
                        location = COALESCE(?7, location), link = COALESCE(?8, link),
                        canceled = COALESCE(?9, canceled) WHERE chat = ?1 AND id = ?2",
                        params![
                            chat,
                            id,
                            new_name,
                            change.description,
                            change.start,
                            change.end,
                            change.location,
                            change.link,
                            change.canceled
                        ],
                    )?;
                    conn.execute(
                        "UPDATE messages SET text = ?3 WHERE chat = ?1 AND id = ?2",
                        params![chat, id, new_name],
                    )?;
                }
            }
            conn.execute("INSERT OR IGNORE INTO edited (chat, id) VALUES (?1, ?2)", params![chat, id])?;
            conn.execute(
                "INSERT INTO secret_edit_revisions (chat, id, timestamp_ms, message_id) VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(chat, id) DO UPDATE SET timestamp_ms = excluded.timestamp_ms, message_id = excluded.message_id",
                params![chat, id, revision.timestamp_ms, revision.message_id],
            )?;
            Ok(true)
        })();
        match result {
            Ok(value) => {
                conn.execute_batch("RELEASE postal_secret_edit")?;
                Ok(value)
            }
            Err(error) => {
                conn.execute_batch("ROLLBACK TO postal_secret_edit; RELEASE postal_secret_edit")?;
                Err(error)
            }
        }
    }
}

impl StoreWorker {
    pub(crate) async fn replace_event_content(&self, chat: &str, id: &str, event: &NewEvent) -> Result<bool> {
        let (chat, id, event) = (chat.to_owned(), id.to_owned(), event.clone());
        self.run(move |store| store.replace_event_content(&chat, &id, &event)).await
    }

    pub(crate) async fn remember_poll_options(&self, chat: &str, id: &str, options: &[PollOption], allow_add_option: bool) -> Result<()> {
        let (chat, id, options) = (chat.to_owned(), id.to_owned(), options.to_vec());
        self.run(move |store| store.remember_poll_options(&chat, &id, &options, allow_add_option))
            .await
    }

    pub(crate) async fn poll_option_hashes(&self, chat: &str, id: &str, chosen: &[String]) -> Result<Vec<Vec<u8>>> {
        let (chat, id, chosen) = (chat.to_owned(), id.to_owned(), chosen.to_vec());
        self.run(move |store| store.poll_option_hashes(&chat, &id, &chosen)).await
    }

    pub(crate) async fn poll_option_names(&self, chat: &str, id: &str, hashes: &[Vec<u8>]) -> Result<Vec<String>> {
        let (chat, id, hashes) = (chat.to_owned(), id.to_owned(), hashes.to_vec());
        self.run(move |store| store.poll_option_names(&chat, &id, &hashes)).await
    }

    pub(crate) async fn apply_secret_edit(
        &self,
        chat: &str,
        id: &str,
        editor_forms: &[String],
        target_author_forms: &[String],
        edit: &SecretEdit,
        revision: &EditRevision,
    ) -> Result<bool> {
        let (chat, id, editors, authors, edit, revision) = (
            chat.to_owned(),
            id.to_owned(),
            editor_forms.to_vec(),
            target_author_forms.to_vec(),
            edit.clone(),
            revision.clone(),
        );
        self.run(move |store| store.apply_secret_edit(&chat, &id, &editors, &authors, &edit, &revision))
            .await
    }
}

fn migrate_votes(conn: &Connection, chat: &str, id: &str, old: &[PollOption], new: &[PollOption]) -> Result<()> {
    let votes: Vec<(String, String)> = conn
        .prepare("SELECT voter, options FROM poll_votes WHERE chat = ?1 AND poll = ?2")?
        .query_map(params![chat, id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (voter, json) in votes {
        let chosen: Vec<String> = serde_json::from_str(&json)?;
        let mut renamed = Vec::new();
        for name in chosen {
            let hash = old
                .iter()
                .find(|option| option.name == name)
                .map(|option| option.hash)
                .ok_or_else(|| anyhow::anyhow!("vote references unknown poll option"))?;
            if let Some(option) = new.iter().find(|option| option.hash == hash) {
                if !renamed.contains(&option.name) {
                    renamed.push(option.name.clone());
                }
            }
        }
        conn.execute(
            "UPDATE poll_votes SET options = ?4 WHERE chat = ?1 AND poll = ?2 AND voter = ?3",
            params![chat, id, voter, serde_json::to_string(&renamed)?],
        )?;
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum Definition {
    Poll(String, String, Vec<String>, bool),
    Event(
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        bool,
    ),
}

impl Definition {
    fn kind(&self) -> &str {
        match self {
            Self::Poll(..) => "poll",
            Self::Event(..) => "event",
        }
    }
}

fn canonical_actor(conn: &Connection, jid: &str) -> Result<String> {
    let Some((user, server)) = jid.split_once('@') else {
        return Ok(jid.to_owned());
    };
    let bare = format!("{}@{server}", user.split(':').next().unwrap_or(user));
    Ok(names::canonical_chat(conn, &bare)?.into_owned())
}

fn definition(conn: &Connection, chat: &str, id: &str) -> Result<Option<Definition>> {
    let kind: Option<Option<String>> = conn
        .query_row(
            "SELECT media_kind FROM messages WHERE chat = ?1 AND id = ?2",
            params![chat, id],
            |r| r.get(0),
        )
        .optional()?;
    match kind.flatten().as_deref() {
        Some("poll") => {
            let row: Option<(String, String, String, bool)> = conn
                .query_row(
                    "SELECT creator, name, options, multi FROM polls WHERE chat = ?1 AND id = ?2",
                    params![chat, id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            row.map(|(creator, name, options, multi)| {
                Ok(Definition::Poll(
                    canonical_actor(conn, &creator)?,
                    name,
                    serde_json::from_str(&options)?,
                    multi,
                ))
            })
            .transpose()
        }
        Some("event") => {
            let row: Option<(
                String,
                String,
                Option<String>,
                Option<i64>,
                Option<i64>,
                Option<String>,
                Option<String>,
                bool,
            )> = conn
                .query_row(
                    "SELECT creator, name, description, start_at, end_at, location, link, canceled FROM events WHERE chat = ?1 AND id = ?2",
                    params![chat, id],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                            r.get(7)?,
                        ))
                    },
                )
                .optional()?;
            row.map(|(creator, name, description, start, end, location, link, canceled)| {
                Ok(Definition::Event(
                    canonical_actor(conn, &creator)?,
                    name,
                    description,
                    start,
                    end,
                    location,
                    link,
                    canceled,
                ))
            })
            .transpose()
        }
        _ => Ok(None),
    }
}

fn valid_option_metadata(conn: &Connection, chat: &str, id: &str, definition: Option<&Definition>) -> Result<bool> {
    let Some(Definition::Poll(_, _, names, _)) = definition else {
        return Ok(false);
    };
    let json: Option<String> = conn
        .query_row(
            "SELECT options FROM poll_option_hashes WHERE chat = ?1 AND id = ?2",
            params![chat, id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(json) = json else {
        return Ok(false);
    };
    let Ok(options) = serde_json::from_str::<Vec<PollOption>>(&json) else {
        return Ok(false);
    };
    Ok(validate_options(&options).is_ok() && options.iter().map(|option| &option.name).eq(names.iter()))
}

fn merge_one(conn: &Connection, from: &str, to: &str, id: &str) -> Result<()> {
    let source = definition(conn, from, id)?;
    let destination = definition(conn, to, id)?;
    let has_destination_row: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM polls WHERE chat = ?1 AND id = ?2)
            OR EXISTS(SELECT 1 FROM events WHERE chat = ?1 AND id = ?2)",
        params![to, id],
        |r| r.get(0),
    )?;
    let target_kind: Option<Option<String>> = conn
        .query_row(
            "SELECT media_kind FROM messages WHERE chat = ?1 AND id = ?2",
            params![to, id],
            |r| r.get(0),
        )
        .optional()?;
    let source_survives = !has_destination_row
        && source.as_ref().map_or(false, |source| {
            target_kind.as_ref().map_or(true, |kind| kind.as_deref() == Some(source.kind()))
        });
    let equivalent = source.is_some() && source == destination;
    let keep_target_options = valid_option_metadata(conn, to, id, destination.as_ref())?;
    let copy_source_options =
        valid_option_metadata(conn, from, id, source.as_ref())? && (source_survives || (equivalent && !keep_target_options));
    if !keep_target_options {
        conn.execute("DELETE FROM poll_option_hashes WHERE chat = ?1 AND id = ?2", params![to, id])?;
    }
    if copy_source_options {
        conn.execute(
            "INSERT OR IGNORE INTO poll_option_hashes (chat, id, options, allow_add_option)
            SELECT ?1, id, options, allow_add_option FROM poll_option_hashes WHERE chat = ?2 AND id = ?3",
            params![to, from, id],
        )?;
    }
    conn.execute("DELETE FROM poll_option_hashes WHERE chat = ?1 AND id = ?2", params![from, id])?;

    let read_revision = |chat: &str| -> Result<Option<(i64, String)>> {
        Ok(conn
            .query_row(
                "SELECT timestamp_ms, message_id FROM secret_edit_revisions WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    };
    let target_revision = if destination.is_some() { read_revision(to)? } else { None };
    let source_revision = read_revision(from)?;
    let revision = if equivalent {
        match (target_revision, source_revision) {
            (Some(target), Some(source)) if source.0 > target.0 => Some(source),
            (target @ Some(_), _) => target,
            (None, source) => source,
        }
    } else if source_survives {
        source_revision
    } else {
        target_revision
    };
    conn.execute(
        "DELETE FROM secret_edit_revisions WHERE chat IN (?1, ?2) AND id = ?3",
        params![from, to, id],
    )?;
    if let Some((timestamp_ms, message_id)) = revision {
        conn.execute(
            "INSERT INTO secret_edit_revisions (chat, id, timestamp_ms, message_id) VALUES (?1, ?2, ?3, ?4)",
            params![to, id, timestamp_ms, message_id],
        )?;
    }
    Ok(())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let ids: Vec<String> = conn
        .prepare(
            "SELECT id FROM poll_option_hashes WHERE chat IN (?1, ?2)
        UNION SELECT id FROM secret_edit_revisions WHERE chat IN (?1, ?2)",
        )?
        .query_map(params![from, to], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        merge_one(conn, from, to, &id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAT: &str = "group@g.us";
    const CREATOR: &str = "creator@s.whatsapp.net";

    fn setup(kind: &str) -> MessageStore {
        let store = MessageStore::open(std::path::Path::new(":memory:")).unwrap();
        store.conn.lock().unwrap().execute_batch(SCHEMA).unwrap();
        let message = StoredMessage {
            header: MessageHeader {
                chat: CHAT.into(),
                id: "item".into(),
                sender: CREATOR.into(),
                timestamp: 1,
                from_me: false,
            },
            text: "Original".into(),
            media: Media {
                kind: Some(kind.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        store.insert_message(&message).unwrap();
        if kind == "poll" {
            store
                .save_poll(
                    CHAT,
                    "item",
                    CREATOR,
                    "Original",
                    &["A".into(), "B".into()],
                    false,
                    Some(b"poll-secret"),
                )
                .unwrap();
        } else {
            store
                .save_event(
                    CHAT,
                    "item",
                    CREATOR,
                    &NewEvent {
                        name: "Original".into(),
                        description: Some("Details".into()),
                        start: Some(10),
                        end: None,
                        location: None,
                        link: None,
                        canceled: false,
                    },
                    Some(b"event-secret"),
                )
                .unwrap();
        }
        store
    }

    fn option(name: &str, byte: u8) -> PollOption {
        PollOption {
            name: name.into(),
            hash: [byte; 32],
        }
    }
    fn forms() -> Vec<String> {
        vec![CREATOR.into()]
    }
    fn rev(timestamp_ms: i64, message_id: &str) -> EditRevision {
        EditRevision {
            timestamp_ms,
            message_id: message_id.into(),
        }
    }

    fn merge_store() -> MessageStore {
        let store = MessageStore::open(std::path::Path::new(":memory:")).unwrap();
        store.conn.lock().unwrap().execute_batch(SCHEMA).unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute("INSERT INTO lid_pn (lid, pn) VALUES ('123', '456')", [])
            .unwrap();
        store
    }

    fn merge_poll(conn: &Connection, chat: &str, creator: &str, title: &str, options: &[&str], hashes: &[u8], revision: i64) {
        conn.execute(
            "INSERT INTO messages (chat, id, sender, timestamp, from_me, text, media_kind) VALUES (?1, 'item', ?2, 1, 0, ?3, 'poll')",
            params![chat, creator, title],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO polls (chat, id, creator, name, options, multi) VALUES (?1, 'item', ?2, ?3, ?4, 0)",
            params![chat, creator, title, serde_json::to_string(options).unwrap()],
        )
        .unwrap();
        let options: Vec<PollOption> = options.iter().zip(hashes).map(|(name, hash)| option(name, *hash)).collect();
        conn.execute(
            "INSERT INTO poll_option_hashes (chat, id, options, allow_add_option) VALUES (?1, 'item', ?2, 1)",
            params![chat, serde_json::to_string(&options).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO secret_edit_revisions (chat, id, timestamp_ms, message_id) VALUES (?1, 'item', ?2, ?3)",
            params![chat, revision, format!("edit-{revision}")],
        )
        .unwrap();
    }

    #[test]
    fn rename_uses_stable_hashes_for_existing_and_future_votes() {
        let store = setup("poll");
        store
            .remember_poll_options(CHAT, "item", &[option("A", 1), option("B", 2)], true)
            .unwrap();
        store.set_poll_vote(CHAT, "item", "voter", &["A".into()]).unwrap();
        let edit = SecretEdit::Poll(PollEdit {
            name: Some("New title".into()),
            options: vec![option("B", 2), option("Renamed A", 1)],
            selectable: Some(1),
        });
        assert!(store
            .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        let marks = store.marks(CHAT).unwrap();
        assert_eq!(marks.polls[0].votes[0].options, vec!["Renamed A"]);
        assert_eq!(store.poll_option_names(CHAT, "item", &[vec![1; 32]]).unwrap(), vec!["Renamed A"]);
        assert_eq!(
            store.poll_option_hashes(CHAT, "item", &["Renamed A".into()]).unwrap(),
            vec![vec![1; 32]]
        );
        assert_eq!(store.messages_for(CHAT, 1).unwrap()[0].text, "New title");
        assert_eq!(
            store.poll_secret(CHAT, "item").unwrap().unwrap().secret.as_slice(),
            b"poll-secret".as_slice()
        );
    }

    #[test]
    fn reorder_and_removed_identity_do_not_transfer_vote_by_position() {
        let store = setup("poll");
        store
            .remember_poll_options(CHAT, "item", &[option("A", 1), option("B", 2)], false)
            .unwrap();
        store.set_poll_vote(CHAT, "item", "voter", &["A".into(), "B".into()]).unwrap();
        let edit = SecretEdit::Poll(PollEdit {
            options: vec![option("C", 3), option("B", 2)],
            ..Default::default()
        });
        assert!(store
            .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        assert_eq!(store.marks(CHAT).unwrap().polls[0].votes[0].options, vec!["B"]);
        assert!(store.poll_option_names(CHAT, "item", &[vec![1; 32]]).is_err());
    }

    #[test]
    fn wrong_sender_and_missing_target_leave_rows_unchanged() {
        let store = setup("poll");
        let edit = SecretEdit::Poll(PollEdit {
            name: Some("Spoof".into()),
            ..Default::default()
        });
        assert!(!store
            .apply_secret_edit(CHAT, "item", &["other@s.whatsapp.net".into()], &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        assert!(!store
            .apply_secret_edit(CHAT, "item", &forms(), &["other@s.whatsapp.net".into()], &edit, &rev(2000, "e1"))
            .unwrap());
        assert!(!store
            .apply_secret_edit(CHAT, "missing", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        assert_eq!(store.messages_for(CHAT, 1).unwrap()[0].text, "Original");
        assert!(store.marks(CHAT).unwrap().edited.is_empty());
    }

    #[test]
    fn event_partial_cancel_preserves_secret_and_responses() {
        let store = setup("event");
        store.set_event_response(CHAT, "item", "guest", "yes").unwrap();
        let edit = SecretEdit::Event(EventEdit {
            canceled: Some(true),
            ..Default::default()
        });
        assert!(store
            .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        let event = &store.marks(CHAT).unwrap().events[0];
        assert!(event.canceled);
        assert_eq!(event.name, "Original");
        assert_eq!(event.description.as_deref(), Some("Details"));
        assert_eq!(event.responses.len(), 1);
        assert_eq!(
            store.event_secret(CHAT, "item").unwrap().unwrap().secret.as_slice(),
            b"event-secret".as_slice()
        );
        assert_eq!(store.marks(CHAT).unwrap().edited, vec!["item"]);
    }

    #[test]
    fn failing_revision_insert_rolls_back_definition_preview_and_votes() {
        let store = setup("poll");
        store
            .remember_poll_options(CHAT, "item", &[option("A", 1), option("B", 2)], false)
            .unwrap();
        store.set_poll_vote(CHAT, "item", "voter", &["A".into()]).unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_revision AFTER INSERT ON secret_edit_revisions BEGIN SELECT RAISE(FAIL, 'injected failure'); END;",
            )
            .unwrap();
        let edit = SecretEdit::Poll(PollEdit {
            name: Some("Changed".into()),
            options: vec![option("B", 2), option("A2", 1)],
            ..Default::default()
        });
        assert!(store
            .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .is_err());
        assert_eq!(store.messages_for(CHAT, 1).unwrap()[0].text, "Original");
        let poll = &store.marks(CHAT).unwrap().polls[0];
        assert_eq!(poll.name, "Original");
        assert_eq!(poll.options, vec!["A", "B"]);
        assert_eq!(poll.votes[0].options, vec!["A"]);
        assert_eq!(store.poll_option_names(CHAT, "item", &[vec![1; 32]]).unwrap(), vec!["A"]);
        let revisions: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM secret_edit_revisions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(revisions, 0);
    }

    #[test]
    fn add_option_requires_explicit_permission() {
        let store = setup("poll");
        let edit = SecretEdit::AddOption(option("C", 3));
        assert!(!store
            .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit, &rev(2000, "e1"))
            .unwrap());
        store
            .remember_poll_options(CHAT, "item", &[option("A", 1), option("B", 2)], true)
            .unwrap();
        assert!(store
            .apply_secret_edit(
                CHAT,
                "item",
                &["participant@s.whatsapp.net".into()],
                &forms(),
                &edit,
                &rev(2000, "e1")
            )
            .unwrap());
        assert_eq!(store.marks(CHAT).unwrap().polls[0].options, vec!["A", "B", "C"]);
    }

    #[test]
    fn legacy_poll_hashes_use_stored_names() {
        let store = setup("poll");
        assert_eq!(
            store.poll_option_hashes(CHAT, "item", &["B".into()]).unwrap(),
            vec![compute_option_hash("B").to_vec()]
        );
        assert_eq!(
            store.poll_option_names(CHAT, "item", &[compute_option_hash("A").to_vec()]).unwrap(),
            vec!["A"]
        );
    }

    #[test]
    fn revisions_reject_stale_replay_and_equal_time_conflicts_for_poll_and_event() {
        for kind in ["poll", "event"] {
            let store = setup(kind);
            let edit = |name: &str| match kind {
                "poll" => SecretEdit::Poll(PollEdit {
                    name: Some(name.into()),
                    ..Default::default()
                }),
                _ => SecretEdit::Event(EventEdit {
                    name: Some(name.into()),
                    ..Default::default()
                }),
            };
            assert!(!store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("Too early"), &rev(999, "early"))
                .unwrap());
            assert!(store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("First"), &rev(1000, "edit-a"))
                .unwrap());
            assert!(!store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("Replay"), &rev(1000, "edit-a"))
                .unwrap());
            assert!(!store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("Tie"), &rev(1000, "edit-z"))
                .unwrap());
            assert!(!store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("Stale"), &rev(999, "edit-old"))
                .unwrap());
            assert_eq!(store.messages_for(CHAT, 1).unwrap()[0].text, "First");
            assert!(store
                .apply_secret_edit(CHAT, "item", &forms(), &forms(), &edit("Second"), &rev(1001, "edit-b"))
                .unwrap());
            let revision: (i64, String) = store
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT timestamp_ms, message_id FROM secret_edit_revisions WHERE chat = ?1 AND id = ?2",
                    params![CHAT, "item"],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(revision, (1001, "edit-b".into()));
            assert_eq!(store.messages_for(CHAT, 1).unwrap()[0].text, "Second");
        }
    }

    #[test]
    fn outbound_event_replacement_clears_fields_and_rolls_back_link_failure() {
        let store = setup("event");
        let original = NewEvent {
            name: "https://old.test".into(),
            description: Some("Details".into()),
            start: Some(10),
            end: Some(20),
            location: Some("Somewhere".into()),
            link: Some("https://join.test".into()),
            canceled: false,
        };
        store.save_event(CHAT, "item", CREATOR, &original, None).unwrap();
        store.set_event_response(CHAT, "item", "guest@s.whatsapp.net", "going").unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "UPDATE messages SET text = ?3, preview_url = 'https://preview.test' WHERE chat = ?1 AND id = ?2",
                params![CHAT, "item", original.name],
            )
            .unwrap();
            super::super::links::refresh(&conn, CHAT, "item").unwrap();
            conn.execute_batch(
                "CREATE TEMP TRIGGER fail_outbound_link_refresh BEFORE UPDATE OF link_urls ON messages
                BEGIN SELECT RAISE(FAIL, 'synthetic link failure'); END;",
            )
            .unwrap();
        }
        let before = serde_json::to_value(store.marks(CHAT).unwrap()).unwrap();
        let replacement = NewEvent {
            name: "https://new.test".into(),
            canceled: true,
            ..Default::default()
        };
        assert!(store
            .replace_event_content(CHAT, "item", &replacement)
            .unwrap_err()
            .to_string()
            .contains("synthetic link failure"));
        assert_eq!(store.message(CHAT, "item").unwrap().text, original.name);
        assert_eq!(serde_json::to_value(store.marks(CHAT).unwrap()).unwrap(), before);
        {
            let conn = store.conn.lock().unwrap();
            let links: String = conn
                .query_row(
                    "SELECT link_urls FROM messages WHERE chat = ?1 AND id = ?2",
                    params![CHAT, "item"],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&links).unwrap(),
                ["https://old.test", "https://preview.test"]
            );
            conn.execute_batch("DROP TRIGGER fail_outbound_link_refresh;").unwrap();
        }
        assert!(store.replace_event_content(CHAT, "item", &replacement).unwrap());
        let marks = store.marks(CHAT).unwrap();
        let updated = &marks.events[0];
        assert_eq!(updated.name, replacement.name);
        assert!(
            updated.description.is_none()
                && updated.start.is_none()
                && updated.end.is_none()
                && updated.location.is_none()
                && updated.link.is_none()
        );
        assert!(updated.canceled);
        assert_eq!(updated.responses[0].response, "going");
        assert_eq!(marks.edited, ["item"]);
        assert_eq!(store.event_secret(CHAT, "item").unwrap().unwrap().secret, b"event-secret");
        assert_eq!(store.message(CHAT, "item").unwrap().text, replacement.name);
        let links: String = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT link_urls FROM messages WHERE chat = ?1 AND id = ?2",
                params![CHAT, "item"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Vec<String>>(&links).unwrap(), ["https://new.test"]);
        assert!(!store.replace_event_content(CHAT, "missing", &replacement).unwrap());
        assert!(!setup("poll").replace_event_content(CHAT, "item", &replacement).unwrap());
    }

    #[test]
    fn alias_merge_keeps_destination_definition_and_matching_metadata() {
        let store = merge_store();
        let (from, to) = ("123@lid", "456@s.whatsapp.net");
        let conn = store.conn.lock().unwrap();
        merge_poll(&conn, from, "123@lid", "Renamed", &["New A", "B"], &[1, 2], 3000);
        merge_poll(&conn, to, "456@s.whatsapp.net", "Old", &["A", "B"], &[3, 2], 2000);
        merge(&conn, from, to).unwrap();
        let names: String = conn
            .query_row(
                "SELECT options FROM poll_option_hashes WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| r.get(0),
            )
            .unwrap();
        let options: Vec<PollOption> = serde_json::from_str(&names).unwrap();
        assert_eq!(options, vec![option("A", 3), option("B", 2)]);
        let revision: i64 = conn
            .query_row(
                "SELECT timestamp_ms FROM secret_edit_revisions WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revision, 2000);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM poll_option_hashes WHERE chat = ?1", [from], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn alias_merge_keeps_destination_hash_mapping_for_equivalent_definition() {
        let store = merge_store();
        let (from, to) = ("123@lid", "456@s.whatsapp.net");
        let conn = store.conn.lock().unwrap();
        merge_poll(&conn, from, "123:7@lid", "Same", &["A", "B"], &[1, 2], 4000);
        merge_poll(&conn, to, "456@s.whatsapp.net", "Same", &["A", "B"], &[8, 9], 3000);
        merge(&conn, from, to).unwrap();
        let json: String = conn
            .query_row(
                "SELECT options FROM poll_option_hashes WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<PollOption>>(&json).unwrap(),
            vec![option("A", 8), option("B", 9)]
        );
        let revision: (i64, String) = conn
            .query_row(
                "SELECT timestamp_ms, message_id FROM secret_edit_revisions WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(revision, (4000, "edit-4000".into()));
    }

    #[test]
    fn alias_merge_moves_sole_source_metadata_and_drops_orphans() {
        let store = merge_store();
        let (from, to) = ("123@lid", "456@s.whatsapp.net");
        let conn = store.conn.lock().unwrap();
        merge_poll(&conn, from, "123@lid", "Only", &["A", "B"], &[1, 2], 3000);
        conn.execute(
            "INSERT INTO poll_option_hashes (chat, id, options) VALUES (?1, 'orphan', '[]')",
            [from],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO secret_edit_revisions (chat, id, timestamp_ms, message_id) VALUES (?1, 'orphan', 5000, 'orphan-edit')",
            [from],
        )
        .unwrap();
        merge(&conn, from, to).unwrap();
        let json: String = conn
            .query_row(
                "SELECT options FROM poll_option_hashes WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<PollOption>>(&json).unwrap(),
            vec![option("A", 1), option("B", 2)]
        );
        assert_eq!(
            conn.query_row(
                "SELECT timestamp_ms FROM secret_edit_revisions WHERE chat = ?1 AND id = 'item'",
                [to],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3000
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM poll_option_hashes WHERE id = 'orphan'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM secret_edit_revisions WHERE id = 'orphan'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
