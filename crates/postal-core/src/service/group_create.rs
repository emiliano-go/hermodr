use super::*;
use crate::message_ref::{MessageFailure, MessageRef};
use crate::store::group_audit::GroupAuditKind as AuditKind;
use std::collections::HashSet;
use whatsapp_rust::wacore::iq::groups::{
    GroupCreateOptions, GroupMetadataOutcome, GroupParticipantOptions, GroupQueryIq, GroupSubject,
    GROUP_SIZE_LIMIT,
};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GroupCreateParticipantState {
    Added,
    Pending,
    Unconfirmed,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupCreateParticipant {
    pub jid: String,
    pub state: GroupCreateParticipantState,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupCreateResult {
    pub jid: String,
    pub subject: String,
    pub participants: Vec<GroupCreateParticipant>,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "wire-types", ts(as = "Option<Vec<MessageFailure>>", optional))]
    pub warning_refs: Vec<MessageFailure>,
}

impl WhatsAppService {
    pub async fn group_creation_contacts(&self, query: &str) -> Result<Vec<SearchResult>> {
        let rows = if query.trim().is_empty() {
            self.store.search_names("", 50).await?.into_iter().map(|(jid, name, saved)| SearchResult {
                number: jid.split('@').next().unwrap_or_default().to_string(), jid, name, saved,
                kind: "contact".into(), has_messages: false, aliases: Vec::new(),
            }).collect()
        } else { self.search(query).await? };
        let mut seen = HashSet::new();
        let mut contacts = Vec::new();
        for mut row in rows {
            let Ok(jid) = creation_address(&row.jid) else { continue };
            if self.is_self_jid(&jid) { continue; }
            row.jid = contacts::resolve_chat(None, &self.store, &jid).await;
            let jid = creation_address(&row.jid)?;
            if self.is_self_jid(&jid) || !seen.insert(jid) { continue; }
            row.number = row.jid.split('@').next().unwrap_or_default().to_string();
            contacts.push(row);
        }
        Ok(contacts)
    }

    pub async fn create_group(&self, subject: &str, jids: &[String], current: impl Fn() -> Result<()> + Send) -> Result<GroupCreateResult> {
        current()?;
        anyhow::ensure!(self.is_connected(), MessageRef::new("error.not_connected"));
        let (subject, selected) = creation_input(subject, jids)?;
        let mut participants = Vec::new();
        let mut seen = HashSet::new();
        for requested in selected {
            anyhow::ensure!(!self.is_self_jid(&requested), MessageRef::new("error.group_creator_selected"));
            let resolved = contacts::resolve_chat(Some(&self.client), &self.store, &requested).await;
            current()?;
            let phone = creation_address(&resolved)?;
            anyhow::ensure!(phone.is_pn(), MessageRef::new("error.group_creation_phone").with_param("address", requested.to_string()));
            anyhow::ensure!(!self.is_self_jid(&phone), MessageRef::new("error.group_creator_selected"));
            if seen.insert(phone.clone()) { participants.push((requested, phone)); }
        }
        let options = GroupCreateOptions::new(&subject).with_participants(participants.iter()
            .map(|(_, phone)| GroupParticipantOptions::new(phone.clone())).collect());
        current()?;
        let created = self.client.groups().create_group(options).await
            .map_err(anyhow::Error::from)?;
        let jid = created.metadata.id.to_non_ad();
        anyhow::ensure!(jid.is_group() && !jid.user.is_empty(), MessageRef::new("error.group_created_address"));
        let mut result = GroupCreateResult {
            jid: jid.to_string(), subject: created.metadata.subject.unwrap_or(subject),
            participants: creation_outcomes(&participants, &HashSet::new(), &HashSet::new()), warnings: Vec::new(), warning_refs: Vec::new(),
        };
        let timestamp = created.metadata.creation_time.and_then(|value| i64::try_from(value).ok())
            .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs() as i64).unwrap_or(0));
        if let Err(error) = self.store.save_created_group(&result.jid, &result.subject, timestamp).await {
            creation_warning(&mut result, "warning.group_local_save", format!("Group created on WhatsApp, but could not be saved locally: {error}"), Some(format!("{error:#}")));
        }
        self.after_group_change(&result.jid);
        self.audit_local_group_change(&result.jid, AuditKind::Create, None, None, None, None, Some(&result.subject)).await.logged();
        if let Err(error) = current() {
            creation_warning(&mut result, "warning.group_membership_unverified", format!("Group created; participant membership was not verified: {error}"), Some(format!("{error:#}")));
            return Ok(result);
        }
        self.verify_created_members(&jid, &participants, &mut result).await;
        Ok(result)
    }

    async fn verify_created_members(&self, jid: &Jid, selected: &[(Jid, Jid)], result: &mut GroupCreateResult) {
        let metadata = match self.client.execute(GroupQueryIq::new(jid)).await {
            Ok(GroupMetadataOutcome::Full(metadata)) => *metadata,
            Ok(GroupMetadataOutcome::NotModified) => {
                creation_warning(result, "warning.group_roster_unavailable", "Group created; WhatsApp did not return a fresh member list.".into(), None);
                return;
            }
            Err(error) => {
                creation_warning(result, "warning.group_membership_unverified", format!("Group created; participant membership could not be verified: {error}"), Some(format!("{error:#}")));
                return;
            }
        };
        let mut metadata = whatsapp_rust::GroupMetadata::from(metadata);
        self.client.groups().resolve_participant_addresses(&mut metadata).await;
        let members = metadata.participants.iter().flat_map(|member| {
            [Some(&member.jid), member.phone_number.as_ref(), member.lid.as_ref()]
                .into_iter().flatten().map(Jid::to_non_ad)
        }).collect::<HashSet<_>>();
        let mut pending = HashSet::new();
        if selected.iter().any(|(_, phone)| !members.contains(phone)) {
            match self.client.groups().get_membership_requests(jid.clone()).await {
                Ok(requests) => {
                    for request in requests {
                        let resolved = contacts::resolve_chat(Some(&self.client), &self.store, &request.jid).await;
                        if let Ok(jid) = creation_address(&resolved) { pending.insert(jid); }
                    }
                }
                Err(error) => creation_warning(result, "warning.group_requests_unverified", format!("Pending join requests could not be checked: {error}"), Some(format!("{error:#}"))),
            }
        }
        result.participants = creation_outcomes(selected, &members, &pending);
    }
}

fn creation_warning(result: &mut GroupCreateResult, code: &str, legacy: String, diagnostic: Option<String>) {
    result.warnings.push(legacy);
    result.warning_refs.push(MessageFailure { message: MessageRef::new(code), diagnostic });
}

fn creation_address(value: &str) -> Result<Jid> {
    let jid: Jid = value.parse::<Jid>().map_err(|error| anyhow::Error::new(
        MessageRef::new("error.participant_address")
    ).context(error.to_string()))?;
    anyhow::ensure!(!jid.user.is_empty() && jid.user.chars().all(|value| value.is_ascii_digit())
        && (jid.is_pn() || jid.is_lid()), MessageRef::new("error.participant_address"));
    Ok(jid.to_non_ad())
}

fn creation_input(subject: &str, jids: &[String]) -> Result<(String, Vec<Jid>)> {
    let subject = subject.trim();
    anyhow::ensure!(!subject.is_empty(), MessageRef::new("error.group_subject_required"));
    let subject = GroupSubject::new(subject).map_err(|error| anyhow::Error::new(
        MessageRef::new("error.group_subject_invalid")
    ).context(error.to_string()))?.into_string();
    // ponytail: pinned SDK group limit; use server props if larger groups are needed.
    anyhow::ensure!(!jids.is_empty() && jids.len() < GROUP_SIZE_LIMIT, MessageRef::new("error.group_creation_count").with_param("limit", serde_json::Number::from(GROUP_SIZE_LIMIT - 1)));
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for value in jids {
        let jid = creation_address(value)?;
        if seen.insert(jid.clone()) { selected.push(jid); }
    }
    Ok((subject, selected))
}

fn creation_outcomes(selected: &[(Jid, Jid)], members: &HashSet<Jid>, pending: &HashSet<Jid>) -> Vec<GroupCreateParticipant> {
    selected.iter().map(|(requested, phone)| GroupCreateParticipant {
        jid: requested.to_string(), state: if members.contains(phone) || members.contains(requested) {
            GroupCreateParticipantState::Added
        } else if pending.contains(phone) || pending.contains(requested) { GroupCreateParticipantState::Pending }
        else { GroupCreateParticipantState::Unconfirmed },
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_validates_subject_and_people_before_network_work() {
        let person = "100@s.whatsapp.net".to_string();
        assert!(creation_input("  ", std::slice::from_ref(&person)).is_err());
        assert!(creation_input(&"🦀".repeat(100), std::slice::from_ref(&person)).is_ok());
        assert!(creation_input(&"🦀".repeat(101), std::slice::from_ref(&person)).is_err());
        let error = creation_input("Group", &[]).unwrap_err();
        let reference = error.downcast_ref::<MessageRef>().unwrap();
        assert_eq!(reference.code, "error.group_creation_count");
        assert_eq!(serde_json::to_value(reference).unwrap()["params"]["limit"], GROUP_SIZE_LIMIT - 1);
        assert!(creation_input("Group", &vec![person.clone(); GROUP_SIZE_LIMIT]).is_err());
        for value in ["@s.whatsapp.net", "1@g.us", "1@newsletter", "status@broadcast", "name@lid", "bad"] {
            assert!(creation_input("Group", &[value.into()]).is_err());
        }
        let (subject, selected) = creation_input(" Group ", &[person, "100:2@s.whatsapp.net".into(), "200@lid".into()]).unwrap();
        assert_eq!(subject, "Group");
        assert_eq!(selected.iter().map(ToString::to_string).collect::<Vec<_>>(), ["100@s.whatsapp.net", "200@lid"]);
    }

    #[test]
    fn creation_outcomes_require_evidence_and_keep_requested_aliases() {
        let jid = |value: &str| creation_address(value).unwrap();
        let selected = vec![(jid("200@lid"), jid("100@s.whatsapp.net")), (jid("300@s.whatsapp.net"), jid("300@s.whatsapp.net")),
            (jid("400@s.whatsapp.net"), jid("400@s.whatsapp.net"))];
        let outcomes = creation_outcomes(&selected, &HashSet::from([jid("100@s.whatsapp.net")]), &HashSet::from([jid("300@s.whatsapp.net")]));
        assert_eq!(outcomes[0].jid, "200@lid");
        assert!(matches!(outcomes[0].state, GroupCreateParticipantState::Added));
        assert!(matches!(outcomes[1].state, GroupCreateParticipantState::Pending));
        assert!(matches!(outcomes[2].state, GroupCreateParticipantState::Unconfirmed));
        let unknown = creation_outcomes(&selected, &HashSet::new(), &HashSet::new());
        assert!(unknown.iter().all(|value| matches!(value.state, GroupCreateParticipantState::Unconfirmed)));
    }
    #[test]
    fn creation_warnings_keep_acknowledged_group_and_legacy_wire_fields() {
        let mut result = GroupCreateResult {
            jid: "100@g.us".into(), subject: "Synthetic group".into(),
            participants: Vec::new(), warnings: Vec::new(), warning_refs: Vec::new(),
        };
        assert!(serde_json::to_value(&result).unwrap().get("warning_refs").is_none());
        creation_warning(&mut result, "warning.group_local_save", "legacy warning".into(), Some("synthetic SQL detail".into()));
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["jid"], "100@g.us");
        assert_eq!(value["warnings"][0], "legacy warning");
        assert_eq!(value["warning_refs"][0]["code"], "warning.group_local_save");
        assert_eq!(value["warning_refs"][0]["diagnostic"], "synthetic SQL detail");
        assert!(result.participants.is_empty());
    }
}
