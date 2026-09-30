use super::*;
use std::time::Instant;
use whatsapp_rust::{CappingStatus, NewChatMessageCapping};
use whatsapp_rust::wacore::types::events::ServerAck;

const NOTICE_KIND: &str = "NEW_CHAT_MESSAGE_CAPPED";
// ponytail: one check per account/minute; cache results if concurrent chats need notices.
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

impl Inbound {
    pub(super) fn on_message_capping_ack(&self, ack: &ServerAck) {
        if self.one_time_only || !rejected_message(ack) { return; }
        let Some(client) = self.client_for_events.get().cloned() else { return };
        if !self.connected.load(Ordering::SeqCst) || !claim_check(&self.message_capping_check, Instant::now()) { return; }
        let handler = self.clone();
        let ack = ack.clone();
        tokio::spawn(async move {
            let mex = client.mex();
            let info = match tokio::time::timeout(Duration::from_secs(15), mex.fetch_new_chat_message_capping_info()).await {
                Ok(Ok(info)) => info,
                result => { log::warn!("could not check new-chat message cap: {result:?}"); return; }
            };
            if !handler.same_capping_client(&client) { return; }
            let Some((id, params)) = capping_notice(&info, unix_now(), &ack.id) else { return };
            let Some(chat) = capping_chat(&handler.store, &ack).await else { return };
            if !handler.same_capping_client(&client) { return; }
            let timestamp = ack.timestamp.map(|at| at.timestamp()).unwrap_or_else(unix_now);
            handler.store_notice(&chat, id, timestamp, NOTICE_KIND, params, String::new()).await;
        });
    }

    fn same_capping_client(&self, client: &Arc<Client>) -> bool {
        self.connected.load(Ordering::SeqCst) && !client.shutdown_signal().is_fired() &&
            self.client_for_events.get().is_some_and(|current| Arc::ptr_eq(current, client))
    }
}

fn rejected_message(ack: &ServerAck) -> bool {
    ack.error.is_some() && ack.class.as_deref() == Some("message") &&
        !ack.from.as_ref().is_some_and(|jid| jid.is_group())
}

fn claim_check(check: &Mutex<Option<Instant>>, now: Instant) -> bool {
    let mut checked = check.lock().unwrap();
    if checked.is_some_and(|at| now.saturating_duration_since(at) < CHECK_INTERVAL) { return false; }
    *checked = Some(now);
    true
}

fn direct_chat(chat: &str) -> bool {
    chat.rsplit_once('@').is_some_and(|(user, server)| !user.is_empty() && matches!(server, "s.whatsapp.net" | "lid"))
}

async fn capping_chat(store: &StoreWorker, ack: &ServerAck) -> Option<String> {
    if let Some(chat) = store.chat_of_message(&ack.id).await.observed().flatten() {
        let outgoing = store.message(&chat, &ack.id).await.observed().is_some_and(|message| message.header.from_me);
        return (direct_chat(&chat) && outgoing).then_some(chat);
    }
    ack.from.as_ref().map(|jid| jid.to_non_ad().to_string()).filter(|chat| direct_chat(chat))
}

fn capping_notice(info: &NewChatMessageCapping, now: i64, message_id: &str) -> Option<(String, Vec<String>)> {
    if info.capping_status != Some(CappingStatus::Capped) { return None; }
    let now = info.server_sent_timestamp.filter(|at| *at > 0).unwrap_or(now);
    let end = info.cycle_end_timestamp;
    if end.is_some_and(|at| now <= 0 || at <= now) { return None; }
    let cycle = info.cycle_start_timestamp.filter(|at| *at > 0).map(|at| format!("cycle-{at}"))
        .or_else(|| end.map(|at| format!("until-{at}")))
        .unwrap_or_else(|| format!("message-{message_id}"));
    Some((format!("message-capping-{cycle}"), vec![
        info.total_quota.map(|quota| quota.to_string()).unwrap_or_default(),
        info.used_quota.map(|quota| quota.to_string()).unwrap_or_default(),
        end.map(|at| at.to_string()).unwrap_or_default(),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rejected_direct_message_acks_claim_one_bounded_check_per_minute() {
        let ack = |class: Option<&str>, error: Option<&str>, from: Option<&str>| ServerAck::builder()
            .id("synthetic".to_owned()).maybe_class(class.map(str::to_owned)).maybe_error(error.map(str::to_owned))
            .maybe_from(from.map(|chat| chat.parse().unwrap())).build();
        assert!(rejected_message(&ack(Some("message"), Some("unknown-code"), Some("10@s.whatsapp.net"))));
        for event in [ack(Some("message"), None, None), ack(Some("receipt"), Some("479"), None),
            ack(None, Some("479"), None), ack(Some("message"), Some("479"), Some("10@g.us"))] {
            assert!(!rejected_message(&event));
        }
        let check = Mutex::new(None);
        let now = Instant::now();
        assert!(claim_check(&check, now));
        assert!(!claim_check(&check, now + Duration::from_secs(59)));
        assert!(claim_check(&check, now + CHECK_INTERVAL));
    }

    #[test]
    fn authoritative_status_and_server_clock_control_cap_notices_without_guessed_fields() {
        let mut info = NewChatMessageCapping::default();
        info.total_quota = Some(10);
        info.used_quota = Some(10);
        info.cycle_start_timestamp = Some(100);
        info.cycle_end_timestamp = Some(300);
        for status in [None, Some(CappingStatus::None), Some(CappingStatus::FirstWarning),
            Some(CappingStatus::Other("UNKNOWN".into()))] {
            info.capping_status = status;
            assert!(capping_notice(&info, 200, "first").is_none());
        }
        info.capping_status = Some(CappingStatus::Capped);
        assert_eq!(capping_notice(&info, 200, "first"), Some(("message-capping-cycle-100".into(), vec!["10".into(), "10".into(), "300".into()])));
        assert_eq!(capping_notice(&info, 200, "first"), capping_notice(&info, 200, "second"));
        assert!(capping_notice(&info, 300, "first").is_none());
        assert!(capping_notice(&info, 0, "first").is_none());
        info.server_sent_timestamp = Some(200);
        assert!(capping_notice(&info, 400, "first").is_some());
        info.server_sent_timestamp = Some(300);
        assert!(capping_notice(&info, 100, "first").is_none());
        info.server_sent_timestamp = None;
        info.cycle_start_timestamp = None;
        info.cycle_end_timestamp = None;
        info.total_quota = None;
        info.used_quota = None;
        assert_eq!(capping_notice(&info, 200, "first"), Some(("message-capping-message-first".into(), vec![String::new(); 3])));
    }

    #[tokio::test]
    async fn notices_use_proven_outgoing_peers_and_existing_cycle_deduplication() {
        let (handler, mut received) = super::super::protocol_tests::inbound().await;
        let mut ack = ServerAck::builder().id("outgoing".to_owned()).class("message".to_owned()).error("synthetic-code".to_owned())
            .from("s.whatsapp.net".parse().unwrap()).build();
        assert!(capping_chat(&handler.store, &ack).await.is_none());
        let mut row = StoredMessage::default();
        row.header.chat = "10@lid".into(); row.header.id = "outgoing".into(); row.header.from_me = true;
        row.header.timestamp = 100; row.text = "synthetic".into();
        handler.store.insert_message(&row).await.unwrap();
        assert_eq!(capping_chat(&handler.store, &ack).await.as_deref(), Some("10@lid"));
        row.header.id = "incoming".into(); row.header.from_me = false;
        handler.store.insert_message(&row).await.unwrap();
        ack.id = "incoming".into();
        assert!(capping_chat(&handler.store, &ack).await.is_none());
        ack.id = "outgoing".into();
        ack.from = Some("20@s.whatsapp.net".parse().unwrap());
        assert_eq!(capping_chat(&handler.store, &ack).await.as_deref(), Some("10@lid"));
        ack.id = "unknown".into();
        assert_eq!(capping_chat(&handler.store, &ack).await.as_deref(), Some("20@s.whatsapp.net"));
        row.header.chat = "30@g.us".into(); row.header.id = "group-outgoing".into(); row.header.from_me = true;
        handler.store.insert_message(&row).await.unwrap();
        ack.id = "group-outgoing".into();
        assert!(capping_chat(&handler.store, &ack).await.is_none());
        let params = vec!["10".into(), "10".into(), "300".into()];
        handler.store_notice("10@lid", "message-capping-cycle-100".into(), 200, NOTICE_KIND, params.clone(), String::new()).await;
        handler.store_notice("10@lid", "message-capping-cycle-100".into(), 201, NOTICE_KIND, params, String::new()).await;
        assert!(received.try_recv().is_ok());
        assert!(received.try_recv().is_err());
    }
}
