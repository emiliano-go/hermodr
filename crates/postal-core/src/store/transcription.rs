use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StoredTranscript {
    pub chat: String,
    pub id: String,
    pub text: String,
    pub language: Option<String>,
    pub provider: String,
    pub created_at: i64,
}

pub(crate) struct TranscriptionMedia {
    pub chat: String,
    pub path: Option<String>,
    pub duration_ms: u64,
    pub spoiler: bool,
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let exists = conn
        .prepare("PRAGMA table_info(chat_settings)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == "auto_transcribe");
    if !exists {
        conn.execute_batch("ALTER TABLE chat_settings ADD COLUMN auto_transcribe INTEGER CHECK(auto_transcribe IN (0,1));")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS transcripts (
          chat TEXT NOT NULL, id TEXT NOT NULL, text TEXT NOT NULL,
          language TEXT, provider TEXT NOT NULL, created_at INTEGER NOT NULL,
          PRIMARY KEY(chat,id));
         CREATE TRIGGER IF NOT EXISTS transcripts_message_delete AFTER DELETE ON messages
         BEGIN DELETE FROM transcripts WHERE chat=OLD.chat AND id=OLD.id; END;
         CREATE TRIGGER IF NOT EXISTS transcripts_view_once_insert AFTER INSERT ON view_once
         BEGIN DELETE FROM transcripts WHERE chat=NEW.chat AND id=NEW.id; END;
         CREATE TRIGGER IF NOT EXISTS transcripts_message_revoke AFTER UPDATE OF revoked,deleted ON messages
         WHEN NEW.revoked != 0 OR NEW.deleted != 0
         BEGIN DELETE FROM transcripts WHERE chat=NEW.chat AND id=NEW.id; END;
         CREATE TRIGGER IF NOT EXISTS transcripts_message_move AFTER UPDATE OF chat,id ON messages
         WHEN OLD.chat != NEW.chat OR OLD.id != NEW.id
         BEGIN UPDATE OR IGNORE transcripts SET chat=NEW.chat,id=NEW.id WHERE chat=OLD.chat AND id=OLD.id;
           DELETE FROM transcripts WHERE chat=OLD.chat AND id=OLD.id; END;"
    )?;
    Ok(())
}

impl MessageStore {
    pub fn message_transcript(&self, chat: &str, id: &str) -> Result<Option<StoredTranscript>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn.query_row("SELECT chat,id,text,language,provider,created_at FROM transcripts WHERE chat=?1 AND id=?2",
            params![chat.as_ref(),id], |r| Ok(StoredTranscript { chat:r.get(0)?, id:r.get(1)?, text:r.get(2)?, language:r.get(3)?, provider:r.get(4)?, created_at:r.get(5)? })).optional()?)
    }

    pub fn save_transcript(&self, transcript: &StoredTranscript) -> Result<()> {
        anyhow::ensure!(
            transcript.text.len() <= 128 * 1024
                && transcript.language.as_ref().is_none_or(|s| s.len() <= 64)
                && !transcript.provider.is_empty()
                && transcript.provider.len() <= 100,
            "invalid transcript"
        );
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, &transcript.chat)?;
        let count = conn.execute("INSERT INTO transcripts(chat,id,text,language,provider,created_at)
            SELECT ?1,?2,?3,?4,?5,?6 WHERE EXISTS (SELECT 1 FROM messages m WHERE chat=?1 AND id=?2 AND revoked=0 AND deleted=0 AND media_kind='audio'
              AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id))
            ON CONFLICT(chat,id) DO UPDATE SET text=excluded.text,language=excluded.language,provider=excluded.provider,created_at=excluded.created_at",
            params![chat.as_ref(),transcript.id,transcript.text,transcript.language,transcript.provider,transcript.created_at])?;
        anyhow::ensure!(count == 1, "voice note no longer exists");
        Ok(())
    }

    pub fn chat_auto_transcribe(&self, chat: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT auto_transcribe FROM chat_settings WHERE jid=?1",
                [chat.as_ref()],
                |r| r.get::<_, Option<bool>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn set_chat_auto_transcribe(&self, chat: &str, enabled: Option<bool>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO chat_settings(jid,auto_download,auto_transcribe) VALUES (?1,1,?2)
            ON CONFLICT(jid) DO UPDATE SET auto_transcribe=excluded.auto_transcribe",
            params![chat.as_ref(), enabled],
        )?;
        Ok(())
    }

    pub(crate) fn transcription_media(
        &self,
        chat: &str,
        id: &str,
    ) -> Result<Option<TranscriptionMedia>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn.query_row("SELECT chat,media_path,media_duration,spoiler FROM messages m WHERE chat=?1 AND id=?2 AND revoked=0 AND deleted=0 AND media_kind='audio'
             AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat=m.chat AND v.id=m.id)",
            params![chat.as_ref(),id], |r|Ok(TranscriptionMedia {chat:r.get(0)?,path:r.get(1)?,duration_ms:r.get::<_,Option<u32>>(2)?.unwrap_or(0) as u64*1000,spoiler:r.get(3)?})).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> MessageStore {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE messages(chat TEXT,id TEXT,revoked INTEGER DEFAULT 0,deleted INTEGER DEFAULT 0,media_kind TEXT DEFAULT 'audio',media_path TEXT,media_duration INTEGER,spoiler INTEGER DEFAULT 0,PRIMARY KEY(chat,id));
            CREATE TABLE chat_settings(jid TEXT PRIMARY KEY,auto_download INTEGER NOT NULL); CREATE TABLE lid_pn(lid TEXT,pn TEXT);
            CREATE TABLE view_once(chat TEXT,id TEXT,opened INTEGER DEFAULT 0,PRIMARY KEY(chat,id));
            INSERT INTO messages(chat,id) VALUES ('1@s.whatsapp.net','one'),('1@s.whatsapp.net','two');").unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        MessageStore {
            conn: Mutex::new(conn),
            pending_rsvp_limit: 512,
        }
    }
    fn transcript(id: &str) -> StoredTranscript {
        StoredTranscript {
            chat: "1@s.whatsapp.net".into(),
            id: id.into(),
            text: "synthetic".into(),
            language: Some("en".into()),
            provider: "local-whisper".into(),
            created_at: 1,
        }
    }
    #[test]
    fn cache_retranscription_retention_revoke_and_missing_message() {
        let s = fixture();
        s.save_transcript(&transcript("one")).unwrap();
        let mut updated = transcript("one");
        updated.text = "replacement".into();
        s.save_transcript(&updated).unwrap();
        assert_eq!(
            s.message_transcript(&updated.chat, "one")
                .unwrap()
                .unwrap()
                .text,
            "replacement"
        );
        assert!(s.save_transcript(&transcript("absent")).is_err());
        s.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM messages WHERE id='one'", [])
            .unwrap();
        assert!(s
            .message_transcript(&updated.chat, "one")
            .unwrap()
            .is_none());
        s.save_transcript(&transcript("two")).unwrap();
        s.conn
            .lock()
            .unwrap()
            .execute("UPDATE messages SET revoked=1 WHERE id='two'", [])
            .unwrap();
        assert!(s
            .message_transcript(&updated.chat, "two")
            .unwrap()
            .is_none());
        assert!(s.save_transcript(&transcript("two")).is_err());
    }
    #[test]
    fn chat_override_preserves_download_policy_and_aliases() {
        let s = fixture();
        let chat = "1@s.whatsapp.net";
        assert_eq!(s.chat_auto_transcribe(chat).unwrap(), None);
        s.set_chat_auto_transcribe(chat, Some(false)).unwrap();
        assert_eq!(s.chat_auto_transcribe(chat).unwrap(), Some(false));
        s.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE chat_settings SET auto_download=0 WHERE jid=?1",
                [chat],
            )
            .unwrap();
        s.set_chat_auto_transcribe(chat, Some(true)).unwrap();
        assert_eq!(s.chat_auto_download(chat).unwrap(), Some(false));
        s.set_chat_auto_transcribe(chat, None).unwrap();
        assert_eq!(s.chat_auto_transcribe(chat).unwrap(), None);
        s.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO lid_pn(lid,pn) VALUES ('99','1')", [])
            .unwrap();
        s.set_chat_auto_transcribe("99@lid", Some(true)).unwrap();
        assert_eq!(s.chat_auto_transcribe(chat).unwrap(), Some(true));
        s.save_transcript(&transcript("one")).unwrap();
        assert!(s.message_transcript("99@lid", "one").unwrap().is_some());
    }

    #[test]
    fn view_once_notes_never_supply_audio_or_keep_a_transcript() {
        let s = fixture();
        s.save_transcript(&transcript("one")).unwrap();
        assert!(s
            .transcription_media("1@s.whatsapp.net", "one")
            .unwrap()
            .is_some());
        s.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO view_once(chat,id) VALUES ('1@s.whatsapp.net','one')",
                [],
            )
            .unwrap();
        assert!(s
            .transcription_media("1@s.whatsapp.net", "one")
            .unwrap()
            .is_none());
        assert!(s
            .message_transcript("1@s.whatsapp.net", "one")
            .unwrap()
            .is_none());
        assert!(s.save_transcript(&transcript("one")).is_err());
    }
}
