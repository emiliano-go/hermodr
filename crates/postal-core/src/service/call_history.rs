use super::*;
use buffa::Enumeration;
use crate::store::call_history::{CallHistoryUpdate, CallRecord, MAX_CALL_PARTICIPANTS};
use crate::message_ref::MessageRef;
use whatsapp_rust::wacore::types::events::CallLogSync;
use whatsapp_rust::wacore_binary::Jid;

pub fn capture_call_record(event: &CallLogSync, is_own: impl Fn(&Jid) -> bool) -> Result<CallHistoryUpdate> {
    anyhow::ensure!(event.record.participants.len() <= MAX_CALL_PARTICIPANTS,
        MessageRef::new("error.calls_participants_limit"));
    let creator = event.call_creator_jid.to_non_ad();
    let peer_jids = event.record.participants.iter().filter_map(|participant| {
        let value = participant.user_jid.as_deref()?;
        if value.len() > 256 || value.chars().any(char::is_control) { return None; }
        let jid = value.parse::<Jid>().ok()?.to_non_ad();
        if jid.user.is_empty() || !(jid.is_pn() || jid.is_lid()) || is_own(&jid)
            || event.from_me && jid == creator { return None; }
        Some(jid.to_string())
    }).collect();
    Ok(CallHistoryUpdate {
        call_id: event.call_id.clone(), creator_jid: creator.to_string(),
        group_jid: event.record.group_jid.clone(), peer_jids, from_me: event.from_me,
        is_video: event.record.is_video,
        outcome_raw: event.record.call_result.map(|value| value.to_i32()),
        call_type_raw: event.record.call_type.map(|value| value.to_i32()),
        start_time_raw: event.record.start_time, duration_raw: event.record.duration,
        mutation_at_ms: event.timestamp.timestamp_millis(), from_full_sync: event.from_full_sync,
    })
}

pub(super) async fn apply_call_log_event(
    store: &StoreWorker, event: &CallLogSync, is_own: impl Fn(&Jid) -> bool + Send,
) -> Result<bool> {
    let update = capture_call_record(event, is_own)?;
    store.run(move |store| store.capture_call(&update)).await
}

impl WhatsAppService {
    /// Recent sync revisions; raw call clocks have no verified unit contract yet.
    pub async fn call_history(&self, limit: u32) -> Result<Vec<CallRecord>> {
        self.store.run(move |store| store.call_history(limit)).await
    }
}

#[cfg(test)]
#[path = "call_history_tests.rs"]
mod tests;
