use super::*;
use whatsapp_rust::wacore::types::message::MessageInfo;

impl Inbound {
    pub(super) async fn retire_unavailable(&self, store: &StoreWorker, chat: &str, id: &str) {
        if let Some(header) = store.retire_unavailable(chat, id).await.observed().flatten() {
            let message = StoredMessage { header, local: LocalState { read: true, deleted: true, ..Default::default() }, ..Default::default() };
            let _ = self.events.send(ServiceEvent::hint(&message, false));
        }
    }

    pub(super) async fn store_unavailable(&self, info: &MessageInfo) {
        if self.one_time_only { return; }
        let chat = canonical_chat(None, &self.store, &info.source.chat, &info.source.sender,
            info.source.sender_alt.as_ref()).await;
        let header = MessageHeader {
            chat, id: info.id.to_string(), sender: info.source.sender.to_string(),
            timestamp: info.timestamp.timestamp(), from_me: info.source.is_from_me,
        };
        if let Some(stored) = self.store.insert_unavailable(&header).await.observed().flatten() {
            let _ = self.events.send(ServiceEvent::hint(&stored, false));
        }
    }

    pub(super) async fn history_unavailable(&self, store: &StoreWorker, chat: &str, web: &wa::WebMessageInfo, own: Option<&str>) -> bool {
        let Some(key) = web.key.as_option() else { return false };
        let Some(id) = key.id.clone() else { return false };
        let from_me = key.from_me.unwrap_or(false);
        let header = MessageHeader {
            chat: chat.to_string(), id,
            sender: web.participant.clone().or_else(|| key.participant.clone())
                .unwrap_or_else(|| if from_me { own.unwrap_or_default().to_owned() } else { chat.to_owned() }),
            timestamp: web.message_timestamp.unwrap_or(0) as i64, from_me,
        };
        store.insert_unavailable(&header).await.observed().flatten().is_some()
    }
}

#[cfg(test)]
#[path = "unavailable_tests.rs"]
mod tests;
