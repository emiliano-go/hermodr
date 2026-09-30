//! Delivery state: incoming receipts and acks, and the read receipts we send.

use super::*;
use whatsapp_rust::wacore::types::events::{Receipt, ServerAck};

impl Inbound {
    // A receipt names the messages it refers to, so the
    // outgoing row can move to delivered or read.
    pub(super) async fn on_receipt(&self, receipt: &Receipt) {
        let Self { store, events, .. } = self;
        let status = match receipt.r#type {
            ReceiptType::Read
            | ReceiptType::ReadSelf
            | ReceiptType::Played
            | ReceiptType::PlayedSelf => Some("read"),
            ReceiptType::Delivered | ReceiptType::Sender => {
                Some("delivered")
            }
            ReceiptType::Sent => Some("sent"),
            _ => None,
        };
        // Kept per recipient for the message info screen; our
        // own devices' receipts say nothing about the others.
        let kind = match receipt.r#type {
            ReceiptType::Delivered => Some("delivered"),
            ReceiptType::Read => Some("read"),
            ReceiptType::Played => Some("played"),
            _ => None,
        };
        if let Some(kind) = kind {
            let recipient = receipt.source.sender.to_non_ad().to_string();
            let at = receipt.timestamp.timestamp();
            for id in receipt.message_ids.iter() {
                store.record_receipt(id.as_str(), &recipient, kind, at).await.logged();
            }
        }
        if let Some(status) = status {
            let chat = receipt.source.chat.to_string();
            for id in receipt.message_ids.iter() {
                if let Some(true) =
                    store.set_delivery_state(&chat, id.as_str(), status).await.observed()
                {
                    if let Some(updated) = store.message(&chat, id.as_str()).await.observed() {
                        let _ = events.send(ServiceEvent::status(&updated, status));
                    }
                } else if let Some(updated) =
                    store.set_delivery_state_by_id(id.as_str(), status).await.observed()
                {
                    for message in updated {
                        let _ = events.send(ServiceEvent::status(&message, status));
                    }
                }
            }
        }
    }

    // The server accepted our stanza, so it is at least sent.
    // The ack only sometimes names the chat, and the named
    // JID can differ in form from the stored one, so the
    // id alone is the reliable correlator.
    pub(super) async fn on_server_ack(&self, ack: &ServerAck) {
        let Self { store, events, .. } = self;
        let accepted = ack.error.is_none();
        let is_message =
            matches!(ack.class.as_deref(), None | Some("message"));
        if accepted && is_message {
            let mut done = false;
            if let Some(chat) = ack.from.as_ref() {
                let chat = chat.to_string();
                if let Some(true) = store.set_delivery_state(&chat, &ack.id, "sent").await.observed()
                {
                    if let Some(updated) = store.message(&chat, &ack.id).await.observed() {
                        let _ = events.send(ServiceEvent::status(&updated, "sent"));
                    }
                    done = true;
                }
            }
            if !done {
                if let Some(updated) =
                    store.set_delivery_state_by_id(&ack.id, "sent").await.observed()
                {
                    for message in updated {
                        let _ = events.send(ServiceEvent::status(&message, "sent"));
                    }
                }
            }
        }
    }
}

impl WhatsAppService {
    /// Whether the account has read receipts turned off in its privacy
    /// settings. The protocol client keeps this in sync with the server; when
    /// true, read and played receipts must not be sent.
    pub fn read_receipts_disabled(&self) -> bool {
        self.client
            .persistence_manager()
            .get_device_snapshot()
            .read_receipts_disabled
    }

    /// Marks a chat's incoming messages as read, and with `receipts` tells
    /// their senders. Returns how many changed.
    pub async fn mark_read(&self, chat: &str, receipts: bool) -> Result<usize> {
        let unread = if receipts { self.store.unread_ids(chat).await? } else { Vec::new() };
        let changed = self.store.mark_read(chat).await?;
        self.send_read_receipts(chat, unread).await?;
        if changed > 0 {
            let range = self.read_range(chat, None).await;
            self.sync_chat_read(chat, range).await;
        }
        self.clear_unread_mark(chat).await;
        Ok(changed)
    }

    /// Marks incoming messages up to and including `id` as read.
    ///
    /// Used when a chat is opened at its unread divider: only what has actually
    /// been scrolled past is read, so messages below stay unread.
    pub async fn mark_read_until(&self, chat: &str, id: &str, receipts: bool) -> Result<usize> {
        let unread = if receipts { self.store.unread_until(chat, id).await? } else { Vec::new() };
        let changed = self.store.mark_read_until(chat, id).await?;
        self.send_read_receipts(chat, unread).await?;
        if changed > 0 {
            let range = self.read_range(chat, Some(id)).await;
            self.sync_chat_read(chat, range).await;
        }
        self.clear_unread_mark(chat).await;
        Ok(changed)
    }

    /// Tells the account a chat was read, so the phone's badge clears too. This
    /// is the syncd read state, not a receipt: it is sent even with receipts off.
    async fn sync_chat_read(&self, chat: &str, range: Option<whatsapp_rust::SyncActionMessageRange>) {
        let Ok(jid) = chat.parse::<Jid>() else { return };
        if let Err(e) = self.client.chat_actions().mark_chat_as_read(&jid, true, range).await {
            // The one operation a returning user triggers without sending:
            // a timeout here is the first sign of a half-open link.
            self.note_error(&e);
            log::warn!("could not sync the read mark for {chat}: {e}");
        }
    }

    /// The message range naming where a chat was read to: the boundary message
    /// itself, or the newest one when the whole chat was read.
    async fn read_range(
        &self,
        chat: &str,
        up_to: Option<&str>,
    ) -> Option<whatsapp_rust::SyncActionMessageRange> {
        let remote = chat.parse::<Jid>().ok()?;
        let boundary = match up_to {
            Some(id) => self.store.message(chat, id).await.observed()?,
            None => self.store.messages_for(chat, 1).await.observed()?.into_iter().next()?,
        };
        let participant = (remote.is_group() && !boundary.header.from_me)
            .then(|| boundary.header.sender.parse::<Jid>().ok().map(|j| j.to_non_ad()))
            .flatten();
        let key = whatsapp_rust::message_key(
            boundary.header.id.clone(),
            &remote,
            boundary.header.from_me,
            participant.as_ref(),
        );
        Some(whatsapp_rust::message_range(
            boundary.header.timestamp,
            None,
            vec![(key, boundary.header.timestamp)],
        ))
    }

    /// Opening a chat lifts a manual unread mark locally; the account is told
    /// by [`sync_chat_read`](Self::sync_chat_read).
    async fn clear_unread_mark(&self, chat: &str) {
        let Ok(jid) = chat.parse::<Jid>() else { return };
        self.store.clear_marked_unread(&jid.to_non_ad().to_string()).await.logged();
    }

    /// Sends read receipts for the given `(id, sender)` pairs, grouped per author.
    async fn send_read_receipts(&self, chat: &str, unread: Vec<(String, String)>) -> Result<()> {
        if unread.is_empty() {
            return Ok(());
        }
        let to: Jid = chat.parse()?;
        // A group receipt names the author, one receipt per author; a direct
        // chat needs none.
        let mut by_sender: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (id, sender) in unread {
            let key = if to.is_group() { sender } else { String::new() };
            by_sender.entry(key).or_default().push(id);
        }
        for (sender, ids) in by_sender {
            let sender = sender.parse::<Jid>().ok().map(|j| j.to_non_ad());
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            if let Err(e) = self.client.mark_as_read(&to, sender.as_ref(), &ids).await {
                log::warn!("could not send read receipts: {e}");
            }
        }
        Ok(())
    }

    /// Tells the sender that a voice note was played or view-once media opened.
    pub async fn mark_played(&self, chat: &str, id: &str, sender: &str) -> Result<()> {
        let to: Jid = chat.parse()?;
        let sender = if to.is_group() { sender.parse::<Jid>().ok().map(|j| j.to_non_ad()) } else { None };
        self.client
            .mark_as_played(&to, sender.as_ref(), &[id])
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    }
}
