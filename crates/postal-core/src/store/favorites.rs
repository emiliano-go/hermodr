use std::{path::Path, sync::Mutex};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};

pub(crate) struct FavoriteDb {
    conn: Mutex<(Connection, u64)>,
}

pub(crate) type FavoriteWorker = super::worker::Worker<FavoriteDb>;

impl FavoriteWorker {
    pub(crate) async fn open_with_key(path: &Path, key: Option<crate::database_crypto::DatabaseKey>) -> Result<Self> {
        let path = path.to_owned();
        Ok(Self::new(tokio::task::spawn_blocking(move || FavoriteDb::open_with_key(&path, key.as_ref())).await??))
    }
}

impl FavoriteDb {
    #[cfg(test)]
    fn open(path: &Path) -> Result<Self> {
        Self::open_with_key(path, None)
    }

    fn open_with_key(path: &Path, key: Option<&crate::database_crypto::DatabaseKey>) -> Result<Self> {
        if path != Path::new(":memory:") {
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = crate::database_crypto::open_database(path, key, rusqlite::OpenFlags::default()).context("open favorite chats")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS favorite_chats (
            rank INTEGER PRIMARY KEY, jid TEXT NOT NULL UNIQUE CHECK(length(jid) > 0));")?;
        Ok(Self { conn: Mutex::new((conn, 0)) })
    }

    pub(crate) fn list(&self) -> Result<Vec<String>> {
        let state = self.conn.lock().unwrap();
        let mut stmt = state.0.prepare("SELECT jid FROM favorite_chats ORDER BY rank")?;
        let rows = stmt.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub(crate) fn replace(&self, revision: u64, chats: &[String]) -> Result<bool> {
        let mut state = self.conn.lock().unwrap();
        if revision <= state.1 { return Ok(false); }
        let tx = state.0.transaction()?;
        tx.execute("DELETE FROM favorite_chats", [])?;
        for (rank, chat) in chats.iter().enumerate() {
            tx.execute("INSERT INTO favorite_chats (rank, jid) VALUES (?1, ?2)", params![i64::try_from(rank)?, chat])?;
        }
        tx.commit()?;
        state.1 = revision;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_replacements_replay_clear_and_rollback_atomically() {
        let db = FavoriteDb::open(Path::new(":memory:")).unwrap();
        let first = vec!["200@s.whatsapp.net".into(), "100@g.us".into()];
        assert!(db.replace(2, &first).unwrap());
        assert!(!db.replace(1, &["old@g.us".into()]).unwrap());
        assert_eq!(db.list().unwrap(), first);
        assert!(db.replace(3, &["duplicate@g.us".into(), "duplicate@g.us".into()]).is_err());
        assert_eq!(db.list().unwrap(), first);
        assert!(db.replace(3, &[]).unwrap());
        assert!(db.list().unwrap().is_empty());
    }

    #[test]
    fn favorites_survive_restart_without_message_history_or_pins() {
        let root = std::env::temp_dir().join(format!("postal-favorites-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = root.join("favorites.db");
        let expected = vec!["200@s.whatsapp.net".into(), "100@g.us".into()];
        {
            let history = crate::store::MessageStore::open(Path::new(":memory:")).unwrap();
            history.set_pinned("300@g.us", true).unwrap();
            let favorites = FavoriteDb::open(&path).unwrap();
            favorites.replace(1, &expected).unwrap();
            assert_eq!(history.pinned_chats().unwrap(), vec!["300@g.us"]);
        }
        {
            let history = crate::store::MessageStore::open(Path::new(":memory:")).unwrap();
            assert!(history.pinned_chats().unwrap().is_empty());
            let favorites = FavoriteDb::open(&path).unwrap();
            assert_eq!(favorites.list().unwrap(), expected);
            favorites.replace(1, &["100@g.us".into()]).unwrap();
        }
        assert_eq!(FavoriteDb::open(&path).unwrap().list().unwrap(), vec!["100@g.us"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
