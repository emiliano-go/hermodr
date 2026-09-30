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
    pub(super) async fn on_group_mode_notice(&self, node: &whatsapp_rust::wacore_binary::NodeRef<'_>) {
        use whatsapp_rust::wacore::stanza::groups::GroupNotification;
        use whatsapp_rust::wacore_binary::NodeContentRef;
        // ponytail: raw fallback until upstream exposes these mode payloads in typed actions.
        if node.tag != "notification" || node.attrs().optional_string("type").as_deref() != Some("w:gp2") { return; }
        let Some(children) = node.children() else { return };
        if !children.iter().any(|child| matches!(child.tag.as_ref(), "member_link_mode" | "member_share_group_history_mode" | "group_history" | "no_group_history")) { return; }
        let Some(notification) = GroupNotification::try_from_node_ref(node) else { return };
        if !notification.group_jid.is_group() { return; }
        let Ok(at) = i64::try_from(notification.timestamp) else { return };
        let chat = notification.group_jid.to_non_ad().to_string();
        let sender = notification.participant.as_ref().map(|jid| jid.to_non_ad().to_string()).unwrap_or_default();
        if let Some(participant) = notification.participant.as_ref() {
            remember_lid_pn(&self.store, participant, notification.participant_pn.as_ref()).await;
        }
        for (index, child) in children.iter().enumerate() {
            let (kind, toggle) = match child.tag.as_ref() {
                "member_link_mode" => ("GROUP_MEMBER_LINK_MODE", None),
                "member_share_group_history_mode" => ("GROUP_MEMBER_SHARE_GROUP_HISTORY_MODE", None),
                "group_history" => ("GROUP_CHANGE_RECENT_HISTORY_SHARING", Some("on")),
                "no_group_history" => ("GROUP_CHANGE_RECENT_HISTORY_SHARING", Some("off")),
                _ => continue,
            };
            let value = match child.content.as_ref() {
                Some(NodeContentRef::String(value)) if value.len() <= 64 => Some(value.as_ref()),
                Some(NodeContentRef::Bytes(value)) if value.len() <= 64 => std::str::from_utf8(value.as_ref()).ok(),
                _ => None,
            };
            let params = toggle.or(value).map(str::to_owned).into_iter().collect::<Vec<_>>();
            let identity = notification.notification_id.as_ref().filter(|id| !id.is_empty()).cloned()
                .unwrap_or_else(|| format!("live-{at}-{}-{sender}-{}", child.tag, params.join(",")));
            self.store_notice(&chat, format!("group-mode-{identity}-{index}"), at, kind, params, sender.clone()).await;
        }
    }

    pub(super) fn check_community_owner(&self, update: &GroupUpdate, previous: Option<&GroupInfo>) {
        let Some(old_owner) = departed_owner(&update.action, previous) else { return };
        let Some(client) = self.client_for_events.get().cloned() else { return };
        let handler = self.clone();
        let chat = update.group_jid.to_non_ad().to_string();
        let timestamp = update.timestamp.timestamp();
        let id = format!("owner-{}-{}", update.notification_id.as_deref().unwrap_or("live"), timestamp);
        tokio::spawn(async move {
            let Ok(jid) = chat.parse::<Jid>() else { return };
            let groups = client.groups();
            let metadata = match tokio::time::timeout(Duration::from_secs(15), groups.fetch_metadata(&jid)).await {
                Ok(Ok(metadata)) => metadata,
                result => { log::warn!("could not verify community owner change in {chat}: {result:?}"); return; }
            };
            let owner = metadata.participants.iter().find(|member| member.is_super_admin());
            if let Some(owner) = owner {
                remember_lid_pn(&handler.store, &owner.jid, owner.phone_number.as_ref().or(owner.lid.as_ref())).await;
            }
            handler.record_owner_change(&chat, id, timestamp, &old_owner,
                metadata.is_parent_group, owner.map(|member| &member.jid)).await;
        });
    }

    pub(super) async fn record_owner_change(&self, chat: &str, id: String, timestamp: i64,
        old_owner: &str, community: bool, new_owner: Option<&Jid>) {
        if !community { return; }
        let (Ok(old), Some(new)) = (old_owner.parse::<Jid>(), new_owner) else { return };
        let old = resolve_chat(None, &self.store, &old).await;
        let new = resolve_chat(None, &self.store, new).await;
        if old == new { return; }
        let id = format!("{id}-{old}-{new}");
        self.store_notice(chat, id, timestamp, "COMMUNITY_OWNER_CHANGED", vec![old, new], String::new()).await;
    }

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
    pub(super) async fn on_group_update(&self, update: &GroupUpdate, community: bool) {
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
            GroupNotificationAction::Description { .. } => (if community { "COMMUNITY_CHANGE_DESCRIPTION" } else { "GROUP_CHANGE_DESCRIPTION" }, vec![]),
            GroupNotificationAction::Locked { .. } => ("GROUP_CHANGE_RESTRICT", vec!["on".into()]),
            GroupNotificationAction::Unlocked => ("GROUP_CHANGE_RESTRICT", vec!["off".into()]),
            GroupNotificationAction::Announce => ("GROUP_CHANGE_ANNOUNCE", vec!["on".into()]),
            GroupNotificationAction::NotAnnounce => ("GROUP_CHANGE_ANNOUNCE", vec!["off".into()]),
            GroupNotificationAction::Ephemeral { expiration, .. } => ("CHANGE_EPHEMERAL_SETTING", vec![expiration.to_string()]),
            GroupNotificationAction::MembershipApprovalMode { enabled } =>
                ("GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE", vec![if *enabled { "on" } else { "off" }.into()]),
            GroupNotificationAction::MembershipApprovalRequest { .. } =>
                ("GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST", update.participant.iter().map(|jid| jid.to_non_ad().to_string()).collect()),
            GroupNotificationAction::CreatedMembershipRequests { requests, .. } =>
                ("GROUP_MEMBERSHIP_JOIN_APPROVAL_REQUEST", jids(requests)),
            GroupNotificationAction::MemberAddMode { mode } => ("GROUP_MEMBER_ADD_MODE", vec![mode.clone()]),
            GroupNotificationAction::Invite { .. } | GroupNotificationAction::RevokeInvite => ("GROUP_CHANGE_INVITE_LINK", vec![]),
            GroupNotificationAction::LinkedGroupPromote { participants } => ("COMMUNITY_PARTICIPANT_PROMOTE", jids(participants)),
            GroupNotificationAction::LinkedGroupDemote { participants } => ("COMMUNITY_PARTICIPANT_DEMOTE", jids(participants)),
            GroupNotificationAction::ChangeNumber { new_owner, .. } =>
                ("INDIVIDUAL_CHANGE_NUMBER", update.participant.iter().chain(new_owner.iter()).map(|jid| jid.to_non_ad().to_string()).collect()),
            GroupNotificationAction::Link { raw, .. } => ("COMMUNITY_LINK_SUB_GROUP", linked_groups(raw)),
            GroupNotificationAction::Unlink { raw, .. } => ("COMMUNITY_UNLINK_SUB_GROUP", linked_groups(raw)),
            GroupNotificationAction::Create { .. } => (if community { "COMMUNITY_CREATE" } else { "GROUP_CREATE" }, vec![]),
            GroupNotificationAction::Delete { .. } => (if community { "COMMUNITY_PARENT_GROUP_DELETED" } else { "GROUP_DELETE" }, vec![]),
            _ => return,
        };
        let identity = update.notification_id.clone()
            .unwrap_or_else(|| format!("{}-{kind}-{}", update.timestamp.timestamp(), params.join(",")));
        let id = format!("group-{identity}-{}", update.action_index);
        let author = update.participant.as_ref().or_else(|| match update.action.as_ref() {
            GroupNotificationAction::Subject { subject_owner, .. } => subject_owner.as_ref(),
            _ => None,
        });
        let sender = author.map(|jid| jid.to_non_ad().to_string()).unwrap_or_default();
        self.store_notice(&chat, id, update.timestamp.timestamp(), kind, params, sender).await;
    }

    /// A call nobody answered here. The offer does not say voice or video.
    pub(super) async fn on_missed_call(&self, call: &MissedCall) {
        let chat = resolve_chat(self.client_for_events.get().map(Arc::as_ref), &self.store, &call.from).await;
        let at = call.timestamp.timestamp();
        let mut known = false;
        for kind in ["CALL_MISSED_VOICE", "CALL_MISSED_VIDEO"] {
            if self.store.has_system_near(&chat, kind, &[], at, true).await.observed().unwrap_or(false) { known = true; break; }
        }
        if !known {
            self.store_notice(&chat, format!("call-{}", call.call_id), at, "CALL_MISSED", vec![], String::new()).await;
        }
    }

    /// Stores and announces one system line, unless that change is already drawn.
    pub(super) async fn store_notice(&self, chat: &str, id: String, timestamp: i64, kind: &str, params: Vec<String>, sender: String) {
        let Self { store, events, .. } = self;
        if store.message(chat, &id).await.observed().is_some() || store.has_system_near(chat, kind, &params, timestamp, true).await.observed().unwrap_or(false) {
            return;
        }
        let mut row = system_row(chat, id, timestamp, kind.to_string(), params);
        row.header.sender = sender;
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
        self.store_notice(&chat, format!("notice-{kind}-{now}"), now, kind, vec![user], String::new()).await;
    }
}

pub(super) fn departed_owner(action: &GroupNotificationAction, previous: Option<&GroupInfo>) -> Option<String> {
    use whatsapp_rust::wacore::types::wire_enums::GroupParticipantType;
    let GroupNotificationAction::Remove { participants, .. } = action else { return None };
    let cached = previous.filter(|info| info.community)
        .and_then(|info| info.participants.iter().find(|member| member.owner));
    participants.iter().find(|member| {
        member.r#type == Some(GroupParticipantType::SuperAdmin) || cached.is_some_and(|owner| {
            [Some(&member.jid), member.phone_number.as_ref(), member.lid.as_ref()]
                .into_iter().flatten().any(|jid| jid.to_non_ad().to_string() == owner.jid)
        })
    }).map(|member| member.jid.to_non_ad().to_string())
}

fn linked_groups(raw: &whatsapp_rust::wacore_binary::Node) -> Vec<String> {
    raw.get_children_by_tag("group").into_iter()
        .filter_map(|group| group.attrs().optional_jid("jid").map(|jid| jid.to_non_ad().to_string())).collect()
}
