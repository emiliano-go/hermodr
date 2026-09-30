use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ContactIdentity {
    pub contact_saved: Option<bool>,
    pub saved_name: Option<String>,
    pub legacy_name: Option<String>,
    pub push_name: Option<String>,
    pub username: Option<String>,
    pub number: Option<String>,
    pub own: bool,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS contact_identity (
        jid TEXT PRIMARY KEY, push_name TEXT, username TEXT);")?;
    Ok(())
}

pub(super) fn migrate_baseline(conn: &Connection) -> Result<()> {
    for (column, kind) in [("contact_saved", "INTEGER"), ("saved_name", "TEXT"), ("contact_timestamp", "INTEGER")] {
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('contact_identity') WHERE name = ?1)", [column], |row| row.get(0))?;
        if !exists { conn.execute_batch(&format!("ALTER TABLE contact_identity ADD COLUMN {column} {kind};"))?; }
    }
    Ok(())
}

fn latest_contact_state(conn: &Connection, forms: &[String]) -> Result<Option<(bool, Option<String>, i64)>> {
    let mut latest: Option<(bool, Option<String>, i64)> = None;
    for form in forms {
        let state: Option<(bool, Option<String>, i64)> = conn.query_row(
            "SELECT contact_saved, saved_name, contact_timestamp FROM contact_identity
             WHERE jid = ?1 AND contact_saved IS NOT NULL AND contact_timestamp IS NOT NULL",
            [form], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        if let Some(state) = state {
            if latest.as_ref().is_none_or(|known| (state.2, !state.0) > (known.2, !known.0)) {
                latest = Some(state);
            }
        }
    }
    Ok(latest)
}

impl MessageStore {
    pub fn set_contact_state(&self, jid: &str, name: Option<&str>, saved: bool, timestamp: i64) -> Result<bool> {
        let forms = super::names::name_forms(self, jid)?;
        let name = saved.then_some(name).flatten().map(str::trim).filter(|name| !name.is_empty());
        {
            let mut conn = self.conn.lock().unwrap();
            if let Some((known_saved, _, known_time)) = latest_contact_state(&conn, &forms)? {
                if timestamp < known_time || (timestamp == known_time && (!known_saved || saved)) {
                    return Ok(false);
                }
            }
            let tx = conn.savepoint()?;
            for form in &forms {
                tx.execute("INSERT INTO contact_identity(jid, contact_saved, saved_name, contact_timestamp)
                    VALUES (?1, ?2, ?3, ?4) ON CONFLICT(jid) DO UPDATE SET
                    contact_saved = excluded.contact_saved, saved_name = excluded.saved_name,
                    contact_timestamp = excluded.contact_timestamp", params![form, saved, name, timestamp])?;
            }
            tx.commit()?;
        }
        if saved {
            if let Some(name) = name { self.set_saved_name(jid, name)?; }
        } else {
            self.clear_saved_name(jid)?;
        }
        Ok(true)
    }

    pub fn set_push_name(&self, jid: &str, name: &str) -> Result<bool> {
        let name = name.trim();
        if name.is_empty() || jid.is_empty() { return Ok(false); }
        let mut changed = false;
        for form in super::names::name_forms(self, jid)? {
            changed |= self.conn.lock().unwrap().execute(
                "INSERT INTO contact_identity(jid, push_name) VALUES (?1, ?2)
                 ON CONFLICT(jid) DO UPDATE SET push_name = excluded.push_name
                 WHERE push_name IS NOT excluded.push_name", params![form, name],
            )? != 0;
            self.set_name(&form, name)?;
        }
        Ok(changed)
    }

    pub fn set_username(&self, jid: &str, username: &str) -> Result<bool> {
        let username = username.trim().trim_start_matches('@');
        if username.is_empty() || jid.is_empty() { return Ok(false); }
        let mut changed = false;
        for form in super::names::name_forms(self, jid)? {
            changed |= self.conn.lock().unwrap().execute(
                "INSERT INTO contact_identity(jid, username) VALUES (?1, ?2)
                 ON CONFLICT(jid) DO UPDATE SET username = excluded.username
                 WHERE username IS NOT excluded.username", params![form, username],
            )? != 0;
        }
        Ok(changed)
    }

    pub fn contact_identity(&self, jid: &str) -> Result<ContactIdentity> {
        let Some((user, server)) = jid.split_once('@') else { return Ok(ContactIdentity::default()); };
        let user = user.split(':').next().unwrap_or(user);
        if server != "lid" && server != "s.whatsapp.net" { return Ok(ContactIdentity::default()); }
        let bare = format!("{user}@{server}");
        let conn = self.conn.lock().unwrap();
        let mapping: Option<(String, String)> = conn.query_row(
            "SELECT lid, pn FROM lid_pn WHERE lid = ?1 OR pn = ?1 LIMIT 1", [user], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let mut forms = vec![jid.to_owned(), bare];
        let mut identity = ContactIdentity { number: (server == "s.whatsapp.net").then(|| user.to_owned()), ..Default::default() };
        if let Some((lid, pn)) = mapping {
            forms.push(format!("{lid}@lid")); forms.push(format!("{pn}@s.whatsapp.net"));
            identity.number = Some(pn);
        }
        if let Some((saved, name, _)) = latest_contact_state(&conn, &forms)? {
            identity.contact_saved = Some(saved);
            if saved { identity.saved_name = name; }
        }
        for form in forms {
            if identity.contact_saved.is_none() && identity.legacy_name.is_none() {
                identity.legacy_name = conn.query_row("SELECT name FROM names WHERE jid = ?1 AND saved = 1", [&form], |row| row.get(0)).optional()?;
            }
            let detail: Option<(Option<String>, Option<String>)> = conn.query_row(
                "SELECT push_name, username FROM contact_identity WHERE jid = ?1", [&form], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            if let Some((push, username)) = detail {
                if identity.push_name.is_none() { identity.push_name = push; }
                if identity.username.is_none() { identity.username = username; }
            }
        }
        Ok(identity)
    }
}

impl StoreWorker {
    pub(crate) async fn set_contact_state(&self, jid: &str, name: Option<&str>, saved: bool, timestamp: i64) -> Result<bool> {
        let (jid, name) = (jid.to_owned(), name.map(str::to_owned));
        self.run(move |store| store.set_contact_state(&jid, name.as_deref(), saved, timestamp)).await
    }
    pub(crate) async fn set_push_name(&self, jid: &str, name: &str) -> Result<bool> {
        let (jid, name) = (jid.to_owned(), name.to_owned());
        self.run(move |store| store.set_push_name(&jid, &name)).await
    }
    pub(crate) async fn set_username(&self, jid: &str, username: &str) -> Result<bool> {
        let (jid, username) = (jid.to_owned(), username.to_owned());
        self.run(move |store| store.set_username(&jid, &username)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_identity_keeps_saved_provenance_push_and_username_across_address_forms() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.set_lid_pn("77", "59891954564").unwrap();
        store.set_push_name("77@lid", "Push").unwrap();
        store.set_contact_state("59891954564@s.whatsapp.net", Some("Saved"), true, 10).unwrap();
        store.set_username("77@lid", "username").unwrap();
        let identity = store.contact_identity("77:3@lid").unwrap();
        assert_eq!(identity.saved_name.as_deref(), Some("Saved"));
        assert_eq!(identity.push_name.as_deref(), Some("Push"));
        assert_eq!(identity.username.as_deref(), Some("username"));
        assert_eq!(identity.number.as_deref(), Some("59891954564"));
        store.set_contact_state("59891954564@s.whatsapp.net", Some("Renamed"), true, 20).unwrap();
        assert_eq!(store.contact_identity("77@lid").unwrap().saved_name.as_deref(), Some("Renamed"));
        store.set_saved_name("77:3@lid", "Renamed").unwrap();
        store.set_contact_state("59891954564@s.whatsapp.net", None, false, 30).unwrap();
        assert!(store.contact_identity("77@lid").unwrap().saved_name.is_none());
        assert!(store.contact_identity("77:3@lid").unwrap().saved_name.is_none());
        store.set_contact_state("77@lid", Some("12345"), true, 40).unwrap();
        store.set_push_name("77@lid", "New push").unwrap();
        assert_eq!(store.contact_identity("77@lid").unwrap().saved_name.as_deref(), Some("12345"));
        store.set_username("88@lid", "only_username").unwrap();
        let username_only = store.contact_identity("88@lid").unwrap();
        assert!(username_only.number.is_none());
        assert_eq!(username_only.username.as_deref(), Some("only_username"));
        assert!(store.contact_identity("1@g.us").unwrap().saved_name.is_none());
    }

    #[test]
    fn contact_identity_baseline_preserves_legacy_and_orders_authority_across_late_aliases() {
        let legacy = Connection::open_in_memory().unwrap();
        legacy.execute_batch("CREATE TABLE names (jid TEXT PRIMARY KEY, name TEXT NOT NULL, saved INTEGER NOT NULL);
            CREATE TABLE contact_identity (jid TEXT PRIMARY KEY, push_name TEXT, username TEXT);
            INSERT INTO names VALUES ('77@lid', 'Old label', 1);
            INSERT INTO contact_identity VALUES ('77@lid', 'Actual push', NULL);").unwrap();
        migrate_baseline(&legacy).unwrap();
        let preserved: (String, bool) = legacy.query_row("SELECT name, saved FROM names", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(preserved, ("Old label".into(), true));
        assert!(legacy.query_row("SELECT contact_saved FROM contact_identity", [], |row| row.get::<_, Option<bool>>(0)).unwrap().is_none());

        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.set_saved_name("77@lid", "Old label").unwrap();
        store.set_saved_name("2@s.whatsapp.net", "Address book B").unwrap();
        store.set_lid_pn("77", "59891954564").unwrap();
        store.set_push_name("59891954564@s.whatsapp.net", "Actual push").unwrap();
        store.set_username("77@lid", "actual_username").unwrap();
        let unknown = store.contact_identity("77@lid").unwrap();
        assert_eq!(unknown.contact_saved, None);
        assert_eq!(unknown.saved_name, None);
        assert_eq!(unknown.legacy_name.as_deref(), Some("Old label"));
        assert_eq!(unknown.push_name.as_deref(), Some("Actual push"));
        store.set_contact_state("77@lid", Some("Fresh saved"), true, 10).unwrap();
        store.set_push_name("77@lid", "New push").unwrap();
        assert_eq!(store.contact_identity("59891954564@s.whatsapp.net").unwrap().saved_name.as_deref(), Some("Fresh saved"));
        store.set_contact_state("77@lid", None, false, 20).unwrap();
        let removed = store.contact_identity("77@lid").unwrap();
        assert_eq!(removed.contact_saved, Some(false));
        assert!(removed.saved_name.is_none() && removed.legacy_name.is_none());

        store.set_saved_name("88@lid", "Unconfirmed alias").unwrap();
        store.set_contact_state("447911123456@s.whatsapp.net", None, false, 20).unwrap();
        store.set_lid_pn("88", "447911123456").unwrap();
        assert_eq!(store.contact_identity("88@lid").unwrap().contact_saved, Some(false));
        assert!(!store.set_contact_state("88@lid", Some("Stale"), true, 10).unwrap());
        assert!(!store.set_contact_state("88@lid", Some("Tie"), true, 20).unwrap());
        assert!(store.set_contact_state("88@lid", Some("New saved"), true, 30).unwrap());
        assert_eq!(store.contact_identity("447911123456@s.whatsapp.net").unwrap().saved_name.as_deref(), Some("New saved"));
        assert!(store.set_contact_state("447911123456@s.whatsapp.net", None, false, 30).unwrap());
        assert_eq!(store.contact_identity("88:3@lid").unwrap().contact_saved, Some(false));
        let untouched = store.contact_identity("2@s.whatsapp.net").unwrap();
        assert_eq!(untouched.contact_saved, None);
        assert_eq!(untouched.legacy_name.as_deref(), Some("Address book B"));
        store.set_username("99@lid", "username_only").unwrap();
        let username_only = store.contact_identity("99@lid").unwrap();
        assert_eq!(username_only.contact_saved, None);
        assert!(username_only.number.is_none());
        assert_eq!(username_only.username.as_deref(), Some("username_only"));
    }
}
