use super::*;
use std::sync::atomic::Ordering;
use whatsapp_rust::wacore::{net::DisconnectReason, types::events::{ConnectFailureReason, Disconnected, LoggedOut}};

async fn fixture(one_time_only: bool) -> (Inbound, broadcast::Receiver<ServiceEvent>, Vec<StoredMessage>) {
    let (mut inbound, notices) = protocol_tests::inbound().await;
    inbound.one_time_only = one_time_only;
    let mut rows = Vec::new();
    for (chat, id, text) in [("77@lid", "cached-direct", "Saved direct message"), ("123@g.us", "cached-group", "Saved group message")] {
        let mut row = StoredMessage::default();
        row.header.chat = chat.into();
        row.header.id = id.into();
        row.header.sender = "77@lid".into();
        row.text = text.into();
        inbound.store.insert_message_row(&row).await.unwrap();
        rows.push(inbound.store.message(chat, id).await.unwrap());
    }
    assert!(inbound.client_for_events.get().is_none());
    (inbound, notices, rows)
}

async fn assert_cached_rows(inbound: &Inbound, before: &[StoredMessage]) {
    for row in before {
        let current = inbound.store.message(&row.header.chat, &row.header.id).await.unwrap();
        assert_eq!(serde_json::to_value(current).unwrap(), serde_json::to_value(row).unwrap());
    }
    assert!(inbound.client_for_events.get().is_none());
}

#[tokio::test]
async fn disconnect_clears_primary_and_companion_links_preserving_cached_rows() {
    for one_time_only in [false, true] {
        let (inbound, mut notices, rows) = fixture(one_time_only).await;
        let event = Event::Disconnected(Disconnected::builder().reason(DisconnectReason::StreamEnded).build());
        inbound.connected.store(true, Ordering::SeqCst);
        inbound.handle(&event).await;
        assert!(!inbound.connected.load(Ordering::SeqCst));
        assert!(matches!(notices.try_recv().unwrap(), ServiceEvent::Disconnected));
        assert!(matches!(notices.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
        assert_cached_rows(&inbound, &rows).await;
        inbound.handle(&event).await;
        assert!(!inbound.connected.load(Ordering::SeqCst));
        assert!(matches!(notices.try_recv().unwrap(), ServiceEvent::Disconnected));
        assert!(matches!(notices.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
        assert_cached_rows(&inbound, &rows).await;
    }
}

#[tokio::test]
async fn logout_clears_primary_and_companion_links_preserving_cached_rows() {
    for one_time_only in [false, true] {
        let (inbound, mut notices, rows) = fixture(one_time_only).await;
        let event = Event::LoggedOut(Box::new(LoggedOut::builder().on_connect(false).reason(ConnectFailureReason::LoggedOut).build()));
        inbound.connected.store(true, Ordering::SeqCst);
        inbound.handle(&event).await;
        assert!(!inbound.connected.load(Ordering::SeqCst));
        assert!(matches!(notices.try_recv().unwrap(), ServiceEvent::LoggedOut));
        assert!(matches!(notices.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
        assert_cached_rows(&inbound, &rows).await;
        inbound.handle(&event).await;
        assert!(!inbound.connected.load(Ordering::SeqCst));
        assert!(matches!(notices.try_recv().unwrap(), ServiceEvent::LoggedOut));
        assert!(matches!(notices.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
        assert_cached_rows(&inbound, &rows).await;
    }
}
