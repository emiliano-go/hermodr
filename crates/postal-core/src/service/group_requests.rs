use super::*;
use crate::message_ref::MessageRef;
use whatsapp_rust::wacore::iq::groups::{GroupMetadataOutcome, GroupQueryIq, ParticipantChangeResponse};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupJoinRequest {
    pub jid: String,
    pub name: String,
    pub request_time: Option<u64>,
}

impl WhatsAppService {
    pub async fn group_join_requests(&self, chat: &str) -> Result<Vec<GroupJoinRequest>> {
        let group = self.join_request_admin(chat).await?;
        let requests = self.client.groups().get_membership_requests(group).await
            .map_err(anyhow::Error::from)?;
        let mut seen = std::collections::HashSet::new();
        let requests = requests.into_iter().filter(|request| seen.insert(request.jid.to_non_ad())).collect::<Vec<_>>();
        let jids = requests.iter().map(|request| request.jid.to_non_ad().to_string()).collect::<Vec<_>>();
        let names = self.names_for(&jids).await;
        Ok(requests.into_iter().map(|request| {
            let jid = request.jid.to_non_ad().to_string();
            GroupJoinRequest { name: names.get(&jid).cloned().unwrap_or_else(|| request.jid.user.to_string()),
                jid, request_time: request.request_time }
        }).collect())
    }

    pub async fn change_group_join_requests(&self, chat: &str, jids: &[String], approve: bool) -> Result<Vec<ParticipantChange>> {
        let selected = request_participants(jids)?;
        let group = self.join_request_admin(chat).await?;
        let groups = self.client.groups();
        let result = if approve { groups.approve_membership_requests(group, &selected).await }
            else { groups.reject_membership_requests(group, &selected).await }
            .map_err(anyhow::Error::from)?;
        self.after_group_change(chat);
        Ok(result.iter().map(|response| request_change(response, &selected)).collect())
    }

    async fn join_request_admin(&self, chat: &str) -> Result<Jid> {
        let group = request_group(chat)?;
        let mut metadata = match self.client.execute(GroupQueryIq::new(&group)).await
            .map_err(anyhow::Error::from)? {
            GroupMetadataOutcome::Full(metadata) => whatsapp_rust::GroupMetadata::from(*metadata),
            GroupMetadataOutcome::NotModified => anyhow::bail!(MessageRef::new("error.group_role_unverified")),
        };
        self.client.groups().resolve_participant_addresses(&mut metadata).await;
        anyhow::ensure!(self.is_group_admin(&metadata), MessageRef::new("error.group_requests_admin"));
        Ok(group)
    }
}

fn request_group(chat: &str) -> Result<Jid> {
    let jid: Jid = chat.parse::<Jid>().map_err(|error| anyhow::Error::new(MessageRef::new("error.group_requests_group")).context(error.to_string()))?;
    anyhow::ensure!(jid.is_group() && !jid.user.is_empty(), MessageRef::new("error.group_requests_group"));
    Ok(jid.to_non_ad())
}

fn request_participants(jids: &[String]) -> Result<Vec<Jid>> {
    anyhow::ensure!(!jids.is_empty() && jids.len() <= 1024, MessageRef::new("error.group_requests_count").with_param("limit", serde_json::Number::from(1024)));
    let mut seen = std::collections::HashSet::new();
    let mut selected = Vec::new();
    for jid in groups::parse_jids(jids)? {
        anyhow::ensure!(!jid.user.is_empty() && (jid.is_pn() || jid.is_lid()), MessageRef::new("error.group_request_address"));
        let jid = jid.to_non_ad();
        if seen.insert(jid.clone()) { selected.push(jid); }
    }
    Ok(selected)
}

fn request_change(response: &ParticipantChangeResponse, selected: &[Jid]) -> ParticipantChange {
    let mut change = groups::change_of(response);
    if let Some(jid) = selected.iter().find(|jid| {
        **jid == response.jid.to_non_ad() || response.phone_number.as_ref().is_some_and(|phone| **jid == phone.to_non_ad())
    }) { change.jid = jid.to_string(); }
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_request_targets_require_groups_and_real_user_addresses() {
        assert!(request_group("1@g.us").is_ok());
        for chat in ["1@s.whatsapp.net", "@g.us", "not a jid"] { assert!(request_group(chat).is_err()); }
        assert!(request_participants(&[]).is_err());
        for jid in ["@s.whatsapp.net", "1@g.us", "1@newsletter", "not a jid"] {
            assert!(request_participants(&[jid.into()]).is_err());
        }
        let selected = request_participants(&["100:2@s.whatsapp.net".into(), "100@s.whatsapp.net".into(), "200@lid".into()]).unwrap();
        assert_eq!(selected.iter().map(ToString::to_string).collect::<Vec<_>>(), ["100@s.whatsapp.net", "200@lid"]);
        assert!(request_participants(&vec!["100@s.whatsapp.net".into(); 1025]).is_err());
    }

    #[test]
    fn join_request_results_keep_the_requested_phone_alias_and_server_refusal() {
        use whatsapp_rust::wacore::protocol::ProtocolNode;
        let selected = request_participants(&["100@s.whatsapp.net".into()]).unwrap();
        let node = NodeBuilder::new("participant").attr("jid", "200@lid").attr("phone_number", "100@s.whatsapp.net")
            .attr("type", "403").attr("error", "not-authorized").build();
        let response = ParticipantChangeResponse::try_from_node(&node).unwrap();
        let result = request_change(&response, &selected);
        assert_eq!(result.jid, "100@s.whatsapp.net");
        assert!(!result.ok);
        assert_eq!(result.code.as_deref(), Some("403"));
    }
}
