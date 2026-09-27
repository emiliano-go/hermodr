//! Messages: sending, editing and marking them, per-chat settings, and turning
//! protocol messages into stored rows.

use super::*;

impl Service {
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
        let to: Jid = chat.parse()?;
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
                    group_subject: self.store.name_for(chat).ok().flatten(),
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
            self.client.send_message(to, message).await?
        };

        // Keep the preview with our own copy, so the sender sees it too.
        let thumbnail = preview.as_ref().and_then(|p| p.thumbnail.as_deref()).map(thumb_uri);

        let mut message = self.own_message(chat, &result.message_id, text, "", to_self);
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
        self.store.insert_message(&message)?;
        let _ = self.events.send(ServiceEvent::hint(&message, true));
        Ok(())
    }

    /// Replaces the text of one of our own messages.
    pub async fn edit_message(&self, chat: &str, id: &str, text: impl Into<String>) -> Result<()> {
        let to: Jid = chat.parse()?;
        let text = text.into();
        if text.trim().is_empty() {
            anyhow::bail!("an edit cannot be empty");
        }
        let existing = self.store.message(chat, id)?;
        if !existing.header.from_me {
            anyhow::bail!("only your own messages can be edited");
        }
        self.client
            .edit_message(to, id, wa::Message::text(text.clone()))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.update_message_content(chat, id, &text)?;
        if let Ok(updated) = self.store.message(chat, id) {
            // Status-only: the row refetches, without following or marking read.
            let _ = self.events.send(ServiceEvent::hint(&updated, false));
        }
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Pins or unpins a chat, mirroring it to the account.
    pub async fn set_pinned(&self, chat: &str, pinned: bool) -> Result<()> {
        let jid: Jid = chat.parse()?;
        self.store.set_pinned(&jid.to_non_ad().to_string(), pinned)?;
        let actions = self.client.chat_actions();
        let result = if pinned {
            actions.pin_chat(&jid).await
        } else {
            actions.unpin_chat(&jid).await
        };
        result.map_err(|e| anyhow::anyhow!(e.to_string()))
    }

    /// The chat a stored message id belongs to.
    pub fn chat_for_message(&self, id: &str) -> Result<Option<String>> {
        self.store.chat_of_message(id)
    }

    /// Sets a chat's auto download override.
    pub fn set_chat_auto_download(&self, chat: &str, enabled: bool) -> Result<()> {
        self.store.set_chat_auto_download(chat, enabled)
    }

    /// Deletes every message stored on this device; the phone keeps its copy.
    pub fn clear_history(&self) -> Result<usize> {
        self.store.clear_history()
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
        let jid: Jid = chat.parse()?;
        self.client
            .send_reaction(jid, Self::message_key(chat, id, sender, from_me), emoji)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_reaction(chat, id, "@me", emoji)?;
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
        self.store.set_starred(chat, id, starred)?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Pins a message for everyone in the chat for a week, or unpins it.
    pub async fn pin_message(&self, chat: &str, id: &str, sender: &str, from_me: bool, pinned: bool) -> Result<()> {
        use whatsapp_rust::send::PinDuration;
        let jid: Jid = chat.parse()?;
        let key = Self::message_key(chat, id, sender, from_me);
        let sent = if pinned {
            self.client.pin_message(jid, key, PinDuration::Days7).await
        } else {
            self.client.unpin_message(jid, key).await
        };
        sent.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.set_message_pin(chat, pinned.then_some(id))?;
        let _ = self.events.send(ServiceEvent::Marks { chat: chat.to_string() });
        Ok(())
    }

    /// Deletes a message for everyone: ours as the sender, anyone's as an admin.
    pub async fn delete_for_everyone(&self, chat: &str, id: &str, sender: &str, from_me: bool) -> Result<()> {
        use whatsapp_rust::send::RevokeType;
        let jid: Jid = chat.parse()?;
        let kind = if from_me {
            RevokeType::Sender
        } else {
            RevokeType::Admin { original_sender: sender.parse::<Jid>()?.to_non_ad() }
        };
        self.client
            .revoke_message(jid, id, kind)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.store.revoke_message(chat, id)?;
        if let Ok(updated) = self.store.message(chat, id) {
            let _ = self.events.send(ServiceEvent::hint(&updated, false));
        }
        Ok(())
    }

    /// Deletes a message from our devices only.
    pub async fn delete_for_me(&self, chat: &str, id: &str, sender: &str, from_me: bool, timestamp: i64) -> Result<()> {
        let jid: Jid = chat.parse()?;
        let participant = Self::participant(chat, sender, from_me);
        self.client
            .chat_actions()
            .delete_message_for_me(&jid, participant.as_ref(), id, from_me, true, Some(timestamp))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        // The stored copy is gone, so its media file has no other referent.
        if let Ok(message) = self.store.message(chat, id) {
            if let Some(path) = message.media.path.as_deref() {
                let _ = std::fs::remove_file(path);
            }
        }
        self.store.delete_message(chat, id)
    }

    /// Sends a copy of a stored message to another chat.
    pub async fn forward(&self, from_chat: &str, id: &str, to_chat: &str) -> Result<()> {
        let message = self.store.message(from_chat, id)?;
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
                    self.send_sticker_as(to_chat, bytes, true).await?;
                } else {
                    let options = SendOptions { gif, forwarded: true, ..Default::default() };
                    self.send_media(to_chat, &name, bytes, caption, None, options).await?;
                }
            }
            None if message.media.kind.is_some() => {
                anyhow::bail!("download the media before forwarding it")
            }
            None => {
                let to: Jid = to_chat.parse()?;
                let to_self = self.is_self_jid(&to);
                let content = wa::Message {
                    extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
                        text: Some(message.text.clone()),
                        context_info: MessageField::some(*forwarded_context(None)),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let result = self.client.send_message(to, content).await?;
                self.store.set_forwarded(to_chat, &result.message_id)?;
                let stored = self.own_message(to_chat, &result.message_id, message.text, "", to_self);
                self.store.insert_message(&stored)?;
                let _ = self.events.send(ServiceEvent::hint(&stored, true));
            }
        }
        Ok(())
    }

    pub fn marks(&self, chat: &str) -> Result<crate::store::ChatMarks> {
        self.store.marks(chat)
    }

    /// Marks a view-once message opened and deletes its media.
    pub fn open_view_once(&self, chat: &str, id: &str) -> Result<()> {
        if let Some(path) = self.store.open_view_once(chat, id)? {
            let _ = std::fs::remove_file(path);
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
    pub fn message_info(&self, id: &str) -> Result<Vec<crate::store::MessageReceipt>> {
        self.store.receipts(id)
    }

    /// Starred messages across every chat, newest first.
    pub fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        self.store.starred_messages()
    }

    /// Messages that mention us, in one chat or all of them, newest first.
    pub fn pings(&self, chat: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.store.pings(chat, 500)
    }

    /// Up to `limit` messages in a chat containing `query`, newest first.
    pub fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.store.search_messages(chat, query.trim(), limit)
    }

    pub fn chat_retention(&self, chat: &str) -> Result<crate::store::ChatRetention> {
        self.store.chat_retention(chat)
    }

    /// Sets a chat's own retention and applies it at once.
    pub fn set_chat_retention(&self, chat: &str, retention: &crate::store::ChatRetention) -> Result<()> {
        self.store.set_chat_retention(chat, retention)?;
        self.store.enforce_retention()?;
        Ok(())
    }

    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, chat: &str) -> Result<Option<bool>> {
        self.store.chat_auto_download(chat)
    }

    /// The chat's (typing, read receipts) overrides; `None` follows the global setting.
    pub fn chat_privacy(&self, chat: &str) -> Result<(Option<bool>, Option<bool>)> {
        self.store.chat_privacy(chat)
    }

    pub fn set_chat_privacy(&self, chat: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        self.store.set_chat_privacy(chat, typing, receipts)
    }

    /// Unread messages that mention us, oldest first.
    pub fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        self.store.unread_mentions(chat)
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
        let to: Jid = chat.parse()?;
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
        let quoted = wa::Message::text(reply_to_text);
        let mention_all = mentions.iter().any(|m| m == "@all");
        let mentioned: Vec<String> = mentions.iter().filter(|m| *m != "@all").cloned().collect();
        let mut context =
            build_quote_context_with_info(reply_to_id, &sender, &quoted_chat, &to, &quoted);
        if !mentioned.is_empty() {
            context.mentioned_jid = mentioned.clone();
        }
        if mention_all {
            context.group_mentions = vec![wa::GroupMention {
                group_jid: Some(chat.to_string()),
                group_subject: self.store.name_for(chat).ok().flatten(),
            }];
        }

        use whatsapp_rust::wacore::proto_helpers::MessageBuilderExt;
        let message = wa::Message::text_with_context(text.clone(), context);
        let result = self.client.send_message(to, message).await?;

        let mut stored = self.own_message(chat, &result.message_id, text, "", to_self);
        stored.quote = Quote {
            id: Some(reply_to_id.to_string()),
            text: Some(reply_to_text.to_string()),
            sender: Some(if sender.to_string() == self.own_jid() {
                "@me".to_string()
            } else {
                reply_to_sender.to_string()
            }),
            chat: quote_chat.map(str::to_string),
            ..Default::default()
        };
        self.store.insert_message(&stored)?;
        let _ = self.events.send(ServiceEvent::hint(&stored, true));
        Ok(())
    }

    /// Stored messages for a chat, newest first.
    pub fn messages(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        self.store.messages_for(chat, limit)
    }

    /// Chat summaries, most recently active first.
    pub fn chats(&self) -> Result<Vec<crate::store::ChatSummary>> {
        self.store.chats()
    }
}

/// Marks a message context as forwarded, keeping anything already in it (a quote).
pub(super) fn forwarded_context(context: Option<Box<wa::ContextInfo>>) -> Box<wa::ContextInfo> {
    let mut context = context.unwrap_or_default();
    context.is_forwarded = Some(true);
    context.forwarding_score = Some(context.forwarding_score.unwrap_or(0) + 1);
    context
}

/// Whether a received message carries the "Forwarded" label.
pub(super) fn is_forwarded(message: &wa::Message) -> bool {
    message_context(message).is_some_and(|c| c.is_forwarded == Some(true))
}

/// The new group member tag, if this message sets one; empty clears it.
pub(super) fn member_label_change(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::GROUP_MEMBER_LABEL_CHANGE) {
        return None;
    }
    Some(protocol.member_label.as_option()?.label.clone().unwrap_or_default())
}

/// The edited message's id and its new text (or caption), if this message is an edit.
pub(super) fn edit_of(message: &wa::Message) -> Option<(String, String)> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::MESSAGE_EDIT) {
        return None;
    }
    let target = protocol.key.as_option()?.id.clone().filter(|id| !id.is_empty())?;
    let edited = protocol.edited_message.as_option()?;
    let text = edited.text_content().or_else(|| edited.get_caption())?.to_string();
    Some((target, text))
}

/// The id of the message a revoke refers to, if this message is a revoke.
pub(super) fn revoke_target(message: &wa::Message) -> Option<String> {
    use wa::message::protocol_message::Type;
    let protocol = message.get_base_message().protocol_message.as_option()?;
    if protocol.r#type != Some(Type::REVOKE) {
        return None;
    }
    let key = protocol.key.as_option()?;
    key.id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

/// Extracts the quoted message id and text from an incoming message.
///
/// `context_info` lives on each inner message type rather than on `Message`
/// itself, so the carriers a reply can arrive on are checked in turn.
fn quote_of(message: &wa::Message) -> Option<(String, String, String, String, Option<Vec<u8>>, Option<String>)> {
    let base = message.get_base_message();
    let context = base
        .extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            base.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })?;

    let id = context.stanza_id.as_ref()?.to_string();
    let author = context
        .participant
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let quoted = context.quoted_message.as_option()?;
    // A quoted media message has no text, so name its type instead of saying
    // "media". The caption, when there is one, wins.
    let (kind, name) = if quoted.image_message.as_option().is_some() {
        ("image", None)
    } else if quoted.video_message.as_option().is_some() {
        ("video", None)
    } else if quoted.audio_message.as_option().is_some() {
        ("audio", None)
    } else if let Some(document) = quoted.document_message.as_option() {
        ("document", document.file_name.clone())
    } else {
        ("", None)
    };
    let text = quoted
        .text_content()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .unwrap_or_else(|| match kind {
            "image" => "Photo".to_string(),
            "video" => "Video".to_string(),
            "audio" => "Voice message".to_string(),
            "document" => name.unwrap_or_else(|| "Document".to_string()),
            _ => "[media]".to_string(),
        });
    let thumb = quoted
        .image_message
        .as_option()
        .and_then(|m| m.jpeg_thumbnail.clone())
        .or_else(|| {
            quoted
                .video_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        })
        .or_else(|| {
            quoted
                .document_message
                .as_option()
                .and_then(|m| m.jpeg_thumbnail.clone())
        });
    Some((id, author, text, kind.to_string(), thumb, context.remote_jid.clone()))
}

/// Converts a protocol message into a storable one, downloading any media.
///
/// Returns `None` for messages that carry neither text nor media, so protocol
/// traffic does not fill the store with empty rows.
pub(super) async fn incoming_message(
    inbound: &InboundMessage,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let info = &inbound.info;
    let header = MessageHeader {
        chat: info.source.chat.to_string(),
        id: info.id.to_string(),
        sender: info.source.sender.to_string(),
        timestamp: info.timestamp.timestamp(),
        from_me: info.source.is_from_me,
    };
    stored_message(&inbound.message, header, client, media_dir, auto_download).await
}

/// Builds the stored form of a message that arrived live or through history sync.
pub(super) async fn stored_message(
    message: &wa::Message,
    header: MessageHeader,
    client: Option<&Client>,
    media_dir: Option<&Path>,
    auto_download: bool,
) -> Option<StoredMessage> {
    let id = header.id.as_str();
    let mut text = message.text_content().unwrap_or_default().to_string();

    let mut media_kind = None;
    let mut media_path = None;
    let mut media_thumb = None;
    let mut media_ref = None;
    let mut media_duration = None;

    if let Some(media) = detect_media(message) {
        media_kind = Some(media.kind.to_string());
        media_duration = media.duration;

        // The thumbnail rides in the message, so it is kept even when the file
        // itself is not downloaded.
        media_thumb = media.thumb.as_deref().map(thumb_uri);

        if auto_download {
            // Download when a destination and a client are available. A failure
            // still records the message, so the text and metadata are not lost.
            if let (Some(client), Some(dir)) = (client, media_dir) {
                let started = std::time::Instant::now();
                match client.download(media.downloadable.as_ref()).await {
                    Ok(bytes) => {
                        log::debug!("downloaded {id} {} ({} KB) in {:?}", media.kind, bytes.len() / 1024, started.elapsed());
                        if std::fs::create_dir_all(dir).is_ok() {
                            let path = dir.join(format!("{}.{}", id, media.extension()));
                            if std::fs::write(&path, &bytes).is_ok() {
                                media_path = Some(path.to_string_lossy().to_string());
                            }
                        }
                    }
                    Err(e) => log::warn!("failed to download {id} media: {e}"),
                }
            }
        } else {
            media_ref = Some(media_locator(message));
        }

        if text.is_empty() {
            text = format!("[{}]", media.kind);
        }
    }

    // Polls and events render as cards; the row carries their title.
    if media_kind.is_none() {
        if let Some((question, _, _)) = poll_of(message) {
            text = question;
            media_kind = Some("poll".to_string());
        } else if let Some(event) = event_of(message) {
            text = event.name;
            media_kind = Some("event".to_string());
        }
    }

    // Kinds without a dedicated view still arrive as readable cards.
    if text.is_empty() && media_kind.is_none() {
        if let Some((kind, card, thumb)) = card_of(message) {
            text = card;
            media_kind = Some(kind.to_string());
            media_thumb = thumb.as_deref().map(thumb_uri);
        }
    }

    if text.is_empty() && media_kind.is_none() {
        return None;
    }

    // A reply carries the quote in the message context. We do not keep the
    // original protobuf, so the text is copied out for display.
    let quote = quote_of(message)
        .map(|(id, sender, text, kind, thumb, chat)| {
            // Quoting our own message should read "You", not our phone number.
            let mine = client
                .map(|c| {
                    [c.pn(), c.lid()]
                        .into_iter()
                        .flatten()
                        .any(|j| j.to_non_ad().to_string() == sender)
                })
                .unwrap_or(false);
            let sender = if mine { "@me".to_string() } else { sender };
            Quote {
                id: Some(id),
                text: Some(text),
                sender: Some(sender),
                chat,
                kind: if kind.is_empty() { None } else { Some(kind) },
                thumb: thumb.as_deref().map(thumb_uri),
            }
        })
        .unwrap_or_default();

    // Names are resolved separately and joined by the store on read; a new
    // message stays unread until its chat is opened.
    Some(StoredMessage {
        header,
        text,
        media: Media { kind: media_kind, path: media_path, thumb: media_thumb, duration: media_duration, locator: media_ref },
        quote,
        link: link_preview(message),
        ..Default::default()
    })
}

/// Location and contact messages as a kind, readable text and the map
/// thumbnail when one was sent; `None` for anything else.
fn card_of(message: &wa::Message) -> Option<(&'static str, String, Option<Vec<u8>>)> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    let lines = |parts: Vec<Option<String>>| {
        parts.into_iter().flatten().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n")
    };
    let map = |lat: Option<f64>, lng: Option<f64>| Some(format!("https://maps.google.com/?q={},{}", lat?, lng?));
    if let Some(at) = base.location_message.as_option() {
        let text = lines(vec![at.name.clone(), at.address.clone(), map(at.degrees_latitude, at.degrees_longitude)]);
        return Some(("location", text, at.jpeg_thumbnail.clone()));
    }
    if let Some(at) = base.live_location_message.as_option() {
        let text = lines(vec![at.caption.clone(), map(at.degrees_latitude, at.degrees_longitude)]);
        return Some(("live_location", text, at.jpeg_thumbnail.clone()));
    }
    // A vCard's TEL lines carry the numbers.
    let phones = |vcard: &Option<String>| {
        vcard
            .iter()
            .flat_map(|v| v.lines())
            .filter(|l| l.to_ascii_uppercase().starts_with("TEL"))
            .filter_map(|l| l.split_once(':').map(|(_, n)| n.trim().to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(contact) = base.contact_message.as_option() {
        let text = lines(vec![contact.display_name.clone(), Some(phones(&contact.vcard))]);
        return Some(("contact", text, None));
    }
    if let Some(list) = base.contacts_array_message.as_option() {
        let people = list
            .contacts
            .iter()
            .map(|c| lines(vec![c.display_name.clone(), Some(phones(&c.vcard))]).replace('\n', " · "))
            .collect::<Vec<_>>();
        return Some(("contact", lines(vec![list.display_name.clone(), Some(people.join("\n"))]), None));
    }
    None
}

/// A received thumbnail as a `data:` URI, stored in the row rather than as one
/// file per message.
pub(super) fn thumb_uri(jpeg: &[u8]) -> String {
    use base64::Engine as _;
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg))
}

/// The message cut down to what `download_media` needs: the media entry
/// without its thumbnail (kept in the row) or the context it quotes.
fn media_locator(message: &wa::Message) -> Vec<u8> {
    let mut slim = wa::Message {
        image_message: message.image_message.clone(),
        video_message: message.video_message.clone(),
        audio_message: message.audio_message.clone(),
        document_message: message.document_message.clone(),
        sticker_message: message.sticker_message.clone(),
        ..Default::default()
    };
    if let Some(m) = slim.image_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.video_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.audio_message.as_option_mut() {
        m.context_info = Default::default();
    }
    if let Some(m) = slim.document_message.as_option_mut() {
        m.jpeg_thumbnail = None;
        m.context_info = Default::default();
    }
    if let Some(m) = slim.sticker_message.as_option_mut() {
        m.context_info = Default::default();
    }
    buffa::Message::encode_to_vec(&slim)
}

/// The link preview a message carries.
fn link_preview(message: &wa::Message) -> LinkCard {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let Some(text) = message
        .get_base_message()
        .extended_text_message
        .as_option()
    else {
        return LinkCard::default();
    };
    // `matched_text` is the URL as it appeared in the message.
    let Some(url) = text.matched_text.clone() else {
        return LinkCard::default();
    };
    LinkCard {
        url: Some(url),
        title: text.title.clone(),
        desc: text.description.clone(),
        thumb: text.jpeg_thumbnail.as_deref().map(thumb_uri),
        ..Default::default()
    }
}

/// The JIDs a message mentions, from whichever message type carries them.
fn message_context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    use whatsapp_rust::wacore::proto_helpers::MessageExt;
    let base = message.get_base_message();
    base.extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            base.image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
}

/// Whether a message mentions us: directly, or everyone through @all.
pub(super) fn mentions_me(message: &wa::Message, own: &[String]) -> bool {
    let Some(context) = message_context(message) else {
        return false;
    };
    if !context.group_mentions.is_empty() {
        return true;
    }
    context.mentioned_jid.iter().any(|mention| {
        let bare = mention.split(':').next().unwrap_or(mention);
        own.iter().any(|me| me == mention || me == bare)
    })
}
