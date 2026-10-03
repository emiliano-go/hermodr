use super::*;
use crate::store::call_history::MAX_CALL_PARTICIPANTS;

fn event(from_me: bool, is_incoming: Option<bool>, peers: &[&str]) -> CallLogSync {
    CallLogSync::builder()
        .call_creator_jid("111@s.whatsapp.net".parse().unwrap()).call_id("index-id".into())
        .from_me(from_me).timestamp(whatsapp_rust::wacore::time::from_millis_or_now(1000))
        .from_full_sync(true)
        .record(Box::new(wa::CallLogRecord {
            is_incoming, call_id: Some("different-record-id".into()),
            call_creator_jid: Some("999@lid".into()), start_time: Some(17), duration: Some(91),
            participants: peers.iter().map(|jid| wa::call_log_record::ParticipantInfo {
                user_jid: Some((*jid).into()), ..Default::default()
            }).collect(), ..Default::default()
        })).build()
}

fn own(jid: &Jid) -> bool { matches!(jid.user.as_str(), "111" | "222") }

#[test]
fn public_event_index_direction_and_raw_clocks_survive_record_conflicts() {
    for from_me in [true, false] {
        for incoming in [None, Some(true), Some(false)] {
            let input = capture_call_record(&event(from_me, incoming, &[]), own).unwrap();
            assert_eq!(input.call_id, "index-id"); assert_eq!(input.creator_jid, "111@s.whatsapp.net");
            assert_eq!(input.from_me, from_me); assert_eq!(input.start_time_raw, Some(17));
            assert_eq!(input.duration_raw, Some(91)); assert_eq!(input.mutation_at_ms, 1000);
            assert!(input.from_full_sync); assert!(input.is_video.is_none());
        }
    }
}

#[test]
fn peer_projection_filters_own_pn_lid_invalid_and_nonuser_participants() {
    let input = capture_call_record(&event(true, None, &[
        "111@s.whatsapp.net", "222@lid", "333@lid", "123@g.us", "not-a-jid",
    ]), own).unwrap();
    assert_eq!(input.peer_jids, ["333@lid"]);
    let own_creator = capture_call_record(&event(true, None, &["111@s.whatsapp.net"]), |_| false).unwrap();
    assert!(own_creator.peer_jids.is_empty());
}

#[test]
fn public_event_capture_rejects_unbounded_participant_payload_before_projection() {
    let peers = vec!["333@lid"; MAX_CALL_PARTICIPANTS + 1];
    assert!(capture_call_record(&event(true, None, &peers), own).is_err());
}

#[tokio::test]
async fn event_to_store_capture_reports_changes_and_replay_once_without_protocol() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let worker = StoreWorker::new(store);
    let event = event(true, Some(true), &["222@lid", "333@lid"]);
    assert!(apply_call_log_event(&worker, &event, own).await.unwrap());
    assert!(!apply_call_log_event(&worker, &event, own).await.unwrap());
    let rows = worker.run(|store| store.call_history(200)).await.unwrap();
    assert_eq!(rows.len(), 1); assert_eq!(rows[0].chat.as_deref(), Some("333@lid"));
    assert!(rows[0].from_me);
}

#[tokio::test]
async fn actual_inbound_call_capture_signals_once_and_companion_does_not_create_calls_or_notices() {
    let (inbound, mut received) = super::super::protocol_tests::inbound().await;
    let mut record = event(false, Some(false), &[]);
    record.record.call_result = Some(wa::call_log_record::CallResult::MISSED); record.record.is_video = Some(true);
    let incoming = Event::CallLogSync(record);
    inbound.handle(&incoming).await;
    assert!(matches!(received.try_recv().unwrap(), ServiceEvent::CallHistoryChanged));
    assert!(matches!(received.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
    inbound.handle(&incoming).await;
    assert!(matches!(received.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
    let calls = inbound.store.run(|store| store.call_history(200)).await.unwrap();
    assert_eq!(calls.len(), 1); assert_eq!(calls[0].outcome, crate::store::call_history::CallOutcome::Missed);
    assert_eq!(inbound.store.count().await.unwrap(), 0);
    assert!(inbound.store.chats().await.unwrap().is_empty());
    let (mut companion, mut ignored) = super::super::protocol_tests::inbound().await;
    companion.one_time_only = true;
    companion.handle(&incoming).await;
    assert!(matches!(ignored.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
    assert!(companion.store.run(|store| store.call_history(200)).await.unwrap().is_empty());
    assert_eq!(companion.store.count().await.unwrap(), 0);
    assert!(companion.store.chats().await.unwrap().is_empty());
}
