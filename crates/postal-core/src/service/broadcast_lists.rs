use super::*;
use crate::store::broadcast_lists::BroadcastList;

pub(crate) async fn remember_broadcast_list(
    store: &StoreWorker,
    chat: &str,
    source_id: &str,
    recipients: &[Jid],
) -> Result<bool> {
    let recipients = recipients
        .iter()
        .map(|jid| jid.to_non_ad().to_string())
        .collect();
    store
        .remember_broadcast_list(chat, source_id, recipients)
        .await
}

pub fn writable_target(chat: &str) -> Result<Jid> {
    let target: Jid = chat.parse()?;
    anyhow::ensure!(
        !target.is_broadcast_list(),
        "Sending to broadcast lists is not supported."
    );
    Ok(target)
}

impl WhatsAppService {
    pub async fn broadcast_list(&self, chat: &str) -> Result<Option<BroadcastList>> {
        self.store.broadcast_list(chat).await
    }
}

#[cfg(test)]
#[path = "broadcast_lists_tests.rs"]
mod tests;
