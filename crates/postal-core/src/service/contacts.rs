//! Contacts: names, address forms (LID and phone), search and local aliases.

use super::*;

/// Stores the LID and phone forms of one sender when a message carries both.
pub(super) fn remember_lid_pn(store: &MessageStore, sender: &Jid, alt: Option<&Jid>) {
    let Some(alt) = alt else { return };
    let (lid, pn) = match (sender.is_lid(), alt.is_lid()) {
        (true, false) => (sender, alt),
        (false, true) => (alt, sender),
        _ => return,
    };
    store.set_lid_pn(&lid.user, &pn.user).logged();
}

/// The chat key a direct message belongs under: the phone-number form when the
/// mapping is known, so both address forms share one chat. Groups and already
/// numbered chats pass through untouched.
pub(super) async fn canonical_chat(
    client: Option<&Client>,
    store: &MessageStore,
    chat: &Jid,
    sender: &Jid,
    alt: Option<&Jid>,
) -> String {
    let bare = chat.to_non_ad();
    if chat.is_group() || !bare.is_lid() {
        return bare.to_string();
    }
    // A direct chat's sender is the contact, and the message often carries the
    // contact's other form.
    if sender.to_non_ad().to_string() == bare.to_string() {
        if let Some(other) = alt {
            let other = other.to_non_ad();
            if other.is_pn() {
                store.set_lid_pn(&bare.user, &other.user).logged();
                return other.to_string();
            }
        }
    }
    resolve_chat(client, store, chat).await
}

/// The chat key a state change for `jid` belongs under: the phone-number form
/// for a mapped LID, so archive, pin, mute and read marks sit on the same row
/// as the chat's messages. Groups and already numbered chats pass through.
pub(super) async fn resolve_chat(
    client: Option<&Client>,
    store: &MessageStore,
    jid: &Jid,
) -> String {
    let bare = jid.to_non_ad();
    if !bare.is_lid() {
        return bare.to_string();
    }
    let pn = match client {
        Some(client) => other_form(client, store, &bare).await.map(|(_, pn)| pn),
        None => store.lid_pn(&bare.user).observed().flatten().map(|(_, pn)| pn),
    };
    match pn {
        Some(pn) => {
            let user = pn.split('@').next().unwrap_or(&pn);
            format!("{user}@s.whatsapp.net")
        }
        None => bare.to_string(),
    }
}

/// The other address form of a bare user JID, from the session or our own record of it.
pub(super) async fn other_form(client: &Client, store: &MessageStore, bare: &Jid) -> Option<(String, String)> {
    if let Some(Some(entry)) = client.get_lid_pn_entry(bare).await.observed() {
        let (lid, pn) = (entry.lid.to_string(), entry.phone_number.to_string());
        store.set_lid_pn(&lid, &pn).logged();
        return Some((lid, pn));
    }
    store.lid_pn(&bare.user).observed().flatten()
}

/// The user part of a JID, without the device suffix or server.
pub(super) fn user_part(jid: &str) -> String {
    jid.split('@')
        .next()
        .unwrap_or(jid)
        .split(':')
        .next()
        .unwrap_or(jid)
        .to_string()
}

/// Every address form one contact is known under, the JID given first.
///
/// A contact is the same person whichever form the UI happens to hold, so
/// alias writes go through every form and reads come back under each of them.
/// Without this an alias added from a group roster (a LID) would be invisible
/// to a direct chat (a phone-number JID), and the second write would be refused
/// as a clash. A contact the core has not mapped to a twin has just the one.
pub(super) fn contact_forms(store: &MessageStore, jid: &str) -> Vec<String> {
    let mut forms = vec![jid.to_string()];
    let twin = match store.lid_pn(&user_part(jid)).observed().flatten() {
        Some((_, pn)) if jid.ends_with("@lid") => format!("{pn}@s.whatsapp.net"),
        Some((lid, _)) => format!("{lid}@lid"),
        _ => return forms,
    };
    if !forms.contains(&twin) {
        forms.push(twin);
    }
    forms
}

/// Copies address-book names onto the LID form of the same address.
///
/// The address book is keyed by phone number, while messages in an
/// LID-addressed chat carry the LID. The library's own mapping table bridges
/// the two, so names already learned apply to existing history instead of only
/// to messages that arrive after this point.
pub(super) fn backfill_lid_names(session_path: &std::path::Path, store: &MessageStore) {
    use std::collections::HashMap;

    let mappings = (|| -> Result<HashMap<String, String>> {
        let conn = rusqlite::Connection::open_with_flags(
            session_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let mut stmt = conn.prepare("SELECT lid, phone_number FROM lid_pn_mapping")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })();
    let Some(by_lid) = mappings.observed() else { return };
    if by_lid.is_empty() {
        return;
    }
    // Keep the library's mapping in our own store, so a chat key resolves to
    // its phone-number form even before a message carries both address forms.
    {
        let _commit = store.batch();
        for (lid, phone) in &by_lid {
            store.set_lid_pn(lid, phone).logged();
        }
    }

    let Some(saved) = store.saved_names().observed() else {
        return;
    };
    let by_phone: HashMap<&str, &str> = saved
        .iter()
        .filter_map(|(jid, name)| {
            jid.strip_suffix("@s.whatsapp.net")
                .map(|phone| (phone, name.as_str()))
        })
        .collect();
    if by_phone.is_empty() {
        return;
    }

    for address in store.known_addresses().observed().unwrap_or_default() {
        let Some((user, server)) = address.split_once('@') else {
            continue;
        };
        if server != "lid" {
            continue;
        }
        let bare = user.split(':').next().unwrap_or(user);
        let Some(phone) = by_lid.get(bare) else {
            continue;
        };
        match by_phone.get(phone.as_str()) {
            Some(name) => {
                store.set_saved_name(&address, name).logged();
                store.set_saved_name(&format!("{bare}@lid"), name).logged();
            }
            // Without a saved name, show the phone number instead of the LID,
            // which nobody can read.
            None => {
                store.set_name(&address, phone).logged();
                store.set_name(&format!("{bare}@lid"), phone).logged();
            }
        }
    }
}

impl WhatsAppService {
    /// Our own JID without a device suffix, or empty before pairing.
    pub fn own_jid(&self) -> String {
        self.client
            .pn()
            .or_else(|| self.client.lid())
            .map(|j| j.to_non_ad().to_string())
            .unwrap_or_default()
    }

    /// Our own display name, as peers see it. Empty before it is known.
    ///
    /// Read from the cached device snapshot rather than `profile`, which would
    /// cost three network round trips for the same name.
    pub fn push_name(&self) -> String {
        self.client.push_name()
    }

    /// Whether a destination is our own account, in either addressing form.
    ///
    /// A message to ourselves needs no network receipt to be delivered, so it
    /// is stored as delivered rather than left pending.
    pub(super) fn is_self_jid(&self, jid: &Jid) -> bool {
        match (self.client.pn(), self.client.lid()) {
            (Some(pn), Some(lid)) => jid.matches_user_or_lid(&pn, Some(&lid)),
            (Some(pn), None) => jid.matches_user_or_lid(&pn, None),
            (None, Some(lid)) => jid.matches_user_or_lid(&lid, None),
            (None, None) => false,
        }
    }

    /// Best known name per JID, looked up under both its LID and phone form;
    /// our own addresses read as our push name. Falls back to the phone number
    /// digits, and leaves out JIDs nothing is known about.
    pub async fn names_for(&self, jids: &[String]) -> std::collections::HashMap<String, String> {
        let numeric = |n: &str| is_placeholder_name(n);
        let own: Vec<String> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|j| j.to_non_ad().to_string())
            .collect();
        let push_name = self.client.push_name();
        let mut out = std::collections::HashMap::new();
        let mut unknown: Vec<(String, Jid)> = Vec::new();
        for jid in jids {
            let Ok(parsed) = jid.parse::<Jid>() else { continue };
            let bare = parsed.to_non_ad();
            let key = bare.to_string();
            if own.contains(&key) && !push_name.is_empty() {
                out.insert(jid.clone(), push_name.clone());
                continue;
            }
            let mut name = self.store.name_for(&key).observed().flatten();
            let mut number = bare.is_pn().then(|| bare.user.to_string());
            if name.as_deref().is_none_or(numeric) {
                if let Some((lid, pn)) = other_form(&self.client, &self.store, &bare).await {
                    let other = if bare.is_lid() {
                        format!("{pn}@s.whatsapp.net")
                    } else {
                        format!("{lid}@lid")
                    };
                    number = Some(pn);
                    if let Some(found) = self.store.name_for(&other).observed().flatten() {
                        if !numeric(&found) {
                            name = Some(found);
                        }
                    }
                }
            }
            match name.filter(|n| !numeric(n)) {
                Some(name) => {
                    out.insert(jid.clone(), name);
                }
                None => {
                    if let Some(number) = number {
                        out.insert(jid.clone(), number);
                    }
                    unknown.push((jid.clone(), bare));
                }
            }
        }
        // Push names only travel with messages; for anyone we have not heard
        // from, the username or verified business name is the next best thing.
        unknown.retain(|(_, jid)| !self.nameless.lock().unwrap().contains(&jid.to_string()));
        if !unknown.is_empty() {
            let query: Vec<Jid> = unknown.iter().map(|(_, j)| j.clone()).collect();
            match self.user_info(&query).await {
                Ok(infos) => {
                    let mut learned = 0;
                    let mut nameless = self.nameless.lock().unwrap();
                    for (asked, jid) in unknown.iter() {
                        let info = infos.values().find(|i| {
                            i.jid.user == jid.user || i.lid.as_ref().is_some_and(|l| l.user == jid.user)
                        });
                        let found = info.and_then(|i| {
                            i.verified_name
                                .as_ref()
                                .and_then(|v| v.name.clone())
                                .or_else(|| i.username.as_ref().map(|u| u.to_string()))
                        });
                        if let Some(found) = found.filter(|n| !n.trim().is_empty()) {
                            self.store.set_name(&jid.to_string(), &found).logged();
                            out.insert(asked.clone(), found);
                            learned += 1;
                        } else {
                            nameless.insert(jid.to_string());
                        }
                    }
                    log::debug!(
                        "names: asked the server about {}, learned {learned}, {} still unnamed",
                        unknown.len(),
                        unknown.len() - learned,
                    );
                }
                Err(e) => log::warn!("names: user info query for {} JID(s) failed: {e}", unknown.len()),
            }
        }
        out
    }

    /// Everything a profile card shows about someone, fetched fresh.
    pub async fn user_profile(&self, jid: &str) -> Result<UserProfile> {
        let bare = jid.parse::<Jid>()?.to_non_ad();
        let key = bare.to_string();
        let mut profile = UserProfile { jid: key.clone(), ..Default::default() };
        profile.number = if bare.is_pn() {
            Some(bare.user.to_string())
        } else {
            other_form(&self.client, &self.store, &bare).await.map(|(_, pn)| pn)
        };
        if let Ok(infos) = self.user_info(std::slice::from_ref(&bare)).await {
            if let Some(info) = infos.into_values().next() {
                profile.about = info.status.filter(|s| !s.is_empty());
                profile.username = info.username.map(|u| u.to_string());
                profile.business = info.verified_name.and_then(|v| v.name);
            }
        }
        profile.name = self
            .names_for(std::slice::from_ref(&key))
            .await
            .remove(&key)
            .filter(|n| !is_placeholder_name(n));
        Ok(profile)
    }

    /// Chats, contacts and groups matching a query.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        use std::collections::HashSet;

        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }

        let local = self.store.chats()?;
        let local_counts: std::collections::HashMap<&str, i64> =
            local.iter().map(|c| (c.chat.as_str(), c.message_count)).collect();
        let has_local_messages = |jid: &str| local_counts.get(jid).is_some_and(|&n| n > 0);
        // A local alias is the one thing that can find a contact whose name and
        // number say nothing about the query, so it is matched alongside them.
        // Read once up front: an account holds a handful of aliases.
        let aliases = self.all_aliases()?;
        let of = |jid: &str| -> Vec<String> { aliases.get(jid).cloned().unwrap_or_default() };
        let mut results: Vec<SearchResult> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // Local chats first: they have history and are what a search usually
        // means. A cleared chat stays local but reports no messages.
        for chat in &local {
            let number = user_part(&chat.chat);
            let name = chat.display_name.clone().unwrap_or_else(|| number.clone());
            if name.to_lowercase().contains(&needle) || number.contains(&needle) {
                seen.insert(chat.chat.clone());
                results.push(SearchResult {
                    kind: if chat.chat.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                    saved: self.store.name_is_saved(&chat.chat).observed().unwrap_or(false),
                    jid: chat.chat.clone(),
                    name,
                    number,
                    has_messages: chat.message_count > 0,
                    aliases: of(&chat.chat),
                });
            }
        }

        // Address book and learned names.
        for (jid, name, saved) in self.store.search_names(&needle, 50)? {
            if !seen.insert(jid.clone()) {
                continue;
            }
            results.push(SearchResult {
                kind: if jid.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                saved,
                jid: jid.clone(),
                name,
                number: user_part(&jid),
                has_messages: has_local_messages(&jid),
                aliases: of(&jid),
            });
        }

        // Contacts found by nothing but an alias. One row is one contact, so
        // this runs per alias rather than per address form; the form a name is
        // known under is preferred, since a phone number reads better than a
        // bare LID.
        for (jid, alias) in self.aliases.all()? {
            if !alias.to_lowercase().contains(&needle) {
                continue;
            }
            let forms = self.contact_forms(&jid);
            if forms.iter().any(|form| seen.contains(form)) {
                continue;
            }
            // A form the contact has a name under beats the one the alias was
            // stored against, and a phone number beats a bare LID, which is a
            // number nobody recognises.
            let named = forms
                .iter()
                .find_map(|form| self.store.name_for(form).observed().flatten().filter(|n| !is_placeholder_name(n)))
                .or_else(|| forms.iter().find(|f| f.ends_with("@s.whatsapp.net")).cloned())
                .unwrap_or_else(|| jid.clone());
            let number = user_part(&named);
            let name = self.store.name_for(&named).observed().flatten().unwrap_or_else(|| number.clone());
            for form in &forms {
                seen.insert(form.clone());
            }
            results.push(SearchResult {
                kind: "contact".to_string(),
                saved: self.store.name_is_saved(&named).observed().unwrap_or(false),
                jid: named.clone(),
                name,
                number,
                has_messages: forms.iter().any(|form| has_local_messages(form)),
                aliases: of(&named),
            });
        }

        // Groups from the account, including ones with no local history.
        for (jid, subject) in self.group_overviews().await {
            if subject.to_lowercase().contains(&needle) && seen.insert(jid.clone()) {
                results.push(SearchResult {
                    jid: jid.clone(),
                    name: subject,
                    number: String::new(),
                    kind: "group".into(),
                    saved: false,
                    has_messages: has_local_messages(&jid),
                    aliases: of(&jid),
                });
            }
        }

        results.truncate(50);
        Ok(results)
    }

    /// The address forms one contact answers to: [`contact_forms`].
    fn contact_forms(&self, jid: &str) -> Vec<String> {
        contact_forms(&self.store, jid)
    }

    /// Every alias in the account, keyed by each address form of its contact.
    ///
    /// The account holds a handful of aliases at most, so the UI takes them
    /// all in one go rather than asking per contact.
    pub fn all_aliases(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        use std::collections::HashMap;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for (jid, alias) in self.aliases.all()? {
            for form in self.contact_forms(&jid) {
                out.entry(form).or_default().push(alias.clone());
            }
        }
        Ok(out)
    }

    /// Gives a contact another local alias for `@` addressing. Rejected when
    /// another contact already answers to it, so an alias names one person.
    pub fn add_alias(&self, jid: &str, alias: &str) -> Result<()> {
        self.aliases.add(&self.contact_forms(jid), alias)
    }

    /// Drops one of a contact's aliases. Every address form is tried, since
    /// which one the row was written under depends on what was known at the
    /// time the alias was added.
    pub fn remove_alias(&self, jid: &str, alias: &str) -> Result<()> {
        for form in self.contact_forms(jid) {
            self.aliases.remove(&form, alias)?;
        }
        Ok(())
    }
}
