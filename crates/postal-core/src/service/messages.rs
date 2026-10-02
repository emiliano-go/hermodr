//! Messages: sending, editing and marking them, per-chat settings, and turning
//! protocol messages into stored rows.

use super::*;
use crate::store::group_audit::GroupAuditKind as AuditKind;

impl WhatsAppService {
    /// Sends a text message to a chat.
    ///
    /// The sent message is stored and dispatched locally. WhatsApp does not echo
    /// a message back to the device that sent it, so without this the sender
    /// would not see their own message until the store was next reloaded.
    pub async fn send_text(
        &self,
        chat: &str,
        text: impl Into<String>,
        mentions: Vec<String>,
    ) -> Result<()> {
        let to = broadcast_lists::writable_target(chat)?;
        self.unarchive_on_send(chat).await;
        let to_self = self.is_self_jid(&to);
        let text = text.into();
        // `@all` is a group mention, carried separately from member mentions.
        let mention_all = mentions.iter().any(|m| m == "@all");
        let mentioned: Vec<String> = mentions.iter().filter(|m| *m != "@all").cloned().collect();

        // A link in the text gets an Open Graph preview, fetched off the runtime.
        let preview = if mentioned.is_empty() {
            match first_url(&text) {
                Some(url) => tokio::task::spawn_blocking(move || fetch_link_preview(&url))
                    .await
                    .ok()
                    .flatten(),
                None => None,
            }
        } else {
            None
        };

        let result = if mentioned.is_empty() && !mention_all && preview.is_none() {
            self.client.send_text(to, text.clone()).await?
        } else {
            let mut context = wa::ContextInfo {
                mentioned_jid: mentioned.clone(),
                ..Default::default()
            };
            if mention_all {
                context.group_mentions = vec![wa::GroupMention {
                    group_jid: Some(chat.to_string()),
                    group_subject: self.store.name_for(chat).await.observed().flatten(),
                }];
            }
            let extended = wa::message::ExtendedTextMessage {
                text: Some(text.clone()),
                matched_text: preview.as_ref().map(|p| p.url.clone()),
                title: preview.as_ref().and_then(|p| p.title.clone()),
                description: preview.as_ref().and_then(|p| p.description.clone()),
                jpeg_thumbnail: preview.as_ref().and_then(|p| p.thumbnail.clone()),
                context_info: MessageField::some(context),
                ..Default::default()
            };
            let message = wa::Message {
                extended_text_message: MessageField::some(extended),
                ..Default::default()
            };
            group_history::guard_ordinary_message(&message)?;
            self.client.send_message(to, message).await?
        };

        // Keep the preview with our own copy, so the sender sees it too.
        let thumbnail = preview.as_ref().and_then(|p| p.thumbnail.as_deref()).map(thumb_uri);

        let mut message = self.own_message(chat, &result.message_id, text, "", to_self);
        message.history_shareable = group_history::is_shareable_text(&result.message);
        if let Some(p) = &preview {
            message.link = LinkCard {
                url: Some(p.url.clone()),
                title: p.title.clone(),
                desc: p.description.clone(),
                thumb: thumbnail,
                site: p.site.clone(),
                color: p.color.clone(),
            };
        }
        let message = self.store.insert_message_row(&message).await?;
        let _ = self.events.send(ServiceEvent::arrival(&message));
        Ok(())
    }

    /// Replaces the text of one of our own messages.
    pub async fn edit_message(&self, chat: &str, id: &str, text: impl Into<String>) -> Result<()> {
        let to = broadcast_lists::writable_target(chat)?;
        let text = text.into();
        if text.trim().is_empty() {
            anyhow::bail!("an edit cannot be empty");
        }
        let existing = self.store.message(chat, id).await?;
        anyhow::ensure!(!existing.is_unavailable(), "this message is unavailable on this device");
        if !existing.header.from_me {
            anyhow::bail!("only your own messages can be edited");
        }
        let sent = self.client
            .edit_message(to, id, wa::Message::text(text.clone()))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.update_message_content(chat, id, &text).await?;
        if group_audit::audit_message_allowed(&existing) {
            self.audit_local_group_change(chat, AuditKind::MessageEdit, Some(&existing.header.sender),
                Some(id), Some(&sent.message_id), None, None).await.logged();
        }
        if let Some(updated) = self.store.message(chat, id).await.observed() {
            // Status-only: the row refetches, without following or marking read.
            let _ = self.events.send(ServiceEvent::hint(&updated, false));
        }
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// The message range WhatsApp Web attaches to an archive action, so the
    /// receiving devices can resolve conflicts. Built from the chat's newest
    /// messages; `None` when there are none to name.
    async fn archive_range(&self, chat: &str) -> Option<whatsapp_rust::SyncActionMessageRange> {
        let remote = chat.parse::<Jid>().ok()?;
        let messages = self.store.messages_for(chat, 3).await.observed()?;
        let last = messages.first()?.header.timestamp;
        let mut keys = Vec::with_capacity(messages.len());
        for m in &messages {
            let participant = (remote.is_group() && !m.header.from_me)
                .then(|| m.header.sender.parse::<Jid>().ok().map(|j| j.to_non_ad()))
                .flatten();
            keys.push((
                whatsapp_rust::message_key(
                    m.header.id.clone(),
                    &remote,
                    m.header.from_me,
                    participant.as_ref(),
                ),
                m.header.timestamp,
            ));
        }
        Some(whatsapp_rust::message_range(last, None, keys))
    }

    /// Archives or unarchives a chat, mirroring it to the account.
    pub async fn set_archived(&self, chat: &str, archived: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let key = resolve_chat(Some(self.client.as_ref()), &self.store, &jid).await;
        let jid: Jid = key.parse()?;
        log::debug!("set_archived {key}: {archived}");
        self.store.set_archived(&key, archived).await?;
        let range = self.archive_range(&key).await;
        let actions = self.client.chat_actions();
        let result = if archived {
            actions.archive_chat(&jid, range).await
        } else {
            actions.unarchive_chat(&jid, range).await
        };
        result.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Sending to an archived chat brings it back to the main list, as WhatsApp
    /// does; the account is told too so the phone agrees.
    pub(super) async fn unarchive_on_send(&self, chat: &str) {
        let Ok(jid) = chat.parse::<Jid>() else { return };
        let bare = jid.to_non_ad().to_string();
        if !self.store.is_archived(&bare).await.observed().unwrap_or(false) {
            return;
        }
        self.store.set_archived(&bare, false).await.logged();
        let _ = self.events.send(ServiceEvent::ChatStateChanged { chat: bare });
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client.chat_actions().unarchive_chat(&jid, None).await {
                log::warn!("could not unarchive {jid}: {e}");
            }
        });
    }

    /// Mutes a chat until `until` (Unix seconds; -1 indefinitely, 0 unmutes).
    pub async fn set_muted(&self, chat: &str, until: i64) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let key = resolve_chat(Some(self.client.as_ref()), &self.store, &jid).await;
        let jid: Jid = key.parse()?;
        self.store.set_muted_until(&key, until).await?;
        let actions = self.client.chat_actions();
        let result = match until {
            0 => actions.unmute_chat(&jid).await,
            u if u < 0 => actions.mute_chat(&jid).await,
            u => actions.mute_chat_until(&jid, u * 1000).await,
        };
        result.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Marks a chat unread by hand, or clears that mark, mirroring it to the account.
    pub async fn set_marked_unread(&self, chat: &str, unread: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let key = resolve_chat(Some(self.client.as_ref()), &self.store, &jid).await;
        let jid: Jid = key.parse()?;
        self.store.set_marked_unread(&key, unread).await?;
        self.client
            .chat_actions()
            .mark_chat_as_read(&jid, !unread, None)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// Leaves a group.
    pub async fn leave_group(&self, chat: &str) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.client.groups().leave(jid).await.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// The chat a stored message id belongs to.
    pub async fn chat_for_message(&self, id: &str) -> Result<Option<String>> {
        self.store.chat_of_message(id).await
    }

    /// Sets a chat's auto download override.
    pub async fn set_chat_auto_download(&self, chat: &str, enabled: bool) -> Result<()> {
        self.store.set_chat_auto_download(chat, enabled).await
    }

    pub async fn chat_sound_muted(&self, chat: &str) -> Result<Option<bool>> {
        self.store.chat_sound_muted(chat).await
    }

    pub async fn set_chat_sound_muted(&self, chat: &str, muted: Option<bool>) -> Result<()> {
        self.store.set_chat_sound_muted(chat, muted).await
    }

    /// Whether @all mentions stay silent in this chat.
    pub async fn chat_mute_at_all(&self, chat: &str) -> Result<bool> {
        self.store.chat_mute_at_all(chat).await
    }

    /// Mutes or unmutes @all mentions in one chat; direct mentions still ping.
    pub async fn set_chat_mute_at_all(&self, chat: &str, muted: bool) -> Result<()> {
        self.store.set_chat_mute_at_all(chat, muted).await
    }

    /// Deletes every message stored on this device; the phone keeps its copy.
    pub async fn clear_history(&self) -> Result<usize> {
        self.cancel_older_requests(None).await?;
        let removed = self.store.clear_history().await?;
        self.prune_quote_files().await?;
        Ok(removed)
    }

    /// Clears one chat locally: its messages go, the empty chat stays.
    pub async fn clear_chat(&self, chat: &str) -> Result<usize> {
        self.cancel_older_requests(Some(chat)).await?;
        let removed = self.store.clear_chat(chat).await?;
        if chat.ends_with("@g.us") { let _ = self.events.send(ServiceEvent::GroupAuditChanged { chat: chat.into() }); }
        self.prune_quote_files().await?;
        Ok(removed)
    }

    /// Deletes one chat locally: its messages go and it leaves the list until
    /// a new message arrives. Never touches the phone or the other side.
    pub async fn delete_chat(&self, chat: &str) -> Result<usize> {
        self.cancel_older_requests(Some(chat)).await?;
        let removed = self.store.delete_chat(chat).await?;
        if chat.ends_with("@g.us") { let _ = self.events.send(ServiceEvent::GroupAuditChanged { chat: chat.into() }); }
        self.prune_quote_files().await?;
        Ok(removed)
    }

    /// The key that names a message to the server: groups need its sender.
    fn message_key(chat: &str, id: &str, sender: &str, from_me: bool) -> wa::MessageKey {
        let participant = (chat.ends_with("@g.us") && !from_me)
            .then(|| sender.parse::<Jid>().map(|j| j.to_non_ad().to_string()).unwrap_or_default());
        wa::MessageKey {
            remote_jid: Some(chat.to_string()),
            from_me: Some(from_me),
            id: Some(id.to_string()),
            participant,
            ..Default::default()
        }
    }

    /// The sender as the app-state actions want it: set only for others in groups.
    fn participant(chat: &str, sender: &str, from_me: bool) -> Option<Jid> {
        (chat.ends_with("@g.us") && !from_me)
            .then(|| sender.parse::<Jid>().ok().map(|j| j.to_non_ad()))
            .flatten()
    }

    /// Reacts to a message; an empty emoji takes our reaction back.
    pub async fn react(&self, chat: &str, id: &str, sender: &str, from_me: bool, emoji: &str) -> Result<()> {
        let jid = broadcast_lists::writable_target(chat)?;
        self.client
            .send_reaction(jid, Self::message_key(chat, id, sender, from_me), emoji)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_reaction(chat, id, "@me", emoji).await?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    pub async fn star(&self, chat: &str, id: &str, sender: &str, from_me: bool, starred: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let participant = Self::participant(chat, sender, from_me);
        let actions = self.client.chat_actions();
        let done = if starred {
            actions.star_message(&jid, participant.as_ref(), id, from_me).await
        } else {
            actions.unstar_message(&jid, participant.as_ref(), id, from_me).await
        };
        done.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_starred(chat, id, starred).await?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Pins a message for everyone in the chat for a week, or unpins it.
    pub async fn pin_message(&self, chat: &str, id: &str, sender: &str, from_me: bool, pinned: bool) -> Result<()> {
        use whatsapp_rust::send::PinDuration;
        let jid = broadcast_lists::writable_target(chat)?;
        let key = Self::message_key(chat, id, sender, from_me);
        let previous = if jid.is_group() { self.store.message(chat, id).await.observed() } else { None };
        let sent = if pinned {
            self.client.pin_message(jid, key, PinDuration::Days7).await
        } else {
            self.client.unpin_message(jid, key).await
        };
        let sent = sent.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let pin = history_pins::live_message_pin(sent.message.as_ref(), unix_now() * 1000)?
            .ok_or_else(|| anyhow::anyhow!("pin confirmation contains no target"))?;
        let chat_key = chat.to_owned();
        self.store.run(move |store| store.apply_message_pin_update(&chat_key, &pin, false)).await?;
        if previous.as_ref().is_some_and(group_audit::audit_message_allowed) {
            self.audit_local_group_change(chat, if pinned { AuditKind::MessagePin } else { AuditKind::MessageUnpin },
                Some(sender), Some(id), Some(&sent.message_id), None, Some(if pinned { "true" } else { "false" })).await.logged();
        }
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Deletes a message for everyone: ours as the sender, anyone's as an
    /// admin. The local copy stays, greyed out, so a message never becomes
    /// unavailable here.
    pub async fn delete_for_everyone(&self, chat: &str, id: &str, sender: &str, from_me: bool) -> Result<()> {
        self.revoke_for_everyone(chat, id, sender, from_me).await?;
        if let Some(updated) = self.store.message(chat, id).await.observed() {
            let _ = self.events.send(ServiceEvent::hint(&updated, false));
        }
        Ok(())
    }

    /// Sends and records one revoke; the caller hints.
    async fn revoke_for_everyone(&self, chat: &str, id: &str, sender: &str, from_me: bool) -> Result<()> {
        use whatsapp_rust::send::RevokeType;
        let jid = broadcast_lists::writable_target(chat)?;
        let previous = if jid.is_group() { self.store.message(chat, id).await.observed() } else { None };
        let kind = if from_me {
            RevokeType::Sender
        } else {
            RevokeType::Admin { original_sender: sender.parse::<Jid>()?.to_non_ad() }
        };
        let sent = self.client
            .revoke_message(jid, id, kind)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.revoke_message(chat, id).await?;
        if let Some(previous) = previous.filter(group_audit::audit_message_allowed) {
            let old = if previous.local.revoked { "revoked" } else { "not_revoked" };
            self.audit_local_group_change(chat, AuditKind::MessageDelete, Some(sender), Some(id),
                Some(&sent.message_id), Some(old), Some("revoked")).await.logged();
        }
        Ok(())
    }

    /// Deletes a message on this device only: the row is kept and flagged, so
    /// the chat can show it greyed out. Nothing is sent to WhatsApp, and no
    /// file is removed, so the copy stays recoverable.
    pub async fn delete_for_me(&self, chat: &str, id: &str) -> Result<()> {
        self.store.set_message_deleted(chat, id, true).await?;
        Ok(())
    }

    /// Deletes several messages at once, for everyone or on this device only.
    /// Every message is attempted; the first failure stops the run.
    pub async fn delete_messages(&self, chat: &str, ids: &[String], everyone: bool) -> Result<()> {
        if everyone {
            for id in ids {
                let existing = self.store.message(chat, id).await?;
                let sender = existing.header.sender.clone();
                let from_me = existing.header.from_me;
                self.revoke_for_everyone(chat, id, &sender, from_me).await?;
            }
            let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        } else {
            for id in ids {
                self.delete_for_me(chat, id).await?;
            }
        }
        Ok(())
    }

    /// Sends a copy of a stored message to another chat.
    pub async fn forward(&self, from_chat: &str, id: &str, to_chat: &str) -> Result<()> {
        let to = broadcast_lists::writable_target(to_chat)?;
        let message = self.store.message(from_chat, id).await?;
        anyhow::ensure!(!message.is_unavailable(), "this message is unavailable on this device");
        anyhow::ensure!(!message.spoiler, "spoiler forwarding is unavailable until every media path preserves its wrapper");
        // Uncaptioned media is stored as `[kind]`, which must not become a caption.
        let placeholder = message.media.kind.as_ref().map(|kind| format!("[{kind}]"));
        let text = message.text.trim();
        let caption = (!text.is_empty() && placeholder.as_deref() != Some(text))
            .then(|| message.text.clone());
        match message.media.path.as_deref().filter(|p| Path::new(p).is_file()) {
            Some(path) => {
                let bytes = std::fs::read(path)?;
                let name = Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                let gif = message.media.kind.as_deref() == Some("gif");
                if message.media.kind.as_deref() == Some("sticker") {
                    self.send_sticker_as(to_chat, bytes, true, None).await?;
                } else {
                    let options = SendOptions { gif, forwarded: true, ..Default::default() };
                    self.send_media(to_chat, &name, bytes, caption, None, options).await?;
                }
            }
            None if message.media.kind.is_some() => {
                anyhow::bail!("download the media before forwarding it")
            }
            None => {
                let to_self = self.is_self_jid(&to);
                let content = wa::Message {
                    extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
                        text: Some(message.text.clone()),
                        context_info: MessageField::some(*forwarded_context(None)),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                group_history::guard_ordinary_message(&content)?;
                let result = self.client.send_message(to, content).await?;
                self.store.set_forwarded(to_chat, &result.message_id).await?;
                let stored = self.own_message(to_chat, &result.message_id, message.text, "", to_self);
                let stored = self.store.insert_message_row(&stored).await?;
                let _ = self.events.send(ServiceEvent::arrival(&stored));
            }
        }
        Ok(())
    }

    pub async fn marks(&self, chat: &str) -> Result<crate::store::ChatMarks> {
        let mut marks = self.store.marks(chat).await?;
        self.enrich_quiz_marks(chat, &mut marks).await?;
        Ok(marks)
    }

    pub async fn marks_for(&self, chat: &str, ids: &[String]) -> Result<crate::store::ChatMarks> {
        let mut marks = self.store.marks_for(chat, Some(ids)).await?;
        self.enrich_quiz_marks(chat, &mut marks).await?;
        Ok(marks)
    }

    /// Marks a view-once message opened and deletes its media.
    pub async fn open_view_once(&self, chat: &str, id: &str) -> Result<()> {
        if let Some(path) = self.store.open_view_once(chat, id).await? {
            remove_cached_file(path);
        }
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// A message we just sent, as the store keeps it until the server confirms.
    /// An empty `kind` means text only.
    pub(super) fn own_message(&self, chat: &str, id: &str, text: String, kind: &str, to_self: bool) -> StoredMessage {
        StoredMessage {
            header: MessageHeader {
                chat: chat.to_string(),
                id: id.to_string(),
                sender: self.own_jid(),
                timestamp: unix_now(),
                from_me: true,
            },
            text,
            media: Media { kind: (!kind.is_empty()).then(|| kind.to_string()), ..Default::default() },
            // Not read: we cannot know whether the recipient has read it. A
            // message to ourselves needs no receipt to count as delivered.
            local: LocalState {
                status: Some(if to_self { "delivered".into() } else { "pending".into() }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Who got, read and played one of our messages.
    pub async fn message_info(&self, id: &str) -> Result<Vec<crate::store::MessageReceipt>> {
        self.store.receipts(id).await
    }

    /// Starred messages across every chat, newest first.
    pub async fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        self.store.starred_messages().await
    }

    /// Messages that mention us, in one chat or all of them, newest first.
    pub async fn pings(&self, chat: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.store.pings(chat, 500).await
    }

    /// Mentions with the global @all mute applied on top of per-chat mutes.
    pub async fn pings_with(&self, chat: Option<&str>, mute_all_at_all: bool) -> Result<Vec<StoredMessage>> {
        self.store.pings_with(chat, 500, mute_all_at_all).await
    }

    /// Up to `limit` messages in a chat containing `query`, newest first.
    pub async fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.store.search_messages(chat, query.trim(), limit).await
    }

    pub async fn chat_retention(&self, chat: &str) -> Result<crate::store::ChatRetention> {
        self.store.chat_retention(chat).await
    }

    /// Sets a chat's own retention and applies it at once.
    pub async fn set_chat_retention(&self, chat: &str, retention: &crate::store::ChatRetention) -> Result<()> {
        self.cancel_older_requests(Some(chat)).await?;
        self.store.set_chat_retention(chat, retention).await?;
        let retention = self.disk_retention.clone();
        let older_waits = self.older_waits.clone();
        self.store.run(move |store| {
            let protected = older_waits.lock().unwrap().protected_chats(store)?;
            retention.enforce_protected(store, &protected)
        }).await?;
        self.prune_quote_files().await?;
        Ok(())
    }

    /// The per chat auto download override, if one is set.
    pub async fn chat_auto_download(&self, chat: &str) -> Result<Option<bool>> {
        self.store.chat_auto_download(chat).await
    }

    /// The chat's (typing, read receipts) overrides; `None` follows the global setting.
    pub async fn chat_privacy(&self, chat: &str) -> Result<(Option<bool>, Option<bool>)> {
        self.store.chat_privacy(chat).await
    }

    pub async fn set_chat_privacy(&self, chat: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        self.store.set_chat_privacy(chat, typing, receipts).await
    }

    /// Unread messages that mention us, oldest first.
    pub async fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        self.store.unread_mentions(chat).await
    }

    /// Unread mentions with the global @all mute applied on top of per-chat mutes.
    pub async fn unread_mentions_with(&self, chat: &str, mute_all_at_all: bool) -> Result<Vec<String>> {
        self.store.unread_mentions_with(chat, mute_all_at_all).await
    }

    /// Sends a text message quoting an earlier one.
    ///
    /// The quote is rebuilt from the stored message rather than the original
    /// protobuf, which we do not keep; the recipient renders the quoted text.
    pub async fn send_reply(
        &self,
        chat: &str,
        text: impl Into<String>,
        reply_to_id: &str,
        reply_to_sender: &str,
        reply_to_text: &str,
        mentions: Vec<String>,
        // The chat the quoted message is in, when it is not this one (a group
        // message answered privately).
        quote_chat: Option<&str>,
    ) -> Result<()> {
        let to = broadcast_lists::writable_target(chat)?;
        self.unarchive_on_send(chat).await;
        let to_self = self.is_self_jid(&to);
        let quoted_chat: Jid = match quote_chat {
            Some(other) => other.parse()?,
            None => to.clone(),
        };
        // The quoted author must be the address without a device suffix: a
        // participant like `123:98@lid` is not resolvable by recipients, who
        // then attribute the quoted message to the sender of the reply.
        let sender: Jid = reply_to_sender.parse::<Jid>()?.to_non_ad();
        let text = text.into();

        use whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info;
        // The quoted message is the message itself where the store has it, and
        // nothing at all otherwise: the recipient resolves a bare stanza id from
        // its own history, but renders a stand-in as if it were the real thing.
        let quoted = self.quoted_message(&quoted_chat.to_string(), reply_to_id, reply_to_text).await;
        let mut context =
            build_quote_context_with_info(reply_to_id, &sender, &quoted_chat, &to, quoted.as_ref().unwrap_or(&wa::Message::text("")));
        if quoted.is_none() {
            context.quoted_message = Default::default();
        }
        let mention_all = mentions.iter().any(|m| m == "@all");
        let mentioned: Vec<String> = mentions.iter().filter(|m| *m != "@all").cloned().collect();
        if !mentioned.is_empty() {
            context.mentioned_jid = mentioned.clone();
        }
        if mention_all {
            context.group_mentions = vec![wa::GroupMention {
                group_jid: Some(chat.to_string()),
                group_subject: self.store.name_for(chat).await.observed().flatten(),
            }];
        }

        use whatsapp_rust::wacore::proto_helpers::MessageBuilderExt;
        let message = wa::Message::text_with_context(text.clone(), context);
        group_history::guard_ordinary_message(&message)?;
        let result = self.client.send_message(to, message).await?;

        let mut stored = self.own_message(chat, &result.message_id, text, "", to_self);
        stored.quote = self.local_quote(&quoted_chat.to_string(), reply_to_id, reply_to_sender, sender.to_string() == self.own_jid()).await;
        stored.quote.chat = quote_chat.map(str::to_string);
        let stored = self.store.insert_message_row(&stored).await?;
        let _ = self.events.send(ServiceEvent::arrival(&stored));
        Ok(())
    }

    /// What a reply should quote when the message it answers has no text of its own.
    ///
    /// A view-once is stored as a placeholder, and an attachment that is not
    /// The message a reply points at, rebuilt as faithfully as the store allows.
    ///
    /// This is what the recipient renders, so it has to be the message itself
    /// and not a stand-in: a text stub sent in place of a view-once makes the
    /// reply claim it quotes those words. A real client sends the original,
    /// which is not kept here, so a media message is rebuilt from its own
    /// locator — the media submessage with its view-once flag — and a text
    /// message from its text. When neither exists the quote is left empty for
    /// the recipient to resolve from its own history, which is what it does
    /// with a reply whose quoted content was withheld.
    pub(super) async fn quoted_message(&self, chat: &str, id: &str, text: &str) -> Option<wa::Message> {
        if self.store.message(chat, id).await.observed().is_some_and(|row| row.spoiler) {
            return Some(wa::Message::text("[Spoiler]"));
        }
        if let Some(bytes) = self.store.media_ref_for(chat, id).await.observed().flatten() {
            if let Ok(message) = <wa::Message as buffa::Message>::decode(&mut bytes.as_slice()) {
                if detect_media(&message).is_some() {
                    // The locator is the bare media; a view-once is quoted in its wrapper.
                    let once = self.store.is_view_once(chat, id).await.observed()?;
                    return Some(if once { wrap_view_once(message) } else { message });
                }
            }
        }
        // A view-once reaches a linked device as a stub with no content. A reply
        // someone else sent quoting it carries the real one, which is quoted
        // back when stored; otherwise an empty view-once of the same kind.
        if let Some(row) = self.store.message(chat, id).await.observed() {
            if row.media.kind.as_deref() == Some("view_once") {
                use whatsapp_rust::wacore::proto_helpers::MessageExt;
                let copy = self
                    .store
                    .view_once_copy(chat, id)
                    .await.observed()
                    .flatten()
                    .and_then(|b| <wa::Message as buffa::Message>::decode(&mut b.as_slice()).ok())
                    .filter(|m| detect_media(m.get_base_message()).is_some());
                return Some(match copy {
                    // Rows stored before the whole quote was kept hold the bare media.
                    Some(quoted) if quoted.is_view_once() => quoted,
                    Some(media) => wrap_view_once(media),
                    None => empty_view_once(row.media.once_kind.as_deref()),
                });
            }
        }
        (!text.trim().is_empty()).then(|| wa::Message::text(text))
    }

    /// The quote to store beside a reply this account sent, so it renders the
    /// message it answers rather than a label.
    pub(super) async fn local_quote(&self, chat: &str, id: &str, sender: &str, is_me: bool) -> Quote {
        let row = self.store.message(chat, id).await.observed();
        let once = self.store.is_view_once(chat, id).await.observed();
        let kind = row.as_ref().and_then(|m| m.media.kind.clone());
        let text = row
            .as_ref()
            .map(|m| if m.spoiler { "[Spoiler]".into() } else { m.text.clone() })
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                let once = once?;
                kind.as_deref().map(|k| match (once, k) {
                    (true, _) => "View once message".to_string(),
                    (false, "image") => "Photo".to_string(),
                    (false, "video") => "Video".to_string(),
                    (false, "audio") => "Voice message".to_string(),
                    (false, "sticker") => "Sticker".to_string(),
                    (false, "poll") => "Poll".to_string(),
                    (false, "document") => "Document".to_string(),
                    _ => "Media message".to_string(),
                })
            })
            .unwrap_or_default();
        Quote {
            id: Some(id.to_string()),
            text: Some(text),
            sender: Some(if is_me { "@me".to_string() } else { sender.to_string() }),
            kind,
            thumb: row.and_then(|m| (!m.spoiler).then_some(m.media.thumb).flatten()),
            view_once: once.unwrap_or(true),
            recoverable: false,
            ..Default::default()
        }
    }

    /// Stored messages for a chat, newest first.
    pub async fn message_page(&self, chat: &str, limit: u32, cursor: Option<crate::store::MessageCursor>,
        direction: crate::store::MessagePageDirection, anchor_id: Option<&str>) -> Result<crate::store::MessagePage> {
        let cursor = if let Some(id) = anchor_id {
            match self.store.message(chat, id).await {
                Ok(message) => Some(crate::store::MessageCursor { timestamp: message.header.timestamp, id: message.header.id, sort_order: message.local.sort_order }),
                Err(error) if error.downcast_ref::<rusqlite::Error>() == Some(&rusqlite::Error::QueryReturnedNoRows) => {
                    return Ok(crate::store::MessagePage { messages: vec![], has_more: false });
                }
                Err(error) => return Err(error),
            }
        } else { cursor };
        self.store.message_page(chat, limit, cursor.as_ref(), direction).await
    }

    pub async fn messages(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        self.store.messages_for(chat, limit).await
    }

    /// Chat summaries, most recently active first.
    pub async fn chats(&self) -> Result<Vec<crate::store::ChatSummary>> {
        self.store.chats().await
    }

    /// Chat summaries with the global @all mute applied on top of per-chat mutes.
    pub async fn chats_with(&self, mute_all_at_all: bool) -> Result<Vec<crate::store::ChatSummary>> {
        self.store.chats_with(mute_all_at_all).await
    }
}
