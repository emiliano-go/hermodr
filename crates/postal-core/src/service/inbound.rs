//! The protocol event handler: live messages and the small state events.

use super::*;
use std::sync::atomic::AtomicUsize;
use whatsapp_rust::wacore::types::events as wa_events;
use whatsapp_rust::wacore::types::events::MessageBatch;

/// What the protocol event handler shares with the service, cloned per event.
#[derive(Clone)]
pub(super) struct Inbound {
    pub(super) store: StoreWorker,
    pub(super) disk_retention: Arc<DiskRetentionManager>,
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
    /// Whether a fetchable view-once is downloaded at once and kept as an
    /// ordinary attachment instead of one-time.
    pub(super) keep_view_once: Arc<AtomicBool>,
    /// Whether this link exists only for one-time media: only view-once
    /// messages are ingested, and every other store change is skipped.
    pub(super) one_time_only: bool,
    /// What a companion wake has read and kept, for its one summary line.
    pub(super) tally: Arc<CompanionTally>,
}

/// Counters behind the companion's catch-up summary.
#[derive(Default)]
pub(super) struct CompanionTally {
    seen: AtomicUsize,
    processed: AtomicUsize,
}

impl CompanionTally {
    fn note(&self, seen: usize, processed: usize) {
        self.seen.fetch_add(seen, Ordering::Relaxed);
        self.processed.fetch_add(processed, Ordering::Relaxed);
    }

    fn take(&self) -> (usize, usize) {
        (
            self.seen.swap(0, Ordering::Relaxed),
            self.processed.swap(0, Ordering::Relaxed),
        )
    }
}

/// Our own addresses, so a mention can be recognised whichever form it uses.
fn own_addresses(client: Option<&Client>) -> Vec<String> {
    client
        .map(|c| {
            [c.pn(), c.lid()]
                .into_iter()
                .flatten()
                .map(|j| j.to_non_ad().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// One message batch's shared state. The store is the batch handle, so every
/// write in the loop commits together.
struct BatchCtx<'a> {
    store: &'a StoreWorker,
    client: Option<Arc<Client>>,
    own: Vec<String>,
    media_dir: Option<PathBuf>,
    touched: Vec<String>,
}

/// One inbound message's resolved identity, shared by every step below.
struct Incoming {
    chat: String,
    sender: String,
    from_me: bool,
    id: String,
}


/// Whether the one-time companion cares about an event: the view-once it
/// keeps, its own link state, and the drain markers its summary hangs off.
fn one_time_relevant(event: &Event) -> bool {
    match event {
        Event::Messages(_)
        | Event::Disconnected(_)
        | Event::LoggedOut(_)
        | Event::OfflineSyncPreview(_)
        | Event::OfflineSyncCompleted(_) => true,
        Event::UndecryptableMessage(stub) => {
            stub.unavailable_type == whatsapp_rust::wacore::types::events::UnavailableType::ViewOnce
        }
        _ => false,
    }
}

impl Inbound {
    pub(super) async fn handle(&self, event: &Event) {
        if self.one_time_only && !one_time_relevant(event) {
            return;
        }
        match event {
            Event::Messages(batch) => self.on_messages(batch).await,
            Event::Disconnected(_) => self.on_disconnected(),
            Event::LoggedOut(reason) => self.on_logged_out(reason),
            Event::Receipt(receipt) => self.on_receipt(receipt).await,
            Event::ServerAck(ack) => self.on_server_ack(ack).await,
            Event::ContactUpdate(update) => self.on_contact_update(update).await,
            Event::ContactRemoved(removed) => self.on_contact_removed(removed).await,
            Event::OfflineSyncPreview(preview) => self.on_sync_preview(preview),
            Event::OfflineSyncCompleted(_) => self.on_sync_completed(),
            Event::OfflineSyncInterrupted(interrupted) => self.on_sync_interrupted(interrupted),
            Event::HistorySync(sync) => self.on_history_sync(sync).await,
            Event::ChatPresence(update) => self.on_chat_presence(update).await,
            Event::Presence(presence) => self.on_presence(presence).await,
            Event::IdentityChange(change) => self.on_identity_change(change).await,
            Event::DeviceListUpdate(update) => self.on_device_change(update).await,
            Event::PictureUpdate(update) => self.on_picture_update(update).await,
            Event::UndecryptableMessage(stub)
                if stub.unavailable_type
                    == whatsapp_rust::wacore::types::events::UnavailableType::ViewOnce =>
            {
                self.on_view_once_stub(stub).await
            }
            Event::GroupUpdate(update) => self.on_group_changed(update).await,
            Event::MissedCall(call) => self.on_missed_call(call).await,
            Event::UndecryptableMessage(stub) => self.on_undecryptable(stub),
            Event::PinUpdate(pin) => self.on_pin_update(pin).await,
            Event::ArchiveUpdate(update) => self.on_archive_update(update).await,
            Event::MuteUpdate(update) => self.on_mute_update(update).await,
            Event::MarkChatAsReadUpdate(update) => self.on_mark_read_update(update).await,
            Event::FavoriteStickerUpdate(update) => {
                self.on_sticker_favorite(
                    &update.filehash,
                    update.action.is_favorite.unwrap_or(false),
                    update.timestamp.timestamp(),
                    &update.action,
                )
                .await;
            }
            Event::RemoveRecentStickerUpdate(update) => {
                self.on_sticker_recent_removed(&update.filehash, update.timestamp.timestamp()).await;
            }
            _ => {}
        }
    }

    fn on_disconnected(&self) {
        log::warn!("disconnected");
        self.connected.store(false, Ordering::SeqCst);
        let _ = self.events.send(ServiceEvent::Disconnected);
    }

    fn on_logged_out(&self, reason: &wa_events::LoggedOut) {
        log::warn!("logged out: {reason:?}");
        self.connected.store(false, Ordering::SeqCst);
        let _ = self.events.send(ServiceEvent::LoggedOut);
    }

    /// The name the user saved for a contact comes from the address book and
    /// outranks the push name the contact set for themselves.
    async fn on_contact_update(&self, update: &wa_events::ContactUpdate) {
        let name = update
            .action
            .full_name
            .as_deref()
            .or(update.action.first_name.as_deref());
        if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
            self.store.set_saved_name(&update.jid.to_string(), name).await.logged();
        }
    }

    async fn on_contact_removed(&self, removed: &wa_events::ContactRemoved) {
        self.store.clear_saved_name(&removed.jid.to_string()).await.logged();
    }

    /// Progress for the initial catch-up, so the UI can show how much of the
    /// backlog is still arriving.
    fn on_sync_preview(&self, preview: &wa_events::OfflineSyncPreview) {
        let pending = preview.messages.max(0) as usize;
        log::info!("offline sync: {pending} message(s) pending");
        if self.one_time_only {
            self.tally.take();
        }
        {
            let mut p = self.sync_progress.lock().unwrap();
            p.pending = pending;
            p.applied = 0;
            p.offline_done = false;
            p.last_emit = None;
            p.last_progress = Some(std::time::Instant::now());
        }
        if pending > 0 {
            let _ = self.events.send(ServiceEvent::Syncing { pending, applied: 0 });
        }
    }

    fn on_sync_completed(&self) {
        if self.one_time_only {
            let (seen, processed) = self.tally.take();
            log::info!("companion catch-up: read {seen} message(s), {processed} one-time ingested");
        }
        log::info!("offline sync complete");
        {
            let mut p = self.sync_progress.lock().unwrap();
            p.offline_done = true;
            p.last_progress = Some(std::time::Instant::now());
        }
        let _ = self.events.send(ServiceEvent::Synced);
    }

    /// The drain ended without its end marker, so the rest redelivers on the
    /// next connection. Treat it as an end for the readiness gate rather than
    /// waiting for a completion that will not come this connection.
    fn on_sync_interrupted(&self, interrupted: &wa_events::OfflineSyncInterrupted) {
        let delivered = interrupted.delivered.max(0) as usize;
        log::warn!("offline sync interrupted after {delivered} message(s); the rest will redeliver");
        {
            let mut p = self.sync_progress.lock().unwrap();
            p.offline_done = true;
            p.last_progress = Some(std::time::Instant::now());
        }
        let _ = self.events.send(ServiceEvent::Synced);
    }

    async fn on_chat_presence(&self, update: &wa_events::ChatPresenceUpdate) {
        let state = match (update.state, update.media) {
            (ChatPresence::Composing, ChatPresenceMedia::Audio) => "recording",
            (ChatPresence::Composing, _) => "typing",
            _ => "paused",
        };
        let _ = self.events.send(ServiceEvent::Typing {
            chat: canonical_chat(
                self.client_for_events.get().map(|c| c.as_ref()),
                &self.store,
                &update.source.chat,
                &update.source.sender,
                None,
            )
            .await,
            sender: update.source.sender.to_non_ad().to_string(),
            state: state.to_string(),
        });
    }

    async fn on_presence(&self, presence: &wa_events::PresenceUpdate) {
        // A chat is keyed by phone number; presence may name the LID.
        let mut jid = presence.from.to_non_ad();
        if jid.is_lid() {
            if let Some(client) = self.client_for_events.get() {
                if let Some(Some(entry)) = client.get_lid_pn_entry(&jid).await.observed() {
                    jid = Jid::new(&*entry.phone_number, whatsapp_rust::wacore_binary::Server::Pn);
                }
            }
        }
        let _ = self.events.send(ServiceEvent::Presence {
            jid: jid.to_string(),
            online: !presence.unavailable,
            last_seen: presence.last_seen.map(|t| t.timestamp()),
        });
    }

    async fn on_picture_update(&self, update: &wa_events::PictureUpdate) {
        let jid = update.jid.to_non_ad().to_string();
        if let Some(dir) = self.media_dir.as_deref() {
            let path = avatar_path(dir, &jid);
            remove_cached_file(path.with_extension("none"));
            remove_cached_file(path);
            remove_cached_file(avatar_full_path(dir, &jid));
        }
        if update.jid.is_group() {
            let at = update.timestamp.timestamp();
            let id = format!("group-picture-{at}-{}-{}", update.picture_id.as_deref().unwrap_or("removed"), update.removed);
            let author = update.author.as_ref().map(|jid| jid.to_non_ad().to_string()).unwrap_or_default();
            self.store_notice(&jid, id, at, "GROUP_CHANGE_ICON", vec![], author).await;
        }
        let _ = self.events.send(ServiceEvent::AvatarChanged { jid });
    }

    /// Linked devices never receive view-once media: the server sends a stub
    /// instead, kept as a placeholder that points at the phone.
    async fn on_view_once_stub(&self, stub: &wa_events::UndecryptableMessage) {
        let info = &stub.info;
        let chat = canonical_chat(
            self.client_for_events.get().map(|c| c.as_ref()),
            &self.store,
            &info.source.chat,
            &info.source.sender,
            info.source.sender_alt.as_ref(),
        )
        .await;
        let id = info.id.to_string();
        if self.store.message(&chat, &id).await.observed().is_some() {
            return;
        }
        log::debug!("view-once in {chat}: arrived as a bare stub (no media)");
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
        self.store.set_view_once(&chat, &id, from_me).await.logged();
        if self.store.insert_message(&message).await.observed().is_some() {
            let _ = self.events.send(ServiceEvent::hint(&message, true));
        }
    }

    /// Settings, admins, members or the name changed: what we cached about the
    /// group (who may send, who is admin) is stale.
    async fn on_group_changed(&self, update: &wa_events::GroupUpdate) {
        use whatsapp_rust::wacore::stanza::groups::GroupNotificationAction;
        let chat = update.group_jid.to_non_ad().to_string();
        log::debug!("group {chat} changed: {:?}", update.action);
        let previous = self.group_cache.lock().unwrap().remove(&chat);
        let community = previous.as_ref().is_some_and(|info| info.community)
            || self.groups_cache.lock().unwrap().as_ref().is_some_and(|groups|
                groups.iter().any(|group| group.id.to_non_ad().to_string() == chat && group.is_parent_group()));
        *self.groups_cache.lock().unwrap() = None;
        if let GroupNotificationAction::Subject { subject, subject_owner, subject_owner_pn, .. } = update.action.as_ref() {
            if let Some(owner) = subject_owner {
                remember_lid_pn(&self.store, owner, subject_owner_pn.as_ref()).await;
            }
            self.store.set_name(&chat, subject).await.logged();
        }
        self.on_group_update(update, community).await;
        self.check_community_owner(update, previous.as_ref());
        let _ = self.events.send(ServiceEvent::GroupChanged { chat });
    }

    fn on_undecryptable(&self, stub: &wa_events::UndecryptableMessage) {
        log::warn!(
            "could not decrypt message {} in {} from {} ({:?})",
            stub.info.id,
            stub.info.source.chat,
            stub.info.source.sender,
            stub.unavailable_type,
        );
    }

    /// Chat pins are account state; mirror them so the list matches the phone.
    async fn on_pin_update(&self, pin: &wa_events::PinUpdate) {
        let pinned = pin.action.pinned.unwrap_or(false);
        let jid = resolve_chat(self.client_for_events.get().map(|c| c.as_ref()), &self.store, &pin.jid).await;
        self.store.set_pinned(&jid, pinned).await.logged();
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: jid });
    }

    async fn on_archive_update(&self, update: &wa_events::ArchiveUpdate) {
        let archived = update.action.archived.unwrap_or(false);
        let jid = resolve_chat(self.client_for_events.get().map(|c| c.as_ref()), &self.store, &update.jid).await;
        log::debug!(
            "archive update for {jid}: archived={archived} full_sync={}",
            update.from_full_sync
        );
        self.store.set_archived(&jid, archived).await.logged();
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: jid });
    }

    async fn on_mute_update(&self, update: &wa_events::MuteUpdate) {
        let jid = resolve_chat(self.client_for_events.get().map(|c| c.as_ref()), &self.store, &update.jid).await;
        let until = match (update.action.muted.unwrap_or(false), update.action.mute_end_timestamp) {
            (false, _) => 0,
            (true, Some(ms)) if ms > 0 => ms / 1000,
            (true, _) => -1,
        };
        self.store.set_muted_until(&jid, until).await.logged();
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: jid });
    }

    async fn on_mark_read_update(&self, update: &wa_events::MarkChatAsReadUpdate) {
        let jid = resolve_chat(self.client_for_events.get().map(|c| c.as_ref()), &self.store, &update.jid).await;
        let read = update.action.read.unwrap_or(true);
        self.store.set_marked_unread(&jid, !read).await.logged();
        if read {
            // Another device read the chat; clear the messages here too,
            // or the unread badge stays though nothing is unseen.
            let through = update
                .action
                .message_range
                .as_option()
                .and_then(|range| range.last_message_timestamp);
            let changed = match through {
                Some(ts) => self.store.mark_read_through(&jid, ts).await.observed().unwrap_or(0),
                None => self.store.mark_read(&jid).await.observed().unwrap_or(0),
            };
            log::debug!("chat read on another device: {jid} ({changed} message(s))");
        }
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: jid });
    }

    async fn on_messages(&self, batch: &MessageBatch) {
        let started = std::time::Instant::now();
        let mut ingested = 0usize;
        let batch_guard = self.store.batch().await;
        let client = self.client_for_events.get().cloned();
        let own = own_addresses(client.as_deref());
        let mut ctx = BatchCtx {
            store: &batch_guard,
            client,
            own,
            media_dir: self.media_dir.clone(),
            touched: Vec::with_capacity(batch.messages.len()),
        };
        for inbound in batch.messages.iter() {
            // A companion keeps view-once media only; everything else belongs to
            // the main link, which stored it when it arrived.
            if self.one_time_only {
                if !inbound.message.is_view_once() {
                    continue;
                }
                ingested += 1;
            }
            let Some(incoming) = self.resolve_incoming(inbound, &ctx).await else {
                continue;
            };
            ctx.touched.push(incoming.chat.clone());
            let author = if incoming.from_me {
                ctx.own.first().cloned().unwrap_or_else(|| incoming.sender.clone())
            } else {
                inbound.info.source.sender.to_non_ad().to_string()
            };
            remember_structures(ctx.store, &incoming.chat, &incoming.id, &author, &inbound.message).await;
            if self.apply_control(&ctx, inbound, &incoming).await {
                continue;
            }
            self.store_incoming(&ctx, inbound, &incoming).await;
        }
        let (removed, pruning) = self.enforce_retention(&mut ctx).await;
        if self.one_time_only {
            self.tally.note(batch.messages.len(), ingested);
        }
        batch_guard.finish().await.logged();
        log::debug!(
            "{} live message(s) in {:?} (retention {:?}, pruned {removed})",
            batch.messages.len(),
            started.elapsed(),
            pruning.elapsed(),
        );
    }

    /// Resolves a stanza's chat, sender and flags, recording the names it
    /// carries. `None` for a broadcast status, which is not a conversation.
    async fn resolve_incoming(&self, inbound: &InboundMessage, ctx: &BatchCtx<'_>) -> Option<Incoming> {
        let push_name = inbound.info.push_name.to_string();
        let sender = inbound.info.source.sender.to_string();
        // The batch holds the store's write lease, so LIDs resolve from what is
        // already known; an unmapped one keeps its LID form and is looked up
        // and folded off the batch.
        let chat = canonical_chat(
            None,
            ctx.store,
            &inbound.info.source.chat,
            &inbound.info.source.sender,
            inbound.info.source.sender_alt.as_ref(),
        )
        .await;
        if chat == "status@broadcast" {
            return None;
        }
        if chat.ends_with("@lid") {
            spawn_lid_lookup(ctx.client.clone(), ctx.store, &chat);
        }
        let is_group = inbound.info.source.is_group || chat.ends_with("@g.us");
        let from_me = inbound.info.source.is_from_me;
        // Address-book names are keyed by phone number, but an LID-addressed
        // chat names its sender with a LID, so the two never match on their
        // own. The source carries the other form; copy the name across so the
        // saved one is what gets shown.
        remember_lid_pn(ctx.store, &inbound.info.source.sender, inbound.info.source.sender_alt.as_ref()).await;
        if let Some(alt) = inbound.info.source.sender_alt.as_ref().map(|j| j.to_string()) {
            self.remember_alt_name(ctx, &sender, &chat, is_group, from_me, &alt).await;
        }
        if !push_name.is_empty() {
            self.remember_push_name(ctx, &sender, &chat, is_group, from_me, &push_name).await;
        }
        Some(Incoming {
            chat,
            sender,
            from_me,
            id: inbound.info.id.to_string(),
        })
    }

    /// Copies the address-book name for the sender's other address form onto
    /// the form this message uses.
    async fn remember_alt_name(
        &self,
        ctx: &BatchCtx<'_>,
        sender: &str,
        chat: &str,
        is_group: bool,
        from_me: bool,
        alt: &str,
    ) {
        let known = ctx.store.name_for(alt).await.observed().flatten().filter(|n| !is_placeholder_name(n));
        let is_saved = known.is_some();
        // Fall back to the phone number, never the unreadable LID.
        let name = known.unwrap_or_else(|| alt.split('@').next().unwrap_or(alt).to_string());
        if is_saved {
            ctx.store.set_saved_name(sender, &name).await.logged();
            if !is_group && !from_me {
                ctx.store.set_saved_name(chat, &name).await.logged();
            }
            return;
        }
        // The bare number is only a placeholder; it must not replace a push
        // name that a message without one would otherwise erase.
        if ctx.store.name_for(sender).await.observed().flatten().is_none() {
            ctx.store.set_name(sender, &name).await.logged();
        }
        if !is_group && !from_me && ctx.store.name_for(chat).await.observed().flatten().is_none() {
            ctx.store.set_name(chat, &name).await.logged();
        }
    }

    /// Records the push name a message carries, on the sender and, for a direct
    /// chat, on the chat itself.
    async fn remember_push_name(
        &self,
        ctx: &BatchCtx<'_>,
        sender: &str,
        chat: &str,
        is_group: bool,
        from_me: bool,
        push_name: &str,
    ) {
        // Push names never override a saved one.
        ctx.store.set_name(sender, push_name).await.logged();
        // A participant's JID has no device suffix while a message's sender
        // does, so store the bare form too or the group member list cannot find
        // the name.
        if let Some((user, server)) = sender.split_once('@') {
            let bare = format!("{}@{}", user.split(':').next().unwrap_or(user), server);
            if bare != sender {
                ctx.store.set_name(&bare, push_name).await.logged();
            }
        }
        // A one-to-one chat is named after its contact. A group is named by its
        // subject, and a message we sent must never name a chat after us.
        if !is_group && !from_me {
            ctx.store.set_name(chat, push_name).await.logged();
        }
    }

    /// Applies a message that is state for something else rather than a message
    /// of its own: a vote, an RSVP, a reaction, a pin, a label, a revoke, an
    /// edit or a sticker pack. Returns whether it was one.
    async fn apply_control(&self, ctx: &BatchCtx<'_>, inbound: &InboundMessage, incoming: &Incoming) -> bool {
        let base = inbound.message.get_base_message();
        if let Some(protocol) = base.protocol_message.as_option()
            .filter(|protocol| protocol.r#type == Some(wa::message::protocol_message::Type::EPHEMERAL_SETTING))
        {
            let Some(expiration) = protocol.ephemeral_expiration else { return true };
            let mut row = system_row(&incoming.chat, incoming.id.clone(), inbound.info.timestamp.timestamp(),
                "CHANGE_EPHEMERAL_SETTING".into(), vec![expiration.to_string()]);
            row.header.sender = inbound.info.source.sender.to_non_ad().to_string();
            if ctx.store.insert_message(&row).await.observed().is_some() {
                let _ = self.events.send(ServiceEvent::hint(&row, false));
            }
            return true;
        }
        if base.poll_update_message.as_option().is_some() {
            self.apply_poll_vote(ctx, inbound, incoming).await;
            return true;
        }
        if base.enc_event_response_message.as_option().is_some() {
            self.apply_event_response(ctx, inbound, incoming).await;
            return true;
        }
        if base.reaction_message.as_option().is_some() {
            self.apply_reaction(ctx, &incoming.chat, incoming.from_me, inbound).await;
            return true;
        }
        if base.pin_in_chat_message.as_option().is_some() {
            self.apply_message_pin(ctx, &incoming.chat, base).await;
            return true;
        }
        if let Some(label) = member_label_change(&inbound.message) {
            self.apply_label_change(&incoming.chat, &inbound.info.source.sender.to_non_ad().to_string(), label);
            return true;
        }
        if let Some(target) = revoke_target(&inbound.message) {
            self.apply_revoke(ctx, &incoming.chat, &target).await;
            return true;
        }
        if let Some((target, update)) =
            live_location_edit_of(&inbound.message, inbound.info.timestamp.timestamp())
        {
            self.apply_live_location_edit(ctx, &incoming.chat, &target, update).await;
            return true;
        }
        if let Some((target, text)) = edit_of(&inbound.message) {
            self.apply_edit(ctx, &incoming.chat, &target, &text).await;
            return true;
        }
        if base.sticker_pack_message.is_set() {
            self.on_sticker_pack(&inbound.message).await;
            return true;
        }
        false
    }

    /// A vote on a poll. The key is derived from the creator's and voter's
    /// addresses, and an LID-addressed group uses the LID form, so every form
    /// either side is known by is tried.
    async fn apply_poll_vote(&self, ctx: &BatchCtx<'_>, inbound: &InboundMessage, incoming: &Incoming) {
        use whatsapp_rust::wacore::poll::{compute_option_hash, PollVoteCiphertext};
        let Some(update) = inbound.message.get_base_message().poll_update_message.as_option() else { return };
        let chat = &incoming.chat;
        let poll_id = update.poll_creation_message_key.as_option().and_then(|k| k.id.clone());
        let def = match poll_id.as_deref() { Some(id) => ctx.store.poll_secret(chat, id).await.observed().flatten(), None => None };
        let Some((poll_id, def, vote)) = poll_id.zip(def).zip(update.vote.as_option()).map(|((id, def), vote)| (id, def, vote)).filter(|_| ctx.client.is_some()) else {
            return;
        };
        let client = ctx.client.as_deref().expect("checked above");
        let mut creators = vec![def.creator.clone()];
        if ctx.own.contains(&def.creator) {
            creators.extend(ctx.own.iter().filter(|j| **j != def.creator).cloned());
        }
        let mut voters = vec![inbound.info.source.sender.to_non_ad()];
        if let Some(alt) = inbound.info.source.sender_alt.as_ref() {
            voters.push(alt.to_non_ad());
        }
        if incoming.from_me {
            voters.extend(ctx.own.iter().filter_map(|j| j.parse::<Jid>().ok()));
        }
        let voter = voters[0].clone();
        let mut opened = Err(anyhow::anyhow!("no address pair opened it"));
        'pairs: for creator in creators.iter().filter_map(|c| c.parse::<Jid>().ok()) {
            for voter in &voters {
                let cipher = PollVoteCiphertext {
                    enc_payload: vote.enc_payload.as_deref().unwrap_or_default(),
                    enc_iv: vote.enc_iv.as_deref().unwrap_or_default(),
                };
                if let Ok(hashes) = client.polls().decrypt_vote(cipher, &def.secret, &poll_id, &creator, voter).await {
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
                let who = if incoming.from_me { "@me".to_string() } else { voter.to_string() };
                ctx.store.set_poll_vote(chat, &poll_id, &who, &chosen).await.logged();
                let _ = self.events.send(ServiceEvent::Marks { chat: chat.clone() });
            }
            Err(e) => log::warn!("could not open a vote on poll {poll_id}: {e}"),
        }
    }

    /// An RSVP to an event. Our own events may have been answered under our
    /// other address, so every creator form is tried.
    async fn apply_event_response(&self, ctx: &BatchCtx<'_>, inbound: &InboundMessage, incoming: &Incoming) {
        let Some(response) = inbound.message.get_base_message().enc_event_response_message.as_option() else { return };
        let chat = &incoming.chat;
        let event_id = response.event_creation_message_key.as_option().and_then(|k| k.id.clone());
        let def = match event_id.as_deref() { Some(id) => ctx.store.event_secret(chat, id).await.observed().flatten(), None => None };
        let Some((event_id, def)) = event_id.zip(def) else { return };
        let responder = inbound.info.source.sender.to_non_ad().to_string();
        let mut creators = vec![def.creator.clone()];
        if ctx.own.contains(&def.creator) {
            creators.extend(ctx.own.iter().filter(|j| **j != def.creator).cloned());
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
                let who = if incoming.from_me { "@me".to_string() } else { responder };
                ctx.store.set_event_response(chat, &event_id, &who, response_name(answer.response)).await.logged();
                let _ = self.events.send(ServiceEvent::Marks { chat: chat.clone() });
            }
            None => log::warn!("could not open an RSVP to event {event_id}"),
        }
    }

    async fn apply_reaction(&self, ctx: &BatchCtx<'_>, chat: &str, from_me: bool, inbound: &InboundMessage) {
        let Some(reaction) = inbound.message.get_base_message().reaction_message.as_option() else { return };
        let Some(target) = reaction.key.as_option().and_then(|k| k.id.clone()) else { return };
        let who = if from_me {
            "@me".to_string()
        } else {
            inbound.info.source.sender.to_non_ad().to_string()
        };
        let emoji = reaction.text.clone().unwrap_or_default();
        ctx.store.set_reaction(chat, &target, &who, &emoji).await.logged();
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
    }

    async fn apply_message_pin(&self, ctx: &BatchCtx<'_>, chat: &str, base: &wa::Message) {
        use wa::message::pin_in_chat_message::Type;
        let Some(pin) = base.pin_in_chat_message.as_option() else { return };
        let target = pin.key.as_option().and_then(|k| k.id.clone());
        let pinned = pin.r#type == Some(Type::PIN_FOR_ALL);
        ctx.store.set_message_pin(chat, target.as_deref().filter(|_| pinned)).await.logged();
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
    }

    /// A member label change updates the cached participant and tells the UI.
    fn apply_label_change(&self, chat: &str, member: &str, label: String) {
        if let Some(info) = self.group_cache.lock().unwrap().get_mut(chat) {
            if let Some(p) = info.participants.iter_mut().find(|p| p.jid == member) {
                p.label = (!label.is_empty()).then(|| label.clone());
            }
        }
        let _ = self.events.send(ServiceEvent::MemberLabel {
            chat: chat.to_string(),
            jid: member.to_string(),
            label,
        });
    }

    /// A revoke is a protocol message naming the original; mark it revoked
    /// rather than dropping the notice. The local text and media stay and the
    /// UI greys the bubble out, so nothing becomes unavailable here. Stopping
    /// a live location arrives this way, and keeps its last position instead.
    async fn apply_revoke(&self, ctx: &BatchCtx<'_>, chat: &str, target: &str) {
        if let Some(existing) = ctx.store.message(chat, target).await.observed() {
            if existing.media.kind.as_deref() == Some("live_location") {
                if ctx.store.end_live_location(chat, target).await.observed() == Some(true) {
                    if let Some(updated) = ctx.store.message(chat, target).await.observed() {
                        let _ = self.events.send(ServiceEvent::hint(&updated, false));
                    }
                }
                return;
            }
        }
        if let Some(true) = ctx.store.revoke_message(chat, target).await.observed() {
            if let Some(updated) = ctx.store.message(chat, target).await.observed() {
                let _ = self.events.send(ServiceEvent::hint(&updated, false));
            }
        }
    }

    /// A live location edit moves the share in place. Late or replayed updates
    /// are dropped by sequence; an edit with no position marks it stopped.
    async fn apply_live_location_edit(
        &self,
        ctx: &BatchCtx<'_>,
        chat: &str,
        target: &str,
        update: LiveLocationUpdate,
    ) {
        let Some(existing) = ctx.store.message(chat, target).await.observed() else { return };
        if existing.media.kind.as_deref() != Some("live_location") {
            return;
        }
        let Some(stored) = existing.live_location else { return };
        let changed = match update {
            LiveLocationUpdate::Ended => {
                if stored.ended {
                    return;
                }
                ctx.store.end_live_location(chat, target).await.observed().unwrap_or(false)
            }
            LiveLocationUpdate::Moved { mut live, thumb } => {
                if let (Some(old), Some(new)) = (stored.sequence, live.sequence) {
                    if new <= old {
                        log::debug!("live location update {target} out of order ({new} <= {old}); ignored");
                        return;
                    }
                }
                // The share's own clock and expiry survive updates that omit them.
                live.started_at = stored.started_at;
                live.expires_at = live.expires_at.or(stored.expires_at);
                let thumb = thumb.as_deref().map(thumb_uri);
                ctx.store
                    .update_live_location(chat, target, &live, thumb)
                    .await
                    .observed()
                    .unwrap_or(false)
            }
        };
        if changed {
            if let Some(updated) = ctx.store.message(chat, target).await.observed() {
                let _ = self.events.send(ServiceEvent::hint(&updated, false));
            }
        }
    }

    async fn apply_edit(&self, ctx: &BatchCtx<'_>, chat: &str, target: &str, text: &str) {
        if let Some(true) = ctx.store.update_message_content(chat, target, text).await.observed() {
            if let Some(updated) = ctx.store.message(chat, target).await.observed() {
                let _ = self.events.send(ServiceEvent::hint(&updated, false));
            }
            let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        }
    }

    /// Decodes and stores one ordinary message, then starts any media fetch.
    async fn store_incoming(&self, ctx: &BatchCtx<'_>, inbound: &InboundMessage, incoming: &Incoming) {
        let auto_download = self.auto_download_for(ctx, inbound, &incoming.chat).await;
        let Some(mut message) = incoming_message(
            &incoming.chat, inbound, ctx.client.as_deref(), ctx.media_dir.as_deref(), false,
        )
        .await
        else {
            return;
        };
        // Mentions stay `@<number>` as on the wire; the UI resolves them when
        // drawn, so later names apply.
        message.local.mentioned = mentions_me(&inbound.message, &ctx.own);
        if inbound.message.is_view_once() {
            ctx.store.set_view_once(&incoming.chat, &message.header.id, incoming.from_me).await.logged();
        }
        if is_forwarded(&inbound.message) {
            ctx.store.set_forwarded(&incoming.chat, &message.header.id).await.logged();
        }
        self.recall_quoted(ctx, &incoming.chat, &message).await;
        self.store_and_track(ctx, &incoming.chat, &message).await;
        self.spawn_media_fetch(ctx, &incoming.chat, &message, auto_download);
    }

    /// Whether to fetch this message's media now. Stickers and voice notes are
    /// small and read as part of the conversation, so WhatsApp always fetches
    /// them; anything else follows the chat's setting.
    async fn auto_download_for(&self, ctx: &BatchCtx<'_>, inbound: &InboundMessage, chat: &str) -> bool {
        let base = inbound.message.get_base_message();
        let small = base.sticker_message.is_set()
            || base.audio_message.as_option().is_some_and(|a| a.ptt == Some(true));
        if small {
            return true;
        }
        ctx.store
            .chat_auto_download(chat)
            .await
            .observed()
            .flatten()
            .unwrap_or(self.auto_download_default)
    }

    /// Pairing only brings recent days; a reply to something older pulls that
    /// chat's past so the quote can be opened.
    async fn recall_quoted(&self, ctx: &BatchCtx<'_>, chat: &str, message: &StoredMessage) {
        let (Some(quoted), Some(client)) = (message.quote.id.clone(), ctx.client.clone()) else { return };
        let quoted_chat = message.quote.chat.clone().unwrap_or_else(|| chat.to_string());
        if ctx.store.message(&quoted_chat, &quoted).await.is_ok() || !recall_allowed(&quoted_chat) {
            return;
        }
        let store = ctx.store.clone();
        tokio::spawn(async move {
            fetch_older(&client, &store, &quoted_chat, 50).await.logged();
        });
    }

    /// Stores the row, counts it for the loading gate, records a sticker and
    /// moves an archived chat back unless the account keeps them archived.
    async fn store_and_track(&self, ctx: &BatchCtx<'_>, chat: &str, message: &StoredMessage) {
        if let Err(e) = ctx.store.insert_message(message).await {
            log::error!("could not store message {} in {chat}: {e}", message.header.id);
            return;
        }
        if message.media.kind.as_deref() == Some("sticker") {
            if let Err(e) = record_sticker(ctx.store, message).await {
                log::warn!("could not record sticker {}: {e}", message.header.id);
            }
        }
        self.track_sync_progress();
        self.maybe_unarchive(ctx, message).await;
    }

    /// Counts backlog progress so the loading screen's bar tracks stored
    /// messages, not raw inbound events.
    fn track_sync_progress(&self) {
        let mut p = self.sync_progress.lock().unwrap();
        if p.pending == 0 || p.offline_done {
            return;
        }
        p.applied = (p.applied + 1).min(p.pending);
        p.last_progress = Some(std::time::Instant::now());
        let emit = p.applied >= p.pending
            || p.last_emit.map_or(true, |t| t.elapsed() >= std::time::Duration::from_millis(50));
        if emit {
            p.last_emit = Some(std::time::Instant::now());
            let (pending, applied) = (p.pending, p.applied);
            drop(p);
            let _ = self.events.send(ServiceEvent::Syncing { pending, applied });
        }
    }

    /// A new incoming message moves an archived chat back to the main list
    /// unless the account keeps archived chats archived. The account is told
    /// too, so the phone cannot re-archive it later.
    async fn maybe_unarchive(&self, ctx: &BatchCtx<'_>, message: &StoredMessage) {
        let chat = message.header.chat.clone();
        if message.header.from_me
            || self.keep_archived.load(Ordering::SeqCst)
            || !ctx.store.is_archived(&chat).await.observed().unwrap_or(false)
        {
            return;
        }
        ctx.store.set_archived(&chat, false).await.logged();
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: chat.clone() });
        let (Some(client), Ok(jid)) = (ctx.client.clone(), chat.parse::<Jid>()) else { return };
        tokio::spawn(async move {
            if let Err(e) = client.chat_actions().unarchive_chat(&jid, None).await {
                log::warn!("could not unarchive {jid}: {e}");
            }
        });
    }

    /// Fetches the file for an ordinary message, keeping a view-once as
    /// ordinary media when this link can receive it. That overrides the
    /// auto-download setting: the view-once setting decides.
    fn spawn_media_fetch(&self, ctx: &BatchCtx<'_>, chat: &str, message: &StoredMessage, auto_download: bool) {
        if message.media.kind.as_deref() == Some("view_once") {
            log::debug!(
                "view-once in {chat}: arrived with fetchable media: {}",
                message.media.locator.is_some()
            );
        }
        let keep_once = message.media.kind.as_deref() == Some("view_once")
            && message.media.locator.is_some()
            && self.keep_view_once.load(Ordering::SeqCst);
        let fetch = match ((auto_download || keep_once) && message.media.locator.is_some(), &ctx.client, &ctx.media_dir) {
            (true, Some(client), Some(dir)) => {
                Some((client.clone(), dir.clone(), message.header.id.clone(), keep_once))
            }
            _ => None,
        };
        let _ = self.events.send(ServiceEvent::hint(message, true));
        let Some((client, dir, id, keep_once)) = fetch else { return };
        let (store, events, downloads) = (ctx.store.clone(), self.events.clone(), self.downloads.clone());
        let chat = chat.to_string();
        tokio::spawn(async move {
            let Ok(_permit) = downloads.acquire().await else { return };
            match fetch_media(&client, &store, &dir, &chat, &id).await {
                Ok(updated) => {
                    if keep_once {
                        if let Err(e) = store.keep_view_once(&chat, &id).await {
                            log::warn!("could not keep view-once {id}: {e}");
                        } else if let Some(kept) = store.message(&chat, &id).await.observed() {
                            log::info!("kept one-time {id} in {chat}");
                            // The mark is gone, so the row reloads as ordinary
                            // media already holding the file.
                            let _ = events.send(ServiceEvent::hint(&kept, false));
                            let _ = events.send(ServiceEvent::Marks { chat: chat.clone() });
                            return;
                        }
                    }
                    // Non-fresh: the row refetches coalesced, no follow or lookup.
                    let _ = events.send(ServiceEvent::hint(&updated, false));
                }
                Err(e) => log::warn!("failed to download {id} media: {e}"),
            }
        });
    }

    /// Bounds the store right after writes so the limit holds even if the
    /// process stops. Returns how many rows went and when the pruning started.
    async fn enforce_retention(&self, ctx: &mut BatchCtx<'_>) -> (usize, std::time::Instant) {
        let pruning = std::time::Instant::now();
        ctx.touched.sort_unstable();
        ctx.touched.dedup();
        let retention = self.disk_retention.clone();
        let touched = std::mem::take(&mut ctx.touched);
        let removed = match ctx.store.run(move |store| retention.enforce_for(store, &touched)).await {
            Ok(removed) => removed,
            Err(e) => {
                log::error!("retention failed: {e}");
                0
            }
        };
        if removed > 0 {
            let _ = self.events.send(ServiceEvent::RetentionApplied { removed });
            // A pruned reply can be the last one naming a recovered view-once.
            let directory = ctx.media_dir.clone();
            if let Err(e) = ctx.store.run(move |store| prune_quote_files(directory.as_deref(), store)).await {
                log::error!("pruning recovered view-once files failed: {e}");
            }
        }
        (removed, pruning)
    }
}
