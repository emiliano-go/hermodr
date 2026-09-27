//! Display names and the LID to phone-number map.

use super::*;

pub(super) fn canonical_chat<'a>(conn: &Connection, jid: &'a str) -> Result<std::borrow::Cow<'a, str>> {
    let Some(user) = jid.strip_suffix("@lid") else { return Ok(jid.into()) };
    let lid = user.split(':').next().unwrap_or(user);
    let pn: Option<String> = conn.query_row("SELECT pn FROM lid_pn WHERE lid = ?1", [lid], |r| r.get(0)).optional()?;
    Ok(match pn {
        Some(pn) => format!("{pn}@s.whatsapp.net").into(),
        None => jid.into(),
    })
}

impl MessageStore {
    pub(crate) fn canonical_chat<'a>(&self, jid: &'a str) -> Result<std::borrow::Cow<'a, str>> {
        canonical_chat(&self.conn.lock().unwrap(), jid)
    }

    /// Records a display name for a JID, from a push name or group query.
    ///
    /// Empty names are ignored: a message with no push name should not erase a
    /// name learned earlier. A name from the address book is never overwritten
    /// by one the contact chose for themselves.
    pub fn set_name(&self, jid: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || jid.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        if is_placeholder_name(name) {
            conn.execute("INSERT OR IGNORE INTO names (jid, name, saved) VALUES (?1, ?2, 0)", params![jid, name])?;
            return Ok(());
        }
        conn.execute(
            &format!(
                "INSERT INTO names (jid, name, saved) VALUES (?1, ?2, 0)
                 ON CONFLICT(jid) DO UPDATE SET name = excluded.name, saved = 0
                 WHERE saved = 0 OR {PLACEHOLDER_SQL}"
            ),
            params![jid, name],
        )?;
        Ok(())
    }

    /// Records a name from the account's address book, which takes priority
    /// over any push name already stored.
    pub fn set_saved_name(&self, jid: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || jid.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO names (jid, name, saved) VALUES (?1, ?2, 1)
             ON CONFLICT(jid) DO UPDATE SET name = excluded.name, saved = 1",
            params![jid, name],
        )?;
        Ok(())
    }

    /// Drops the address-book flag when a contact is removed, so the name can
    /// later be replaced by a push name.
    pub fn clear_saved_name(&self, jid: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE names SET saved = 0 WHERE jid = ?1", params![jid])?;
        Ok(())
    }

    /// How many address-book names are stored.
    pub fn saved_name_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count = conn.query_row("SELECT COUNT(*) FROM names WHERE saved = 1", [], |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(count as usize)
    }

    /// Every JID that appears as a message sender or a chat.
    pub fn known_addresses(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT DISTINCT sender FROM messages UNION SELECT DISTINCT chat FROM messages")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Address-book names, as `(jid, name)` pairs.
    pub fn saved_names(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid, name FROM names WHERE saved = 1")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Whether the stored name for a JID came from the address book.
    pub fn name_is_saved(&self, jid: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT saved FROM names WHERE jid = ?1",
            params![jid],
            |r| r.get::<_, i32>(0),
        )
        .optional()?
        .is_some_and(|v| v != 0))
    }

    /// Names matching a query, for the search box.
    pub fn search_names(&self, query: &str, limit: u32) -> Result<Vec<(String, String, bool)>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query.to_lowercase());
        let mut stmt = conn.prepare(
            "SELECT jid, name, saved FROM names
             WHERE lower(name) LIKE ?1 OR lower(jid) LIKE ?1
             ORDER BY saved DESC, name LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)? != 0,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// The display name for a JID, if known.
    pub fn name_for(&self, jid: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let name = conn
            .query_row("SELECT name FROM names WHERE jid = ?1", params![jid], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(name)
    }

    /// Remembers that a LID user and a phone number are the same person.
    ///
    /// Kept here because the session's own mapping is lost when a device re-pairs.
    pub fn set_lid_pn(&self, lid: &str, pn: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let previous: Option<String> = conn.query_row("SELECT pn FROM lid_pn WHERE lid = ?1", [lid], |r| r.get(0)).optional()?;
        if previous.as_deref() == Some(pn) { return Ok(()); }
        let tx = conn.savepoint()?;
        tx.execute(
            "INSERT INTO lid_pn (lid, pn) VALUES (?1, ?2) ON CONFLICT(lid) DO UPDATE SET pn = excluded.pn",
            params![lid, pn],
        )?;
        chats::fold_chat(&tx, &format!("{lid}@lid"), &format!("{pn}@s.whatsapp.net"))?;
        tx.commit()?;
        Ok(())
    }

    /// `(lid, pn)` user parts for either form of a user part.
    pub fn lid_pn(&self, user: &str) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT lid, pn FROM lid_pn WHERE lid = ?1 OR pn = ?1 LIMIT 1",
                params![user],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}

impl StoreWorker {
    pub(crate) async fn set_name(&self, jid: &str, name: &str) -> Result<()> {
        let jid = jid.to_owned();
        let name = name.to_owned();
        self.run(move |store| store.set_name(&jid, &name)).await
    }

    pub(crate) async fn set_saved_name(&self, jid: &str, name: &str) -> Result<()> {
        let jid = jid.to_owned();
        let name = name.to_owned();
        self.run(move |store| store.set_saved_name(&jid, &name)).await
    }

    pub(crate) async fn clear_saved_name(&self, jid: &str) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.clear_saved_name(&jid)).await
    }

    pub(crate) async fn saved_name_count(&self) -> Result<usize> {
        self.run(move |store| store.saved_name_count()).await
    }

    pub(crate) async fn name_is_saved(&self, jid: &str) -> Result<bool> {
        let jid = jid.to_owned();
        self.run(move |store| store.name_is_saved(&jid)).await
    }

    pub(crate) async fn search_names(&self, query: &str, limit: u32) -> Result<Vec<(String, String, bool)>> {
        let query = query.to_owned();
        self.run(move |store| store.search_names(&query, limit)).await
    }

    pub(crate) async fn name_for(&self, jid: &str) -> Result<Option<String>> {
        let jid = jid.to_owned();
        self.run(move |store| store.name_for(&jid)).await
    }

    pub(crate) async fn set_lid_pn(&self, lid: &str, pn: &str) -> Result<()> {
        let lid = lid.to_owned();
        let pn = pn.to_owned();
        self.run(move |store| store.set_lid_pn(&lid, &pn)).await
    }

    pub(crate) async fn lid_pn(&self, user: &str) -> Result<Option<(String, String)>> {
        let user = user.to_owned();
        self.run(move |store| store.lid_pn(&user)).await
    }
}
