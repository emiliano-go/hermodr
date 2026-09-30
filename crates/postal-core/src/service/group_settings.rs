use super::*;
use serde::{Deserialize, Serialize};
use whatsapp_rust::wacore::iq::groups::{GroupDescription, GroupMetadataOutcome, GroupQueryIq, GroupSubject, MembershipApprovalMode};

#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GroupSettings {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub description_id: Option<String>,
    pub announce: bool,
    pub locked: bool,
    pub approval: bool,
    pub member: bool,
    pub admin: bool,
    pub can_edit_info: bool,
    pub can_edit_picture: bool,
    pub community: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum GroupSettingChange {
    Subject { text: String },
    Description { text: Option<String>, previous_id: Option<String> },
    Announce { enabled: bool },
    Locked { enabled: bool },
    Approval { enabled: bool },
}

impl WhatsAppService {
    async fn read_group_settings(&self, group: &Jid) -> Result<GroupSettings> {
        let mut metadata = match self.client.execute(GroupQueryIq::new(group)).await
            .map_err(|error| anyhow::anyhow!(error.to_string()))? {
            GroupMetadataOutcome::Full(metadata) => whatsapp_rust::GroupMetadata::from(*metadata),
            GroupMetadataOutcome::NotModified => anyhow::bail!("current group settings could not be verified"),
        };
        anyhow::ensure!(metadata.id.to_non_ad() == group.to_non_ad(), "the server returned another group");
        self.client.groups().resolve_participant_addresses(&mut metadata).await;
        let own: Vec<Jid> = [self.client.pn(), self.client.lid()].into_iter().flatten().map(|jid| jid.to_non_ad()).collect();
        let member = metadata.participants.iter().any(|participant|
            matches_own(&own, &participant.jid, participant.phone_number.as_ref(), participant.lid.as_ref()));
        let admin = member && self.is_group_admin(&metadata);
        Ok(GroupSettings {
            subject: metadata.subject, description: metadata.description, description_id: metadata.description_id,
            announce: metadata.is_announcement, locked: metadata.is_locked, approval: metadata.membership_approval,
            member, admin, can_edit_info: member && (!metadata.is_locked || admin), can_edit_picture: admin,
            community: metadata.is_parent_group,
        })
    }

    pub async fn group_settings(&self, chat: &str, current: impl Fn() -> Result<()> + Send) -> Result<GroupSettings> {
        current()?;
        let group = settings_group(chat)?;
        let response = self.read_group_settings(&group).await;
        current()?;
        response
    }

    pub async fn change_group_setting(&self, chat: &str, change: GroupSettingChange, current: impl Fn() -> Result<()> + Send) -> Result<()> {
        current()?;
        let group = settings_group(chat)?;
        let settings = self.read_group_settings(&group).await?;
        validate_change(&settings, &change)?;
        let groups = self.client.groups();
        let subject = match &change { GroupSettingChange::Subject { text } => Some(text.clone()), _ => None };
        current()?;
        let response = match &change {
            GroupSettingChange::Subject { text } => groups.set_subject(group.clone(), subject_value(text)?).await,
            GroupSettingChange::Description { text, previous_id } => groups.set_description(group.clone(), description_value(text.clone())?,
                previous_id.as_deref().into()).await,
            GroupSettingChange::Announce { enabled } => groups.set_announce(group.clone(), *enabled).await,
            GroupSettingChange::Locked { enabled } => groups.set_locked(group.clone(), *enabled).await,
            GroupSettingChange::Approval { enabled } => groups.set_membership_approval(group.clone(),
                if *enabled { MembershipApprovalMode::On } else { MembershipApprovalMode::Off }).await,
        };
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let chat = group.to_string();
        if let Some(subject) = subject { self.store.set_name(&chat, subject.as_str()).await.logged(); }
        self.after_group_change(&chat);
        Ok(())
    }

    pub async fn set_group_picture(&self, chat: &str, bytes: Vec<u8>, current: impl Fn() -> Result<()> + Send) -> Result<()> {
        current()?;
        let group = settings_group(chat)?;
        let picture = tokio::task::spawn_blocking(move || picture_value(&bytes)).await??;
        let settings = self.read_group_settings(&group).await?;
        anyhow::ensure!(settings.member && settings.admin, "only group admins can change the picture");
        current()?;
        let response = if let Some(picture) = picture { self.client.groups().set_profile_picture(group.clone(), picture).await }
            else { self.client.groups().remove_profile_picture(group.clone()).await };
        current()?;
        response.map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let jid = group.to_string();
        invalidate_avatar_cache(self.media_dir.as_deref(), &jid);
        let _ = self.events.send(ServiceEvent::AvatarChanged { jid });
        Ok(())
    }
}

fn settings_group(chat: &str) -> Result<Jid> {
    let group: Jid = chat.parse()?;
    anyhow::ensure!(group.is_group() && !group.user.is_empty(), "choose a group");
    Ok(group.to_non_ad())
}

fn matches_own(own: &[Jid], jid: &Jid, phone: Option<&Jid>, lid: Option<&Jid>) -> bool {
    [Some(jid), phone, lid].into_iter().flatten()
        .any(|candidate| own.iter().any(|own| own.to_non_ad() == candidate.to_non_ad()))
}

fn validate_change(settings: &GroupSettings, change: &GroupSettingChange) -> Result<()> {
    anyhow::ensure!(settings.member, "you are no longer a member of this group");
    if matches!(change, GroupSettingChange::Subject { .. } | GroupSettingChange::Description { .. }) {
        anyhow::ensure!(!settings.locked || settings.admin, "only group admins can edit this group's info");
    } else { anyhow::ensure!(settings.admin, "only group admins can change this setting"); }
    if let GroupSettingChange::Description { previous_id, .. } = change {
        anyhow::ensure!(previous_id == &settings.description_id, "the group description changed; refresh it before saving");
    }
    Ok(())
}

fn subject_value(text: &str) -> Result<GroupSubject> {
    anyhow::ensure!(!text.trim().is_empty(), "enter a group name");
    GroupSubject::new(text.to_owned())
}

fn description_value(text: Option<String>) -> Result<Option<GroupDescription>> {
    text.filter(|text| !text.is_empty()).map(GroupDescription::new).transpose()
}

fn picture_value(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    if bytes.is_empty() { return Ok(None); }
    square_jpeg(bytes, 640).map(Some).ok_or_else(|| anyhow::anyhow!("that file is not an image we can read"))
}

#[cfg(test)]
#[path = "group_settings_tests.rs"]
mod tests;
