//! History: the pairing window, "load older" requests and their answers.

use super::*;
use whatsapp_rust::wacore::types::events::LazyHistorySync;

/// Asks the phone for `count` messages older than the oldest one stored in
/// `chat`; they arrive later as a history sync.
///
/// Returns the request session the phone will answer, or `None` when the chat
/// has nothing stored to page back from and no request was made.
pub(super) async fn fetch_older(client: &Arc<Client>, store: &StoreWorker, chat: &str, count: i32) -> Result<Option<String>> {
    let Some((id, from_me, timestamp)) = store.oldest_message(chat).await? else {
        return Ok(None);
    };
    let jid: Jid = chat.parse()?;
    let session = client
        .fetch_message_history(&jid, &id, from_me, timestamp * 1000, count)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    log::info!("asked the phone for {count} messages before {id} in {chat} (session {session})");
    Ok(Some(session))
}

/// How long a "load older" request stays waitable before it is forgotten.
pub(super) const OLDER_WAIT: Duration = Duration::from_secs(120);

/// Pending "load older" requests, keyed by the session the phone will answer.
///
/// The UI waits on the answer, so the core has to know which request an
/// incoming history sync completes; the session is what the phone echoes back.
#[derive(Default)]
pub(super) struct OlderWaits {
    entries: std::collections::HashMap<String, (std::time::Instant, String)>,
}

impl OlderWaits {
    /// Registers a request, dropping any that were never answered.
    pub(super) fn remember(&mut self, now: std::time::Instant, session: &str, chat: &str) {
        self.entries.retain(|_, (at, _)| now.duration_since(*at) < OLDER_WAIT);
        self.entries.insert(session.to_string(), (now, chat.to_string()));
    }

    /// The chat a request was for, once and only once.
    pub(super) fn resolve(&mut self, session: &str) -> Option<String> {
        self.entries.remove(session).map(|(_, chat)| chat)
    }
}

/// Whether a chat may pull its past again for an unknown quote: once a minute,
/// so a burst of replies to the same old message asks the phone once.
pub(super) fn recall_allowed(chat: &str) -> bool {
    use std::collections::HashMap;
    use std::time::Instant;
    static LAST: std::sync::OnceLock<Mutex<HashMap<String, Instant>>> = std::sync::OnceLock::new();
    let mut last = LAST.get_or_init(Default::default).lock().unwrap();
    let now = Instant::now();
    if last.get(chat).is_some_and(|at| now.duration_since(*at) < Duration::from_secs(60)) {
        return false;
    }
    last.insert(chat.to_string(), now);
    true
}

impl WhatsAppService {
    /// Asks the phone for older messages in a chat.
    ///
    /// The request goes to our own primary device, and the messages arrive
    /// asynchronously through the normal event stream.
    pub async fn load_older(&self, chat: &str, count: i32) -> Result<()> {
        match fetch_older(&self.client, &self.store, chat, count).await? {
            Some(session) => {
                self.older_waits
                    .lock()
                    .unwrap()
                    .remember(std::time::Instant::now(), &session, chat);
                Ok(())
            }
            None => {
                // Nothing is stored to page back from, so no answer will ever
                // come: end the wait now rather than let it time out.
                let _ = self.events.send(ServiceEvent::HistoryLoaded { chats: vec![chat.to_string()] });
                Ok(())
            }
        }
    }

    /// Replaces the retention policy of the running service.
    pub fn set_retention(&self, retention: DiskRetention) {
        self.disk_retention.set_policy(retention);
    }

    /// Pages every chat back through the phone until it has nothing older,
    /// one request at a time, reporting progress as `Backfill` events.
    pub async fn backfill_history(&self) -> Result<()> {
        let chats: Vec<String> = self.store.chats().await?.into_iter().map(|c| c.chat).collect();
        let total = chats.len();
        for (done, chat) in chats.iter().enumerate() {
            let _ = self.events.send(ServiceEvent::Backfill { done, total });
            loop {
                let before = self.store.oldest_message(chat).await?;
                let mut answers = self.events.subscribe();
                let Some(session) = fetch_older(&self.client, &self.store, chat, 50).await? else { break };
                self.older_waits.lock().unwrap().remember(std::time::Instant::now(), &session, chat);
                // Any HistoryLoaded naming the chat is the answer to this request.
                let answered = tokio::time::timeout(OLDER_WAIT, async {
                    loop {
                        match answers.recv().await {
                            Ok(ServiceEvent::HistoryLoaded { chats }) if chats.contains(chat) => return true,
                            Err(broadcast::error::RecvError::Closed) => return false,
                            _ => {}
                        }
                    }
                })
                .await
                .unwrap_or(false);
                if !answered || self.store.oldest_message(chat).await? == before {
                    break;
                }
            }
        }
        let _ = self.events.send(ServiceEvent::Backfill { done: total, total });
        Ok(())
    }
}

impl Inbound {
    pub(super) async fn on_history_sync(&self, sync: &LazyHistorySync) {
        let Some(history) = sync.get() else {
            log::warn!("history sync type {} failed to decode", sync.sync_type());
            return;
        };
        log::info!(
            "history sync type {} ({:?}): {} conversation(s), {} message(s), {} push name(s), {} LID mapping(s)",
            sync.sync_type(),
            sync.peer_data_request_session_id(),
            history.conversations.len(),
            history.conversations.iter().map(|c| c.messages.len()).sum::<usize>(),
            history.pushnames.len(),
            history.phone_number_to_lid_mappings.len(),
        );
        let batch_guard = self.store.batch().await;
        let mut chats = Vec::new();
        let mut added_chats = 0;
        let mut names_learned;
        {
            let store = &batch_guard;
            let client = self.client_for_events.get().cloned();
            let own = client.as_deref().and_then(|c| c.pn()).map(|j| j.to_non_ad().to_string());
            names_learned = self.learn_history_names(store, history).await;
            // The phone's recent stickers ride the initial sync; seed them so
            // the picker shows them without a resync.
            self.seed_recent_stickers(&history.recent_stickers).await;
            for conversation in &history.conversations {
                let (chat, learned) = self
                    .apply_history_conversation(store, conversation, client.as_ref(), own.as_deref())
                    .await;
                names_learned += learned;
                if let Some(chat) = chat {
                    added_chats += 1;
                    chats.push(chat);
                }
            }
        }
        if names_learned > 0 {
            let _ = self.events.send(ServiceEvent::NamesUpdated { count: names_learned });
        }
        self.finish_history_sync(sync, &mut chats, added_chats);
        if !chats.is_empty() {
            let _ = self.events.send(ServiceEvent::HistoryLoaded { chats });
        }
        // "Load older" answers are on-demand chunks, not the pairing sync.
        if let Some(percent) = sync.progress().filter(|_| sync.sync_type() != 6) {
            let _ = self.events.send(ServiceEvent::HistoryProgress { percent });
        }
        batch_guard.finish().await.logged();
    }

    /// Push names and LID/PN mappings the sync carries, as learned names.
    async fn learn_history_names(&self, store: &StoreWorker, history: &wa::HistorySync) -> usize {
        let mut names_learned = 0;
        for push in &history.pushnames {
            if let (Some(id), Some(name)) = (&push.id, &push.pushname) {
                if !name.is_empty() && store.set_name(id, name).await.observed().is_some() {
                    names_learned += 1;
                }
            }
        }
        for mapping in &history.phone_number_to_lid_mappings {
            names_learned += self
                .pair_history_addresses(store, mapping.lid_jid.as_deref(), mapping.pn_jid.as_deref())
                .await;
        }
        names_learned
    }

    /// Records a history row's LID/PN pair, so later rows resolve to one chat.
    async fn pair_history_addresses(&self, store: &StoreWorker, lid: Option<&str>, pn: Option<&str>) -> usize {
        let (Some(lid), Some(pn)) = (lid, pn) else { return 0 };
        let user = |j: &str| j.split(['@', ':']).next().unwrap_or(j).to_string();
        store.set_lid_pn(&user(lid), &user(pn)).await.observed().is_some() as usize
    }

    /// Seeds one conversation's rows. Returns the canonical chat when it gained
    /// messages, and how many names it taught.
    async fn apply_history_conversation(
        &self,
        store: &StoreWorker,
        conversation: &wa::Conversation,
        client: Option<&Arc<Client>>,
        own: Option<&str>,
    ) -> (Option<String>, usize) {
        if conversation.id == "status@broadcast" {
            return (None, 0);
        }
        let mut names_learned = self
            .pair_history_addresses(store, conversation.lid_jid.as_deref(), conversation.pn_jid.as_deref())
            .await;
        let Some(jid) = conversation.id.parse::<Jid>().observed() else { return (None, 0) };
        // The sync holds the store's write lease; the conversation's own LID/PN
        // pair was just recorded, so the chat resolves from the store alone.
        let chat = resolve_chat(None, store, &jid).await;
        if chat.ends_with("@lid") {
            spawn_lid_lookup(client.cloned(), store, &chat);
        }
        self.name_history_chat(store, &chat, conversation).await;
        let mut added = false;
        for entry in &conversation.messages {
            let (row_added, learned) =
                self.apply_history_message(store, &chat, entry, client.map(|c| c.as_ref()), own).await;
            added |= row_added;
            names_learned += learned;
        }
        (added.then_some(chat), names_learned)
    }

    /// Names a conversation from its display name (a contact) or subject (a
    /// group).
    async fn name_history_chat(&self, store: &StoreWorker, chat: &str, conversation: &wa::Conversation) {
        if !chat.ends_with("@g.us") {
            let name = conversation
                .display_name
                .as_deref()
                .or(conversation.username.as_deref())
                .filter(|n| !n.trim().is_empty());
            if let Some(name) = name {
                store.set_name(chat, name).await.logged();
            }
        }
        if let Some(subject) = conversation.name.as_deref().filter(|n| !n.is_empty()) {
            if chat.ends_with("@g.us") {
                store.set_name(chat, subject).await.logged();
            }
        }
    }

    /// Seeds one history message. Returns whether a row was added and how many
    /// names it taught.
    async fn apply_history_message(
        &self,
        store: &StoreWorker,
        chat: &str,
        entry: &wa::HistorySyncMsg,
        client: Option<&Client>,
        own: Option<&str>,
    ) -> (bool, usize) {
        let Some(web) = entry.message.as_option() else { return (false, 0) };
        let Some(key) = web.key.as_option() else { return (false, 0) };
        if let Some(notice) = web.message_stub_type.and_then(system_kind) {
            let Some(id) = key.id.clone() else { return (false, 0) };
            let notice_kind = notice.clone();
            let stored = system_row(
                chat,
                id,
                web.message_timestamp.unwrap_or(0) as i64,
                notice,
                web.message_stub_parameters.clone(),
            );
            let seen = store.message(chat, &stored.header.id).await.observed().is_some()
                || store
                    .has_system_near(chat, &notice_kind, stored.header.timestamp)
                    .await
                    .observed()
                    .unwrap_or(false);
            if !seen && store.insert_message(&stored).await.observed().is_some() {
                return (true, 0);
            }
            return (false, 0);
        }
        let (Some(id), Some(message)) = (key.id.clone(), web.message.as_option()) else {
            return (false, 0);
        };
        let from_me = key.from_me.unwrap_or(false);
        let sender = if from_me {
            own.map(str::to_string).unwrap_or_else(|| chat.to_string())
        } else {
            web.participant
                .clone()
                .or_else(|| key.participant.clone())
                .unwrap_or_else(|| chat.to_string())
        };
        // Learned even from rows we already have: a re-paired device gets its
        // names back from this history.
        let mut learned = 0;
        if let Some(push) = web.push_name.as_deref().filter(|p| !p.is_empty()) {
            if !from_me && store.set_name(&sender, push).await.observed().is_some() {
                learned = 1;
            }
        }
        // A stored row is already complete; rebuilding it would only rewrite
        // its thumbnails.
        if store.message(chat, &id).await.observed().is_some() {
            return (false, learned);
        }
        if let Some(target) = revoke_target(message) {
            store.revoke_message(chat, &target).await.logged();
            return (false, learned);
        }
        remember_structures(store, chat, &id, &sender, message).await;
        let header = MessageHeader {
            chat: chat.to_string(),
            id,
            sender,
            timestamp: web.message_timestamp.unwrap_or(0) as i64,
            from_me,
        };
        // History media is never bulk downloaded; it is fetched on demand like
        // any other.
        let Some(mut stored) = stored_message(message, header, client, self.media_dir.as_deref(), false).await else {
            return (false, learned);
        };
        // Old messages must not raise unread counts.
        stored.local.read = true;
        match store.insert_message(&stored).await {
            Ok(()) => {
                if message.is_view_once() {
                    store.set_view_once(chat, &stored.header.id, stored.header.from_me).await.logged();
                }
                (true, learned)
            }
            Err(e) => {
                log::error!("could not store history message in {chat}: {e}");
                (false, learned)
            }
        }
    }

    /// Resolves a waiting "load older" request and records the chunk for the
    /// readiness gate. An answered request is not a conversation the initial
    /// window added, so it is not counted.
    fn finish_history_sync(&self, sync: &LazyHistorySync, chats: &mut Vec<String>, added_chats: usize) {
        let answered = sync
            .peer_data_request_session_id()
            .and_then(|session| self.older_waits.lock().unwrap().resolve(session));
        if let Some(chat) = answered {
            if !chats.contains(&chat) {
                chats.push(chat);
            }
        }
        // DiskRetention is left to the next live write, so what was just
        // loaded can be seen first. Record the chunk even when it added
        // nothing, so a stream of no-op chunks does not look quiescent while
        // history is still coming.
        let mut p = self.sync_progress.lock().unwrap();
        p.history_chats += added_chats;
        p.last_progress = Some(std::time::Instant::now());
    }
}
