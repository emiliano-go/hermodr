use super::*;
use buffa::Message as _;
use whatsapp_rust::{GroupHistoryRetryToken, GroupHistoryShareOutcome as Outcome, GroupHistorySkipReason as Skip,
    HistorySharePreparation, PreparedGroupHistoryShare};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupHistoryOffer {
    pub enabled: bool,
    pub reason: Option<String>,
    pub max_messages: usize,
    pub time_window_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupHistoryResult {
    pub state: String,
    pub message: String,
    pub retry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupMemberAddResult {
    pub participants: Vec<ParticipantChange>,
    pub history: GroupHistoryResult,
}

enum ShareState {
    Prepared(Box<PreparedGroupHistoryShare>),
    Retry(GroupHistoryRetryToken),
    Finished(GroupHistoryResult),
}

struct PendingShare {
    chat: String,
    state: tokio::sync::Mutex<ShareState>,
    created: std::time::Instant,
}

#[derive(Default)]
pub(super) struct PendingHistoryShares {
    pending: Mutex<std::collections::HashMap<String, Arc<PendingShare>>>,
}

impl WhatsAppService {
    pub async fn group_history_offer(&self, chat: &str) -> GroupHistoryOffer {
        let limits = match chat.parse::<Jid>() {
            Ok(jid) => group_history_policy::query_history_share_limits(&self.client, &jid).await,
            Err(_) => Err(Skip::UnsupportedGroup),
        };
        match limits {
            Ok(limits) => GroupHistoryOffer { enabled: true, reason: None,
                max_messages: limits.max_messages, time_window_seconds: limits.time_window_seconds },
            Err(reason) => GroupHistoryOffer { enabled: false, reason: Some(skip_text(reason).into()), max_messages: 0, time_window_seconds: 0 },
        }
    }

    pub async fn add_group_participants_with_history(&self, chat: &str, jids: &[String], opted_in: &[String]) -> Result<GroupMemberAddResult> {
        let group: Jid = chat.parse()?;
        anyhow::ensure!(group.is_group(), "participants require a group");
        let participants = groups::parse_jids(jids)?;
        let receivers = groups::parse_jids(opted_in)?;
        validate_receivers(&participants, &receivers)?;
        if receivers.is_empty() {
            return Ok(GroupMemberAddResult { participants: self.add_group_participants(chat, jids).await?,
                history: history_result("not_requested", "Recent history was not requested.", None) });
        }
        let limits = group_history_policy::query_history_share_limits(&self.client, &group).await
            .map_err(|reason| anyhow::anyhow!(skip_text(reason)))?;
        let rows = self.store.group_history_text(chat, unix_now(), limits.time_window_seconds, limits.max_messages).await?;
        let messages = rows.into_iter().map(history_wire).collect::<Vec<_>>();
        // ponytail: at most 16 in-memory shares per account; durable recovery needs an upstream persistence format.
        let (id, share) = {
            let mut pending = self.history_shares.pending.lock().unwrap();
            pending.retain(|_, share| share.created.elapsed() < Duration::from_secs(15 * 60));
            anyhow::ensure!(pending.len() < 16, "finish or wait for pending history shares before adding more");
            let id = format!("history-{}", self.client.generate_message_id());
            let share = Arc::new(PendingShare { chat: chat.into(), state: tokio::sync::Mutex::new(
                ShareState::Finished(history_result("preparing", "Preparing recent history.", None))), created: std::time::Instant::now() });
            pending.insert(id.clone(), share.clone());
            (id, share)
        };
        let result = self.prepare_history_add(&group, &participants, &receivers, &messages, &id, &share).await;
        self.after_group_change(chat);
        if result.is_err() { self.history_shares.pending.lock().unwrap().remove(&id); }
        result
    }

    async fn prepare_history_add(&self, group: &Jid, participants: &[Jid], receivers: &[Jid], messages: &[wa::WebMessageInfo], id: &str, share: &Arc<PendingShare>) -> Result<GroupMemberAddResult> {
        let prepared = self.client.groups().prepare_group_history_share(group.clone(), participants, receivers, messages).await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        match prepared {
            HistorySharePreparation::Settled(result) => {
                let retry = retry_token(&result.history_share);
                let history = outcome_view(&result.history_share, retry.as_ref().map(|_| id.to_owned()));
                if let Some(token) = retry { *share.state.lock().await = ShareState::Retry(token); }
                else { self.history_shares.pending.lock().unwrap().remove(id); }
                Ok(GroupMemberAddResult { participants: result.participants.iter().map(groups::change_of).collect(),
                    history })
            }
            HistorySharePreparation::Ready(prepared) => {
                let participants = prepared.participants.iter().map(groups::change_of).collect();
                *share.state.lock().await = ShareState::Prepared(Box::new(prepared));
                let history = self.drive_history_share(id, share).await;
                Ok(GroupMemberAddResult { participants, history })
            }
            _ => anyhow::bail!("unsupported history preparation result"),
        }
    }

    pub async fn retry_group_history(&self, chat: &str, id: &str) -> Result<GroupHistoryResult> {
        let share = self.history_shares.pending.lock().unwrap().get(id).cloned()
            .ok_or_else(|| anyhow::anyhow!("history retry expired; members have already been added"))?;
        anyhow::ensure!(share.chat == chat && share.created.elapsed() < Duration::from_secs(15 * 60), "history retry is unavailable for this group");
        Ok(self.drive_history_share(id, &share).await)
    }

    async fn drive_history_share(&self, id: &str, share: &Arc<PendingShare>) -> GroupHistoryResult {
        let mut state = share.state.lock().await;
        let outcome = match &*state {
            ShareState::Prepared(prepared) => self.client.groups().deliver_prepared_history_share(prepared).await,
            ShareState::Retry(token) => self.client.groups().retry_group_history(token).await,
            ShareState::Finished(result) => return result.clone(),
        };
        let retry = retry_token(&outcome);
        let result = outcome_view(&outcome, retry.as_ref().map(|_| id.to_owned()));
        *state = retry.map(ShareState::Retry).unwrap_or_else(|| ShareState::Finished(result.clone()));
        if result.retry_id.is_none() { self.history_shares.pending.lock().unwrap().remove(id); }
        result
    }
}

fn validate_receivers(participants: &[Jid], receivers: &[Jid]) -> Result<()> {
    anyhow::ensure!(!participants.is_empty() && participants.len() <= 1024, "select between 1 and 1024 participants");
    anyhow::ensure!(receivers.len() <= 1024 && participants.iter().all(|jid| !jid.user.is_empty() && (jid.is_pn() || jid.is_lid())), "invalid participant address");
    anyhow::ensure!(receivers.iter().all(|receiver| participants.iter().any(|jid| jid.to_non_ad() == receiver.to_non_ad())), "history receivers must be selected participants");
    Ok(())
}

fn history_wire(row: StoredMessage) -> wa::WebMessageInfo {
    let status = match row.local.status.as_deref() {
        Some("read") => wa::web_message_info::Status::READ,
        Some("sent") => wa::web_message_info::Status::SERVER_ACK,
        _ => wa::web_message_info::Status::DELIVERY_ACK,
    };
    wa::WebMessageInfo {
        key: MessageField::some(wa::MessageKey { remote_jid: Some(row.header.chat), id: Some(row.header.id),
            from_me: Some(row.header.from_me), participant: Some(row.header.sender.clone()), ..Default::default() }),
        participant: Some(row.header.sender), message_timestamp: Some(row.header.timestamp as u64), status: Some(status),
        message: MessageField::some(wa::Message::text(row.text)), ..Default::default()
    }
}

pub(super) fn is_shareable_text(message: &wa::Message) -> bool {
    let Some(text) = message.conversation.as_deref().filter(|text| !text.is_empty() && text.len() <= 65536) else { return false };
    let mut plain = wa::Message::text(text);
    if let Some(context) = message.message_context_info.as_option() {
        plain.message_context_info = MessageField::some(wa::MessageContextInfo {
            message_secret: context.message_secret.clone(), reporting_token_version: context.reporting_token_version, ..Default::default()
        });
    }
    message.encode_to_vec() == plain.encode_to_vec()
}

pub(super) fn guard_ordinary_message(message: &wa::Message) -> Result<()> {
    anyhow::ensure!(!whatsapp_rust::wacore::send::contains_group_history_payload(message), "history payloads require the opted-in group sharing workflow");
    Ok(())
}

fn history_result(state: &str, message: &str, retry_id: Option<String>) -> GroupHistoryResult {
    GroupHistoryResult { state: state.into(), message: message.into(), retry_id }
}

fn retry_token(outcome: &Outcome) -> Option<GroupHistoryRetryToken> {
    match outcome {
        Outcome::UploadFailed { retry, .. } | Outcome::BundleNotSent { retry, .. }
        | Outcome::BundleRetryableRejection { retry, .. } | Outcome::BundleIndeterminate { retry, .. }
        | Outcome::BundlePartialFanout { retry, .. } | Outcome::NoticeNotSent { retry, .. }
        | Outcome::NoticeRetryableRejection { retry, .. } | Outcome::NoticeIndeterminate { retry, .. } => Some(retry.clone()),
        _ => None,
    }
}

fn outcome_view(outcome: &Outcome, retry_id: Option<String>) -> GroupHistoryResult {
    match outcome {
        Outcome::NotRequested => history_result("not_requested", "Recent history was not requested.", None),
        Outcome::Skipped(reason) => history_result("skipped", skip_text(*reason), None),
        Outcome::PreparationFailed { .. } => history_result("failed", "Members were added, but history preparation failed.", None),
        Outcome::UploadFailed { .. } => history_result("upload_failed", "Members were added, but history upload failed. Retry history only.", retry_id),
        Outcome::BundleNotSent { .. } => history_result("bundle_not_sent", "History bundle was not sent. Retry history only.", retry_id),
        Outcome::BundleRejected { .. } => history_result("rejected", "Server rejected the history bundle.", None),
        Outcome::BundleRetryableRejection { .. } => history_result("retryable_rejection", "Server rejected this attempt. History can be retried with the same ID.", retry_id),
        Outcome::BundleIndeterminate { .. } => history_result("indeterminate", "History bundle acceptance is unknown. Retry checks the same ID.", retry_id),
        Outcome::BundlePartialFanout { encrypted_devices, addressed_devices, .. } => history_result("partial", &format!("History bundle accepted; encrypted for {encrypted_devices} of {addressed_devices} devices. Retry incomplete history."), retry_id),
        Outcome::NoticeNotSent { .. } => history_result("notice_not_sent", "History bundle accepted; group notice was not sent. Retry notice only.", retry_id),
        Outcome::NoticeRejected { .. } => history_result("notice_rejected", "History bundle accepted; server rejected group notice.", None),
        Outcome::NoticeRetryableRejection { .. } => history_result("notice_retryable_rejection", "History bundle accepted; group notice can be retried.", retry_id),
        Outcome::NoticeIndeterminate { .. } => history_result("notice_indeterminate", "History bundle accepted; group notice acceptance is unknown. Retry notice only.", retry_id),
        Outcome::Shared { message_count, .. } => history_result("shared", &format!("Server accepted {message_count} recent messages and the group notice. Recipient delivery is not confirmed."), None),
        _ => history_result("unsupported", "Unknown history outcome; members may already have been added.", None),
    }
}

fn skip_text(reason: Skip) -> &'static str {
    match reason {
        Skip::NoOptedInSuccessfulRecipients => "No successfully added recipient opted in to history.",
        Skip::SenderNotAuthorized => "Your group role cannot share recent history.",
        Skip::UnsupportedGroup => "Recent history sharing is unavailable for this group.",
        Skip::AccountPropsUnavailable => "Account history policy could not be verified.",
        Skip::GroupMetadataUnavailable => "Group history settings could not be verified.",
        Skip::GroupPropsUnavailable => "Group history policy could not be verified.",
        Skip::InvalidProperties => "History policy contains unsupported limits.",
        Skip::SharingDisabled => "Recent history sharing is disabled by account or group policy.",
        Skip::NoEligibleMessages => "No verified recent plain-text messages are eligible for sharing.",
        _ => "Recent history sharing is unavailable.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_provenance_rejects_wrappers_metadata_and_media() {
        let plain = wa::Message::text("synthetic");
        assert!(is_shareable_text(&plain));
        let wrapped = wa::Message { ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(plain.clone()), ..Default::default() }), ..Default::default() };
        assert!(!is_shareable_text(&wrapped));
        let mut hybrid = plain;
        hybrid.image_message = MessageField::some(Default::default());
        assert!(!is_shareable_text(&hybrid));
    }

    #[test]
    fn ordinary_send_rejects_nested_history_and_recipient_scope_is_explicit() {
        let payload = wa::Message { message_history_bundle: MessageField::some(Default::default()), ..Default::default() };
        let wrapper = wa::Message { view_once_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(payload), ..Default::default() }), ..Default::default() };
        assert!(guard_ordinary_message(&wrapper).is_err());
        assert!(guard_ordinary_message(&wa::Message::text("synthetic")).is_ok());
        let selected = vec!["1@s.whatsapp.net".parse().unwrap()];
        assert!(validate_receivers(&selected, &selected).is_ok());
        assert!(validate_receivers(&selected, &["2@s.whatsapp.net".parse().unwrap()]).is_err());
        assert!(validate_receivers(&["1@g.us".parse().unwrap()], &[]).is_err());
        assert!(validate_receivers(&["@s.whatsapp.net".parse().unwrap()], &[]).is_err());
        let result = outcome_view(&Outcome::Shared { bundle_message_id: "bundle".into(), notice_message_id: "notice".into(), message_count: 2 }, None);
        assert_eq!(result.state, "shared");
        assert!(result.message.contains("delivery is not confirmed"));
    }
}
