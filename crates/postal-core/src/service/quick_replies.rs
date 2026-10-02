use super::*;
use crate::store::quick_replies::{QuickReply, QuickRepliesView};
use whatsapp_rust::AppStateResyncMode;

pub(super) async fn apply_quick_reply_event(store: &StoreWorker, event: &Event) -> Result<bool> {
    let Event::QuickReplyUpdate(update) = event else { return Ok(false); };
    let (id, timestamp) = (update.id.clone(), update.timestamp.timestamp_millis());
    let action = &update.action;
    let reply = if action.deleted == Some(true) {
        None
    } else {
        Some(QuickReply {
            id: id.clone(),
            shortcut: action.shortcut.clone().ok_or_else(|| anyhow::anyhow!("quick reply has no shortcut"))?,
            message: action.message.clone().ok_or_else(|| anyhow::anyhow!("quick reply has no message"))?,
            keywords: action.keywords.clone(), count: action.count.unwrap_or(0),
            associated_label_ids: action.associated_label_ids.clone(),
        })
    };
    store.run(move |store| store.set_quick_reply(&id, reply, timestamp)).await
}

impl WhatsAppService {
    pub async fn quick_replies_view(&self) -> Result<QuickRepliesView> {
        self.store.run(|store| Ok(QuickRepliesView {
            complete: false, replies: store.quick_replies()?,
        })).await
    }

    pub async fn sync_quick_replies(&self, current: impl Fn() -> Result<()> + Send) -> Result<()> {
        for mode in [AppStateResyncMode::Incremental, AppStateResyncMode::Snapshot] {
            current()?;
            anyhow::ensure!(self.is_connected(), "not connected yet");
            let report = self.client.resync_app_state([WAPatchName::Regular], mode).await?;
            current()?;
            anyhow::ensure!(report.all_synced() && report.synced.contains(&WAPatchName::Regular),
                "quick replies are still synchronizing");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::{wacore::types::events::QuickReplyUpdate, waproto::whatsapp::sync_action_value::QuickReplyAction};

    fn update(action: QuickReplyAction, timestamp: i64, full: bool) -> Event {
        Event::QuickReplyUpdate(QuickReplyUpdate::builder()
            .id("opaque-id".to_owned())
            .timestamp(whatsapp_rust::wacore::time::from_millis_or_now(timestamp))
            .action(Box::new(action)).from_full_sync(full).build())
    }

    #[tokio::test]
    async fn live_and_full_sync_events_store_once_then_apply_deletion() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let action = QuickReplyAction {
            shortcut: Some("thanks".into()), message: Some("Thank you".into()),
            keywords: vec!["payment".into()], associated_label_ids: vec!["billing".into()],
            ..Default::default()
        };
        assert!(apply_quick_reply_event(&store, &update(action.clone(), 10, false)).await.unwrap());
        assert!(!apply_quick_reply_event(&store, &update(action, 10, true)).await.unwrap());
        let rows = store.run(MessageStore::quick_replies).await.unwrap();
        assert_eq!(rows[0].count, 0);
        assert_eq!(rows[0].associated_label_ids, vec!["billing"]);
        let delete = update(QuickReplyAction { deleted: Some(true), ..Default::default() }, 20, true);
        assert!(apply_quick_reply_event(&store, &delete).await.unwrap());
        assert!(store.run(MessageStore::quick_replies).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn malformed_live_event_does_not_erase_cached_reply() {
        let store = StoreWorker::new(MessageStore::open(Path::new(":memory:")).unwrap());
        let live = update(QuickReplyAction {
            shortcut: Some("hello".into()), message: Some("Hello".into()), ..Default::default()
        }, 10, false);
        apply_quick_reply_event(&store, &live).await.unwrap();
        let malformed = update(QuickReplyAction { shortcut: Some("hello".into()), ..Default::default() }, 20, false);
        assert!(apply_quick_reply_event(&store, &malformed).await.is_err());
        assert_eq!(store.run(MessageStore::quick_replies).await.unwrap()[0].message, "Hello");
        let connected = Event::Connected(whatsapp_rust::wacore::types::events::Connected::builder().build());
        assert!(!apply_quick_reply_event(&store, &connected).await.unwrap());
    }
}
