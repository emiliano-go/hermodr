//! Delivery state: incoming receipts and acks, and the read receipts we send.

use super::*;
use whatsapp_rust::wacore::types::events::{Receipt, ServerAck};

impl Inbound {
    // A receipt names the messages it refers to, so the
    // outgoing row can move to delivered or read.
    pub(super) fn on_receipt(&self, receipt: &Receipt) {
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
                store.record_receipt(id.as_str(), &recipient, kind, at).logged();
            }
        }
        if let Some(status) = status {
            let chat = receipt.source.chat.to_string();
            for id in receipt.message_ids.iter() {
                if let Ok(true) =
                    store.set_delivery_state(&chat, id.as_str(), status)
                {
                    if let Ok(updated) = store.message(&chat, id.as_str()) {
                        let _ = events.send(ServiceEvent::hint(&updated, false));
                    }
                } else if let Ok(updated) =
                    store.set_delivery_state_by_id(id.as_str(), status)
                {
                    for message in updated {
                        let _ = events.send(ServiceEvent::hint(&message, false));
                    }
                }
            }
        }
    }

    // The server accepted our stanza, so it is at least sent.
    // The ack only sometimes names the chat, and the named
    // JID can differ in form from the stored one, so the
    // id alone is the reliable correlator.
    pub(super) fn on_server_ack(&self, ack: &ServerAck) {
        let Self { store, events, .. } = self;
        let accepted = ack.error.is_none();
        let is_message =
            matches!(ack.class.as_deref(), None | Some("message"));
        if accepted && is_message {
            let mut done = false;
            if let Some(chat) = ack.from.as_ref() {
                let chat = chat.to_string();
                if let Ok(true) = store.set_delivery_state(&chat, &ack.id, "sent")
                {
                    if let Ok(updated) = store.message(&chat, &ack.id) {
                        let _ = events.send(ServiceEvent::hint(&updated, false));
                    }
                    done = true;
                }
            }
            if !done {
                if let Ok(updated) =
                    store.set_delivery_state_by_id(&ack.id, "sent")
                {
                    for message in updated {
                        let _ = events.send(ServiceEvent::hint(&message, false));
                    }
                }
            }
        }
    }
}

impl Service {
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
        let unread = if receipts { self.store.unread_ids(chat)? } else { Vec::new() };
        let changed = self.store.mark_read(chat)?;
        self.send_read_receipts(chat, unread).await?;
        self.clear_unread_mark(chat).await;
        Ok(changed)
    }

    /// Marks incoming messages up to and including `id` as read.
    ///
    /// Used when a chat is opened at its unread divider: only what has actually
    /// been scrolled past is read, so messages below stay unread.
    pub async fn mark_read_until(&self, chat: &str, id: &str, receipts: bool) -> Result<usize> {
        let unread = if receipts { self.store.unread_until(chat, id)? } else { Vec::new() };
        let changed = self.store.mark_read_until(chat, id)?;
        self.send_read_receipts(chat, unread).await?;
        self.clear_unread_mark(chat).await;
        Ok(changed)
    }

    /// Opening a chat lifts a manual unread mark, here and on the account.
    async fn clear_unread_mark(&self, chat: &str) {
        let Ok(jid) = chat.parse::<Jid>() else { return };
        if !self.store.clear_marked_unread(&jid.to_non_ad().to_string()).unwrap_or(false) {
            return;
        }
        if let Err(e) = self.client.chat_actions().mark_chat_as_read(&jid, true, None).await {
            log::warn!("could not sync the read mark: {e}");
        }
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
