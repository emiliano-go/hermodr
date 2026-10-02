use super::*;
use crate::store::member_profiles::{
    member_address, MemberNote, MemberProfileLocal, MemberRosterEntry,
};
use serde::Deserialize;
use whatsapp_rust::wacore::iq::{
    business::BusinessProfileSpec,
    contacts::ProfilePictureLookup,
    groups::{GroupMetadataOutcome, GroupQueryIq},
    usync::UsyncSubprotocolError,
};

const LIVE_TTL: i64 = 30;

fn profile_person(value: &str) -> Option<String> {
    let jid: Jid = value.parse().ok()?;
    member_address(&jid.to_non_ad().to_string()).ok()
}

fn profile_private(row: &StoredMessage) -> bool {
    row.spoiler
        || row.local.revoked
        || row.local.deleted
        || row.media.once_kind.is_some()
        || matches!(row.media.kind.as_deref(), Some("view_once" | "unknown"))
        || row.system.kind.as_deref() == Some("UNAVAILABLE_MESSAGE")
}

pub(super) async fn record_member_group_update(
    store: &StoreWorker,
    update: &whatsapp_rust::wacore::types::events::GroupUpdate,
) -> Result<()> {
    use whatsapp_rust::wacore::stanza::groups::GroupNotificationAction as Action;
    if !update.group_jid.is_group() {
        return Ok(());
    }
    let (members, action) = match update.action.as_ref() {
        Action::Add { participants, .. } => (participants, "add"),
        Action::Remove { participants, .. } => (participants, "remove"),
        Action::Promote { participants } | Action::LinkedGroupPromote { participants } => {
            (participants, "promote")
        }
        Action::Demote { participants } | Action::LinkedGroupDemote { participants } => {
            (participants, "demote")
        }
        _ => return Ok(()),
    };
    let chat = update.group_jid.to_non_ad().to_string();
    for member in members {
        let lid = member
            .lid
            .as_ref()
            .or_else(|| member.jid.is_lid().then_some(&member.jid));
        let pn = member
            .phone_number
            .as_ref()
            .or_else(|| member.jid.is_pn().then_some(&member.jid));
        if let (Some(lid), Some(pn)) = (lid, pn) {
            store
                .set_lid_pn(&lid.to_non_ad().to_string(), &pn.to_non_ad().to_string())
                .await?;
        }
        let Some(jid) = pn
            .or(lid)
            .and_then(|jid| profile_person(&jid.to_non_ad().to_string()))
        else {
            continue;
        };
        store
            .record_member_change(&chat, &jid, action, None, update.timestamp.timestamp())
            .await?;
    }
    Ok(())
}

pub(super) async fn record_member_message_context(
    store: &StoreWorker,
    row: &StoredMessage,
    message: &wa::Message,
) -> Result<()> {
    if profile_private(row) {
        return Ok(());
    }
    let decoded = super::message_decode::decoded_message(message);
    if decoded.spoiler || decoded.view_once {
        return Ok(());
    }
    if row.header.chat.ends_with("@g.us") {
        if let Some(protocol) = decoded.message.protocol_message.as_option() {
            if protocol.r#type
                == Some(wa::message::protocol_message::Type::GROUP_MEMBER_LABEL_CHANGE)
            {
                if let (Some(tag), Some(jid)) = (
                    protocol.member_label.as_option(),
                    profile_person(&row.header.sender),
                ) {
                    store
                        .record_member_change(
                            &row.header.chat,
                            &jid,
                            "label",
                            tag.label.as_deref().filter(|value| !value.is_empty()),
                            row.header.timestamp,
                        )
                        .await?;
                }
                return Ok(());
            }
        }
    }
    let context = super::message_decode::message_context(decoded.message);
    let targets = context
        .into_iter()
        .flat_map(|context| &context.mentioned_jid)
        .filter_map(|jid| profile_person(jid))
        .collect();
    store
        .record_member_mentions(&row.header.chat, &row.header.id, targets)
        .await?;
    let groups = context
        .into_iter()
        .flat_map(|context| &context.group_mentions)
        .filter_map(|group| group.group_jid.as_deref())
        .filter_map(|group| group.parse::<Jid>().ok())
        .filter(|group| group.is_group())
        .map(|group| group.to_non_ad().to_string())
        .collect();
    store
        .record_member_group_mentions(&row.header.chat, &row.header.id, groups)
        .await
}

pub(super) async fn record_member_history_context(
    store: &StoreWorker,
    row: &StoredMessage,
    message: &wa::WebMessageInfo,
) -> Result<()> {
    if let Some(message) = message.message.as_option() {
        record_member_message_context(store, row, message).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum MemberFieldState {
    Available,
    Unavailable,
    Restricted,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberProfileField<T> {
    pub state: MemberFieldState,
    pub value: Option<T>,
    pub error: Option<String>,
    pub stale: bool,
}

impl<T> MemberProfileField<T> {
    fn available(value: T) -> Self {
        Self {
            state: MemberFieldState::Available,
            value: Some(value),
            error: None,
            stale: false,
        }
    }
    fn unavailable() -> Self {
        Self {
            state: MemberFieldState::Unavailable,
            value: None,
            error: None,
            stale: false,
        }
    }
    fn failed(message: &str) -> Self {
        Self {
            state: MemberFieldState::Error,
            value: None,
            error: Some(message.into()),
            stale: false,
        }
    }
    fn restricted() -> Self {
        Self {
            state: MemberFieldState::Restricted,
            value: None,
            error: Some("Access denied by server.".into()),
            stale: false,
        }
    }
}

fn retain_failed<T: Clone>(field: &mut MemberProfileField<T>, cached: &MemberProfileField<T>) {
    if field.state == MemberFieldState::Error && cached.value.is_some() {
        field.value = cached.value.clone();
        field.stale = true;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberBusinessHours {
    pub day: String,
    pub mode: String,
    pub open_minutes: Option<u32>,
    pub close_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberBusinessProfile {
    pub name: Option<String>,
    pub description: String,
    pub email: Option<String>,
    pub websites: Vec<String>,
    pub address: Option<String>,
    pub categories: Vec<String>,
    pub timezone: Option<String>,
    pub hours: Option<Vec<MemberBusinessHours>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberProfileLive {
    pub fetched_at: i64,
    pub about: MemberProfileField<String>,
    pub username: MemberProfileField<String>,
    pub photo: MemberProfileField<String>,
    pub photo_id: Option<String>,
    pub business: MemberProfileField<MemberBusinessProfile>,
    pub business_name: MemberProfileField<String>,
    pub device_count: MemberProfileField<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MemberProfile {
    #[serde(flatten)]
    pub legacy: UserProfile,
    pub local: MemberProfileLocal,
    pub live: Option<MemberProfileLive>,
    pub live_cached: bool,
    pub live_stale: bool,
    pub moderation_admin_verified: bool,
    pub moderation_verified_at: Option<i64>,
    pub moderation_error: Option<String>,
}

fn fresh(at: i64, now: i64) -> bool {
    now >= at && now - at < LIVE_TTL
}

fn observed_field<T>(
    value: Option<T>,
    error: Option<&UsyncSubprotocolError>,
) -> MemberProfileField<T> {
    if let Some(error) = error {
        return match error.code {
            Some(401 | 403) => MemberProfileField::restricted(),
            Some(404 | 204) => MemberProfileField::unavailable(),
            _ => MemberProfileField::failed("Profile field query failed."),
        };
    }
    value
        .map(MemberProfileField::available)
        .unwrap_or_else(MemberProfileField::unavailable)
}

fn device_count(devices: &[u16]) -> Option<u32> {
    (!devices.is_empty()).then(|| {
        devices
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len() as u32
    })
}

fn legacy_profile(local: &MemberProfileLocal, live: Option<&MemberProfileLive>) -> UserProfile {
    let identity = &local.identity;
    UserProfile {
        jid: local.jid.clone(),
        name: identity
            .saved_name
            .clone()
            .or_else(|| identity.legacy_name.clone())
            .or_else(|| identity.push_name.clone()),
        number: identity.number.clone(),
        username: live
            .and_then(|value| value.username.value.clone())
            .or_else(|| identity.username.clone()),
        about: live.and_then(|value| value.about.value.clone()),
        business: live.and_then(|value| value.business_name.value.clone()),
    }
}

fn validate_moderation(
    own_admin: bool,
    present: bool,
    target_self: bool,
    target_owner: bool,
    target_admin: bool,
    action: &str,
) -> Result<()> {
    anyhow::ensure!(
        matches!(action, "promote" | "demote" | "remove"),
        "Unsupported member moderation action."
    );
    anyhow::ensure!(own_admin, "Current account is not a group admin.");
    anyhow::ensure!(present, "Member is no longer in this group.");
    anyhow::ensure!(
        !target_self,
        "Member moderation cannot target this account."
    );
    anyhow::ensure!(!target_owner, "Group owner cannot be moderated.");
    anyhow::ensure!(
        action != "promote" || !target_admin,
        "Member is already an admin."
    );
    anyhow::ensure!(
        action != "demote" || target_admin,
        "Member is no longer an admin."
    );
    Ok(())
}

impl WhatsAppService {
    pub async fn member_moderation_preflight(
        &self,
        group: &str,
        jid: &str,
        action: &str,
        current: impl Fn() -> Result<()> + Send,
    ) -> Result<()> {
        current()?;
        anyhow::ensure!(self.is_connected(), "Account is disconnected.");
        let group: Jid = group.parse()?;
        anyhow::ensure!(
            group.is_group() && group.device == 0 && group.agent == 0 && group.integrator == 0,
            "Member moderation requires a bare group address."
        );
        let target: Jid = member_address(jid)?.parse()?;
        let response = self.client.execute(GroupQueryIq::new(&group)).await;
        current()?;
        let mut metadata = match response.map_err(|error| anyhow::anyhow!(error.to_string()))? {
            GroupMetadataOutcome::Full(metadata) => whatsapp_rust::GroupMetadata::from(*metadata),
            GroupMetadataOutcome::NotModified => {
                anyhow::bail!("Current group roster could not be verified.")
            }
        };
        anyhow::ensure!(
            metadata.id.to_non_ad() == group,
            "Server returned another group."
        );
        current()?;
        self.client
            .groups()
            .resolve_participant_addresses(&mut metadata)
            .await;
        current()?;
        let own: Vec<Jid> = [self.client.pn(), self.client.lid()]
            .into_iter()
            .flatten()
            .map(|jid| jid.to_non_ad())
            .collect();
        let matches = |member: &whatsapp_rust::GroupParticipant, target: &Jid| {
            [&member.jid]
                .into_iter()
                .chain(member.phone_number.as_ref())
                .chain(member.lid.as_ref())
                .any(|jid| jid.to_non_ad() == *target)
        };
        let own_admin = metadata
            .participants
            .iter()
            .any(|member| member.is_admin() && own.iter().any(|own| matches(member, own)));
        let member = metadata
            .participants
            .iter()
            .find(|member| matches(member, &target));
        validate_moderation(
            own_admin,
            member.is_some(),
            member.is_some_and(|member| own.iter().any(|own| matches(member, own))),
            member.is_some_and(|member| member.is_super_admin()),
            member.is_some_and(|member| member.is_admin()),
            action,
        )?;
        current()
    }

    pub async fn member_profile(
        &self,
        jid: &str,
        group: Option<&str>,
        live: bool,
        force: bool,
        current: impl Fn() -> Result<()> + Send + Sync,
    ) -> Result<MemberProfile> {
        current()?;
        let jid = member_address(jid)?;
        let now = unix_now();
        let local = self
            .store
            .member_profile_local(&jid, group, &self.own_jid(), now)
            .await;
        current()?;
        let mut local = local?;
        let cached = self.store.member_live_cache(&jid).await;
        current()?;
        let mut cached = cached?
            .and_then(|(payload, _)| serde_json::from_str::<MemberProfileLive>(&payload).ok());
        if let Some(cached) = &mut cached {
            self.validate_member_photo(&jid, cached);
        }
        let needs_refresh = live
            && (force
                || cached
                    .as_ref()
                    .is_none_or(|value| !fresh(value.fetched_at, now)));
        let mut live_cached = cached.is_some();
        if needs_refresh {
            anyhow::ensure!(self.is_connected(), "Account is disconnected.");
            cached = Some(
                self.fetch_member_live(&jid, cached.as_ref(), &current)
                    .await?,
            );
            current()?;
            let payload = serde_json::to_string(cached.as_ref().unwrap())?;
            let saved = self
                .store
                .cache_member_live(&jid, &payload, cached.as_ref().unwrap().fetched_at)
                .await;
            current()?;
            saved?;
            local = self
                .store
                .member_profile_local(&jid, group, &self.own_jid(), unix_now())
                .await?;
            current()?;
            live_cached = false;
        }
        let mut moderation_admin_verified = false;
        let mut moderation_verified_at = None;
        let mut moderation_error = None;
        if live && group.is_some_and(|chat| chat.ends_with("@g.us")) {
            current()?;
            if self.is_connected() {
                let settings = self.group_settings(group.unwrap(), &current).await;
                current()?;
                match settings {
                    Ok(settings) => {
                        moderation_admin_verified = settings.member && settings.admin;
                        moderation_verified_at = Some(unix_now());
                    }
                    Err(_) => {
                        moderation_error = Some("Current admin role could not be verified.".into())
                    }
                }
            } else {
                moderation_error = Some("Account is disconnected.".into());
            }
        }
        current()?;
        Ok(MemberProfile {
            legacy: legacy_profile(&local, cached.as_ref()),
            local,
            live_stale: cached.as_ref().is_some_and(|value| {
                !fresh(value.fetched_at, unix_now())
                    || value.about.stale
                    || value.username.stale
                    || value.photo.stale
                    || value.business.stale
                    || value.business_name.stale
                    || value.device_count.stale
            }),
            live: cached,
            live_cached,
            moderation_admin_verified,
            moderation_verified_at,
            moderation_error,
        })
    }

    pub async fn set_member_note(
        &self,
        jid: &str,
        group: Option<&str>,
        text: &str,
        warnings: u32,
        current: impl Fn() -> Result<()> + Send,
    ) -> Result<MemberNote> {
        current()?;
        let result = self
            .store
            .set_member_note(jid, group, text, warnings, unix_now())
            .await;
        current()?;
        result
    }

    pub(crate) async fn record_group_profile_snapshot(
        &self,
        chat: &str,
        info: &GroupInfo,
    ) -> Result<()> {
        let roster = info
            .participants
            .iter()
            .map(|member| MemberRosterEntry {
                jid: member.jid.clone(),
                admin: member.admin,
                owner: member.owner,
                label: member.label.clone(),
            })
            .collect();
        self.store
            .record_group_profile_snapshot(
                chat,
                info.subject.as_deref(),
                roster,
                info.admin,
                unix_now(),
            )
            .await
    }

    fn validate_member_photo(&self, jid: &str, live: &mut MemberProfileLive) {
        if let Some(photo) = &live.photo.value {
            let valid = self.media_dir.as_ref().is_some_and(|dir| {
                let expected = avatar_path(dir, jid);
                Path::new(photo) == expected && expected.is_file()
            });
            if !valid {
                live.photo = MemberProfileField::unavailable();
            }
        }
    }

    async fn fetch_member_live(
        &self,
        jid: &str,
        cached: Option<&MemberProfileLive>,
        current: &(impl Fn() -> Result<()> + Send + Sync),
    ) -> Result<MemberProfileLive> {
        let target: Jid = jid.parse()?;
        let mut result = MemberProfileLive {
            fetched_at: unix_now(),
            about: MemberProfileField::unavailable(),
            username: MemberProfileField::unavailable(),
            photo: MemberProfileField::unavailable(),
            photo_id: None,
            business: MemberProfileField::unavailable(),
            business_name: MemberProfileField::unavailable(),
            device_count: MemberProfileField::unavailable(),
        };
        current()?;
        self.fill_member_info(&target, &mut result, current).await?;
        current()?;
        self.fill_member_photo(&target, &mut result, cached, current)
            .await?;
        current()?;
        if let Some(cached) = cached {
            retain_failed(&mut result.about, &cached.about);
            retain_failed(&mut result.username, &cached.username);
            retain_failed(&mut result.photo, &cached.photo);
            retain_failed(&mut result.business, &cached.business);
            retain_failed(&mut result.business_name, &cached.business_name);
            retain_failed(&mut result.device_count, &cached.device_count);
        }
        result.fetched_at = unix_now();
        Ok(result)
    }

    async fn fill_member_info(
        &self,
        target: &Jid,
        result: &mut MemberProfileLive,
        current: &(impl Fn() -> Result<()> + Send + Sync),
    ) -> Result<()> {
        current()?;
        let info = self.user_info(std::slice::from_ref(target)).await;
        current()?;
        let mut info = match info {
            Ok(info) => info,
            Err(_) => {
                result.about = MemberProfileField::failed("Profile query failed.");
                result.username = MemberProfileField::failed("Profile query failed.");
                result.business = MemberProfileField::failed("Profile query failed.");
                result.business_name = MemberProfileField::failed("Profile query failed.");
                result.device_count = MemberProfileField::failed("Profile query failed.");
                return Ok(());
            }
        };
        let Some(info) = info.remove(target) else {
            return Ok(());
        };
        result.about = observed_field(info.status, info.status_error.as_ref());
        result.username = observed_field(
            info.username.map(|value| value.to_string()),
            info.username_error.as_ref(),
        );
        result.device_count =
            observed_field(device_count(&info.devices), info.devices_error.as_ref());
        let name = info.verified_name.and_then(|value| value.name);
        result.business_name = observed_field(name.clone(), info.business_error.as_ref());
        if let Some(username) = result.username.value.as_deref() {
            current()?;
            let stored = self.store.set_username(&target.to_string(), username).await;
            current()?;
            stored?;
        }
        if target.is_pn() && info.lid_error.is_none() {
            if let Some(lid) = info.lid.filter(|lid| lid.is_lid()) {
                current()?;
                let stored = self
                    .store
                    .set_lid_pn(&lid.to_non_ad().to_string(), &target.to_string())
                    .await;
                current()?;
                stored?;
            }
        }
        if info.business_error.is_some() {
            result.business = observed_field(None, info.business_error.as_ref());
        } else if info.is_business {
            current()?;
            result.business = self.fetch_member_business(target, name, current).await?;
            current()?;
        }
        Ok(())
    }

    async fn fetch_member_business(
        &self,
        target: &Jid,
        name: Option<String>,
        current: &(impl Fn() -> Result<()> + Send + Sync),
    ) -> Result<MemberProfileField<MemberBusinessProfile>> {
        current()?;
        let business = self.client.execute(BusinessProfileSpec::new(target)).await;
        current()?;
        Ok(match business {
            Ok(Some(value)) => MemberProfileField::available(MemberBusinessProfile {
                name,
                description: value.description,
                email: value.email,
                websites: value.website,
                address: value.address,
                categories: value
                    .categories
                    .into_iter()
                    .map(|value| value.name)
                    .collect(),
                timezone: value.business_hours.timezone,
                hours: value.business_hours.business_config.map(|hours| {
                    hours
                        .into_iter()
                        .map(|value| MemberBusinessHours {
                            day: value.day_of_week.to_string(),
                            mode: value.mode.to_string(),
                            open_minutes: value.open_time,
                            close_minutes: value.close_time,
                        })
                        .collect()
                }),
            }),
            Ok(None) => MemberProfileField::unavailable(),
            Err(_) => MemberProfileField::failed("Business profile query failed."),
        })
    }

    async fn fill_member_photo(
        &self,
        target: &Jid,
        result: &mut MemberProfileLive,
        cached: Option<&MemberProfileLive>,
        current: &(impl Fn() -> Result<()> + Send + Sync),
    ) -> Result<()> {
        let jid = target.to_string();
        current()?;
        let picture = self
            .client
            .contacts()
            .lookup_profile_picture(
                target,
                true,
                cached.and_then(|value| value.photo_id.as_deref()),
            )
            .await;
        current()?;
        result.photo = match picture {
            Ok(ProfilePictureLookup::Found(picture)) => {
                result.photo_id = Some(picture.id);
                if cached.and_then(|value| value.photo_id.as_ref()) != result.photo_id.as_ref() {
                    invalidate_avatar_cache(self.media_dir.as_deref(), &jid);
                }
                current()?;
                let photo = self.avatar(&jid, false).await;
                current()?;
                match photo {
                    Ok(Some(path)) => MemberProfileField::available(path),
                    Ok(None) => MemberProfileField::unavailable(),
                    Err(_) => MemberProfileField::failed("Profile picture download failed."),
                }
            }
            Ok(ProfilePictureLookup::Unchanged) => {
                result.photo_id = cached.and_then(|value| value.photo_id.clone());
                cached
                    .map(|value| value.photo.clone())
                    .unwrap_or_else(MemberProfileField::unavailable)
            }
            Ok(ProfilePictureLookup::NotAuthorized) => MemberProfileField::restricted(),
            Ok(ProfilePictureLookup::NotFound) => MemberProfileField::unavailable(),
            Ok(ProfilePictureLookup::RateOverlimit) => {
                MemberProfileField::failed("Profile picture query rate limited.")
            }
            _ => MemberProfileField::failed("Profile picture query failed."),
        };
        current()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "member_profiles_tests.rs"]
mod tests;
