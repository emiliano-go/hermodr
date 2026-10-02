use super::*;

#[derive(Clone)]
pub(crate) struct QuizDefinition {
    pub creator: String,
    pub secret: Option<Vec<u8>>,
    pub name: String,
    pub options: Vec<String>,
    pub correct_hash: Option<Vec<u8>>,
    pub answer_valid: bool,
}

#[derive(Clone)]
pub(crate) struct QuizCipher {
    pub update_id: String,
    pub voter: String,
    pub alt: Option<String>,
    pub from_me: bool,
    pub source_time: Option<i64>,
    pub payload: Vec<u8>,
    pub iv: Vec<u8>,
}

pub(crate) struct QuizCipherSnapshot {
    pub records: Vec<QuizCipher>,
    pub suppressed: bool,
}

const PRIVATE_SOURCE_SQL: &str = "s.revoked<>0 OR s.spoiler<>0 OR s.media_once_kind IS NOT NULL
    OR s.media_kind='view_once' OR (s.system_kind IS NOT NULL AND s.system_kind<>'UNAVAILABLE_MESSAGE')
    OR ((s.deleted<>0 OR s.system_kind='UNAVAILABLE_MESSAGE') AND NOT EXISTS(
        SELECT 1 FROM quiz_source_retirements r WHERE r.chat=s.chat AND r.poll=c.poll AND r.update_id=s.id))";

pub(super) fn migrate_source_retirements(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS quiz_source_retirements (
        chat TEXT NOT NULL, poll TEXT NOT NULL, update_id TEXT NOT NULL,
        PRIMARY KEY(chat,poll,update_id));",
    )?;
    Ok(())
}

pub(super) fn revoke_source(conn: &Connection, chat: &str, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM quiz_source_retirements WHERE chat=?1 AND update_id=?2",
        params![chat, id],
    )?;
    conn.execute(
        "UPDATE quiz_vote_ciphers SET enc_payload=X'',enc_iv=X'' WHERE chat=?1 AND update_id=?2",
        params![chat, id],
    )?;
    Ok(())
}

pub(super) fn redact_private_sources(conn: &Connection) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE quiz_vote_ciphers AS c SET enc_payload=X'',enc_iv=X''
            WHERE EXISTS(SELECT 1 FROM messages s WHERE s.chat=c.chat AND s.id=c.update_id
            AND ({PRIVATE_SOURCE_SQL}))"
        ),
        [],
    )?;
    Ok(())
}

fn private_source(conn: &Connection, chat: &str, poll: &str, update: &str) -> Result<bool> {
    Ok(conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM messages s JOIN (SELECT ?3 AS poll) c WHERE s.chat=?1 AND s.id=?2
        AND ({PRIVATE_SOURCE_SQL}))"
        ),
        params![chat, update, poll],
        |r| r.get(0),
    )?)
}

fn authorize_source(
    conn: &Connection,
    chat: &str,
    poll: &str,
    update: &str,
) -> Result<Option<bool>> {
    let recoverable: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages
        WHERE chat=?1 AND id=?2 AND system_kind='UNAVAILABLE_MESSAGE' AND deleted=0 AND revoked=0
        AND spoiler=0 AND media_once_kind IS NULL AND COALESCE(media_kind,'')<>'view_once')",
        params![chat, update],
        |r| r.get(0),
    )?;
    if recoverable {
        return Ok(Some(
            conn.execute(
                "INSERT OR IGNORE INTO quiz_source_retirements VALUES(?1,?2,?3)",
                params![chat, poll, update],
            )? > 0,
        ));
    }
    Ok((!private_source(conn, chat, poll, update)?).then_some(false))
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS quiz_polls (
        chat TEXT NOT NULL, id TEXT NOT NULL, original_name TEXT NOT NULL,
        original_options TEXT NOT NULL, correct_hash BLOB, answer_valid INTEGER NOT NULL,
        PRIMARY KEY(chat,id));
        CREATE TABLE IF NOT EXISTS quiz_vote_ciphers (
        chat TEXT NOT NULL, poll TEXT NOT NULL, update_id TEXT NOT NULL,
        voter TEXT NOT NULL, alt TEXT, from_me INTEGER NOT NULL,
        source_time INTEGER, enc_payload BLOB NOT NULL, enc_iv BLOB NOT NULL,
        captured_at INTEGER NOT NULL, PRIMARY KEY(chat,poll,update_id));
        CREATE INDEX IF NOT EXISTS quiz_cipher_order ON quiz_vote_ciphers(chat,poll,source_time,update_id);")?;
    Ok(())
}

fn public_parent(conn: &Connection, chat: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages m WHERE chat=?1 AND id=?2
        AND deleted=0 AND revoked=0 AND spoiler=0 AND media_once_kind IS NULL
        AND media_kind='poll' AND system_kind IS NULL
        AND NOT EXISTS(SELECT 1 FROM hidden_chats WHERE jid=m.chat))",
        params![chat, id],
        |r| r.get(0),
    )?)
}

impl MessageStore {
    pub(crate) fn save_quiz_definition(
        &self,
        chat: &str,
        id: &str,
        creator: &str,
        name: &str,
        options: &[String],
        correct_hash: Option<&[u8]>,
        answer_valid: bool,
        secret: Option<&[u8]>,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?;
        if !public_parent(&tx, &chat, id)? {
            return Ok(false);
        }
        let json = serde_json::to_string(options)?;
        let filled = tx.execute("INSERT INTO polls(chat,id,creator,name,options,multi,secret) VALUES(?1,?2,?3,?4,?5,0,?6)
            ON CONFLICT(chat,id) DO UPDATE SET secret=excluded.secret WHERE polls.secret IS NULL AND excluded.secret IS NOT NULL",
            params![chat.as_ref(),id,creator,name,json,secret])? > 0;
        let changed = tx.execute("INSERT INTO quiz_polls VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(chat,id) DO UPDATE SET answer_valid=CASE
              WHEN original_name<>excluded.original_name OR original_options<>excluded.original_options
                OR correct_hash IS NOT excluded.correct_hash THEN 0 ELSE answer_valid END
            WHERE answer_valid<>0 AND (original_name<>excluded.original_name OR original_options<>excluded.original_options
                OR correct_hash IS NOT excluded.correct_hash)",
            params![chat.as_ref(),id,name,json,correct_hash,answer_valid])? > 0;
        tx.commit()?;
        Ok(changed || filled)
    }

    pub(crate) fn is_quiz(&self, chat: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM quiz_polls WHERE chat=?1 AND id=?2)",
            params![chat.as_ref(), id],
            |r| r.get(0),
        )?)
    }

    pub(crate) fn quiz_definition(&self, chat: &str, id: &str) -> Result<Option<QuizDefinition>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        if !public_parent(&conn, &chat, id)? {
            return Ok(None);
        }
        let row = conn.query_row("SELECT p.creator,p.secret,q.original_name,q.original_options,q.correct_hash,q.answer_valid
            FROM quiz_polls q JOIN polls p ON p.chat=q.chat AND p.id=q.id WHERE q.chat=?1 AND q.id=?2",
            params![chat.as_ref(),id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<Vec<u8>>>(1)?,
                r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<Vec<u8>>>(4)?,r.get::<_,bool>(5)?))).optional()?;
        row.map(
            |(creator, secret, name, options, correct_hash, answer_valid)| {
                Ok(QuizDefinition {
                    creator,
                    secret,
                    name,
                    options: serde_json::from_str(&options)?,
                    correct_hash,
                    answer_valid,
                })
            },
        )
        .transpose()
    }

    pub(crate) fn capture_quiz_cipher(
        &self,
        chat: &str,
        poll: &str,
        cipher: QuizCipher,
    ) -> Result<bool> {
        anyhow::ensure!(
            !poll.is_empty()
                && poll.len() <= 256
                && !cipher.update_id.is_empty()
                && cipher.update_id.len() <= 256
                && cipher.voter.len() <= 256
                && cipher.alt.as_ref().is_none_or(|a| a.len() <= 256)
                && cipher.iv.len() <= 64
                && cipher.payload.len() <= 4096,
            "Invalid quiz vote ciphertext."
        );
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE chat=?1 AND id=?2)",
            params![chat.as_ref(), poll],
            |r| r.get(0),
        )?;
        if exists && !public_parent(&tx, &chat, poll)? {
            return Ok(false);
        }
        if !exists {
            let suppressed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM hidden_chats WHERE jid=?1 UNION ALL
                SELECT 1 FROM cleared_chats WHERE jid=?1)",
                [chat.as_ref()],
                |r| r.get(0),
            )?;
            let floor: Option<i64> = tx
                .query_row(
                    "SELECT timestamp FROM chat_history_floor WHERE jid=?1",
                    [chat.as_ref()],
                    |r| r.get(0),
                )
                .optional()?;
            if suppressed || floor.is_some_and(|f| cipher.source_time.is_none_or(|t| t / 1000 < f))
            {
                return Ok(false);
            }
        }
        let Some(granted) = authorize_source(&tx, &chat, poll, &cipher.update_id)? else {
            return Ok(false);
        };
        let now = whatsapp_rust::wacore::time::now_millis();
        let changed = tx.execute(
            "INSERT OR IGNORE INTO quiz_vote_ciphers VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                chat.as_ref(),
                poll,
                cipher.update_id,
                cipher.voter,
                cipher.alt,
                cipher.from_me,
                cipher.source_time,
                cipher.payload,
                cipher.iv,
                now
            ],
        )? > 0;
        trim_ciphers(&tx, &chat, poll, &cipher, now)?;
        tx.commit()?;
        let retained: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM quiz_vote_ciphers WHERE chat=?1 AND poll=?2 AND update_id=?3)",
            params![chat.as_ref(),poll,cipher.update_id], |r|r.get(0))?;
        Ok((changed || granted) && retained)
    }

    #[cfg(test)]
    pub(crate) fn quiz_ciphers(&self, chat: &str, poll: &str) -> Result<Vec<QuizCipher>> {
        Ok(self.quiz_cipher_snapshot(chat, poll)?.records)
    }

    pub(crate) fn quiz_cipher_snapshot(
        &self,
        chat: &str,
        poll: &str,
    ) -> Result<QuizCipherSnapshot> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        if !public_parent(&conn, &chat, poll)? {
            return Ok(QuizCipherSnapshot {
                records: Vec::new(),
                suppressed: false,
            });
        }
        let suppressed: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM quiz_vote_ciphers c
            JOIN messages s ON s.chat=c.chat AND s.id=c.update_id WHERE c.chat=?1 AND c.poll=?2
            AND ({PRIVATE_SOURCE_SQL}))"
            ),
            params![chat.as_ref(), poll],
            |r| r.get(0),
        )?;
        let private = format!("EXISTS(SELECT 1 FROM messages s WHERE s.chat=c.chat AND s.id=c.update_id AND ({PRIVATE_SOURCE_SQL}))");
        let mut stmt = conn.prepare(&format!(
            "SELECT update_id,voter,alt,from_me,source_time,
            CASE WHEN {private} THEN X'' ELSE enc_payload END,
            CASE WHEN {private} THEN X'' ELSE enc_iv END FROM quiz_vote_ciphers c
            WHERE chat=?1 AND poll=?2 ORDER BY COALESCE(source_time,0),update_id"
        ))?;
        let rows = stmt
            .query_map(params![chat.as_ref(), poll], |r| {
                Ok(QuizCipher {
                    update_id: r.get(0)?,
                    voter: r.get(1)?,
                    alt: r.get(2)?,
                    from_me: r.get(3)?,
                    source_time: r.get(4)?,
                    payload: r.get(5)?,
                    iv: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(QuizCipherSnapshot {
            records: rows,
            suppressed,
        })
    }
}

fn trim_ciphers(
    conn: &Connection,
    chat: &str,
    poll: &str,
    cipher: &QuizCipher,
    now: i64,
) -> Result<()> {
    conn.execute(
        "DELETE FROM quiz_vote_ciphers WHERE chat=?1 AND poll=?2
        AND (voter=?3 OR voter=?4 OR alt=?3 OR alt=?4 OR (?5 AND from_me=1)) AND update_id<>(
          SELECT update_id FROM quiz_vote_ciphers WHERE chat=?1 AND poll=?2
            AND (voter=?3 OR voter=?4 OR alt=?3 OR alt=?4 OR (?5 AND from_me=1))
          ORDER BY COALESCE(source_time,0) DESC,update_id DESC LIMIT 1)",
        params![chat, poll, cipher.voter, cipher.alt, cipher.from_me],
    )?;
    conn.execute("DELETE FROM quiz_vote_ciphers WHERE chat=?1
        AND NOT EXISTS(SELECT 1 FROM quiz_polls q WHERE q.chat=quiz_vote_ciphers.chat AND q.id=quiz_vote_ciphers.poll)
        AND (captured_at<?2 OR update_id IN (SELECT c.update_id FROM quiz_vote_ciphers c WHERE c.chat=?1
          AND NOT EXISTS(SELECT 1 FROM quiz_polls q WHERE q.chat=c.chat AND q.id=c.poll)
          ORDER BY captured_at DESC,update_id DESC LIMIT -1 OFFSET 128))",params![chat,now-86_400_000])?;
    conn.execute(
        "DELETE FROM quiz_source_retirements WHERE NOT EXISTS(SELECT 1 FROM quiz_vote_ciphers c
        WHERE c.chat=quiz_source_retirements.chat AND c.poll=quiz_source_retirements.poll
        AND c.update_id=quiz_source_retirements.update_id)",
        [],
    )?;
    Ok(())
}

impl StoreWorker {
    pub(crate) async fn quiz_cipher_snapshot(
        &self,
        chat: &str,
        id: &str,
    ) -> Result<QuizCipherSnapshot> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |s| s.quiz_cipher_snapshot(&chat, &id)).await
    }
    pub(crate) async fn quiz_definition(
        &self,
        chat: &str,
        id: &str,
    ) -> Result<Option<QuizDefinition>> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |s| s.quiz_definition(&chat, &id)).await
    }
    pub(crate) async fn is_quiz(&self, chat: &str, id: &str) -> Result<bool> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |s| s.is_quiz(&chat, &id)).await
    }
    pub(crate) async fn capture_quiz_cipher(
        &self,
        chat: &str,
        id: &str,
        cipher: QuizCipher,
    ) -> Result<bool> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |s| s.capture_quiz_cipher(&chat, &id, cipher))
            .await
    }
}

#[cfg(test)]
#[path = "quiz_polls_tests.rs"]
mod tests;
