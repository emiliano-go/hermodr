use super::*;

impl WhatsAppService {
    pub async fn switcher_catalog(&self) -> Result<Vec<SearchResult>> {
        let aliases = self.aliases.all().await?;
        let mut groups = std::collections::BTreeMap::new();
        if let Some(cached) = self.groups_cache.lock().unwrap().as_ref() {
            for group in cached {
                groups.insert(group.id.to_string(), group.subject.clone());
            }
        }
        for (jid, info) in self.group_cache.lock().unwrap().iter() {
            if info.subject.is_some() { groups.insert(jid.clone(), info.subject.clone()); }
            else { groups.entry(jid.clone()).or_insert(None); }
        }
        self.store.switcher_catalog(aliases, groups.into_iter().collect()).await
    }

    pub async fn switcher_messages(&self, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        self.store.switcher_messages(query, limit).await
    }
}
