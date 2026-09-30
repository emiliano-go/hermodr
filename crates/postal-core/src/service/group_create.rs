use super::*;
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
        anyhow::ensure!(self.is_connected(), "not connected yet");
        let (subject, selected) = creation_input(subject, jids)?;
        let mut participants = Vec::new();
        let mut seen = HashSet::new();
        for requested in selected {
            anyhow::ensure!(!self.is_self_jid(&requested), "you are already included as the group creator");
            let resolved = contacts::resolve_chat(Some(&self.client), &self.store, &requested).await;
            current()?;
            let phone = creation_address(&resolved)?;
            anyhow::ensure!(phone.is_pn(), "phone number mapping is unavailable for {requested}");
            anyhow::ensure!(!self.is_self_jid(&phone), "you are already included as the group creator");
            if seen.insert(phone.clone()) { participants.push((requested, phone)); }
        }
        let options = GroupCreateOptions::new(&subject).with_participants(participants.iter()
            .map(|(_, phone)| GroupParticipantOptions::new(phone.clone())).collect());
        current()?;
        let created = self.client.groups().create_group(options).await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let jid = created.metadata.id.to_non_ad();
        anyhow::ensure!(jid.is_group() && !jid.user.is_empty(), "WhatsApp returned an invalid created group address");
        let mut result = GroupCreateResult {
            jid: jid.to_string(), subject: created.metadata.subject.unwrap_or(subject),
            participants: creation_outcomes(&participants, &HashSet::new(), &HashSet::new()), warnings: Vec::new(),
        };
        let timestamp = created.metadata.creation_time.and_then(|value| i64::try_from(value).ok())
            .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs() as i64).unwrap_or(0));
        if let Err(error) = self.store.save_created_group(&result.jid, &result.subject, timestamp).await {
            result.warnings.push(format!("Group created on WhatsApp, but could not be saved locally: {error}"));
        }
        self.after_group_change(&result.jid);
        if let Err(error) = current() {
            result.warnings.push(format!("Group created; participant membership was not verified: {error}"));
            return Ok(result);
        }
        self.verify_created_members(&jid, &participants, &mut result).await;
        Ok(result)
    }

    async fn verify_created_members(&self, jid: &Jid, selected: &[(Jid, Jid)], result: &mut GroupCreateResult) {
        let metadata = match self.client.execute(GroupQueryIq::new(jid)).await {
            Ok(GroupMetadataOutcome::Full(metadata)) => *metadata,
            Ok(GroupMetadataOutcome::NotModified) => {
                result.warnings.push("Group created; WhatsApp did not return a fresh member list.".into());
                return;
            }
            Err(error) => {
                result.warnings.push(format!("Group created; participant membership could not be verified: {error}"));
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
                Err(error) => result.warnings.push(format!("Pending join requests could not be checked: {error}")),
            }
        }
        result.participants = creation_outcomes(selected, &members, &pending);
    }
}

fn creation_address(value: &str) -> Result<Jid> {
    let jid: Jid = value.parse()?;
    anyhow::ensure!(!jid.user.is_empty() && jid.user.chars().all(|value| value.is_ascii_digit())
        && (jid.is_pn() || jid.is_lid()), "invalid participant address");
    Ok(jid.to_non_ad())
}

fn creation_input(subject: &str, jids: &[String]) -> Result<(String, Vec<Jid>)> {
    let subject = subject.trim();
    anyhow::ensure!(!subject.is_empty(), "enter a group subject");
    let subject = GroupSubject::new(subject).map_err(|error| anyhow::anyhow!(error.to_string()))?.into_string();
    // ponytail: pinned SDK group limit; use server props if larger groups are needed.
    anyhow::ensure!(!jids.is_empty() && jids.len() < GROUP_SIZE_LIMIT, "select between 1 and {} people", GROUP_SIZE_LIMIT - 1);
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
        assert!(creation_input("Group", &[]).is_err());
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
}
