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
    entries: std::collections::HashMap<String, (std::time::Instant, String, bool)>,
    generations: std::collections::HashMap<String, u64>,
    inflight: std::collections::HashMap<u64, OlderInFlight>,
    next_request: u64,
}

struct OlderInFlight {
    chat: String,
    generation: u64,
    started: std::time::Instant,
    early: std::collections::HashMap<String, EarlyHistory>,
}

#[derive(Default)]
struct EarlyHistory {
    floor: Option<i64>,
    notified: bool,
}

struct OlderSendGuard {
    waits: Arc<Mutex<OlderWaits>>,
    request: u64,
    generation: u64,
}

impl Drop for OlderSendGuard {
    fn drop(&mut self) { self.waits.lock().unwrap().inflight.remove(&self.request); }
}

impl OlderWaits {
    pub(super) fn start(&mut self, chat: &str) -> Result<(u64, u64)> {
        self.inflight.retain(|_, request| request.started.elapsed() < OLDER_WAIT);
        anyhow::ensure!(self.inflight.len() < 32, "too many pending history requests");
        self.next_request += 1;
        let request = self.next_request;
        let generation = self.generation(chat);
        self.inflight.insert(request, OlderInFlight {
            chat: chat.to_owned(), generation, started: std::time::Instant::now(), early: Default::default(),
        });
        Ok((request, generation))
    }

    pub(super) fn protected_chats(&mut self, store: &MessageStore) -> Result<Vec<String>> {
        self.inflight.retain(|_, request| request.started.elapsed() < OLDER_WAIT);
        let mut chats = self.inflight.values().map(|request|
            store.canonical_chat(&request.chat).map(|chat| chat.into_owned())).collect::<Result<Vec<_>>>()?;
        chats.sort_unstable();
        chats.dedup();
        Ok(chats)
    }

    fn note_early(&mut self, store: &MessageStore, session: &str, chat: &str, floor: i64) -> Result<()> {
        for request in self.inflight.values_mut() {
            if request.started.elapsed() >= OLDER_WAIT || store.canonical_chat(&request.chat)?.as_ref() != chat { continue; }
            if request.early.len() < 8 || request.early.contains_key(session) {
                let early = request.early.entry(session.to_owned()).or_default();
                early.floor = Some(early.floor.map_or(floor, |oldest| oldest.min(floor)));
            }
        }
        Ok(())
    }

    fn note_early_receipt(&mut self, store: &MessageStore, session: &str, notified: &[String]) -> Result<()> {
        for request in self.inflight.values_mut() {
            if request.started.elapsed() >= OLDER_WAIT || (request.early.len() >= 8 && !request.early.contains_key(session)) { continue; }
            let chat = store.canonical_chat(&request.chat)?;
            let early = request.early.entry(session.to_owned()).or_default();
            early.notified |= notified.iter().any(|notified| notified == chat.as_ref());
        }
        Ok(())
    }

    pub(super) fn acknowledge_for_store(&mut self, store: &MessageStore, request: u64, generation: u64, session: &str,
        events: &broadcast::Sender<ServiceEvent>) -> Result<bool> {
        let Some(intent) = self.inflight.get(&request) else { return Ok(false) };
        let (chat, early) = (intent.chat.clone(), intent.early.get(session).map(|early| (early.floor, early.notified)));
        if intent.started.elapsed() >= OLDER_WAIT || intent.generation != generation || self.generation(&chat) != generation {
            self.inflight.remove(&request);
            return Ok(false);
        }
        if let Some((Some(floor), _)) = early { store.remember_history_floor(&chat, floor)?; }
        self.remember(std::time::Instant::now(), session, &chat);
        if early.is_some() { self.resolve(session); }
        self.inflight.remove(&request);
        if early.is_some_and(|(_, notified)| !notified) {
            let _ = events.send(ServiceEvent::HistoryLoaded { chats: vec![store.canonical_chat(&chat)?.into_owned()] });
        }
        Ok(true)
    }

    /// Registers a request, dropping any that were never answered.
    pub(super) fn remember(&mut self, now: std::time::Instant, session: &str, chat: &str) {
        self.generation(chat);
        self.entries.retain(|_, (at, _, _)| now.duration_since(*at) < OLDER_WAIT);
        if self.entries.len() >= 128 && !self.entries.contains_key(session) {
            if let Some(oldest) = self.entries.iter().min_by_key(|(_, (at, _, answered))| (!*answered, *at))
                .map(|(session, _)| session.clone()) { self.entries.remove(&oldest); }
        }
        self.entries.insert(session.to_string(), (now, chat.to_string(), false));
    }

    fn generation(&mut self, chat: &str) -> u64 {
        *self.generations.entry(chat.to_owned()).or_default()
    }

    fn cancel_chat(&mut self, chat: &str) {
        *self.generations.entry(chat.to_owned()).or_default() += 1;
        self.entries.retain(|_, (_, pending, _)| pending != chat);
        self.inflight.retain(|_, request| request.chat != chat);
    }

    fn cancel_all(&mut self) {
        for generation in self.generations.values_mut() { *generation += 1; }
        self.entries.clear();
        self.inflight.clear();
    }

    pub(super) fn cancel_for_store(&mut self, store: &MessageStore, chat: Option<&str>) -> Result<()> {
        if let Some(chat) = chat {
            let canonical = store.canonical_chat(chat)?.into_owned();
            let mut aliases = vec![canonical.clone()];
            for candidate in self.generations.keys() {
                if store.canonical_chat(candidate)?.as_ref() == canonical {
                    aliases.push(candidate.clone());
                }
            }
            for alias in aliases { self.cancel_chat(&alias); }
        } else { self.cancel_all(); }
        Ok(())
    }

    /// The chat a request was for, once and only once.
    pub(super) fn resolve(&mut self, session: &str) -> Option<String> {
        self.entries.retain(|_, (at, _, _)| at.elapsed() < OLDER_WAIT);
        let (_, chat, answered) = self.entries.get_mut(session)?;
        if *answered { return None; }
        *answered = true;
        Some(chat.clone())
    }

    fn floor_target(&mut self, session: &str) -> Option<String> {
        self.entries.retain(|_, (at, _, _)| at.elapsed() < OLDER_WAIT);
        self.entries.get(session).map(|(_, chat, _)| chat.clone())
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
    async fn begin_older_request(&self, chat: &str) -> Result<(String, OlderSendGuard)> {
        let chat = chat.to_owned();
        let waits = self.older_waits.clone();
        self.store.run(move |store| {
            let chat = store.canonical_chat(&chat)?.into_owned();
            let (request, generation) = waits.lock().unwrap().start(&chat)?;
            Ok((chat, OlderSendGuard { waits, request, generation }))
        }).await
    }

    async fn acknowledge_older_request(&self, guard: &OlderSendGuard, session: &str) -> Result<bool> {
        let (request, generation) = (guard.request, guard.generation);
        let waits = guard.waits.clone();
        let session = session.to_owned();
        let events = self.events.clone();
        self.store.run(move |store| {
            let result = waits.lock().unwrap().acknowledge_for_store(store, request, generation, &session, &events);
            result
        }).await
    }

    pub(super) async fn cancel_older_requests(&self, chat: Option<&str>) -> Result<()> {
        let chat = chat.map(str::to_owned);
        let waits = self.older_waits.clone();
        self.store.run(move |store| {
            let result = waits.lock().unwrap().cancel_for_store(store, chat.as_deref());
            result
        }).await
    }

    /// Asks the phone for older messages in a chat.
    ///
    /// The request goes to our own primary device, and the messages arrive
    /// asynchronously through the normal event stream.
    pub async fn load_older(&self, chat: &str, count: i32) -> Result<()> {
        let (chat, guard) = self.begin_older_request(chat).await?;
        match fetch_older(&self.client, &self.store, &chat, count).await? {
            Some(session) => {
                let remembered = self.acknowledge_older_request(&guard, &session).await?;
                if !remembered {
                    let _ = self.events.send(ServiceEvent::HistoryLoaded { chats: vec![chat] });
                }
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
                let (requested, guard) = self.begin_older_request(chat).await?;
                let Some(session) = fetch_older(&self.client, &self.store, &requested, 50).await? else { break };
                if !self.acknowledge_older_request(&guard, &session).await? { break; }
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
        let request_session = sync.peer_data_request_session_id()
            .filter(|_| sync.sync_type() == wa::history_sync::HistorySyncType::ON_DEMAND as i32);
        let requested = request_session.and_then(|session| self.older_waits.lock().unwrap().floor_target(session));
        let answered = request_session.and_then(|session| self.older_waits.lock().unwrap().resolve(session));
        let mut chats = Vec::new();
        let mut added_chats = 0;
        let mut audit_chats = std::collections::HashSet::new();
        let mut sticker_changes = false;
        let mut names_learned;
        {
            let store = &batch_guard;
            let client = self.client_for_events.get().cloned();
            let own = client.as_deref().and_then(|c| c.pn()).map(|j| j.to_non_ad().to_string());
            names_learned = self.learn_history_names(store, history).await;
            // The phone's recent stickers ride the initial sync; seed them so
            // the picker shows them without a resync.
            sticker_changes |= self.seed_recent_stickers(store, &history.recent_stickers).await.observed() == Some(true);
            for conversation in &history.conversations {
                let (chat, learned, audited) = self
                    .apply_history_conversation(store, conversation, client.as_ref(), own.as_deref(), requested.as_deref(), request_session, &mut sticker_changes)
                    .await;
                names_learned += learned;
                if let Some(chat) = audited { audit_chats.insert(chat); }
                if let Some(chat) = chat {
                    added_chats += 1;
                    chats.push(chat);
                }
            }
        }
        if names_learned > 0 {
            let _ = self.events.send(ServiceEvent::NamesUpdated { count: names_learned });
        }
        self.finish_history_sync(answered, &mut chats, added_chats);
        let notified = if !chats.is_empty() && self.events.send(ServiceEvent::HistoryLoaded { chats: chats.clone() }).is_ok() {
            chats
        } else { Vec::new() };
        if let Some(session) = request_session {
            let (waits, session) = (self.older_waits.clone(), session.to_owned());
            batch_guard.run(move |store| {
                let result = waits.lock().unwrap().note_early_receipt(store, &session, &notified);
                result
            }).await.logged();
        }
        // "Load older" answers are on-demand chunks, not the pairing sync.
        if let Some(percent) = sync.progress().filter(|_| sync.sync_type() != 6) {
            let _ = self.events.send(ServiceEvent::HistoryProgress { percent });
        }
        if batch_guard.finish().await.observed().is_some() {
            for chat in audit_chats { let _ = self.events.send(ServiceEvent::GroupAuditChanged { chat }); }
            if sticker_changes { let _ = self.events.send(ServiceEvent::StickerLibraryChanged { packs: true, favorites: false, recents: true }); }
        }
    }

    /// Push names and LID/PN mappings the sync carries, as learned names.
    async fn learn_history_names(&self, store: &StoreWorker, history: &wa::HistorySync) -> usize {
        let mut names_learned = 0;
        for push in &history.pushnames {
            if let (Some(id), Some(name)) = (&push.id, &push.pushname) {
                if !name.is_empty() && store.set_push_name(id, name).await.observed().is_some() {
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
        requested: Option<&str>,
        request_session: Option<&str>,
        sticker_changes: &mut bool,
    ) -> (Option<String>, usize, Option<String>) {
        if conversation.id == "status@broadcast" {
            return (None, 0, None);
        }
        let mut names_learned = self
            .pair_history_addresses(store, conversation.lid_jid.as_deref(), conversation.pn_jid.as_deref())
            .await;
        let Some(jid) = conversation.id.parse::<Jid>().observed() else { return (None, 0, None) };
        // The sync holds the store's write lease; the conversation's own LID/PN
        // pair was just recorded, so the chat resolves from the store alone.
        let chat = resolve_chat(None, store, &jid).await;
        if chat.ends_with("@lid") {
            spawn_lid_lookup(client.cloned(), store, &chat);
        }
        self.name_history_chat(store, &chat, conversation).await;
        let preserve = if let Some(requested) = requested.and_then(|jid| jid.parse::<Jid>().ok()) {
            resolve_chat(None, store, &requested).await == chat
        } else { false };
        let mut added = false;
        let mut audited = false;
        let mut floor = None::<i64>;
        for entry in &conversation.messages {
            let (row_added, learned) =
                self.apply_history_message(store, &chat, entry, client.map(|c| c.as_ref()), own, &mut audited, sticker_changes).await;
            added |= row_added;
            names_learned += learned;
            let timestamp = if row_added {
                entry.message.as_option().and_then(|web| web.message_timestamp)
                    .and_then(|timestamp| i64::try_from(timestamp).ok())
            } else if preserve || request_session.is_some() {
                if let Some(id) = entry.message.as_option().and_then(|web| web.key.as_option()).and_then(|key| key.id.as_deref()) {
                    store.message(&chat, id).await.observed().map(|row| row.header.timestamp)
                } else { None }
            } else { None };
            if let Some(timestamp) = timestamp.filter(|timestamp| *timestamp > 0) {
                floor = Some(floor.map_or(timestamp, |oldest| oldest.min(timestamp)));
            }
        }
        if let Some(floor) = floor {
            if preserve { store.remember_history_floor(&chat, floor).await.logged(); }
            else if let Some(session) = request_session {
                let (waits, session, chat) = (self.older_waits.clone(), session.to_owned(), chat.clone());
                store.run(move |store| {
                    let result = waits.lock().unwrap().note_early(store, &session, &chat, floor);
                    result
                }).await.logged();
            }
        }
        (added.then(|| chat.clone()), names_learned, audited.then_some(chat))
    }

    /// Names a conversation from its display name (a contact) or subject (a
    /// group).
    async fn name_history_chat(&self, store: &StoreWorker, chat: &str, conversation: &wa::Conversation) {
        if !chat.ends_with("@g.us") {
            if let Some(username) = conversation.username.as_deref() {
                store.set_username(chat, username).await.logged();
            }
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
        audited: &mut bool,
        sticker_changes: &mut bool,
    ) -> (bool, usize) {
        let Some(web) = entry.message.as_option() else { return (false, 0) };
        let pin = history_pins::history_message_pin(web).ok().flatten();
        let target = pin.as_ref().map(|pin| pin.target.clone()).or_else(||
            web.message.as_option().and_then(|message| group_audit::audit_message_target(message, chat)));
        let previous = if chat.ends_with("@g.us") {
            match target.as_deref() { Some(target) => store.message(chat, target).await.observed(), None => None }
        } else { None };
        if self.apply_history_pin(store, chat, web).await {
            *audited |= group_audit::audit_group_history_message(store, chat, web, previous.as_ref()).await.observed() == Some(true);
        }
        let Some(key) = web.key.as_option() else { return (false, 0) };
        if let Some(stub) = web.message_stub_type {
            if stub == wa::web_message_info::StubType::CIPHERTEXT {
                return (self.history_unavailable(store, chat, web, own).await, 0);
            }
            let Some(notice) = system_kind(stub) else { return (false, 0) };
            let Some(id) = key.id.clone() else { return (false, 0) };
            let notice_kind = notice.clone();
            let mut stored = system_row(
                chat,
                id,
                web.message_timestamp.unwrap_or(0) as i64,
                notice,
                web.message_stub_parameters.clone(),
            );
            stored.header.sender = web.participant.clone().or_else(|| key.participant.clone())
                .or_else(|| key.from_me.unwrap_or(false).then(|| own.unwrap_or_default().to_owned()))
                .unwrap_or_default();
            let seen = store.message(chat, &stored.header.id).await.observed().is_some()
                || store
                    .has_system_near(chat, &notice_kind, &stored.system.params, stored.header.timestamp, false)
                    .await
                    .observed()
                    .unwrap_or(false);
            if !seen && store.insert_message(&stored).await.observed().is_some() {
                member_profiles::record_member_history_context(store, &stored, web).await.logged();
                *audited |= group_audit::audit_group_notice(store, &stored, crate::store::group_audit::GroupAuditSource::History).await.observed() == Some(true);
                return (true, 0);
            }
            if seen {
                *audited |= group_audit::audit_group_notice(store, &stored, crate::store::group_audit::GroupAuditSource::History).await.observed() == Some(true);
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
            if !from_me && store.set_push_name(&sender, push).await.observed().is_some() {
                learned = 1;
            }
        }
        // A stored row is already complete; rebuilding it would only rewrite
        // its thumbnails.
        if let Some(row) = store.message(chat, &id).await.observed().filter(|row| !row.is_unavailable() || row.local.revoked || row.local.deleted) {
            if !row.local.revoked && !row.local.deleted && !row.spoiler && row.media.once_kind.is_none() && row.system.kind.is_none() {
                *sticker_changes |= self.on_sticker_pack(store, message).await.observed() == Some(true);
                if row.media.kind.as_deref() == Some("sticker") && record_sticker(store, &row).await.observed() == Some(true) { *sticker_changes = true; }
            }
            return (false, learned);
        }
        if let Some(target) = revoke_target(message) {
            let previous = store.message(chat, &target).await.observed();
            if store.revoke_message(chat, &target).await.observed().is_some() {
                *audited |= group_audit::audit_group_history_message(store, chat, web, previous.as_ref()).await.observed() == Some(true);
            }
            self.retire_unavailable(store, chat, &target).await;
            self.retire_unavailable(store, chat, &id).await;
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
        stored.history_shareable &= web.ephemeral_expiration_timestamp.is_none() && web.ephemeral_duration.is_none()
            && web.ephemeral_start_timestamp.is_none() && matches!(web.status, Some(wa::web_message_info::Status::SERVER_ACK
                | wa::web_message_info::Status::DELIVERY_ACK | wa::web_message_info::Status::READ | wa::web_message_info::Status::PLAYED));
        stored.local.status = match web.status {
            Some(wa::web_message_info::Status::SERVER_ACK) => Some("sent".into()),
            Some(wa::web_message_info::Status::DELIVERY_ACK) => Some("delivered".into()),
            Some(wa::web_message_info::Status::READ | wa::web_message_info::Status::PLAYED) => Some("read".into()),
            _ => None,
        };
        stored.local.read = true;
        match store.insert_history_row(&stored).await {
            Ok(Some(accepted)) => {
                if !accepted.local.revoked && !accepted.local.deleted && !accepted.spoiler && accepted.media.once_kind.is_none() && accepted.system.kind.is_none() {
                    *sticker_changes |= self.on_sticker_pack(store, message).await.observed() == Some(true);
                    if accepted.media.kind.as_deref() == Some("sticker") && record_sticker(store, &accepted).await.observed() == Some(true) { *sticker_changes = true; }
                }
                member_profiles::record_member_history_context(store, &accepted, web).await.logged();
                if accepted.system.kind.is_some() {
                    *audited |= group_audit::audit_group_notice(store, &accepted, crate::store::group_audit::GroupAuditSource::History).await.observed() == Some(true);
                }
                if target.is_some() || member_label_change(message).is_some() {
                    *audited |= group_audit::audit_group_history_message(store, chat, web, previous.as_ref()).await.observed() == Some(true);
                }
                if decoded_message(&message).view_once {
                    store.set_view_once(chat, &stored.header.id, stored.header.from_me).await.logged();
                }
                (true, learned)
            }
            Ok(None) => (false, learned),
            Err(e) => {
                log::error!("could not store history message in {chat}: {e}");
                (false, learned)
            }
        }
    }

    /// Resolves a waiting "load older" request and records the chunk for the
    /// readiness gate. An answered request is not a conversation the initial
    /// window added, so it is not counted.
    fn finish_history_sync(&self, answered: Option<String>, chats: &mut Vec<String>, added_chats: usize) {
        if let Some(chat) = answered {
            if !chats.contains(&chat) {
                chats.push(chat);
            }
        }
        // Record empty chunks too, while their request still ends the UI wait.
        let mut p = self.sync_progress.lock().unwrap();
        p.history_chats += added_chats;
        p.last_progress = Some(std::time::Instant::now());
    }
}

#[cfg(test)]
mod history_floor_request_tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_the_send_future_releases_temporary_history_protection() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let waits = Arc::new(Mutex::new(OlderWaits::default()));
        let (request, generation) = waits.lock().unwrap().start("101@g.us").unwrap();
        let guard = OlderSendGuard { waits: waits.clone(), request, generation };
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = guard;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        assert_eq!(waits.lock().unwrap().protected_chats(&store).unwrap(), vec!["101@g.us"]);
        task.abort();
        let _ = task.await;
        assert!(waits.lock().unwrap().protected_chats(&store).unwrap().is_empty());
    }

    #[test]
    fn expired_or_excessive_requests_cannot_keep_temporary_history_protection() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut waits = OlderWaits::default();
        let (events, _) = broadcast::channel(4);
        let (request, generation) = waits.start("101@g.us").unwrap();
        waits.inflight.get_mut(&request).unwrap().started = std::time::Instant::now() - OLDER_WAIT - Duration::from_secs(1);
        assert!(!waits.acknowledge_for_store(&store, request, generation, "expired", &events).unwrap());
        assert!(waits.protected_chats(&store).unwrap().is_empty());
        for index in 0..32 { waits.start(&format!("{index}@g.us")).unwrap(); }
        assert!(waits.start("overflow@g.us").is_err());
        waits.cancel_all();
        assert!(waits.protected_chats(&store).unwrap().is_empty());
    }
}
