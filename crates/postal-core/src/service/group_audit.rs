use super::*;
use crate::store::group_audit::{GroupAuditFilter, GroupAuditKind as Kind,
    GroupAuditOldSource as OldSource, GroupAuditPage, GroupAuditRecord, GroupAuditSource as Source,
    audit_person as person, audit_private as private, notice_records};
use whatsapp_rust::wacore::{stanza::groups::GroupNotificationAction as Action,
    types::{events::{GroupUpdate, PictureUpdate}, message::MessageInfo}};

fn role(member: &Participant) -> &'static str { if member.owner { "owner" } else if member.admin { "admin" } else { "member" } }

fn base(chat: String, source_id: Option<String>, timestamp: Option<i64>, source: Source) -> GroupAuditRecord {
    GroupAuditRecord { chat, source_id, kind: Kind::Create, actor: None, target: None,
        old_value: None, new_value: None, old_source: None, timestamp: timestamp.filter(|at| *at > 0),
        observed_at: whatsapp_rust::wacore::time::now_millis() / 1000, source, message_id: None }
}

fn group_records(update: &GroupUpdate, previous: Option<&GroupInfo>) -> Vec<GroupAuditRecord> {
    if !update.group_jid.is_group() { return Vec::new(); }
    let chat = update.group_jid.to_non_ad().to_string();
    let actor = person(update.participant_pn.as_ref()).or_else(|| person(update.participant.as_ref()));
    let (kind, notice, params, participants, new_value, old_value) = match update.action.as_ref() {
        Action::Add { participants, reason } => (Kind::Join,
            if reason.as_deref() == Some("invite") { "GROUP_PARTICIPANT_INVITE" } else { "GROUP_PARTICIPANT_ADD" },
            participants.iter().map(|p| p.jid.to_non_ad().to_string()).collect(), Some(participants), Some("present".into()), None),
        Action::Remove { participants, .. } => {
            let left = participants.len() == 1 && update.participant.as_ref().is_some_and(|actor| actor.to_non_ad() == participants[0].jid.to_non_ad());
            (if left { Kind::Leave } else { Kind::Remove }, if left { "GROUP_PARTICIPANT_LEAVE" } else { "GROUP_PARTICIPANT_REMOVE" },
                participants.iter().map(|p| p.jid.to_non_ad().to_string()).collect(), Some(participants), Some("absent".into()), None)
        }
        Action::Promote { participants } | Action::LinkedGroupPromote { participants } => (Kind::Promote,
            if matches!(update.action.as_ref(), Action::LinkedGroupPromote { .. }) { "COMMUNITY_PARTICIPANT_PROMOTE" } else { "GROUP_PARTICIPANT_PROMOTE" },
            participants.iter().map(|p| p.jid.to_non_ad().to_string()).collect(), Some(participants), Some("admin".into()), None),
        Action::Demote { participants } | Action::LinkedGroupDemote { participants } => (Kind::Demote,
            if matches!(update.action.as_ref(), Action::LinkedGroupDemote { .. }) { "COMMUNITY_PARTICIPANT_DEMOTE" } else { "GROUP_PARTICIPANT_DEMOTE" },
            participants.iter().map(|p| p.jid.to_non_ad().to_string()).collect(), Some(participants), Some("member".into()), None),
        Action::Subject { subject, .. } => (Kind::Subject, "GROUP_CHANGE_SUBJECT", vec![subject.clone()], None,
            Some(subject.clone()), previous.and_then(|p| p.subject.clone())),
        Action::Description { description, .. } => (Kind::Description,
            if previous.is_some_and(|p| p.community) { "COMMUNITY_CHANGE_DESCRIPTION" } else { "GROUP_CHANGE_DESCRIPTION" },
            vec![], None, description.clone(), previous.and_then(|p| p.description.clone())),
        Action::Locked { .. } | Action::Unlocked => {
            let locked = matches!(update.action.as_ref(), Action::Locked { .. });
            (Kind::Locked, "GROUP_CHANGE_RESTRICT", vec![if locked { "on" } else { "off" }.into()], None,
                Some(locked.to_string()), previous.map(|p| p.locked.to_string()))
        }
        Action::Announce | Action::NotAnnounce => {
            let announce = matches!(update.action.as_ref(), Action::Announce);
            (Kind::Announce, "GROUP_CHANGE_ANNOUNCE", vec![if announce { "on" } else { "off" }.into()], None,
                Some(announce.to_string()), previous.map(|p| p.announce.to_string()))
        }
        Action::Ephemeral { expiration, .. } => (Kind::Ephemeral, "CHANGE_EPHEMERAL_SETTING", vec![expiration.to_string()], None, Some(expiration.to_string()), None),
        Action::MembershipApprovalMode { enabled } => (Kind::JoinApproval, "GROUP_MEMBERSHIP_JOIN_APPROVAL_MODE",
            vec![if *enabled { "on" } else { "off" }.into()], None, Some(enabled.to_string()), None),
        Action::MemberAddMode { mode } => (Kind::MemberAddMode, "GROUP_MEMBER_ADD_MODE", vec![mode.clone()], None, Some(mode.clone()), None),
        Action::Invite { .. } | Action::RevokeInvite => (Kind::InviteChange, "GROUP_CHANGE_INVITE_LINK", vec![], None, None, None),
        Action::Create { .. } => (Kind::Create, if previous.is_some_and(|p| p.community) { "COMMUNITY_CREATE" } else { "GROUP_CREATE" }, vec![], None, None, None),
        Action::Delete { .. } => (Kind::Delete, if previous.is_some_and(|p| p.community) { "COMMUNITY_PARENT_GROUP_DELETED" } else { "GROUP_DELETE" }, vec![], None, None, None),
        Action::NoFrequentlyForwarded => (Kind::Forwarding, "", vec![], None, Some("restricted".into()), None),
        Action::FrequentlyForwardedOk => (Kind::Forwarding, "", vec![], None, Some("allowed".into()), None),
        _ => return Vec::new(),
    };
    let source_id = if notice.is_empty() {
        update.notification_id.as_ref().map(|id| format!("group-{id}-{}", update.action_index))
    } else {
        let identity = update.notification_id.clone().unwrap_or_else(|| format!("{}-{notice}-{}", update.timestamp.timestamp(), params.join(",")));
        Some(format!("group-{identity}-{}", update.action_index))
    };
    let mut entry = base(chat, source_id.clone(), Some(update.timestamp.timestamp()), Source::Notification);
    entry.message_id = source_id.filter(|_| !notice.is_empty());
    entry.kind = kind;
    entry.actor = actor.or_else(|| match update.action.as_ref() {
        Action::Subject { subject_owner, subject_owner_pn, .. } => person(subject_owner_pn.as_ref()).or_else(|| person(subject_owner.as_ref())),
        _ => None,
    });
    entry.new_value = new_value;
    entry.old_value = old_value;
    if previous.is_some() && matches!(kind, Kind::Subject | Kind::Description | Kind::Locked | Kind::Announce) { entry.old_source = Some(OldSource::Cached); }
    if let Some(participants) = participants {
        participants.iter().map(|participant| {
            let mut entry = entry.clone();
            entry.target = person(participant.phone_number.as_ref()).or_else(|| person(Some(&participant.jid)));
            if let Some(member) = previous.into_iter().flat_map(|p| &p.participants).find(|p|
                entry.target.as_deref() == Some(p.jid.as_str()) || p.jid == participant.jid.to_non_ad().to_string()) {
                entry.old_value = Some(if matches!(kind, Kind::Promote | Kind::Demote) { role(member) } else { "present" }.into());
                entry.old_source = Some(OldSource::Cached);
            }
            entry
        }).collect()
    } else { vec![entry] }
}

pub(super) async fn audit_group_update(store: &StoreWorker, update: &GroupUpdate, previous: Option<&GroupInfo>) -> Result<bool> {
    let records = group_records(update, previous);
    Ok(store.run(move |store| store.record_group_audit(&records)).await? > 0)
}

fn picture_records(update: &PictureUpdate) -> Vec<GroupAuditRecord> {
    if !update.jid.is_group() { return Vec::new(); }
    let id = format!("group-picture-{}-{}-{}", update.timestamp.timestamp(), update.picture_id.as_deref().unwrap_or("removed"), update.removed);
    // SDK picture timestamps can be a local fallback with no presence flag.
    let mut entry = base(update.jid.to_non_ad().to_string(), Some(id.clone()), None, Source::Notification);
    entry.kind = Kind::Picture;
    entry.actor = person(update.author.as_ref());
    entry.new_value = Some(if update.removed { "removed" } else { "set" }.into());
    entry.message_id = Some(id);
    vec![entry]
}

pub(super) async fn audit_group_picture(store: &StoreWorker, update: &PictureUpdate) -> Result<bool> {
    let records = picture_records(update);
    Ok(store.run(move |store| store.record_group_audit(&records)).await? > 0)
}

pub(super) fn audit_message_allowed(message: &StoredMessage) -> bool { !private(message) }

fn message_target(key: Option<&wa::MessageKey>, chat: &str) -> Option<String> {
    let key = key?;
    if let Some(remote) = &key.remote_jid {
        if remote.parse::<Jid>().ok()?.to_non_ad().to_string() != chat { return None; }
    }
    key.id.clone().filter(|id| !id.is_empty())
}

pub(super) fn audit_message_target(message: &wa::Message, chat: &str) -> Option<String> {
    let decoded = decoded_message(message);
    if decoded.spoiler || decoded.view_once { return None; }
    if let Some(pin) = decoded.message.pin_in_chat_message.as_option() {
        use wa::message::pin_in_chat_message::Type;
        if matches!(pin.r#type, Some(Type::PIN_FOR_ALL | Type::UNPIN_FOR_ALL)) { return message_target(pin.key.as_option(), chat); }
    }
    let protocol = decoded.message.protocol_message.as_option()?;
    use wa::message::protocol_message::Type;
    matches!(protocol.r#type, Some(Type::REVOKE | Type::MESSAGE_EDIT)).then(|| message_target(protocol.key.as_option(), chat)).flatten()
}

pub(super) fn local_group_record(chat: &str, kind: Kind, actor: Option<&Jid>) -> GroupAuditRecord {
    let mut record = base(chat.into(), None, None, Source::Local);
    record.kind = kind;
    record.actor = person(actor);
    record
}

pub(super) async fn audit_group_local(store: &StoreWorker, mut record: GroupAuditRecord) -> Result<bool> {
    record.source = Source::Local;
    record.timestamp = record.timestamp.filter(|at| *at > 0);
    record.observed_at = whatsapp_rust::wacore::time::now_millis() / 1000;
    record.target = record.target.as_deref().and_then(|jid| jid.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid)));
    if record.old_value.is_some() || record.old_source.is_some() { record.old_source = Some(OldSource::Cached); }
    if matches!(record.kind, Kind::MessageEdit | Kind::InviteChange) {
        record.old_value = None;
        record.new_value = None;
        record.old_source = None;
    }
    if record.kind == Kind::MessageDelete {
        record.new_value = Some("revoked".into());
        record.old_value = record.old_value.filter(|value| matches!(value.as_str(), "revoked" | "not_revoked"));
        record.old_source = record.old_value.as_ref().map(|_| OldSource::Cached);
    }
    if matches!(record.kind, Kind::MessagePin | Kind::MessageUnpin) {
        record.new_value = Some((record.kind == Kind::MessagePin).to_string());
        record.old_value = record.old_value.filter(|value| matches!(value.as_str(), "true" | "false"));
        record.old_source = record.old_value.as_ref().map(|_| OldSource::Cached);
    }
    Ok(store.run(move |store| store.record_group_audit(&[record])).await? > 0)
}

fn message_records(mut entry: GroupAuditRecord, message: &wa::Message, previous: Option<&StoredMessage>) -> Vec<GroupAuditRecord> {
    let reference = audit_message_target(message, &entry.chat);
    let previous = previous.filter(|row| reference.as_deref() == Some(row.header.id.as_str()) && row.header.chat == entry.chat);
    if previous.is_some_and(private) { return Vec::new(); }
    let decoded = decoded_message(message);
    if decoded.spoiler || decoded.view_once { return Vec::new(); }
    let message = decoded.message;
    if let Some(pin) = message.pin_in_chat_message.as_option() {
        use wa::message::pin_in_chat_message::Type;
        let pinned = match pin.r#type { Some(Type::PIN_FOR_ALL) => true, Some(Type::UNPIN_FOR_ALL) => false, _ => return Vec::new() };
        entry.message_id = message_target(pin.key.as_option(), &entry.chat);
        if entry.message_id.is_none() { return Vec::new(); }
        entry.timestamp = pin.sender_timestamp_ms.filter(|at| *at > 0).map(|at| at / 1000).or(entry.timestamp);
        entry.kind = if pinned { Kind::MessagePin } else { Kind::MessageUnpin };
        entry.new_value = Some(pinned.to_string());
    } else if let Some(protocol) = message.protocol_message.as_option() {
        use wa::message::protocol_message::Type;
        match protocol.r#type {
            Some(Type::REVOKE) | Some(Type::MESSAGE_EDIT) => {
                entry.message_id = message_target(protocol.key.as_option(), &entry.chat);
                if entry.message_id.is_none() { return Vec::new(); }
                if protocol.r#type == Some(Type::MESSAGE_EDIT) {
                    if protocol.edited_message.as_option().is_none_or(|m| { let decoded = decoded_message(m); decoded.spoiler || decoded.view_once }) { return Vec::new(); }
                    entry.kind = Kind::MessageEdit;
                } else {
                    if previous.is_some_and(|p| p.media.kind.as_deref() == Some("live_location")) { return Vec::new(); }
                    entry.kind = Kind::MessageDelete;
                    entry.new_value = Some("revoked".into());
                    entry.old_value = previous.map(|p| if p.local.revoked { "revoked" } else { "not_revoked" }.into());
                    entry.old_source = previous.map(|_| OldSource::Cached);
                }
                entry.target = protocol.key.as_option().and_then(|k| k.participant.as_deref())
                    .and_then(|jid| jid.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid)));
            }
            Some(Type::GROUP_MEMBER_LABEL_CHANGE) => {
                let Some(tag) = protocol.member_label.as_option() else { return Vec::new(); };
                entry.kind = Kind::MemberTag;
                entry.target = entry.actor.clone();
                entry.new_value = tag.label.clone();
            }
            _ => return Vec::new(),
        }
    } else { return Vec::new(); }
    if entry.kind != Kind::MemberTag {
        entry.target = previous.and_then(|p| p.header.sender.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid))).or(entry.target);
    }
    vec![entry]
}

pub(super) async fn audit_group_message(store: &StoreWorker, info: &MessageInfo, message: &wa::Message,
    previous: Option<&StoredMessage>, source: Source) -> Result<bool> {
    if !info.source.chat.is_group() { return Ok(false); }
    let mut entry = base(info.source.chat.to_non_ad().to_string(), Some(info.id.to_string()), Some(info.timestamp.timestamp()), source);
    entry.actor = person(Some(&info.source.sender));
    let records = message_records(entry, message, previous);
    Ok(store.run(move |store| store.record_group_audit(&records)).await? > 0)
}

pub(super) async fn audit_group_history_message(store: &StoreWorker, chat: &str, web: &wa::WebMessageInfo,
    previous: Option<&StoredMessage>) -> Result<bool> {
    let jid: Jid = chat.parse()?;
    if !jid.is_group() { return Ok(false); }
    let key = web.key.as_option();
    let timestamp = web.message_timestamp.and_then(|at| i64::try_from(at).ok());
    let mut entry = base(jid.to_non_ad().to_string(), key.and_then(|k| k.id.clone()), timestamp, Source::History);
    entry.actor = web.participant.as_deref().or_else(|| key.and_then(|k| k.participant.as_deref()))
        .and_then(|jid| jid.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid)));
    let mut records = web.message.as_option().map(|message| message_records(entry.clone(), message, previous)).unwrap_or_default();
    if let Some(pin) = web.pin_in_chat.as_option() {
        use wa::pin_in_chat::Type;
        let pinned = match pin.r#type { Some(Type::PIN_FOR_ALL) => Some(true), Some(Type::UNPIN_FOR_ALL) => Some(false), _ => None };
        if let Some((pinned, target)) = pinned.zip(message_target(pin.key.as_option(), &entry.chat)) {
            let previous = if let Some(row) = previous.filter(|row| row.header.id == target && row.header.chat == entry.chat) {
                Some(row.clone())
            } else {
                let (chat, id) = (entry.chat.clone(), target.clone());
                store.run(move |store| store.audit_message_context(&chat, &id)).await?
            };
            if !previous.as_ref().is_some_and(private) {
                entry.kind = if pinned { Kind::MessagePin } else { Kind::MessageUnpin };
                entry.message_id = Some(target);
                entry.actor = None;
                let server_time = pin.server_timestamp_ms.filter(|at| *at > 0);
                let sender_time = pin.sender_timestamp_ms.filter(|at| *at > 0);
                entry.source_id = Some(format!("pin-metadata:{}", serde_json::to_string(&(entry.source_id.as_deref(), pinned, server_time, sender_time))?));
                entry.timestamp = server_time.or(sender_time).map(|at| at / 1000);
                entry.new_value = Some(pinned.to_string());
                entry.target = previous.as_ref().and_then(|p| p.header.sender.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid)));
                records.push(entry);
            }
        }
    }
    Ok(store.run(move |store| store.record_group_audit(&records)).await? > 0)
}

pub(super) async fn audit_group_notice(store: &StoreWorker, row: &StoredMessage, source: Source) -> Result<bool> {
    let records = notice_records(row, source);
    Ok(store.run(move |store| store.record_group_audit(&records)).await? > 0)
}

impl WhatsAppService {
    pub(super) async fn audit_local_group_change(&self, chat: &str, kind: Kind, target: Option<&str>,
        message_id: Option<&str>, source_id: Option<&str>, old_value: Option<&str>, new_value: Option<&str>) -> Result<bool> {
        let jid: Jid = chat.parse()?;
        if !jid.is_group() { return Ok(false); }
        let chat = jid.to_non_ad().to_string();
        if matches!(kind, Kind::MessageEdit | Kind::MessageDelete | Kind::MessagePin | Kind::MessageUnpin) {
            let Some(id) = message_id else { return Ok(false); };
            let previous = self.store.message(&chat, id).await?;
            if !audit_message_allowed(&previous)
                || (kind == Kind::MessageDelete && previous.media.kind.as_deref() == Some("live_location")) { return Ok(false); }
        }
        let actor = self.client.pn().or_else(|| self.client.lid());
        let mut record = local_group_record(&chat, kind, actor.as_ref());
        record.target = target.and_then(|jid| jid.parse::<Jid>().ok()).and_then(|jid| person(Some(&jid)));
        record.message_id = message_id.map(str::to_owned);
        record.source_id = source_id.map(str::to_owned);
        record.old_value = old_value.map(str::to_owned);
        record.new_value = new_value.map(str::to_owned);
        let changed = audit_group_local(&self.store, record).await?;
        if changed { let _ = self.events.send(ServiceEvent::GroupAuditChanged { chat }); }
        Ok(changed)
    }

    pub async fn group_audit_page(&self, chat: Option<&str>, filter: GroupAuditFilter) -> Result<GroupAuditPage> {
        let chat = chat.map(|chat| chat.parse::<Jid>()).transpose()?;
        anyhow::ensure!(chat.as_ref().is_none_or(|jid| jid.is_group() && !jid.user.is_empty()), "choose a group chat");
        for jid in [filter.actor.as_deref(), filter.target.as_deref(), filter.member.as_deref()].into_iter().flatten() {
            let parsed: Jid = jid.parse()?;
            anyhow::ensure!(person(Some(&parsed)).is_some(), "choose a group member");
        }
        let chat = chat.map(|chat| chat.to_non_ad().to_string());
        self.store.run(move |store| store.group_audit_page(chat.as_deref(), &filter)).await
    }

}

#[cfg(test)]
#[path = "group_audit_tests.rs"]
mod tests;
