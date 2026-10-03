use super::*;
use crate::service::SearchResult;
use std::collections::{BTreeMap, HashMap, HashSet};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub(crate) const SWITCHER_SEARCH_LIMIT: usize = 30;

fn normalized(value: &str) -> String {
    value.nfd().filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase).filter(|character| !character.is_whitespace()).collect()
}

fn fuzzy_score(needle: &str, value: &str) -> Option<f64> {
    let text = normalized(value);
    let mut cursor = 0;
    let mut gaps = 0;
    for character in needle.chars() {
        let next = text[cursor..].find(character)?;
        gaps += next;
        cursor += next + character.len_utf8();
    }
    let score = if text == needle { 3000.0 } else if text.starts_with(needle) { 2000.0 }
        else if text.contains(needle) { 1000.0 } else { 500.0 - gaps as f64 };
    Some(score - text.len() as f64 / 1000.0)
}

fn catalog_score(needle: &str, query: &str, row: &SearchResult) -> Option<f64> {
    let address = if query.contains('@') { &row.jid } else { &row.number };
    std::iter::once(&row.name).chain(std::iter::once(&row.number))
        .chain(std::iter::once(address)).chain(row.aliases.iter())
        .filter_map(|value| fuzzy_score(needle, value)).reduce(f64::max)
}

fn catalog_key(store: &MessageStore, jid: &str) -> Result<Option<String>> {
    let Some((user, server)) = jid.split_once('@') else { return Ok(None) };
    let user = user.split(':').next().unwrap_or(user);
    if user.is_empty() || server.is_empty() || server == "broadcast" { return Ok(None); }
    let bare = format!("{user}@{server}");
    Ok(Some(store.canonical_chat(&bare)?.into_owned()))
}

fn catalog_name(store: &MessageStore, jid: &str, cached: Option<&str>) -> Result<(String, bool)> {
    if !jid.ends_with("@lid") && !jid.ends_with("@s.whatsapp.net") {
        let name = cached.filter(|name| !name.trim().is_empty()).map(str::to_owned)
            .or(store.name_for(jid)?.filter(|name| !name.trim().is_empty()))
            .unwrap_or_else(|| jid.split('@').next().unwrap_or(jid).to_owned());
        return Ok((name, false));
    }
    let identity = store.contact_identity(jid)?;
    let saved = identity.saved_name.or(identity.legacy_name).filter(|name| !is_placeholder_name(name));
    if let Some(name) = saved { return Ok((name, true)); }
    let mut learned = None;
    for form in names::name_forms(store, jid)? {
        if let Some(name) = store.name_for(&form)?.filter(|name| !is_placeholder_name(name)) {
            learned = Some(name); break;
        }
    }
    let name = cached.filter(|name| !name.trim().is_empty()).map(str::to_owned)
        .or(learned)
        .or(identity.push_name).or(identity.username)
        .unwrap_or_else(|| jid.split('@').next().unwrap_or(jid).to_owned());
    Ok((name, false))
}

impl MessageStore {
    pub(crate) fn switcher_catalog(&self, aliases: &[(String, String)], groups: &[(String, Option<String>)]) -> Result<Vec<SearchResult>> {
        let mut chats = self.chats()?;
        chats.sort_by(|left, right| right.last_message_at.cmp(&left.last_message_at).then_with(|| left.chat.cmp(&right.chat)));
        let with_messages: HashSet<_> = chats.iter().filter(|chat| chat.message_count > 0).map(|chat| chat.chat.as_str()).collect();
        let group_names: HashMap<_, _> = groups.iter().map(|(jid, name)| (jid.as_str(), name.as_deref())).collect();
        let mut addresses: Vec<String> = chats.iter().map(|chat| chat.chat.clone()).collect();
        {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT jid FROM names UNION SELECT jid FROM contact_identity
                UNION SELECT jid FROM chats UNION SELECT sender FROM messages UNION SELECT chat FROM messages
                UNION SELECT lid || '@lid' FROM lid_pn UNION SELECT pn || '@s.whatsapp.net' FROM lid_pn")?;
            addresses.extend(stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        addresses.extend(aliases.iter().map(|(jid, _)| jid.clone()));
        addresses.extend(groups.iter().map(|(jid, _)| jid.clone()));
        let mut local_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (jid, alias) in aliases {
            if let Some(key) = catalog_key(self, jid)? { local_aliases.entry(key).or_default().push(alias.clone()); }
        }
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for jid in addresses {
            let Some(jid) = catalog_key(self, &jid)? else { continue };
            if !seen.insert(jid.clone()) { continue; }
            let kind = if jid.ends_with("@g.us") { "group" } else if jid.ends_with("@newsletter") { "channel" } else { "contact" };
            let cached = group_names.get(jid.as_str()).copied().flatten();
            let (name, saved) = catalog_name(self, &jid, cached)?;
            let mut aliases = if kind == "contact" { local_aliases.remove(&jid).unwrap_or_default() } else { Vec::new() };
            aliases.sort(); aliases.dedup();
            results.push(SearchResult {
                number: jid.split('@').next().unwrap_or(&jid).to_owned(),
                has_messages: with_messages.contains(jid.as_str()),
                jid, name, kind: kind.into(), saved, aliases,
            });
        }
        Ok(results)
    }

    pub(crate) fn switcher_search(&self, aliases: &[(String, String)], groups: &[(String, Option<String>)], query: &str) -> Result<Vec<SearchResult>> {
        let query = query.trim();
        if query.is_empty() { return Ok(Vec::new()); }
        let needle = normalized(query);
        let mut ranked: Vec<_> = self.switcher_catalog(aliases, groups)?.into_iter().enumerate()
            .filter_map(|(order, row)| catalog_score(&needle, query, &row).map(|score| (score, order, row))).collect();
        ranked.sort_by(|left, right| right.0.total_cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        Ok(ranked.into_iter().take(SWITCHER_SEARCH_LIMIT).map(|(_, _, row)| row).collect())
    }

    pub(crate) fn switcher_messages(&self, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let query = query.trim();
        if query.is_empty() { return Ok(Vec::new()); }
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_").to_lowercase();
        let pattern = format!("%{escaped}%");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m
            LEFT JOIN names n ON n.jid = m.sender
            WHERE lower(m.text) LIKE ?1 ESCAPE '\\' AND m.deleted = 0 AND m.revoked = 0
              AND m.system_kind IS NULL AND m.spoiler = 0 AND COALESCE(m.media_kind, '') != 'view_once'
              AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat = m.chat AND v.id = m.id)
              AND NOT EXISTS (SELECT 1 FROM hidden_chats h WHERE h.jid = m.chat)
            ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC, m.chat ASC LIMIT ?2"))?;
        let rows = stmt.query_map(params![pattern, limit.clamp(1, 50)], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}

impl StoreWorker {
    pub(crate) async fn switcher_catalog(&self, aliases: Vec<(String, String)>, groups: Vec<(String, Option<String>)>) -> Result<Vec<SearchResult>> {
        self.run(move |store| store.switcher_catalog(&aliases, &groups)).await
    }

    pub(crate) async fn switcher_search(&self, aliases: Vec<(String, String)>, groups: Vec<(String, Option<String>)>, query: &str) -> Result<Vec<SearchResult>> {
        let query = query.to_owned();
        self.run(move |store| store.switcher_search(&aliases, &groups, &query)).await
    }

    pub(crate) async fn switcher_messages(&self, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let query = query.to_owned();
        self.run(move |store| store.switcher_messages(&query, limit)).await
    }
}

#[cfg(test)]
#[path = "quick_switcher_tests.rs"]
mod tests;
