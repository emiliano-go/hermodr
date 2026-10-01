use super::*;
use whatsapp_rust::wacore::types::events::{LabelAssociationUpdate, LabelEditUpdate, MessageLabelAssociationUpdate};

#[test]
fn create_rejects_known_and_deleted_ids_while_edit_requires_present_label() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    assert!(validate_label_id("opaque-id").is_ok());
    assert!(validate_label_id("").is_err());
    assert!(validate_label_save(&store, "opaque-id", true).is_ok());
    assert!(validate_label_save(&store, "opaque-id", false).is_err());
    store.set_label("opaque-id", Some("Name"), None, Some(false), 1).unwrap();
    assert!(validate_label_save(&store, "opaque-id", true).is_err());
    assert!(validate_label_save(&store, "opaque-id", false).is_ok());
    store.set_label("opaque-id", None, None, Some(true), 2).unwrap();
    assert!(validate_label_save(&store, "opaque-id", true).is_err());
    assert!(validate_label_save(&store, "opaque-id", false).is_err());
    store.set_label("partial", None, Some(3), None, 1).unwrap();
    assert!(validate_label_save(&store, "partial", true).is_err());
    assert!(validate_label_save(&store, "partial", false).is_err());
}

#[test]
fn only_ordinary_available_messages_can_be_labeled() {
    let ordinary = StoredMessage::default();
    assert!(validate_label_message(&ordinary).is_ok());
    for field in 0..8 {
        let mut message = ordinary.clone();
        match field {
            0 => message.spoiler = true,
            1 => message.local.deleted = true,
            2 => message.local.revoked = true,
            3 => message.media.once_kind = Some("image".into()),
            4 => message.media.kind = Some("view_once".into()),
            5 => message.media.kind = Some("unknown".into()),
            6 => message.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
            _ => message.system.kind = Some("GROUP_CHANGE".into()),
        }
        assert!(validate_label_message(&message).is_err(), "field {field}");
    }
    for chat in ["1@s.whatsapp.net", "123@lid", "1-2@g.us"] { assert!(label_chat_jid(chat).is_ok()); }
    for chat in ["", "status@broadcast", "123@newsletter", "@g.us"] { assert!(label_chat_jid(chat).is_err(), "{chat}"); }
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut message = ordinary.clone();
    message.header.chat = "1@g.us".into();
    message.header.id = "m".into();
    store.insert_message(&message).unwrap();
    assert!(validate_stored_label_message(&store, "1@g.us", "m").is_ok());
    store.set_view_once("1@g.us", "m", false).unwrap();
    assert!(validate_stored_label_message(&store, "1@g.us", "m").is_err());
    assert!(validate_stored_label_message(&store, "1@g.us", "absent").is_err());
}

#[tokio::test]
async fn label_events_replay_without_reconciling_absent_rows_or_completing_baseline() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    store.run(|s| s.set_label("retained", Some("Retained"), None, Some(false), 1)).await.unwrap();
    let timestamp = whatsapp_rust::wacore::time::from_millis_or_now(10);
    let edit = Event::LabelEditUpdate(LabelEditUpdate::builder().label_id("a".into())
        .timestamp(timestamp).action(Box::new(wa::sync_action_value::LabelEditAction {
            name: Some("A".into()), color: Some(-100), deleted: Some(false), ..Default::default()
        })).from_full_sync(true).build());
    let chat = Event::LabelAssociationUpdate(LabelAssociationUpdate::builder().label_id("a".into())
        .chat_jid("1@g.us".parse().unwrap()).timestamp(timestamp)
        .action(Box::new(wa::sync_action_value::LabelAssociationAction { labeled: Some(true), ..Default::default() }))
        .from_full_sync(true).build());
    let message = Event::MessageLabelAssociationUpdate(MessageLabelAssociationUpdate::builder().label_id("a".into())
        .chat_jid("1@g.us".parse().unwrap()).message_id("m".into()).timestamp(timestamp)
        .action(Box::new(wa::sync_action_value::LabelAssociationAction { labeled: Some(true), ..Default::default() }))
        .from_full_sync(true).build());
    for event in [&edit, &chat, &message] {
        assert!(apply_label_event(&store, event).await.unwrap());
        assert!(!apply_label_event(&store, event).await.unwrap());
    }
    let no_delta = Event::LabelAssociationUpdate(LabelAssociationUpdate::builder().label_id("a".into())
        .chat_jid("2@g.us".parse().unwrap()).timestamp(timestamp)
        .action(Box::default()).from_full_sync(false).build());
    assert!(!apply_label_event(&store, &no_delta).await.unwrap());
    let view = store.run(MessageStore::labels_view).await.unwrap();
    assert!(!view.complete);
    assert_eq!(view.labels.len(), 2);
    assert_eq!(view.chats.len(), 1);
    assert_eq!(view.messages.len(), 1);
}

#[tokio::test]
async fn label_write_rejects_stale_owner_before_await_without_writes_or_events() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let (events, mut received) = broadcast::channel(4);
    let written = Arc::new(AtomicBool::new(false));
    let marker = written.clone();
    assert!(write_label(&store, &events, || anyhow::bail!("stale owner"), move |_| {
        marker.store(true, Ordering::SeqCst); Ok(true)
    }).await.is_err());
    assert!(!written.load(Ordering::SeqCst));
    assert!(received.try_recv().is_err());
}

#[tokio::test]
async fn queued_label_write_checks_exact_owner_after_gate_await() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let batch = store.batch().await;
    let expected = Arc::new(());
    let active = Arc::new(Mutex::new(expected.clone()));
    let slot = active.clone();
    let checked = Arc::new(tokio::sync::Notify::new());
    let signal = checked.clone();
    let (events, mut received) = broadcast::channel(4);
    let queued = store.clone();
    let operation = tokio::spawn(async move {
        write_label(&queued, &events, move || {
            signal.notify_one();
            anyhow::ensure!(Arc::ptr_eq(&expected, &slot.lock().unwrap()), "stale owner");
            Ok(())
        }, |s| s.set_label("a", Some("Wrong owner"), None, Some(false), 1)).await
    });
    checked.notified().await;
    *active.lock().unwrap() = Arc::new(());
    batch.finish().await.unwrap();
    assert!(operation.await.unwrap().is_err());
    assert!(store.run(MessageStore::labels_view).await.unwrap().labels.is_empty());
    assert!(received.try_recv().is_err());
}

#[tokio::test]
async fn label_write_rechecks_owner_after_commit_before_emitting() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let (events, mut received) = broadcast::channel(4);
    let checks = Arc::new(AtomicU64::new(0));
    let counter = checks.clone();
    assert!(write_label(&store, &events, move || {
        anyhow::ensure!(counter.fetch_add(1, Ordering::SeqCst) < 2, "stale owner after await");
        Ok(())
    }, |s| s.set_label("a", Some("Committed while owned"), None, Some(false), 1)).await.is_err());
    assert_eq!(checks.load(Ordering::SeqCst), 3);
    assert_eq!(store.run(MessageStore::labels_view).await.unwrap().labels.len(), 1);
    assert!(received.try_recv().is_err());
    write_label(&store, &events, || Ok(()), |s| s.set_label("a", Some("Changed"), None, None, 2)).await.unwrap();
    assert!(matches!(received.try_recv().unwrap(), ServiceEvent::LabelsChanged));
    write_label(&store, &events, || Ok(()), |s| s.set_label("a", Some("Changed"), None, None, 2)).await.unwrap();
    assert!(received.try_recv().is_err());
}
