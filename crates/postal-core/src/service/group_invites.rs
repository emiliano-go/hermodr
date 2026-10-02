use super::*;
use crate::store::group_audit::GroupAuditKind as AuditKind;
use whatsapp_rust::wacore::iq::groups::{GroupMetadataOutcome, GroupQueryIq};

impl WhatsAppService {
    pub async fn group_invite_link(&self, chat: &str, reset: bool, current: impl Fn() -> Result<()> + Send) -> Result<String> {
        current()?;
        let group = invite_group(chat)?;
        if reset {
            let mut metadata = match self.client.execute(GroupQueryIq::new(&group)).await
                .map_err(|error| anyhow::anyhow!(error.to_string()))? {
                GroupMetadataOutcome::Full(metadata) => whatsapp_rust::GroupMetadata::from(*metadata),
                GroupMetadataOutcome::NotModified => anyhow::bail!("current group role could not be verified"),
            };
            self.client.groups().resolve_participant_addresses(&mut metadata).await;
            anyhow::ensure!(self.is_group_admin(&metadata), "only group admins can reset invite links");
        }
        current()?;
        let response = self.client.groups().get_invite_link(group, reset).await;
        current()?;
        let link = response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        validate_link(&link)?;
        if reset {
            self.audit_local_group_change(chat, AuditKind::InviteChange, None, None, None, None, None).await.logged();
            self.after_group_change(chat);
        }
        Ok(link)
    }

    pub async fn join_group_invite_message(&self, chat: &str, id: &str, current: impl Fn() -> Result<()> + Send) -> Result<(String, bool)> {
        current()?;
        let row = self.store.message(chat, id).await?;
        let invite = stored_v4_invite(&row)?;
        current()?;
        let response = self.client.groups().join_with_invite_v4(invite.group, &invite.code, invite.expiration, invite.admin).await;
        current()?;
        let result = response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let pending = matches!(result, whatsapp_rust::JoinGroupResult::PendingApproval(_));
        let jid = result.group_jid().to_string();
        if !pending { self.after_group_change(&jid); }
        Ok((jid, pending))
    }
}

fn invite_group(chat: &str) -> Result<Jid> {
    let group: Jid = chat.parse()?;
    anyhow::ensure!(group.is_group() && !group.user.is_empty(), "invite target must be a group");
    Ok(group.to_non_ad())
}

fn validate_link(link: &str) -> Result<()> {
    anyhow::ensure!(link.strip_prefix("https://chat.whatsapp.com/")
        .is_some_and(|code| !code.is_empty() && !code.chars().any(char::is_whitespace)), "WhatsApp returned no valid invite link");
    Ok(())
}

struct StoredV4Invite { group: Jid, code: String, expiration: i64, admin: Jid }

fn stored_v4_invite(row: &StoredMessage) -> Result<StoredV4Invite> {
    anyhow::ensure!(!row.header.from_me && !row.local.revoked && !row.local.deleted, "this invitation is no longer available");
    anyhow::ensure!(row.media.kind.as_deref() == Some("group_invite"), "this message is not a V4 group invitation");
    let mut bytes = row.media.locator.as_deref().ok_or_else(|| anyhow::anyhow!("this invitation has no original payload; ask for a new invitation"))?;
    let message = <wa::Message as buffa::Message>::decode(&mut bytes).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let invite = message.group_invite_message.as_option().ok_or_else(|| anyhow::anyhow!("the stored invitation payload is missing"))?;
    let only_invite = wa::Message { group_invite_message: message.group_invite_message.clone(), ..Default::default() };
    anyhow::ensure!(buffa::Message::encode_to_vec(&message) == buffa::Message::encode_to_vec(&only_invite), "mixed invitation payload is not supported");
    let group = invite_group(invite.group_jid.as_deref().unwrap_or_default())?;
    let code = invite.invite_code.clone().filter(|code| !code.is_empty() && code.trim() == code)
        .ok_or_else(|| anyhow::anyhow!("the invitation has no valid code"))?;
    let expiration = invite.invite_expiration.ok_or_else(|| anyhow::anyhow!("the invitation expiry is missing"))?;
    anyhow::ensure!(expiration >= 0, "the invitation expiry is invalid");
    let admin: Jid = row.header.sender.parse()?;
    anyhow::ensure!(!admin.user.is_empty() && (admin.is_pn() || admin.is_lid()), "the inviter address is invalid");
    Ok(StoredV4Invite { group, code, expiration, admin: admin.to_non_ad() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> StoredMessage {
        let message = wa::Message { group_invite_message: MessageField::some(wa::message::GroupInviteMessage {
            group_jid: Some("12345-678@g.us".into()), invite_code: Some("SyntheticInviteCode".into()),
            invite_expiration: Some(1_800_000_000), group_name: Some("Synthetic group".into()), ..Default::default()
        }), ..Default::default() };
        StoredMessage { header: MessageHeader { chat: "100@s.whatsapp.net".into(), id: "v4".into(), sender: "200:4@lid".into(), ..Default::default() },
            media: Media { kind: Some("group_invite".into()), locator: Some(buffa::Message::encode_to_vec(&message)), ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn original_invite_keeps_the_target_code_expiry_and_inviter() {
        let invite = stored_v4_invite(&row()).unwrap();
        assert_eq!(invite.group.to_string(), "12345-678@g.us");
        assert_eq!(invite.code, "SyntheticInviteCode");
        assert_eq!(invite.expiration, 1_800_000_000);
        assert_eq!(invite.admin.to_string(), "200@lid");
        let original = row();
        let json = serde_json::to_value(&original).unwrap();
        assert!(json.get("locator").is_none() && json.get("media_ref").is_none());
        assert!(!json.to_string().contains("SyntheticInviteCode"));
    }

    #[tokio::test]
    async fn received_v4_invitation_keeps_private_parameters_and_spoiler_context() {
        let raw = wa::Message { group_invite_message: MessageField::some(wa::message::GroupInviteMessage {
            group_jid: Some("12345-678@g.us".into()), invite_code: Some("SyntheticInviteCode".into()),
            invite_expiration: Some(1_800_000_000), group_name: Some("Synthetic group".into()),
            caption: Some("Synthetic invitation caption".into()), jpeg_thumbnail: Some(vec![1, 2, 3]),
            group_type: Some(wa::message::group_invite_message::GroupType::PARENT),
            context_info: MessageField::some(wa::ContextInfo { is_spoiler: Some(true), ..Default::default() }), ..Default::default()
        }), ..Default::default() };
        let stored = stored_message(&raw, row().header, None, None, false).await.unwrap();
        assert_eq!(stored.media.kind.as_deref(), Some("group_invite"));
        assert_eq!(stored.text, "Synthetic invitation caption");
        assert!(stored.spoiler && !stored.history_shareable);
        assert!(stored.media.thumb.as_deref().unwrap().starts_with("data:image/jpeg;base64,"));
        let locator = <wa::Message as buffa::Message>::decode(&mut stored.media.locator.as_ref().unwrap().as_slice()).unwrap();
        let invite = locator.group_invite_message.as_option().unwrap();
        assert_eq!(invite.group_jid.as_deref(), Some("12345-678@g.us"));
        assert_eq!(invite.invite_code.as_deref(), Some("SyntheticInviteCode"));
        assert_eq!(invite.invite_expiration, Some(1_800_000_000));
        assert_eq!(invite.group_type, raw.group_invite_message.group_type);
        assert!(invite.jpeg_thumbnail.is_none() && invite.context_info.is_unset());
        assert_eq!(stored_v4_invite(&stored).unwrap().admin.to_string(), "200@lid");
        let json = serde_json::to_string(&stored).unwrap();
        assert!(!json.contains("SyntheticInviteCode") && !json.contains("1800000000"));
    }

    #[test]
    fn unavailable_rows_and_legacy_links_cannot_manufacture_v4_invites() {
        for change in 0..6 {
            let mut row = row();
            match change {
                0 => row.header.from_me = true,
                1 => row.local.revoked = true,
                2 => row.local.deleted = true,
                3 => row.media.locator = None,
                4 => row.header.sender = "123@g.us".into(),
                _ => { row.media.kind = None; row.text = "https://chat.whatsapp.com/SyntheticInviteCode".into(); },
            }
            assert!(stored_v4_invite(&row).is_err(), "case {change}");
        }
    }

    #[test]
    fn malformed_or_mixed_payloads_never_supply_invite_parameters() {
        for change in 0..5 {
            let mut row = row();
            let mut message = <wa::Message as buffa::Message>::decode(&mut row.media.locator.as_ref().unwrap().as_slice()).unwrap();
            let invite = message.group_invite_message.as_option_mut().unwrap();
            match change {
                0 => invite.group_jid = Some("100@s.whatsapp.net".into()),
                1 => invite.invite_code = Some(" ".into()),
                2 => invite.invite_expiration = None,
                3 => invite.invite_expiration = Some(-1),
                _ => message.conversation = Some("unrelated payload".into()),
            }
            row.media.locator = Some(buffa::Message::encode_to_vec(&message));
            assert!(stored_v4_invite(&row).is_err(), "case {change}");
        }
    }

    #[test]
    fn groups_and_acknowledged_links_require_the_expected_shapes() {
        assert!(invite_group("123@g.us").is_ok());
        for chat in ["100@lid", "100@s.whatsapp.net", "status@broadcast", ""] { assert!(invite_group(chat).is_err()); }
        assert!(validate_link("https://chat.whatsapp.com/SyntheticInviteCode").is_ok());
        for link in ["", "https://chat.whatsapp.com/", "https://example.invalid/code", "https://chat.whatsapp.com/invalid code"] {
            assert!(validate_link(link).is_err());
        }
    }
}
