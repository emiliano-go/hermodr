use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ContactIdentity {
    pub saved_name: Option<String>,
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

impl MessageStore {
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
        for form in forms {
            if identity.saved_name.is_none() {
                identity.saved_name = conn.query_row("SELECT name FROM names WHERE jid = ?1 AND saved = 1", [&form], |row| row.get(0)).optional()?;
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
        store.set_saved_name("59891954564@s.whatsapp.net", "Saved").unwrap();
        store.set_username("77@lid", "username").unwrap();
        let identity = store.contact_identity("77:3@lid").unwrap();
        assert_eq!(identity.saved_name.as_deref(), Some("Saved"));
        assert_eq!(identity.push_name.as_deref(), Some("Push"));
        assert_eq!(identity.username.as_deref(), Some("username"));
        assert_eq!(identity.number.as_deref(), Some("59891954564"));
        store.set_saved_name("59891954564@s.whatsapp.net", "Renamed").unwrap();
        assert_eq!(store.contact_identity("77@lid").unwrap().saved_name.as_deref(), Some("Renamed"));
        store.set_saved_name("77:3@lid", "Renamed").unwrap();
        store.clear_saved_name("59891954564@s.whatsapp.net").unwrap();
        assert!(store.contact_identity("77@lid").unwrap().saved_name.is_none());
        assert!(store.contact_identity("77:3@lid").unwrap().saved_name.is_none());
        store.set_saved_name("77@lid", "12345").unwrap();
        store.set_push_name("77@lid", "New push").unwrap();
        assert_eq!(store.contact_identity("77@lid").unwrap().saved_name.as_deref(), Some("12345"));
        store.set_username("88@lid", "only_username").unwrap();
        let username_only = store.contact_identity("88@lid").unwrap();
        assert!(username_only.number.is_none());
        assert_eq!(username_only.username.as_deref(), Some("only_username"));
        assert!(store.contact_identity("1@g.us").unwrap().saved_name.is_none());
    }
}
