//! System lines: protocol stubs from history, and security notices raised live.

use super::*;
use wa::web_message_info::StubType;
use whatsapp_rust::wacore::stanza::groups::{GroupNotificationAction, GroupParticipantInfo};
use whatsapp_rust::wacore::types::call::MissedCall;
use whatsapp_rust::wacore::types::events::{DeviceListUpdate, DeviceListUpdateType, GroupUpdate, IdentityChange};

/// The name a stub is stored under, or `None` for stubs that draw nothing:
/// payment and business-verification bookkeeping, generic notifications, and
/// the ones that are really messages (revokes, undecryptable placeholders).
pub(super) fn system_kind(stub: StubType) -> Option<String> {
    let name = format!("{stub:?}");
    let silent = matches!(
        stub,
        StubType::UNKNOWN
            | StubType::REVOKE
            | StubType::CIPHERTEXT
            | StubType::FUTUREPROOF
            | StubType::GENERIC_NOTIFICATION
            | StubType::GROUP_CREATE_FAILED
    ) || ["PAYMENT_", "BIZ_", "BLUE_MSG_", "VERIFIED_", "NON_VERIFIED_", "UNVERIFIED_"]
        .iter()
        .any(|prefix| name.starts_with(prefix));
    (!silent).then_some(name)
}

/// A stored system line. It is already read, so it never raises an unread count.
pub(super) fn system_row(chat: &str, id: String, timestamp: i64, kind: String, params: Vec<String>) -> StoredMessage {
    StoredMessage {
        header: MessageHeader { chat: chat.to_string(), id, sender: String::new(), timestamp, from_me: false },
        local: LocalState { read: true, ..Default::default() },
        system: crate::store::SystemNotice { kind: Some(kind), params },
        ..Default::default()
    }
}

impl Inbound {
    /// Someone's security code changed (they reinstalled or moved phones).
    pub(super) async fn on_identity_change(&self, change: &IdentityChange) {
        let forms = [Some(&change.user), change.lid_user.as_ref()];
        self.notice(forms, "E2E_IDENTITY_CHANGED").await;
    }

    /// Someone linked or removed a device. Our own list changes whenever we link, so it is skipped.
    pub(super) async fn on_device_change(&self, update: &DeviceListUpdate) {
        let kind = match update.update_type {
            DeviceListUpdateType::Add => "DEVICE_ADDED",
            DeviceListUpdateType::Remove => "DEVICE_REMOVED",
            _ => return,
        };
        let own = self.client_for_events.get().map(|c| [c.pn(), c.lid()]).unwrap_or_default();
        let user = update.user.to_non_ad();
        if own.iter().flatten().any(|j| j.to_non_ad() == user) {
            return;
        }
        self.notice([Some(&update.user), update.lid_user.as_ref()], kind).await;
    }

    /// A live group change, stored under the stub kind history would give the same change.
    pub(super) async fn on_group_update(&self, update: &GroupUpdate) {
        let chat = update.group_jid.to_non_ad().to_string();
        let jids = |list: &[GroupParticipantInfo]| list.iter().map(|p| p.jid.to_non_ad().to_string()).collect::<Vec<_>>();
        let (kind, params) = match update.action.as_ref() {
            GroupNotificationAction::Add { participants, reason } => {
                let kind = if reason.as_deref() == Some("invite") { "GROUP_PARTICIPANT_INVITE" } else { "GROUP_PARTICIPANT_ADD" };
                (kind, jids(participants))
            }
            GroupNotificationAction::Remove { participants, .. } => {
                let actor = update.participant.as_ref().map(|p| p.to_non_ad());
                let left = participants.len() == 1 && actor.is_some_and(|a| a == participants[0].jid.to_non_ad());
                (if left { "GROUP_PARTICIPANT_LEAVE" } else { "GROUP_PARTICIPANT_REMOVE" }, jids(participants))
            }
            GroupNotificationAction::Promote { participants } => ("GROUP_PARTICIPANT_PROMOTE", jids(participants)),
            GroupNotificationAction::Demote { participants } => ("GROUP_PARTICIPANT_DEMOTE", jids(participants)),
            GroupNotificationAction::Modify { participants } => ("GROUP_PARTICIPANT_CHANGE_NUMBER", jids(participants)),
            GroupNotificationAction::Subject { subject, .. } => ("GROUP_CHANGE_SUBJECT", vec![subject.clone()]),
            GroupNotificationAction::Description { .. } => ("GROUP_CHANGE_DESCRIPTION", vec![]),
            GroupNotificationAction::Create { .. } => ("GROUP_CREATE", vec![]),
            GroupNotificationAction::Delete { .. } => ("GROUP_DELETE", vec![]),
            _ => return,
        };
        let id = format!("group-{}-{}", update.notification_id.as_deref().unwrap_or("live"), update.action_index);
        self.store_notice(&chat, id, update.timestamp.timestamp(), kind, params).await;
    }

    /// A call nobody answered here. The offer does not say voice or video.
    pub(super) async fn on_missed_call(&self, call: &MissedCall) {
        let chat = resolve_chat(self.client_for_events.get().map(Arc::as_ref), &self.store, &call.from).await;
        let at = call.timestamp.timestamp();
        let mut known = false;
        for kind in ["CALL_MISSED_VOICE", "CALL_MISSED_VIDEO"] {
            if self.store.has_system_near(&chat, kind, at).await.observed().unwrap_or(false) { known = true; break; }
        }
        if !known {
            self.store_notice(&chat, format!("call-{}", call.call_id), at, "CALL_MISSED", vec![]).await;
        }
    }

    /// Stores and announces one system line, unless that change is already drawn.
    async fn store_notice(&self, chat: &str, id: String, timestamp: i64, kind: &str, params: Vec<String>) {
        let Self { store, events, .. } = self;
        if store.message(chat, &id).await.observed().is_some() || store.has_system_near(chat, kind, timestamp).await.observed().unwrap_or(false) {
            return;
        }
        let row = system_row(chat, id, timestamp, kind.to_string(), params);
        match store.insert_message(&row).await {
            Ok(()) => {
                let _ = events.send(ServiceEvent::hint(&row, false));
            }
            Err(e) => log::error!("could not store a {kind} notice in {chat}: {e:#}"),
        }
    }

    /// Adds a notice to the one-to-one chat stored under either of a user's forms.
    /// No chat means nothing to annotate; a repeat of the chat's last line is dropped.
    async fn notice(&self, forms: [Option<&Jid>; 2], kind: &str) {
        let store = &self.store;
        let mut target = None;
        for jid in forms.into_iter().flatten() {
            let chat = resolve_chat(self.client_for_events.get().map(Arc::as_ref), store, jid).await;
            if matches!(store.oldest_message(&chat).await.observed(), Some(Some(_))) {
                target = Some((chat, jid.to_non_ad().to_string()));
                break;
            }
        }
        let Some((chat, user)) = target else { return };
        let last = store.messages_for(&chat, 1).await.observed().and_then(|m| m.into_iter().next());
        if last.is_some_and(|m| m.system.kind.as_deref() == Some(kind)) {
            return;
        }
        let now = unix_now();
        self.store_notice(&chat, format!("notice-{kind}-{now}"), now, kind, vec![user]).await;
    }
}
