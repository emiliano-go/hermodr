//! History: the pairing window, "load older" requests and their answers.

use super::*;
use whatsapp_rust::wacore::types::events::LazyHistorySync;

/// Asks the phone for `count` messages older than the oldest one stored in
/// `chat`; they arrive later as a history sync.
///
/// Returns the request session the phone will answer, or `None` when the chat
/// has nothing stored to page back from and no request was made.
pub(super) async fn fetch_older(client: &Arc<Client>, store: &MessageStore, chat: &str, count: i32) -> Result<Option<String>> {
    let Some((id, from_me, timestamp)) = store.oldest_message(chat)? else {
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
        let chats: Vec<String> = self.store.chats()?.into_iter().map(|c| c.chat).collect();
        let total = chats.len();
        for (done, chat) in chats.iter().enumerate() {
            let _ = self.events.send(ServiceEvent::Backfill { done, total });
            loop {
                let before = self.store.oldest_message(chat)?;
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
                if !answered || self.store.oldest_message(chat)? == before {
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
        let Self { store, events, client_for_events, media_dir, older_waits, sync_progress, .. } = self;
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
        let _commit = store.batch();
        let client = client_for_events.get().cloned();
        let own = client
            .as_deref()
            .and_then(|c| c.pn())
            .map(|j| j.to_non_ad().to_string());
        let mut names_learned = 0;
        for push in &history.pushnames {
            if let (Some(id), Some(name)) = (&push.id, &push.pushname) {
                if !name.is_empty() && store.set_name(id, name).observed().is_some() {
                    names_learned += 1;
                }
            }
        }
        let pair = |lid: Option<&str>, pn: Option<&str>| {
            let (Some(lid), Some(pn)) = (lid, pn) else { return false };
            let user = |j: &str| j.split(['@', ':']).next().unwrap_or(j).to_string();
            store.set_lid_pn(&user(lid), &user(pn)).observed().is_some()
        };
        for mapping in &history.phone_number_to_lid_mappings {
            if pair(mapping.lid_jid.as_deref(), mapping.pn_jid.as_deref()) {
                names_learned += 1;
            }
        }
        let mut chats = Vec::new();
        for conversation in &history.conversations {
            if conversation.id == "status@broadcast" {
                continue;
            }
            pair(conversation.lid_jid.as_deref(), conversation.pn_jid.as_deref());
            let Some(jid) = conversation.id.parse::<Jid>().observed() else { continue };
            let chat = resolve_chat(client.as_deref(), store, &jid).await;
            if !chat.ends_with("@g.us") {
                let name = conversation
                    .display_name
                    .as_deref()
                    .or(conversation.username.as_deref())
                    .filter(|n| !n.trim().is_empty());
                if let Some(name) = name {
                    store.set_name(&chat, name).logged();
                }
            }
            if let Some(subject) =
                conversation.name.as_deref().filter(|n| !n.is_empty())
            {
                if chat.ends_with("@g.us") {
                    store.set_name(&chat, subject).logged();
                }
            }
            let mut added = false;
            for entry in &conversation.messages {
                let Some(web) = entry.message.as_option() else { continue };
                let Some(key) = web.key.as_option() else { continue };
                if let Some(notice) = web.message_stub_type.and_then(system_kind) {
                    let Some(id) = key.id.clone() else { continue };
                    let notice_kind = notice.clone();
                    let stored = system_row(
                        &chat,
                        id,
                        web.message_timestamp.unwrap_or(0) as i64,
                        notice,
                        web.message_stub_parameters.clone(),
                    );
                    let seen = store.message(&chat, &stored.header.id).observed().is_some()
                        || store.has_system_near(&chat, &notice_kind, stored.header.timestamp).observed().unwrap_or(false);
                    if !seen && store.insert_message(&stored).observed().is_some() {
                        added = true;
                    }
                    continue;
                }
                let (Some(id), Some(message)) =
                    (key.id.clone(), web.message.as_option())
                else {
                    continue;
                };
                let from_me = key.from_me.unwrap_or(false);
                let sender = if from_me {
                    own.clone().unwrap_or_else(|| chat.clone())
                } else {
                    web.participant
                        .clone()
                        .or_else(|| key.participant.clone())
                        .unwrap_or_else(|| chat.clone())
                };
                // Learned even from rows we already have: a re-paired
                // device gets its names back from this history.
                if let Some(push) =
                    web.push_name.as_deref().filter(|p| !p.is_empty())
                {
                    if !from_me && store.set_name(&sender, push).observed().is_some() {
                        names_learned += 1;
                    }
                }
                // A stored row is already complete; rebuilding
                // it would only rewrite its thumbnails.
                if store.message(&chat, &id).observed().is_some() {
                    continue;
                }
                if let Some(target) = revoke_target(message) {
                    store.revoke_message(&chat, &target).logged();
                    continue;
                }
                remember_structures(&store, &chat, &id, &sender, message);
                let header = MessageHeader {
                    chat: chat.clone(),
                    id,
                    sender,
                    timestamp: web.message_timestamp.unwrap_or(0) as i64,
                    from_me,
                };
                // History media is never bulk downloaded;
                // it is fetched on demand like any other.
                let Some(mut stored) = stored_message(
                    message,
                    header,
                    client.as_deref(),
                    media_dir.as_deref(),
                    false,
                )
                .await
                else {
                    continue;
                };
                // Old messages must not raise unread counts.
                stored.local.read = true;
                match store.insert_message(&stored) {
                    Ok(()) => {
                        added = true;
                        if message.is_view_once() {
                            store.set_view_once(&chat, &stored.header.id, stored.header.from_me).logged();
                        }
                    }
                    Err(e) => log::error!("could not store history message in {chat}: {e}"),
                }
            }
            if added {
                chats.push(chat);
            }
        }
        if names_learned > 0 {
            let _ = events.send(ServiceEvent::NamesUpdated { count: names_learned });
        }
        let added_chats = chats.len();
        // A request the UI is still waiting on has now been
        // answered, whether or not it brought new rows: an
        // answer with nothing older is an answer, and without
        // it the UI waits out its timeout and blames the phone.
        let answered = sync.peer_data_request_session_id().and_then(|session| {
            older_waits.lock().unwrap().resolve(session)
        });
        if let Some(chat) = answered {
            if !chats.contains(&chat) {
                chats.push(chat);
            }
        }
        // DiskRetention is left to the next live write, so
        // what was just loaded can be seen first. Record
        // the chunk for the readiness gate even when it
        // added nothing, so a stream of no-op chunks does
        // not look quiescent while history is still coming.
        {
            let mut p = sync_progress.lock().unwrap();
            // An answered request is not a conversation the
            // initial window added, so it is not counted.
            p.history_chats += added_chats;
            p.last_progress = Some(std::time::Instant::now());
        }
        if !chats.is_empty() {
            let _ = events.send(ServiceEvent::HistoryLoaded { chats });
        }
        // "Load older" answers are on-demand chunks, not the pairing sync.
        if let Some(percent) = sync.progress().filter(|_| sync.sync_type() != 6) {
            let _ = events.send(ServiceEvent::HistoryProgress { percent });
        }
    }
}
