use super::*;
use whatsapp_rust::wacore_binary::Jid;

impl MessageStore {
    pub(crate) fn search_usernames(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<(String, String)>> {
        let needle = query.trim().trim_start_matches('@');
        if needle.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT jid,username FROM contact_identity
            WHERE username IS NOT NULL AND instr(lower(username),lower(?1))>0 ORDER BY jid LIMIT ?2")?;
        let mut matches = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in statement.query_map(params![needle, limit.min(50)], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (jid, username) = row?;
            let Ok(jid) = jid.parse::<Jid>() else {
                continue;
            };
            if !(jid.is_pn() || jid.is_lid()) || jid.user.is_empty() {
                continue;
            }
            let bare = jid.to_non_ad().to_string();
            let canonical = names::canonical_chat(&conn, &bare)?.into_owned();
            if seen.insert(canonical.clone()) {
                matches.push((canonical, username));
            }
        }
        Ok(matches)
    }
}

impl StoreWorker {
    pub(crate) async fn search_usernames(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<(String, String)>> {
        let query = query.to_owned();
        self.run(move |store| store.search_usernames(&query, limit))
            .await
    }
}

#[cfg(test)]
#[path = "usernames_tests.rs"]
mod tests;
