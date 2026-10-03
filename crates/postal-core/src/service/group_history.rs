use super::*;
use crate::message_ref::MessageRef;
use buffa::Message as _;
use whatsapp_rust::{GroupHistoryRetryToken, GroupHistoryShareOutcome as Outcome, GroupHistorySkipReason as Skip,
    HistorySharePreparation, PreparedGroupHistoryShare};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupHistoryOffer {
    pub enabled: bool,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    pub reason_ref: Option<MessageRef>,
    pub max_messages: usize,
    pub time_window_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupHistoryResult {
    pub state: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    pub message_ref: Option<MessageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "wire-types", ts(optional))]
    pub diagnostic: Option<String>,
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
            Ok(limits) => GroupHistoryOffer { enabled: true, reason: None, reason_ref: None,
                max_messages: limits.max_messages, time_window_seconds: limits.time_window_seconds },
            Err(reason) => GroupHistoryOffer { enabled: false, reason: Some(skip_text(reason).into()), reason_ref: Some(skip_ref(reason)), max_messages: 0, time_window_seconds: 0 },
        }
    }

    pub async fn add_group_participants_with_history(&self, chat: &str, jids: &[String], opted_in: &[String]) -> Result<GroupMemberAddResult> {
        let group: Jid = chat.parse()?;
        anyhow::ensure!(group.is_group(), MessageRef::new("error.history_group_required"));
        let participants = groups::parse_jids(jids)?;
        let receivers = groups::parse_jids(opted_in)?;
        validate_receivers(&participants, &receivers)?;
        if receivers.is_empty() {
            return Ok(GroupMemberAddResult { participants: self.add_group_participants(chat, jids).await?,
                history: history_result("not_requested", "Recent history was not requested.", None, MessageRef::new("history.not_requested")) });
        }
        let limits = group_history_policy::query_history_share_limits(&self.client, &group).await
            .map_err(|reason| anyhow::Error::new(skip_ref(reason)))?;
        let rows = self.store.group_history_text(chat, unix_now(), limits.time_window_seconds, limits.max_messages).await?;
        let messages = rows.into_iter().map(history_wire).collect::<Vec<_>>();
        // ponytail: at most 16 in-memory shares per account; durable recovery needs an upstream persistence format.
        let (id, share) = {
            let mut pending = self.history_shares.pending.lock().unwrap();
            pending.retain(|_, share| share.created.elapsed() < Duration::from_secs(15 * 60));
            anyhow::ensure!(pending.len() < 16, MessageRef::new("error.history_pending_limit"));
            let id = format!("history-{}", self.client.generate_message_id());
            let share = Arc::new(PendingShare { chat: chat.into(), state: tokio::sync::Mutex::new(
                ShareState::Finished(history_result("preparing", "Preparing recent history.", None, MessageRef::new("history.preparing")))), created: std::time::Instant::now() });
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
            .map_err(anyhow::Error::from)?;
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
            _ => anyhow::bail!(MessageRef::new("error.history_preparation")),
        }
    }

    pub async fn retry_group_history(&self, chat: &str, id: &str) -> Result<GroupHistoryResult> {
        let share = self.history_shares.pending.lock().unwrap().get(id).cloned()
            .ok_or_else(|| anyhow::anyhow!(MessageRef::new("error.history_retry_expired")))?;
        anyhow::ensure!(share.chat == chat && share.created.elapsed() < Duration::from_secs(15 * 60), MessageRef::new("error.history_retry_group"));
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
    anyhow::ensure!(!participants.is_empty() && participants.len() <= 1024, MessageRef::new("error.history_participants").with_param("limit", serde_json::Number::from(1024)));
    anyhow::ensure!(receivers.len() <= 1024 && participants.iter().all(|jid| !jid.user.is_empty() && (jid.is_pn() || jid.is_lid())), MessageRef::new("error.participant_address"));
    anyhow::ensure!(receivers.iter().all(|receiver| participants.iter().any(|jid| jid.to_non_ad() == receiver.to_non_ad())), MessageRef::new("error.history_receivers"));
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
    anyhow::ensure!(!whatsapp_rust::wacore::send::contains_group_history_payload(message), MessageRef::new("error.history_payload_workflow"));
    Ok(())
}

fn history_result(state: &str, message: &str, retry_id: Option<String>, message_ref: MessageRef) -> GroupHistoryResult {
    GroupHistoryResult { state: state.into(), message: message.into(), retry_id, message_ref: Some(message_ref), diagnostic: None }
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
    let mut result = match outcome {
        Outcome::NotRequested => history_result("not_requested", "Recent history was not requested.", None, MessageRef::new("history.not_requested")),
        Outcome::Skipped(reason) => history_result("skipped", skip_text(*reason), None, skip_ref(*reason)),
        Outcome::PreparationFailed { .. } => history_result("failed", "Members were added, but history preparation failed.", None, MessageRef::new("history.failed")),
        Outcome::UploadFailed { .. } => history_result("upload_failed", "Members were added, but history upload failed. Retry history only.", retry_id, MessageRef::new("history.upload_failed")),
        Outcome::BundleNotSent { .. } => history_result("bundle_not_sent", "History bundle was not sent. Retry history only.", retry_id, MessageRef::new("history.bundle_not_sent")),
        Outcome::BundleRejected { .. } => history_result("rejected", "Server rejected the history bundle.", None, MessageRef::new("history.rejected")),
        Outcome::BundleRetryableRejection { .. } => history_result("retryable_rejection", "Server rejected this attempt. History can be retried with the same ID.", retry_id, MessageRef::new("history.retryable_rejection")),
        Outcome::BundleIndeterminate { .. } => history_result("indeterminate", "History bundle acceptance is unknown. Retry checks the same ID.", retry_id, MessageRef::new("history.indeterminate")),
        Outcome::BundlePartialFanout { encrypted_devices, addressed_devices, .. } => history_result("partial", &format!("History bundle accepted; encrypted for {encrypted_devices} of {addressed_devices} devices. Retry incomplete history."), retry_id, MessageRef::new("history.partial").with_param("encrypted_devices", serde_json::Number::from(*encrypted_devices)).with_param("addressed_devices", serde_json::Number::from(*addressed_devices))),
        Outcome::NoticeNotSent { .. } => history_result("notice_not_sent", "History bundle accepted; group notice was not sent. Retry notice only.", retry_id, MessageRef::new("history.notice_not_sent")),
        Outcome::NoticeRejected { .. } => history_result("notice_rejected", "History bundle accepted; server rejected group notice.", None, MessageRef::new("history.notice_rejected")),
        Outcome::NoticeRetryableRejection { .. } => history_result("notice_retryable_rejection", "History bundle accepted; group notice can be retried.", retry_id, MessageRef::new("history.notice_retryable_rejection")),
        Outcome::NoticeIndeterminate { .. } => history_result("notice_indeterminate", "History bundle accepted; group notice acceptance is unknown. Retry notice only.", retry_id, MessageRef::new("history.notice_indeterminate")),
        Outcome::Shared { message_count, .. } => history_result("shared", &format!("Server accepted {message_count} recent messages and the group notice. Recipient delivery is not confirmed."), None, MessageRef::new("history.shared").with_param("message_count", serde_json::Number::from(*message_count))),
        _ => history_result("unsupported", "Unknown history outcome; members may already have been added.", None, MessageRef::new("history.unsupported")),
    };
    result.diagnostic = match outcome {
        Outcome::PreparationFailed { error }
        | Outcome::UploadFailed { error, .. }
        | Outcome::BundleNotSent { error, .. }
        | Outcome::NoticeNotSent { error, .. } => Some(error.clone()),
        Outcome::BundleRejected { error, code, .. }
        | Outcome::NoticeRejected { error, code, .. } => rejection_diagnostic(error.as_deref(), code.as_deref()),
        Outcome::BundleRetryableRejection { error, code, .. }
        | Outcome::NoticeRetryableRejection { error, code, .. } => rejection_diagnostic(error.as_deref(), Some(code)),
        _ => None,
    };
    result
}

fn rejection_diagnostic(error: Option<&str>, code: Option<&str>) -> Option<String> {
    match (error, code) {
        (Some(error), Some(code)) => Some(format!("{code}: {error}")),
        (Some(error), None) => Some(error.to_owned()),
        (None, Some(code)) => Some(code.to_owned()),
        (None, None) => None,
    }
}

fn skip_ref(reason: Skip) -> MessageRef {
    MessageRef::new(match reason {
        Skip::NoOptedInSuccessfulRecipients => "history.skip_no_recipients",
        Skip::SenderNotAuthorized => "history.skip_not_authorized",
        Skip::UnsupportedGroup => "history.skip_unsupported_group",
        Skip::AccountPropsUnavailable => "history.skip_account_policy",
        Skip::GroupMetadataUnavailable => "history.skip_group_metadata",
        Skip::GroupPropsUnavailable => "history.skip_group_policy",
        Skip::InvalidProperties => "history.skip_invalid_limits",
        Skip::SharingDisabled => "history.skip_disabled",
        Skip::NoEligibleMessages => "history.skip_no_messages",
        _ => "history.unavailable",
    })
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
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["message_ref"]["code"], "history.shared");
        assert_eq!(value["message_ref"]["params"]["message_count"], 2);
        assert!(value.get("diagnostic").is_none());
        assert!(result.retry_id.is_none());
    }
    #[test]
    fn history_failures_keep_sdk_diagnostics_and_legacy_outcome() {
        let result = outcome_view(&Outcome::PreparationFailed { error: "synthetic upload cause".into() }, None);
        assert_eq!(result.state, "failed");
        assert_eq!(result.message_ref.as_ref().unwrap().code, "history.failed");
        assert_eq!(result.diagnostic.as_deref(), Some("synthetic upload cause"));
        let result = outcome_view(&Outcome::NoticeRejected {
            bundle_message_id: "accepted-bundle".into(), notice_message_id: "notice".into(),
            error: Some("synthetic rejection".into()), code: Some("403".into()),
        }, None);
        assert_eq!(result.state, "notice_rejected");
        assert_eq!(result.message_ref.as_ref().unwrap().code, "history.notice_rejected");
        assert!(result.message.starts_with("History bundle accepted;"));
        assert_eq!(result.diagnostic.as_deref(), Some("403: synthetic rejection"));
        assert_eq!(rejection_diagnostic(None, Some("409")).as_deref(), Some("409"));
        assert!(rejection_diagnostic(None, None).is_none());
    }

    #[test]
    fn history_skips_and_guard_errors_use_stable_codes() {
        for (reason, code) in [
            (Skip::NoOptedInSuccessfulRecipients, "history.skip_no_recipients"),
            (Skip::SenderNotAuthorized, "history.skip_not_authorized"),
            (Skip::UnsupportedGroup, "history.skip_unsupported_group"),
            (Skip::AccountPropsUnavailable, "history.skip_account_policy"),
            (Skip::GroupMetadataUnavailable, "history.skip_group_metadata"),
            (Skip::GroupPropsUnavailable, "history.skip_group_policy"),
            (Skip::InvalidProperties, "history.skip_invalid_limits"),
            (Skip::SharingDisabled, "history.skip_disabled"),
            (Skip::NoEligibleMessages, "history.skip_no_messages"),
        ] {
            let result = outcome_view(&Outcome::Skipped(reason), None);
            assert_eq!(result.state, "skipped");
            assert_eq!(result.message_ref.as_ref().unwrap().code, code);
            assert!(result.diagnostic.is_none());
        }
        let error = validate_receivers(&["1@g.us".parse().unwrap()], &[]).unwrap_err();
        assert!(error.downcast_ref::<MessageRef>().is_some());
    }
}
