use super::*;
use crate::store::labels::LabelsView;
use whatsapp_rust::AppStateResyncMode;

pub(super) async fn replay_labels(client: &Client) -> Result<()> {
    for mode in [AppStateResyncMode::Incremental, AppStateResyncMode::Snapshot] {
        let report = client.resync_app_state([WAPatchName::Regular], mode).await?;
        anyhow::ensure!(report.all_synced() && report.synced.contains(&WAPatchName::Regular), "labels are still synchronizing");
    }
    Ok(())
}

pub(super) async fn apply_label_event(store: &StoreWorker, event: &Event) -> Result<bool> {
    match event {
        Event::LabelEditUpdate(update) => {
            let (id, action, timestamp) = (update.label_id.clone(), update.action.clone(), update.timestamp.timestamp_millis());
            store.run(move |store| store.set_label(&id, action.name.as_deref(), action.color, action.deleted, timestamp)).await
        }
        Event::LabelAssociationUpdate(update) => {
            let Some(labeled) = update.action.labeled else { return Ok(false); };
            let (id, chat, timestamp) = (update.label_id.clone(), update.chat_jid.to_non_ad().to_string(), update.timestamp.timestamp_millis());
            store.run(move |store| store.set_chat_label(&id, &chat, labeled, timestamp)).await
        }
        Event::MessageLabelAssociationUpdate(update) => {
            let Some(labeled) = update.action.labeled else { return Ok(false); };
            let (id, chat, message, timestamp) = (update.label_id.clone(), update.chat_jid.to_non_ad().to_string(), update.message_id.clone(), update.timestamp.timestamp_millis());
            store.run(move |store| store.set_message_label(&id, &chat, &message, labeled, timestamp)).await
        }
        _ => Ok(false),
    }
}

impl WhatsAppService {
    pub async fn labels_view(&self) -> Result<LabelsView> {
        self.store.run(MessageStore::labels_view).await
    }

    pub async fn labelled_messages(&self, label_ids: &[String], chat: Option<&str>, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let label_ids = label_ids.to_vec();
        let chat = chat.map(label_chat_jid).transpose()?.map(|chat| chat.to_string());
        let query = query.to_owned();
        self.store.run(move |store| store.labelled_messages(&label_ids, chat.as_deref(), &query, limit)).await
    }

    pub async fn save_label(
        &self, id: &str, name: &str, color: i32, create: bool,
        current: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        validate_label_id(id)?;
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "enter a label name");
        current()?;
        anyhow::ensure!(self.is_connected(), "not connected yet");
        let label_id = id.to_owned();
        self.store.run(move |store| validate_label_save(store, &label_id, create)).await?;
        current()?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let response = self.client.labels().create_label(id, name, color).await;
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let (id, name) = (id.to_owned(), name.to_owned());
        write_label(&self.store, &self.events, current,
            move |store| store.set_label(&id, Some(&name), Some(color), Some(false), timestamp)).await
    }

    pub async fn delete_label(&self, id: &str, current: impl Fn() -> Result<()> + Send + Sync + 'static) -> Result<()> {
        validate_label_id(id)?;
        current()?;
        anyhow::ensure!(self.is_connected(), "not connected yet");
        self.require_label(id).await?;
        current()?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let response = self.client.labels().delete_label(id).await;
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let id = id.to_owned();
        write_label(&self.store, &self.events, current,
            move |store| store.set_label(&id, None, None, Some(true), timestamp)).await
    }

    pub async fn label_chat(
        &self, id: &str, chat: &str, labeled: bool, current: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        validate_label_id(id)?;
        let chat = label_chat_jid(chat)?;
        current()?;
        anyhow::ensure!(self.is_connected(), "not connected yet");
        self.require_label(id).await?;
        current()?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let labels = self.client.labels();
        let response = if labeled { labels.add_chat_label(id, &chat).await } else { labels.remove_chat_label(id, &chat).await };
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let (id, chat) = (id.to_owned(), chat.to_string());
        write_label(&self.store, &self.events, current,
            move |store| store.set_chat_label(&id, &chat, labeled, timestamp)).await
    }

    pub async fn label_message(
        &self, id: &str, chat: &str, message_id: &str, labeled: bool,
        current: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) -> Result<()> {
        validate_label_id(id)?;
        anyhow::ensure!(!message_id.is_empty(), "choose a message");
        let chat = label_chat_jid(chat)?;
        current()?;
        anyhow::ensure!(self.is_connected(), "not connected yet");
        self.require_label(id).await?;
        current()?;
        let (target, message) = (chat.to_string(), message_id.to_owned());
        self.store.run(move |store| validate_stored_label_message(store, &target, &message)).await?;
        current()?;
        let timestamp = whatsapp_rust::wacore::time::now_millis();
        let labels = self.client.labels();
        let response = if labeled { labels.add_message_label(id, &chat, message_id).await }
            else { labels.remove_message_label(id, &chat, message_id).await };
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let (id, chat, message_id) = (id.to_owned(), chat.to_string(), message_id.to_owned());
        write_label(&self.store, &self.events, current,
            move |store| store.set_message_label(&id, &chat, &message_id, labeled, timestamp)).await
    }

    async fn require_label(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        anyhow::ensure!(self.store.run(move |store| store.label_exists(&id)).await?, "label is no longer available; refresh labels");
        Ok(())
    }
}

fn validate_label_id(id: &str) -> Result<()> {
    anyhow::ensure!(!id.is_empty(), "label id cannot be empty");
    Ok(())
}

fn validate_label_save(store: &MessageStore, id: &str, create: bool) -> Result<()> {
    if create {
        anyhow::ensure!(!store.label_id_known(id)?, "label id already exists; choose a new id");
    } else {
        anyhow::ensure!(store.label_exists(id)?, "label is no longer available; refresh labels");
    }
    Ok(())
}

async fn write_label(
    store: &StoreWorker, events: &broadcast::Sender<ServiceEvent>,
    current: impl Fn() -> Result<()> + Send + Sync + 'static,
    write: impl FnOnce(&MessageStore) -> Result<bool> + Send + 'static,
) -> Result<()> {
    current()?;
    let current = Arc::new(current);
    let owner = current.clone();
    let changed = store.run(move |store| { owner()?; write(store) }).await?;
    current()?;
    if changed { let _ = events.send(ServiceEvent::LabelsChanged); }
    Ok(())
}

fn label_chat_jid(chat: &str) -> Result<Jid> {
    let jid: Jid = chat.parse()?;
    anyhow::ensure!(!jid.user.is_empty() && (jid.is_pn() || jid.is_lid() || jid.is_group()), "choose a contact or group chat");
    Ok(jid.to_non_ad())
}

fn validate_label_message(message: &StoredMessage) -> Result<()> {
    anyhow::ensure!(!message.local.deleted && !message.local.revoked && !message.spoiler
        && message.system.kind.is_none() && message.media.once_kind.is_none()
        && !matches!(message.media.kind.as_deref(), Some("view_once" | "unknown"))
        && !message.is_unavailable(), "this message cannot be labelled");
    Ok(())
}

fn validate_stored_label_message(store: &MessageStore, chat: &str, message_id: &str) -> Result<()> {
    validate_label_message(&store.message(chat, message_id)?)?;
    anyhow::ensure!(!store.is_view_once(chat, message_id)?, "this message cannot be labelled");
    Ok(())
}

#[cfg(test)]
#[path = "labels_tests.rs"]
mod tests;
