//! The protocol event handler: live messages and the small state events.

use super::*;
use whatsapp_rust::wacore::types::events::MessageBatch;

/// What the protocol event handler shares with the service, cloned per event.
#[derive(Clone)]
pub(super) struct Inbound {
    pub(super) store: Arc<MessageStore>,
    pub(super) events: broadcast::Sender<ServiceEvent>,
    pub(super) connected: Arc<AtomicBool>,
    pub(super) client_for_events: Arc<std::sync::OnceLock<Arc<Client>>>,
    pub(super) media_dir: Option<PathBuf>,
    pub(super) group_cache: Arc<Mutex<std::collections::HashMap<String, GroupInfo>>>,
    pub(super) groups_cache: Arc<Mutex<Option<Vec<whatsapp_rust::GroupOverview>>>>,
    pub(super) older_waits: Arc<Mutex<OlderWaits>>,
    pub(super) downloads: Arc<tokio::sync::Semaphore>,
    pub(super) sync_progress: Arc<Mutex<SyncProgress>>,
    pub(super) auto_download_default: bool,
    /// Whether a new message keeps an archived chat archived. Off moves it back
    /// to the main list and tells the account, as WhatsApp does.
    pub(super) keep_archived: Arc<AtomicBool>,
}

impl Inbound {
    pub(super) async fn handle(&self, event: &Event) {
        let Self { store, events, connected, client_for_events, media_dir, group_cache, groups_cache, sync_progress, .. } =
            self;
        match event {
            Event::Messages(batch) => self.on_messages(batch).await,
            Event::Disconnected(_) => {
                log::warn!("disconnected");
                connected.store(false, Ordering::SeqCst);
                let _ = events.send(ServiceEvent::Disconnected);
            }
            Event::LoggedOut(reason) => {
                log::warn!("logged out: {reason:?}");
                connected.store(false, Ordering::SeqCst);
                let _ = events.send(ServiceEvent::LoggedOut);
            }
            Event::Receipt(receipt) => self.on_receipt(receipt),
            Event::ServerAck(ack) => self.on_server_ack(ack),
            // The name the user saved for a contact comes from
            // the address book and outranks the push name the
            // contact set for themselves.
            Event::ContactUpdate(update) => {
                let name = update
                    .action
                    .full_name
                    .as_deref()
                    .or(update.action.first_name.as_deref());
                if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
                    store.set_saved_name(&update.jid.to_string(), name).logged();
                }
            }
            Event::ContactRemoved(removed) => {
                store.clear_saved_name(&removed.jid.to_string()).logged();
            }
            // Progress for the initial catch-up, so the UI can
            // show how much of the backlog is still arriving.
            Event::OfflineSyncPreview(preview) => {
                let pending = preview.messages.max(0) as usize;
                log::info!("offline sync: {pending} message(s) pending");
                {
                    let mut p = sync_progress.lock().unwrap();
                    p.pending = pending;
                    p.applied = 0;
                    p.offline_done = false;
                    p.last_emit = None;
                    p.last_progress = Some(std::time::Instant::now());
                }
                if pending > 0 {
                    let _ = events
                        .send(ServiceEvent::Syncing { pending, applied: 0 });
                }
            }
            Event::OfflineSyncCompleted(_) => {
                log::info!("offline sync complete");
                {
                    let mut p = sync_progress.lock().unwrap();
                    p.offline_done = true;
                    p.last_progress = Some(std::time::Instant::now());
                }
                let _ = events.send(ServiceEvent::Synced);
            }
            // The drain ended without its end marker, so the rest
            // redelivers on the next connection. Treat it as an
            // end for the readiness gate rather than waiting for a
            // completion that will not come this connection.
            Event::OfflineSyncInterrupted(interrupted) => {
                let delivered = interrupted.delivered.max(0) as usize;
                log::warn!(
                    "offline sync interrupted after {delivered} message(s); the rest will redeliver"
                );
                {
                    let mut p = sync_progress.lock().unwrap();
                    p.offline_done = true;
                    p.last_progress = Some(std::time::Instant::now());
                }
                let _ = events.send(ServiceEvent::Synced);
            }
            // Pairing's recent window and "load older" answers
            // arrive here, never as `Messages`.
            Event::HistorySync(sync) => self.on_history_sync(sync).await,
            Event::ChatPresence(update) => {
                let state = match (update.state, update.media) {
                    (ChatPresence::Composing, ChatPresenceMedia::Audio) => "recording",
                    (ChatPresence::Composing, _) => "typing",
                    _ => "paused",
                };
                let _ = events.send(ServiceEvent::Typing {
                    chat: canonical_chat(
                        client_for_events.get().map(|c| c.as_ref()),
                        store,
                        &update.source.chat,
                        &update.source.sender,
                        None,
                    )
                    .await,
                    sender: update.source.sender.to_non_ad().to_string(),
                    state: state.to_string(),
                });
            }
            Event::Presence(presence) => {
                // A chat is keyed by phone number; presence may name the LID.
                let mut jid = presence.from.to_non_ad();
                if jid.is_lid() {
                    if let Some(client) = client_for_events.get() {
                        if let Ok(Some(entry)) = client.get_lid_pn_entry(&jid).await {
                            jid = Jid::new(&*entry.phone_number, whatsapp_rust::wacore_binary::Server::Pn);
                        }
                    }
                }
                let _ = events.send(ServiceEvent::Presence {
                    jid: jid.to_string(),
                    online: !presence.unavailable,
                    last_seen: presence.last_seen.map(|t| t.timestamp()),
                });
            }
            Event::IdentityChange(change) => self.on_identity_change(change),
            Event::DeviceListUpdate(update) => self.on_device_change(update),
            Event::PictureUpdate(update) => {
                let jid = update.jid.to_non_ad().to_string();
                if let Some(dir) = media_dir.as_deref() {
                    let path = avatar_path(dir, &jid);
                    let _ = std::fs::remove_file(path.with_extension("none"));
                    let _ = std::fs::remove_file(path);
                    let _ = std::fs::remove_file(avatar_full_path(dir, &jid));
                }
                let _ = events.send(ServiceEvent::AvatarChanged { jid });
            }
            // Linked devices never receive view-once media: the
            // server sends a stub instead, kept as a placeholder
            // that points at the phone.
            Event::UndecryptableMessage(stub)
                if stub.unavailable_type
                    == whatsapp_rust::wacore::types::events::UnavailableType::ViewOnce =>
            {
                let info = &stub.info;
                let chat = canonical_chat(
                    client_for_events.get().map(|c| c.as_ref()),
                    store,
                    &info.source.chat,
                    &info.source.sender,
                    info.source.sender_alt.as_ref(),
                )
                .await;
                let id = info.id.to_string();
                if store.message(&chat, &id).is_ok() {
                    return;
                }
                let from_me = info.source.is_from_me;
                let message = StoredMessage {
                    header: MessageHeader {
                        chat: chat.clone(),
                        id: id.clone(),
                        sender: info.source.sender.to_string(),
                        timestamp: info.timestamp.timestamp(),
                        from_me,
                    },
                    media: Media {
                        kind: Some("view_once".into()),
                        once_kind: info.media_type.as_ref().map(once_kind_of),
                        ..Default::default()
                    },
                    local: LocalState { read: from_me, ..Default::default() },
                    ..Default::default()
                };
                store.set_view_once(&chat, &id, from_me).logged();
                if store.insert_message(&message).is_ok() {
                    let _ = events.send(ServiceEvent::hint(&message, true));
                }
            }
            // Settings, admins, members or the name changed: what we
            // cached about the group (who may send, who is admin) is stale.
            Event::GroupUpdate(update) => {
                use whatsapp_rust::wacore::stanza::groups::GroupNotificationAction;
                let chat = update.group_jid.to_non_ad().to_string();
                log::debug!("group {chat} changed: {:?}", update.action);
                group_cache.lock().unwrap().remove(&chat);
                *groups_cache.lock().unwrap() = None;
                if let GroupNotificationAction::Subject { subject, .. } = update.action.as_ref() {
                    store.set_name(&chat, subject).logged();
                }
                self.on_group_update(update);
                let _ = events.send(ServiceEvent::GroupChanged { chat });
            }
            Event::MissedCall(call) => self.on_missed_call(call),
            Event::UndecryptableMessage(stub) => {
                log::warn!(
                    "could not decrypt message {} in {} from {} ({:?})",
                    stub.info.id,
                    stub.info.source.chat,
                    stub.info.source.sender,
                    stub.unavailable_type,
                );
            }
            // Chat pins are account state; mirror them so the
            // list matches the phone.
            Event::PinUpdate(pin) => {
                let pinned = pin.action.pinned.unwrap_or(false);
                let jid = resolve_chat(client_for_events.get().map(|c| c.as_ref()), store, &pin.jid).await;
                store.set_pinned(&jid, pinned).logged();
                let _ = events.send(ServiceEvent::ChatStateChanged { chat: jid });
            }
            Event::ArchiveUpdate(update) => {
                let archived = update.action.archived.unwrap_or(false);
                let jid = resolve_chat(client_for_events.get().map(|c| c.as_ref()), store, &update.jid).await;
                log::debug!(
                    "archive update for {jid}: archived={archived} full_sync={}",
                    update.from_full_sync
                );
                store.set_archived(&jid, archived).logged();
                let _ = events.send(ServiceEvent::ChatStateChanged { chat: jid });
            }
            Event::MuteUpdate(update) => {
                let jid = resolve_chat(client_for_events.get().map(|c| c.as_ref()), store, &update.jid).await;
                let until = match (update.action.muted.unwrap_or(false), update.action.mute_end_timestamp) {
                    (false, _) => 0,
                    (true, Some(ms)) if ms > 0 => ms / 1000,
                    (true, _) => -1,
                };
                store.set_muted_until(&jid, until).logged();
                let _ = events.send(ServiceEvent::ChatStateChanged { chat: jid });
            }
            Event::MarkChatAsReadUpdate(update) => {
                let jid = resolve_chat(client_for_events.get().map(|c| c.as_ref()), store, &update.jid).await;
                store.set_marked_unread(&jid, !update.action.read.unwrap_or(true)).logged();
                let _ = events.send(ServiceEvent::ChatStateChanged { chat: jid });
            }
            _ => {}
        }
    }

    async fn on_messages(&self, batch: &MessageBatch) {
        let Self { store, events, client_for_events, media_dir, group_cache, downloads, sync_progress, auto_download_default, .. } =
            self;
        let auto_download_default = *auto_download_default;
        let started = std::time::Instant::now();
        let _commit = store.batch();
        let client = client_for_events.get().cloned();
        // Our own addresses, so a mention can be
        // recognised whichever form it uses.
        let own: Vec<String> = client
            .as_deref()
            .map(|c| {
                [c.pn(), c.lid()]
                    .into_iter()
                    .flatten()
                    .map(|j| j.to_non_ad().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut touched: Vec<String> = Vec::with_capacity(batch.messages.len());
        for inbound in batch.messages.iter() {
            // The envelope carries the sender's display
            // name, which is the only name source
            // available without a contacts query.
            let push_name = inbound.info.push_name.to_string();
            let sender = inbound.info.source.sender.to_string();
            let raw_chat = inbound.info.source.chat.to_non_ad().to_string();
            let chat = canonical_chat(
                client.as_deref(),
                store,
                &inbound.info.source.chat,
                &inbound.info.source.sender,
                inbound.info.source.sender_alt.as_ref(),
            )
            .await;
            touched.push(chat.clone());
            // The LID form of a direct chat still holding history folds onto the
            // phone-number form, so a split cannot outlive this message.
            if raw_chat != chat && raw_chat.ends_with("@lid") && store.chat_exists(&raw_chat).unwrap_or(false)
            {
                store.merge_chats(&raw_chat, &chat).logged();
            }
            let is_group = inbound.info.source.is_group
                || chat.ends_with("@g.us");
            let from_me = inbound.info.source.is_from_me;

            // Status updates are not a conversation; keep
            // them out of the store so they never show up
            // as a chat.
            if chat == "status@broadcast" {
                continue;
            }

            // Address-book names are keyed by phone
            // number, but an LID-addressed chat names
            // its sender with a LID, so the two never
            // match on their own. The source carries
            // the other form; copy the name across so
            // the saved one is what gets shown.
            remember_lid_pn(
                &store,
                &inbound.info.source.sender,
                inbound.info.source.sender_alt.as_ref(),
            );
            if let Some(alt) =
                inbound.info.source.sender_alt.as_ref().map(|j| j.to_string())
            {
                let known =
                    store.name_for(&alt).ok().flatten().filter(|n| !is_placeholder_name(n));
                let is_saved = known.is_some();
                // Fall back to the phone number, never
                // the unreadable LID.
                let name = known.unwrap_or_else(|| {
                    alt.split('@').next().unwrap_or(&alt).to_string()
                });
                if is_saved {
                    store.set_saved_name(&sender, &name).logged();
                    if !is_group && !from_me {
                        store.set_saved_name(&chat, &name).logged();
                    }
                } else {
                    // The bare number is only a placeholder;
                    // it must not replace a push name that a
                    // message without one would otherwise erase.
                    let unnamed = |jid: &str| {
                        store.name_for(jid).ok().flatten().is_none()
                    };
                    if unnamed(&sender) {
                        store.set_name(&sender, &name).logged();
                    }
                    if !is_group && !from_me && unnamed(&chat) {
                        store.set_name(&chat, &name).logged();
                    }
                }
            }

            if !push_name.is_empty() {
                // Push names never override a saved one.
                store.set_name(&sender, &push_name).logged();
                // A participant's JID has no device suffix
                // while a message's sender does, so store
                // the bare form too or the group member
                // list cannot find the name.
                if let Some((user, server)) = sender.split_once('@') {
                    let bare = format!(
                        "{}@{}",
                        user.split(':').next().unwrap_or(user),
                        server
                    );
                    if bare != sender {
                        store.set_name(&bare, &push_name).logged();
                    }
                }
                // A one-to-one chat is named after its
                // contact. A group is named by its
                // subject, and a message we sent must
                // never name a chat after us, which is
                // what turned a group into our own name.
                if !is_group && !from_me {
                    store.set_name(&chat, &push_name).logged();
                }
            }

            let base = inbound.message.get_base_message();
            let message_id = inbound.info.id.to_string();
            let author = if from_me {
                own.first().cloned().unwrap_or_else(|| sender.clone())
            } else {
                inbound.info.source.sender.to_non_ad().to_string()
            };
            remember_structures(&store, &chat, &message_id, &author, &inbound.message);

            if let Some(update) = base.poll_update_message.as_option() {
                use whatsapp_rust::wacore::poll::{compute_option_hash, PollVoteCiphertext};
                let poll_id = update.poll_creation_message_key.as_option().and_then(|k| k.id.clone());
                let def = poll_id.as_deref().and_then(|id| store.poll_secret(&chat, id).ok().flatten());
                if let (Some(poll_id), Some(def), Some(vote), Some(client)) = (
                    poll_id,
                    def,
                    update.vote.as_option(),
                    client.as_deref(),
                ) {
                    // The key is derived from the creator's and voter's
                    // addresses, and an LID-addressed group uses the LID
                    // form, so try every form either side is known by.
                    let mut creators = vec![def.creator.clone()];
                    if own.contains(&def.creator) {
                        creators.extend(own.iter().filter(|j| **j != def.creator).cloned());
                    }
                    let mut voters = vec![inbound.info.source.sender.to_non_ad()];
                    if let Some(alt) = inbound.info.source.sender_alt.as_ref() {
                        voters.push(alt.to_non_ad());
                    }
                    if from_me {
                        voters.extend(own.iter().filter_map(|j| j.parse::<Jid>().ok()));
                    }
                    let voter = voters[0].clone();
                    let mut opened = Err(anyhow::anyhow!("no address pair opened it"));
                    'pairs: for creator in creators.iter().filter_map(|c| c.parse::<Jid>().ok()) {
                        for voter in &voters {
                            let cipher = PollVoteCiphertext {
                                enc_payload: vote.enc_payload.as_deref().unwrap_or_default(),
                                enc_iv: vote.enc_iv.as_deref().unwrap_or_default(),
                            };
                            if let Ok(hashes) = client
                                .polls()
                                .decrypt_vote(cipher, &def.secret, &poll_id, &creator, voter)
                                .await
                            {
                                opened = Ok(hashes);
                                break 'pairs;
                            }
                        }
                    }
                    match opened {
                        Ok(hashes) => {
                            let chosen: Vec<String> = def
                                .options
                                .iter()
                                .filter(|o| hashes.iter().any(|h| h.as_slice() == compute_option_hash(o)))
                                .cloned()
                                .collect();
                            let who = if from_me { "@me".to_string() } else { voter.to_string() };
                            store.set_poll_vote(&chat, &poll_id, &who, &chosen).logged();
                            let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                        }
                        Err(e) => log::warn!("could not open a vote on poll {poll_id}: {e}"),
                    }
                }
                continue;
            }

            if let Some(response) = base.enc_event_response_message.as_option() {
                let event_id = response.event_creation_message_key.as_option().and_then(|k| k.id.clone());
                let def = event_id.as_deref().and_then(|id| store.event_secret(&chat, id).ok().flatten());
                if let (Some(event_id), Some(def)) = (event_id, def) {
                    let responder = inbound.info.source.sender.to_non_ad().to_string();
                    // Our own events may have been answered under our other address.
                    let mut creators = vec![def.creator.clone()];
                    if own.contains(&def.creator) {
                        creators.extend(own.iter().filter(|j| **j != def.creator).cloned());
                    }
                    let opened = creators.iter().find_map(|creator| {
                        whatsapp_rust::wacore::event::decrypt_event_response_with_secret(
                            response.enc_payload.as_deref().unwrap_or_default(),
                            response.enc_iv.as_deref().unwrap_or_default(),
                            &def.secret,
                            &event_id,
                            creator,
                            &responder,
                        )
                        .ok()
                    });
                    match opened {
                        Some(answer) => {
                            let who = if from_me { "@me".to_string() } else { responder };
                            store.set_event_response(&chat, &event_id, &who, response_name(answer.response)).logged();
                            let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                        }
                        None => log::warn!("could not open an RSVP to event {event_id}"),
                    }
                }
                continue;
            }

            if let Some(reaction) = base.reaction_message.as_option() {
                if let Some(target) =
                    reaction.key.as_option().and_then(|k| k.id.clone())
                {
                    let who = if from_me {
                        "@me".to_string()
                    } else {
                        inbound.info.source.sender.to_non_ad().to_string()
                    };
                    let emoji = reaction.text.clone().unwrap_or_default();
                    store.set_reaction(&chat, &target, &who, &emoji).logged();
                    let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                }
                continue;
            }
            if let Some(pin) = base.pin_in_chat_message.as_option() {
                use wa::message::pin_in_chat_message::Type;
                let target = pin.key.as_option().and_then(|k| k.id.clone());
                let pinned = pin.r#type == Some(Type::PIN_FOR_ALL);
                store
                    .set_message_pin(&chat, target.as_deref().filter(|_| pinned))
                    .logged();
                let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                continue;
            }

            if let Some(label) = member_label_change(&inbound.message) {
                let member =
                    inbound.info.source.sender.to_non_ad().to_string();
                if let Some(info) = group_cache.lock().unwrap().get_mut(&chat) {
                    if let Some(p) =
                        info.participants.iter_mut().find(|p| p.jid == member)
                    {
                        p.label = (!label.is_empty()).then(|| label.clone());
                    }
                }
                let _ = events.send(ServiceEvent::MemberLabel {
                    chat: chat.clone(),
                    jid: member,
                    label,
                });
                continue;
            }

            // A revoke is a protocol message naming the
            // original; mark it deleted rather than
            // dropping the notice, so the chat shows
            // that something was removed.
            if let Some(target) = revoke_target(&inbound.message) {
                if let Ok(true) = store.revoke_message(&chat, &target) {
                    if let Ok(updated) =
                        store.message(&chat, &target)
                    {
                        let _ = events.send(ServiceEvent::hint(&updated, false));
                    }
                }
                continue;
            }

            if let Some((target, text)) = edit_of(&inbound.message) {
                if let Ok(true) = store.update_message_content(&chat, &target, &text) {
                    if let Ok(updated) = store.message(&chat, &target) {
                        let _ = events.send(ServiceEvent::hint(&updated, false));
                    }
                    let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                }
                continue;
            }

            // Stickers and voice notes are small and read as part
            // of the conversation, so WhatsApp always fetches them.
            let base = inbound.message.get_base_message();
            let small = base.sticker_message.is_set()
                || base.audio_message.as_option().is_some_and(|a| a.ptt == Some(true));
            let auto_download = small
                || store
                    .chat_auto_download(&chat)
                    .ok()
                    .flatten()
                    .unwrap_or(auto_download_default);
            let Some(mut message) =
                incoming_message(&chat, inbound, client.as_deref(), media_dir.as_deref(), false)
                    .await
            else {
                continue;
            };
            // Mentions stay `@<number>` as on the wire; the UI
            // resolves them when drawn, so later names apply.
            message.local.mentioned = mentions_me(&inbound.message, &own);
            if inbound.message.is_view_once() {
                store.set_view_once(&chat, &message.header.id, from_me).logged();
            }
            if is_forwarded(&inbound.message) {
                store.set_forwarded(&chat, &message.header.id).logged();
            }
            // Pairing only brings recent days; a reply to something
            // older pulls that chat's past so the quote can be opened.
            if let (Some(quoted), Some(client)) = (message.quote.id.clone(), client.clone()) {
                let quoted_chat = message.quote.chat.clone().unwrap_or_else(|| chat.clone());
                if store.message(&quoted_chat, &quoted).is_err() && recall_allowed(&quoted_chat) {
                    let store = store.clone();
                    tokio::spawn(async move {
                        fetch_older(&client, &store, &quoted_chat, 50).await.logged();
                    });
                }
            }
            if let Err(e) = store.insert_message(&message) {
                log::error!("could not store message {} in {chat}: {e}", message.header.id);
            } else {
                // Count backlog progress so the loading
                // screen's bar tracks stored messages, not
                // raw inbound events.
                {
                    let mut p = sync_progress.lock().unwrap();
                    if p.pending > 0 && !p.offline_done {
                        p.applied = (p.applied + 1).min(p.pending);
                        p.last_progress = Some(std::time::Instant::now());
                        let emit = p.applied >= p.pending
                            || p.last_emit.map_or(true, |t| {
                                t.elapsed()
                                    >= std::time::Duration::from_millis(50)
                            });
                        if emit {
                            p.last_emit = Some(std::time::Instant::now());
                            let (pending, applied) = (p.pending, p.applied);
                            drop(p);
                            let _ = events.send(ServiceEvent::Syncing {
                                pending,
                                applied,
                            });
                        }
                    }
                }
                // A new incoming message moves an archived chat back to the main
                // list unless the account keeps archived chats archived. The
                // account is told too, so the phone cannot re-archive it later.
                if !from_me
                    && !self.keep_archived.load(Ordering::SeqCst)
                    && store.is_archived(&chat).unwrap_or(false)
                {
                    store.set_archived(&chat, false).logged();
                    let _ = events.send(ServiceEvent::ChatStateChanged { chat: chat.clone() });
                    if let (Some(client), Ok(jid)) = (client.clone(), chat.parse::<Jid>()) {
                        tokio::spawn(async move {
                            if let Err(e) =
                                client.chat_actions().unarchive_chat(&jid, None).await
                            {
                                log::warn!("could not unarchive {jid}: {e}");
                            }
                        });
                    }
                }
            }
            let fetch = match (auto_download && message.media.locator.is_some(), &client, &media_dir) {
                (true, Some(client), Some(dir)) => {
                    Some((client.clone(), dir.clone(), message.header.id.clone()))
                }
                _ => None,
            };
            let _ = events.send(ServiceEvent::hint(&message, true));
            if let Some((client, dir, id)) = fetch {
                let (store, events, downloads, chat) =
                    (store.clone(), events.clone(), downloads.clone(), chat.clone());
                tokio::spawn(async move {
                    let Ok(_permit) = downloads.acquire().await else { return };
                    match fetch_media(&client, &store, &dir, &chat, &id).await {
                        Ok(updated) => {
                            // Non-fresh: the row refetches coalesced, no follow or lookup.
                            let _ = events.send(ServiceEvent::hint(&updated, false));
                        }
                        Err(e) => log::warn!("failed to download {id} media: {e}"),
                    }
                });
            }
        }
        // Bound the store right after writes so the
        // limit holds even if the process stops.
        let pruning = std::time::Instant::now();
        touched.sort_unstable();
        touched.dedup();
        let removed = match store.enforce_retention_for(&touched) {
            Ok(removed) => removed,
            Err(e) => {
                log::error!("retention failed: {e}");
                0
            }
        };
        if removed > 0 {
            let _ = events.send(ServiceEvent::RetentionApplied { removed });
            // A pruned reply can be the last one naming a recovered view-once.
            if let Err(e) = prune_quote_files(media_dir.as_deref(), store) {
                log::error!("pruning recovered view-once files failed: {e}");
            }
        }
        log::debug!(
            "{} live message(s) in {:?} (retention {:?}, pruned {removed})",
            batch.messages.len(),
            started.elapsed(),
            pruning.elapsed(),
        );
    }
}
