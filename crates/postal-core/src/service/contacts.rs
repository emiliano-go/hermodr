//! Contacts: names, address forms (LID and phone), search and local aliases.

use super::*;

pub(super) async fn apply_contact_identity(
    store: &StoreWorker,
    update: &whatsapp_rust::wacore::types::events::ContactUpdate,
) -> Result<bool> {
    let lid = update.action.lid_jid.as_deref().and_then(|jid| jid.parse::<Jid>().ok())
        .filter(|jid| jid.is_lid()).or_else(|| update.jid.is_lid().then(|| update.jid.clone()));
    let pn = update.action.pn_jid.as_deref().and_then(|jid| jid.parse::<Jid>().ok())
        .filter(|jid| jid.is_pn()).or_else(|| update.jid.is_pn().then(|| update.jid.clone()));
    let jid = update.jid.to_non_ad().to_string();
    let username = update.action.username.clone();
    store.run(move |store| {
        let mut changed = false;
        if let Some((lid, pn)) = lid.zip(pn) {
            changed = store.lid_pn(&lid.user)?.as_ref().is_none_or(|(_, known)| known != &pn.user);
            store.set_lid_pn(&lid.user, &pn.user)?;
        }
        if let Some(username) = username {
            changed |= store.set_username(&jid, &username)?;
        }
        Ok(changed)
    }).await
}

pub(super) async fn first_stored_name(store: &StoreWorker, forms: &[String]) -> Option<(String, String)> {
    let forms = forms.to_vec();
    store.run(move |store| {
        for form in &forms {
            if store.name_is_saved(form)? {
                if let Some(name) = store.name_for(form)? { return Ok(Some((form.clone(), name))); }
            }
        }
        for form in forms {
            if let Some(name) = store.name_for(&form)?.filter(|name| !is_placeholder_name(name)) {
                return Ok(Some((form, name)));
            }
        }
        Ok(None)
    }).await.observed().flatten()
}

/// Stores the LID and phone forms of one sender when a message carries both.
pub(super) async fn remember_lid_pn(store: &StoreWorker, sender: &Jid, alt: Option<&Jid>) {
    let Some(alt) = alt else { return };
    let (lid, pn) = match (sender.is_lid(), alt.is_lid()) {
        (true, false) => (sender, alt),
        (false, true) => (alt, sender),
        _ => return,
    };
    store.set_lid_pn(&lid.user, &pn.user).await.logged();
}

/// The chat key a direct message belongs under: the phone-number form when the
/// mapping is known, so both address forms share one chat. Groups and already
/// numbered chats pass through untouched.
pub(super) async fn canonical_chat(
    client: Option<&Client>,
    store: &StoreWorker,
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
                store.set_lid_pn(&bare.user, &other.user).await.logged();
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
    store: &StoreWorker,
    jid: &Jid,
) -> String {
    let bare = jid.to_non_ad();
    if !bare.is_lid() {
        return bare.to_string();
    }
    let pn = match client {
        Some(client) => other_form(client, store, &bare).await.map(|(_, pn)| pn),
        None => store.lid_pn(&bare.user).await.observed().flatten().map(|(_, pn)| pn),
    };
    match pn {
        Some(pn) => {
            let user = pn.split('@').next().unwrap_or(&pn);
            format!("{user}@s.whatsapp.net")
        }
        None => bare.to_string(),
    }
}

/// Learns a LID's phone form in the background and folds the chat once it is
/// known. Runs off the message batch: a network lookup under the batch's write
/// lease would stall every later message behind a dead link.
pub(super) fn spawn_lid_lookup(client: Option<Arc<Client>>, store: &StoreWorker, chat: &str) {
    let Some(client) = client else { return };
    let store = store.clone();
    let chat = chat.to_string();
    tokio::spawn(async move {
        let Ok(jid) = chat.parse::<Jid>() else { return };
        if let Some(Some(entry)) = client.get_lid_pn_entry(&jid).await.observed() {
            store
                .set_lid_pn(&entry.lid.to_string(), &entry.phone_number.to_string())
                .await
                .logged();
        }
    });
}

/// The other address form of a bare user JID, from the session or our own record of it.
pub(super) async fn other_form(client: &Client, store: &StoreWorker, bare: &Jid) -> Option<(String, String)> {
    if let Some(Some(entry)) = client.get_lid_pn_entry(bare).await.observed() {
        let (lid, pn) = (user_part(&entry.lid.to_string()), user_part(&entry.phone_number.to_string()));
        store.set_lid_pn(&lid, &pn).await.logged();
        return Some((lid, pn));
    }
    store.lid_pn(&bare.user).await.observed().flatten()
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
pub(super) async fn contact_forms(store: &StoreWorker, jid: &str) -> Vec<String> {
    let mut forms = vec![jid.to_string()];
    let twin = match store.lid_pn(&user_part(jid)).await.observed().flatten() {
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
pub(super) fn backfill_lid_names(session_path: &std::path::Path, store: &MessageStore,
    key: Option<&crate::database_crypto::DatabaseKey>) {
    use std::collections::HashMap;

    let mappings = (|| -> Result<HashMap<String, String>> {
        let conn = crate::database_crypto::open_database(
            session_path, key, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
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

fn editable_contact(address: &str) -> Result<Jid> {
    let jid: Jid = address.parse::<Jid>().map_err(|error| anyhow::Error::new(
        crate::message_ref::MessageRef::new("error.contact_address")
    ).context(error.to_string()))?;
    anyhow::ensure!((jid.is_pn() || jid.is_lid()) && jid.device == 0 && jid.agent == 0 && jid.integrator == 0
        && !jid.user.is_empty() && jid.user.chars().all(|c| c.is_ascii_digit()), crate::message_ref::MessageRef::new("error.contact_address"));
    Ok(jid)
}

fn mapped_contact_phone(target: &Jid, mapping: Option<(String, String)>) -> Result<Jid> {
    if target.is_pn() { return Ok(target.clone()); }
    let (lid, pn) = mapping.ok_or_else(|| anyhow::anyhow!(crate::message_ref::MessageRef::new("error.contact_mapping_unknown")))?;
    anyhow::ensure!(lid == target.user.as_str(), crate::message_ref::MessageRef::new("error.contact_mapping_address"));
    let phone = editable_contact(&format!("{pn}@s.whatsapp.net"))?;
    anyhow::ensure!(phone.is_pn(), crate::message_ref::MessageRef::new("error.contact_mapping_phone"));
    Ok(phone)
}

async fn contact_phone(client: &Client, store: &StoreWorker, address: &str) -> Result<Jid> {
    let target = editable_contact(address)?;
    if target.is_pn() { return Ok(target); }
    if let Some(mapping) = store.lid_pn(&target.user).await? {
        return mapped_contact_phone(&target, Some(mapping));
    }
    let mapping = client.get_lid_pn_entry(&target).await?
        .map(|entry| (entry.lid.to_string(), entry.phone_number.to_string()));
    let phone = mapped_contact_phone(&target, mapping)?;
    store.set_lid_pn(&target.user, &phone.user).await?;
    Ok(phone)
}

async fn commit_contact_change(
    store: &StoreWorker, target: &Jid, name: Option<&str>, timestamp: i64,
    authorize: impl FnOnce() -> bool, send: impl std::future::Future<Output = Result<()>>,
) -> Result<bool> {
    anyhow::ensure!(authorize(), crate::message_ref::MessageRef::new("error.account_changed"));
    send.await?;
    store.set_contact_state(&target.to_string(), name, name.is_some(), timestamp).await
}

impl WhatsAppService {
    pub async fn save_contact(&self, address: &str, full_name: &str, first_name: Option<&str>, save_on_primary_addressbook: bool,
        authorize: impl FnOnce() -> bool) -> Result<()> {
        let full_name = full_name.trim();
        anyhow::ensure!(!full_name.is_empty(), crate::message_ref::MessageRef::new("error.contact_name"));
        anyhow::ensure!(self.is_connected(), crate::message_ref::MessageRef::new("error.not_connected"));
        let target = contact_phone(&self.client, &self.store, address).await?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let first_name = first_name.map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned);
        let changed = commit_contact_change(&self.store, &target, Some(full_name), timestamp, || authorize() && self.is_connected(), async {
            self.client.chat_actions().save_contact(&target, Some(full_name.to_owned()), first_name, save_on_primary_addressbook)
                .await.map_err(anyhow::Error::from)
        }).await?;
        if changed { let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 }); }
        Ok(())
    }

    pub async fn remove_contact(&self, address: &str, authorize: impl FnOnce() -> bool) -> Result<()> {
        anyhow::ensure!(self.is_connected(), crate::message_ref::MessageRef::new("error.not_connected"));
        let target = contact_phone(&self.client, &self.store, address).await?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let changed = commit_contact_change(&self.store, &target, None, timestamp, || authorize() && self.is_connected(), async {
            self.client.chat_actions().remove_contact(&target).await.map_err(anyhow::Error::from)
        }).await?;
        if changed { let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 }); }
        Ok(())
    }

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
    /// Best known name per JID, looked up under both its LID and phone form;
    /// our own addresses read as our push name. Falls back to the phone number
    /// digits, and leaves out JIDs nothing is known about.
    pub async fn names_for(&self, jids: &[String]) -> std::collections::HashMap<String, String> {
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
            if own.contains(&bare.to_string()) && !push_name.is_empty() {
                out.insert(jid.clone(), push_name.clone());
                continue;
            }
            let (name, number) = self.local_name(&bare).await;
            match name.filter(|n| !is_placeholder_name(n)) {
                Some(name) => {
                    out.insert(jid.clone(), name);
                    if bare.is_lid() && number.is_none() {
                        let key = bare.to_string();
                        if self.store.run(move |store| store.contact_identity(&key)).await.observed()
                            .is_some_and(|identity| identity.saved_name.is_none() && identity.username.is_none()) {
                            unknown.push((jid.clone(), bare));
                        }
                    }
                }
                None => {
                    if let Some(number) = number {
                        out.insert(jid.clone(), number);
                    }
                    unknown.push((jid.clone(), bare));
                }
            }
        }
        self.ask_server(&mut out, unknown).await;
        out
    }

    /// The best stored name and number for a bare JID, trying its other address
    /// form when this one says nothing readable.
    async fn local_name(&self, bare: &Jid) -> (Option<String>, Option<String>) {
        let mut forms = vec![bare.to_string()];
        let mut number = bare.is_pn().then(|| bare.user.to_string());
        if let Some((lid, pn)) = other_form(&self.client, &self.store, bare).await {
            forms.push(if bare.is_lid() { format!("{pn}@s.whatsapp.net") } else { format!("{lid}@lid") });
            number = Some(pn);
        }
        let name = first_stored_name(&self.store, &forms).await.map(|(_, name)| name);
        (name, number)
    }

    /// Push names only travel with messages; for anyone we have not heard from,
    /// the username or verified business name is the next best thing.
    async fn ask_server(
        &self,
        out: &mut std::collections::HashMap<String, String>,
        mut unknown: Vec<(String, Jid)>,
    ) {
        unknown.retain(|(_, jid)| !self.nameless.lock().unwrap().contains(&jid.to_string()));
        if unknown.is_empty() {
            return;
        }
        let query: Vec<Jid> = unknown.iter().map(|(_, jid)| jid.clone()).collect();
        match self.user_info(&query).await {
            Ok(infos) => {
                let mut learned = 0;
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
                    if let Some(info) = info.filter(|info| info.jid.is_pn()) {
                        if let Some(lid) = &info.lid {
                            if self.store.set_lid_pn(&lid.user, &info.jid.user).await.observed().is_some() {
                                let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 });
                            }
                        }
                    }
                    if let Some(username) = info.and_then(|i| i.username.as_ref()) {
                        if self.store.set_username(&jid.to_string(), &username.to_string()).await.observed() == Some(true) {
                            let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 });
                        }
                    }
                    if let Some(found) = found.filter(|n| !n.trim().is_empty()) {
                        self.store.set_name(&jid.to_string(), &found).await.logged();
                        out.insert(asked.clone(), found);
                        learned += 1;
                    } else {
                        self.nameless.lock().unwrap().insert(jid.to_string());
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
                if let Some(username) = &profile.username {
                    if self.store.set_username(&key, username).await? { let _ = self.events.send(ServiceEvent::NamesUpdated { count: 1 }); }
                }
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

    pub async fn contact_identities(&self, jids: &[String]) -> Result<std::collections::HashMap<String, crate::store::contact_identity::ContactIdentity>> {
        for jid in jids {
            if let Ok(bare) = jid.parse::<Jid>() { if bare.is_lid() { other_form(&self.client, &self.store, &bare.to_non_ad()).await; } }
        }
        let asked = jids.to_vec();
        let mut identities = self.store.run(move |store| {
            asked.into_iter().map(|jid| store.contact_identity(&jid).map(|identity| (jid, identity))).collect::<Result<std::collections::HashMap<_, _>>>()
        }).await?;
        for (jid, identity) in &mut identities {
            if jid.parse::<Jid>().ok().is_some_and(|jid| self.is_self_jid(&jid)) {
                identity.own = true;
                identity.push_name = Some(self.client.push_name()).filter(|name| !name.is_empty());
            }
        }
        Ok(identities)
    }

    /// Chats, contacts and groups matching a query.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let local = self.store.chats().await?;
        let mut index = SearchIndex::new(needle, &local, self.all_aliases().await?);
        self.search_local_chats(&local, &mut index).await;
        self.search_stored_names(&mut index).await?;
        self.search_cached_usernames(&mut index).await?;
        self.search_alias_matches(&mut index).await?;
        self.search_group_overviews(&mut index).await;
        index.results.truncate(50);
        Ok(index.results)
    }

    /// Local chats first: they have history and are what a search usually
    /// means. A cleared chat stays local but reports no messages.
    async fn search_local_chats(&self, local: &[crate::store::ChatSummary], index: &mut SearchIndex) {
        for chat in local {
            let number = user_part(&chat.chat);
            let name = chat.display_name.clone().unwrap_or_else(|| number.clone());
            if name.to_lowercase().contains(&index.needle) || number.contains(&index.needle) {
                index.seen.insert(chat.chat.clone());
                index.results.push(SearchResult {
                    kind: if chat.chat.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                    saved: self.store.name_is_saved(&chat.chat).await.observed().unwrap_or(false),
                    jid: chat.chat.clone(),
                    name,
                    number,
                    has_messages: chat.message_count > 0,
                    aliases: index.aliases_of(&chat.chat),
                });
            }
        }
    }

    /// Address book and learned names.
    async fn search_stored_names(&self, index: &mut SearchIndex) -> Result<()> {
        for (jid, name, saved) in self.store.search_names(&index.needle, 50).await? {
            if !index.seen.insert(jid.clone()) {
                continue;
            }
            index.results.push(SearchResult {
                kind: if jid.ends_with("@g.us") { "group".to_string() } else { "contact".to_string() },
                saved,
                number: user_part(&jid),
                has_messages: index.has_messages(&jid),
                aliases: index.aliases_of(&jid),
                jid,
                name,
            });
        }
        Ok(())
    }

    async fn search_cached_usernames(&self, index: &mut SearchIndex) -> Result<()> {
        for (jid, username) in self.store.search_usernames(&index.needle, 50).await? {
            if !index.seen.insert(jid.clone()) { continue; }
            let key = jid.clone();
            let (identity, saved) = self.store.run(move |store| Ok((store.contact_identity(&key)?, store.name_is_saved(&key)?))).await?;
            let name = username_search_name(&identity, saved, &username);
            index.results.push(SearchResult {
                kind: "contact".into(), saved,
                number: identity.number.unwrap_or_default(), has_messages: index.has_messages(&jid),
                aliases: index.aliases_of(&jid), jid, name,
            });
        }
        Ok(())
    }

    /// Contacts found by nothing but an alias. One row is one contact, so this
    /// runs per alias rather than per address form; the form a name is known
    /// under is preferred, since a phone number reads better than a bare LID.
    async fn search_alias_matches(&self, index: &mut SearchIndex) -> Result<()> {
        for (jid, alias) in self.aliases.all().await? {
            if !alias.to_lowercase().contains(&index.needle) {
                continue;
            }
            let forms = contact_forms(&self.store, &jid).await;
            if forms.iter().any(|form| index.seen.contains(form)) {
                continue;
            }
            let named = first_stored_name(&self.store, &forms).await.map(|(form, _)| form)
                .or_else(|| forms.iter().find(|f| f.ends_with("@s.whatsapp.net")).cloned())
                .unwrap_or_else(|| jid.clone());
            let number = user_part(&named);
            let name = self.store.name_for(&named).await.observed().flatten().unwrap_or_else(|| number.clone());
            for form in &forms {
                index.seen.insert(form.clone());
            }
            index.results.push(SearchResult {
                kind: "contact".to_string(),
                saved: self.store.name_is_saved(&named).await.observed().unwrap_or(false),
                jid: named.clone(),
                name,
                number,
                has_messages: forms.iter().any(|form| index.has_messages(form)),
                aliases: index.aliases_of(&named),
            });
        }
        Ok(())
    }

    /// Groups from the account, including ones with no local history.
    async fn search_group_overviews(&self, index: &mut SearchIndex) {
        for (jid, subject) in self.group_overviews().await {
            if subject.to_lowercase().contains(&index.needle) && index.seen.insert(jid.clone()) {
                index.results.push(SearchResult {
                    jid: jid.clone(),
                    name: subject,
                    number: String::new(),
                    kind: "group".into(),
                    saved: false,
                    has_messages: index.has_messages(&jid),
                    aliases: index.aliases_of(&jid),
                });
            }
        }
    }

    /// Every alias in the account, keyed by each address form of its contact.
    ///
    /// The account holds a handful of aliases at most, so the UI takes them
    /// all in one go rather than asking per contact.
    pub async fn all_aliases(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        use std::collections::HashMap;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for (jid, alias) in self.aliases.all().await? {
            for form in contact_forms(&self.store, &jid).await {
                out.entry(form).or_default().push(alias.clone());
            }
        }
        Ok(out)
    }

    /// Gives a contact another local alias for `@` addressing. Rejected when
    /// another contact already answers to it, so an alias names one person.
    pub async fn add_alias(&self, jid: &str, alias: &str) -> Result<()> {
        self.aliases.add(&contact_forms(&self.store, jid).await, alias).await
    }

    /// Drops one of a contact's aliases. Every address form is tried, since
    /// which one the row was written under depends on what was known at the
    /// time the alias was added.
    pub async fn remove_alias(&self, jid: &str, alias: &str) -> Result<()> {
        for form in contact_forms(&self.store, jid).await {
            self.aliases.remove(&form, alias).await?;
        }
        Ok(())
    }
}


fn username_search_name(identity: &crate::store::contact_identity::ContactIdentity, saved: bool, username: &str) -> String {
    let known = |value: &Option<String>| value.as_ref().filter(|name| !crate::store::is_placeholder_name(name)).cloned();
    identity.saved_name.clone().or_else(|| saved.then(|| identity.legacy_name.clone()).flatten())
        .or_else(|| known(&identity.push_name)).or_else(|| known(&identity.legacy_name)).unwrap_or_else(|| format!("@{username}"))
}

/// The running state of the contact search: the needle, the local
/// alias map and message counts, and what has been found and already seen.
struct SearchIndex {
    needle: String,
    aliases: std::collections::HashMap<String, Vec<String>>,
    local_counts: std::collections::HashMap<String, i64>,
    seen: std::collections::HashSet<String>,
    results: Vec<SearchResult>,
}

impl SearchIndex {
    fn new(
        needle: String,
        local: &[crate::store::ChatSummary],
        aliases: std::collections::HashMap<String, Vec<String>>,
    ) -> Self {
        SearchIndex {
            needle,
            aliases,
            local_counts: local.iter().map(|chat| (chat.chat.clone(), chat.message_count)).collect(),
            seen: std::collections::HashSet::new(),
            results: Vec::new(),
        }
    }

    fn has_messages(&self, jid: &str) -> bool {
        self.local_counts.get(jid).is_some_and(|count| *count > 0)
    }

    /// A local alias is the one thing that can find a contact whose name and
    /// number say nothing about the query, so it is matched alongside them.
    fn aliases_of(&self, jid: &str) -> Vec<String> {
        self.aliases.get(jid).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod contact_identity_tests {
    use super::*;

    #[test]
    fn username_search_preserves_saved_and_legacy_saved_names_before_push_or_numeric_fallback() {
        let mut identity = crate::store::contact_identity::ContactIdentity {
            legacy_name: Some("1234".into()), push_name: Some("Push name".into()), ..Default::default()
        };
        assert_eq!(username_search_name(&identity, true, "Handle"), "1234");
        identity.saved_name = Some("Saved name".into());
        assert_eq!(username_search_name(&identity, false, "Handle"), "Saved name");
        identity.saved_name = None;
        assert_eq!(username_search_name(&identity, false, "Handle"), "Push name");
        identity.push_name = Some("15550000001".into());
        assert_eq!(username_search_name(&identity, false, "Handle"), "@Handle");
    }

    #[tokio::test]
    async fn contact_identity_action_uses_typed_address_forms_and_username() {
        let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
        let mut update = whatsapp_rust::wacore::types::events::ContactUpdate::builder()
            .jid("59891954564@s.whatsapp.net".parse().unwrap())
            .timestamp("2026-09-30T00:00:00Z".parse().unwrap()).from_full_sync(true)
            .action(Box::new(wa::sync_action_value::ContactAction {
                lid_jid: Some("77:3@lid".into()), pn_jid: Some("59891954564@s.whatsapp.net".into()),
                username: Some("actual_username".into()), ..Default::default()
            })).build();
        assert!(apply_contact_identity(&store, &update).await.unwrap());
        let identity = store.run(|store| store.contact_identity("77@lid")).await.unwrap();
        assert_eq!(identity.number.as_deref(), Some("59891954564"));
        assert_eq!(identity.username.as_deref(), Some("actual_username"));
        assert!(identity.saved_name.is_none());
        assert!(!apply_contact_identity(&store, &update).await.unwrap());
        update.jid = "88@lid".parse().unwrap();
        update.action.lid_jid = None;
        update.action.pn_jid = Some("447911123456@s.whatsapp.net".into());
        update.action.username = Some("second_username".into());
        assert!(apply_contact_identity(&store, &update).await.unwrap());
        assert_eq!(store.run(|store| store.contact_identity("447911123456@s.whatsapp.net")).await.unwrap().username.as_deref(), Some("second_username"));
        update.jid = "99@lid".parse().unwrap();
        update.action.pn_jid = Some("447911123456@g.us".into());
        apply_contact_identity(&store, &update).await.unwrap();
        assert!(store.run(|store| store.contact_identity("99@lid")).await.unwrap().number.is_none());
    }
}

#[cfg(test)]
#[path = "contact_actions_tests.rs"]
mod contact_actions_tests;
