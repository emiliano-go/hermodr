use super::*;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MediaAutoDownload {
    pub image: bool,
    pub video: bool,
    pub audio: bool,
    pub document: bool,
    pub sticker: bool,
    pub gif: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MediaAutoDownloadOverrides {
    pub image: Option<bool>,
    pub video: Option<bool>,
    pub audio: Option<bool>,
    pub document: Option<bool>,
    pub sticker: Option<bool>,
    pub gif: Option<bool>,
}

impl MediaAutoDownload {
    pub fn all(enabled: bool) -> Self {
        Self {
            image: enabled,
            video: enabled,
            audio: enabled,
            document: enabled,
            sticker: enabled,
            gif: enabled,
        }
    }

    pub fn enabled(self, kind: &str) -> bool {
        self.effective(kind, MediaAutoDownloadOverrides::default())
    }

    pub fn effective(self, kind: &str, overrides: MediaAutoDownloadOverrides) -> bool {
        match kind {
            "image" => overrides.image.unwrap_or(self.image),
            "video" | "round_video" => overrides.video.unwrap_or(self.video),
            "audio" => overrides.audio.unwrap_or(self.audio),
            "document" => overrides.document.unwrap_or(self.document),
            "sticker" => overrides.sticker.unwrap_or(self.sticker),
            "gif" => overrides.gif.unwrap_or(self.gif),
            _ => false,
        }
    }
}

impl MediaAutoDownloadOverrides {
    pub fn all(enabled: Option<bool>) -> Self {
        Self {
            image: enabled,
            video: enabled,
            audio: enabled,
            document: enabled,
            sticker: enabled,
            gif: enabled,
        }
    }
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='chat_media_policy')",
        [], |row| row.get(0),
    )?;
    if exists {
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TABLE chat_media_policy (
            jid TEXT PRIMARY KEY,
            image INTEGER CHECK(image IN (0,1)), video INTEGER CHECK(video IN (0,1)),
            audio INTEGER CHECK(audio IN (0,1)), document INTEGER CHECK(document IN (0,1)),
            sticker INTEGER CHECK(sticker IN (0,1)), gif INTEGER CHECK(gif IN (0,1)));
         INSERT INTO chat_media_policy(jid,image,video,audio,document,sticker,gif)
         SELECT jid,auto_download!=0,auto_download!=0,auto_download!=0,
             auto_download!=0,auto_download!=0,auto_download!=0 FROM chat_settings;",
    )?;
    Ok(())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='chat_media_policy')",
        [], |row| row.get(0),
    )?;
    if !exists || from == to {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO chat_media_policy(jid,image,video,audio,document,sticker,gif)
         SELECT ?1,image,video,audio,document,sticker,gif FROM chat_media_policy WHERE jid=?2
         ON CONFLICT(jid) DO UPDATE SET
             image=COALESCE(chat_media_policy.image,excluded.image),
             video=COALESCE(chat_media_policy.video,excluded.video),
             audio=COALESCE(chat_media_policy.audio,excluded.audio),
             document=COALESCE(chat_media_policy.document,excluded.document),
             sticker=COALESCE(chat_media_policy.sticker,excluded.sticker),
             gif=COALESCE(chat_media_policy.gif,excluded.gif)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chat_media_policy WHERE jid=?1", [from])?;
    Ok(())
}

pub(super) fn write_overrides(
    conn: &Connection,
    chat: &str,
    overrides: MediaAutoDownloadOverrides,
) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_media_policy(jid,image,video,audio,document,sticker,gif) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(jid) DO UPDATE SET image=excluded.image,video=excluded.video,audio=excluded.audio,
             document=excluded.document,sticker=excluded.sticker,gif=excluded.gif",
        params![chat, overrides.image, overrides.video, overrides.audio, overrides.document, overrides.sticker, overrides.gif],
    )?;
    Ok(())
}

impl MessageStore {
    pub fn chat_media_auto_download(&self, chat: &str) -> Result<MediaAutoDownloadOverrides> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT image,video,audio,document,sticker,gif FROM chat_media_policy WHERE jid=?1",
                [chat.as_ref()],
                |row| {
                    Ok(MediaAutoDownloadOverrides {
                        image: row.get(0)?,
                        video: row.get(1)?,
                        audio: row.get(2)?,
                        document: row.get(3)?,
                        sticker: row.get(4)?,
                        gif: row.get(5)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_chat_media_auto_download(
        &self,
        chat: &str,
        overrides: MediaAutoDownloadOverrides,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        write_overrides(&conn, &chat, overrides)
    }
}

impl StoreWorker {
    pub(crate) async fn chat_media_auto_download(
        &self,
        chat: &str,
    ) -> Result<MediaAutoDownloadOverrides> {
        let chat = chat.to_owned();
        self.run(move |store| store.chat_media_auto_download(&chat))
            .await
    }

    pub(crate) async fn set_chat_media_auto_download(
        &self,
        chat: &str,
        overrides: MediaAutoDownloadOverrides,
    ) -> Result<()> {
        let chat = chat.to_owned();
        self.run(move |store| store.set_chat_media_auto_download(&chat, overrides))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MessageStore {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE chat_settings(jid TEXT PRIMARY KEY,auto_download INTEGER NOT NULL DEFAULT 1);
            CREATE TABLE lid_pn(lid TEXT,pn TEXT);
            INSERT INTO chat_settings VALUES ('100@s.whatsapp.net',1),('200@s.whatsapp.net',0);
            INSERT INTO lid_pn VALUES ('300','100');").unwrap();
        migrate(&conn).unwrap();
        MessageStore {
            conn: Mutex::new(conn),
        }
    }

    #[test]
    fn legacy_migration_keeps_type_overrides_and_inheritance_on_reopen() {
        let store = fixture();
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::all(Some(true))
        );
        assert_eq!(
            store
                .chat_media_auto_download("200@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::all(Some(false))
        );
        assert_eq!(
            store.chat_media_auto_download("absent").unwrap(),
            MediaAutoDownloadOverrides::default()
        );
        let mixed = MediaAutoDownloadOverrides {
            audio: Some(true),
            image: Some(false),
            ..Default::default()
        };
        store
            .set_chat_media_auto_download("300:7@lid", mixed)
            .unwrap();
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            mixed
        );
        store
            .set_chat_media_auto_download(
                "200@s.whatsapp.net",
                MediaAutoDownloadOverrides::default(),
            )
            .unwrap();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_settings(jid) VALUES ('new-settings-only')",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        drop(conn);
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            mixed
        );
        assert_eq!(
            store
                .chat_media_auto_download("200@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::default()
        );
        assert_eq!(
            store.chat_media_auto_download("new-settings-only").unwrap(),
            MediaAutoDownloadOverrides::default()
        );
        assert_eq!(
            store.chat_auto_download("200@s.whatsapp.net").unwrap(),
            Some(false)
        );
    }

    #[test]
    fn effective_policy_uses_chat_then_global_and_safe_missing_kind_defaults() {
        let global = MediaAutoDownload {
            image: true,
            video: true,
            audio: false,
            ..Default::default()
        };
        let overrides = MediaAutoDownloadOverrides {
            image: Some(false),
            audio: Some(true),
            ..Default::default()
        };
        for (kind, enabled) in [
            ("image", false),
            ("video", true),
            ("round_video", true),
            ("audio", true),
            ("document", false),
            ("sticker", false),
            ("gif", false),
            ("unknown", false),
            ("poll", false),
            ("view_once", false),
        ] {
            assert_eq!(global.effective(kind, overrides), enabled, "{kind}");
        }
        let partial: MediaAutoDownload = serde_json::from_str(r#"{"audio":true}"#).unwrap();
        assert!(partial.enabled("audio"));
        assert!(!partial.enabled("image"));
        assert!(!MediaAutoDownload::all(true).enabled("future_kind"));
    }

    #[test]
    fn address_merge_keeps_source_types_and_canonical_conflicts() {
        merge(&Connection::open_in_memory().unwrap(), "old", "new").unwrap();
        let store = fixture();
        let conn = store.conn.lock().unwrap();
        write_overrides(
            &conn,
            "300@lid",
            MediaAutoDownloadOverrides {
                image: Some(true),
                audio: Some(true),
                gif: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        write_overrides(
            &conn,
            "100@s.whatsapp.net",
            MediaAutoDownloadOverrides {
                image: Some(false),
                video: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        merge(&conn, "300@lid", "100@s.whatsapp.net").unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM chat_media_policy WHERE jid='300@lid'",
                [],
                |row| row.get::<_, i32>(0)
            )
            .unwrap(),
            0
        );
        drop(conn);
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides {
                image: Some(false),
                video: Some(true),
                audio: Some(true),
                gif: Some(false),
                ..Default::default()
            }
        );
    }

    #[test]
    fn legacy_setter_updates_all_types_atomically_and_keeps_other_chats() {
        let store = fixture();
        let neighbor = MediaAutoDownloadOverrides {
            audio: Some(true),
            image: Some(false),
            ..Default::default()
        };
        store
            .set_chat_media_auto_download("neighbor", neighbor)
            .unwrap();
        store
            .set_chat_auto_download("100@s.whatsapp.net", false)
            .unwrap();
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::all(Some(false))
        );
        store
            .set_chat_auto_download("100@s.whatsapp.net", true)
            .unwrap();
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::all(Some(true))
        );
        store
            .set_chat_auto_download("100@s.whatsapp.net", false)
            .unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_media_policy BEFORE UPDATE ON chat_media_policy
             BEGIN SELECT RAISE(ABORT, 'synthetic policy write failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .set_chat_auto_download("100@s.whatsapp.net", true)
                .is_err()
        );
        assert_eq!(
            store.chat_auto_download("100@s.whatsapp.net").unwrap(),
            Some(false)
        );
        assert_eq!(
            store
                .chat_media_auto_download("100@s.whatsapp.net")
                .unwrap(),
            MediaAutoDownloadOverrides::all(Some(false))
        );
        assert_eq!(
            store.chat_media_auto_download("neighbor").unwrap(),
            neighbor
        );
    }
}
