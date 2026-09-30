use std::collections::HashMap;
use whatsapp_rust::{
    Client, GroupHistorySkipReason, GroupMetadata,
    wacore::{
        iq::{abprops::{AbDefault, AbProp, group, web}, groups::{GroupMetadataOutcome, GroupQueryIq}, props::{GroupPropsResponse, GroupPropsSpec, PropsResponse, PropsSpec}},
        store::ab_props::AbPropsCache,
        types::wire_enums::MemberShareHistoryMode,
    },
    wacore_binary::{Jid, JidExt},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryShareLimits {
    pub max_messages: usize,
    pub time_window_seconds: u64,
}

pub(super) async fn query_history_share_limits(
    client: &Client,
    jid: &Jid,
) -> Result<HistoryShareLimits, GroupHistorySkipReason> {
    if !jid.is_group() {
        return Err(GroupHistorySkipReason::UnsupportedGroup);
    }
    let mut metadata = match client.execute(GroupQueryIq::new(jid)).await
        .map_err(|_| GroupHistorySkipReason::GroupMetadataUnavailable)? {
        GroupMetadataOutcome::Full(group) => GroupMetadata::from(*group),
        GroupMetadataOutcome::NotModified => return Err(GroupHistorySkipReason::GroupMetadataUnavailable),
    };
    client.groups().resolve_participant_addresses(&mut metadata).await;
    if metadata.has_capi || metadata.is_parent_group {
        return Err(GroupHistorySkipReason::UnsupportedGroup);
    }
    if !metadata.has_group_history {
        return Err(GroupHistorySkipReason::SharingDisabled);
    }
    let own = [client.pn(), client.lid()];
    let member = metadata.participants.iter().find(|member| {
        std::iter::once(&member.jid).chain(member.phone_number.iter()).chain(member.lid.iter())
            .any(|address| own.iter().flatten().any(|jid| address.user == jid.user && address.server == jid.server))
    }).ok_or(GroupHistorySkipReason::SenderNotAuthorized)?;
    if !member.is_admin() && !member.is_super_admin()
        && metadata.member_share_history_mode != Some(MemberShareHistoryMode::AllMemberShare)
    {
        return Err(GroupHistorySkipReason::SenderNotAuthorized);
    }
    let (account, group) = tokio::try_join!(
        async { client.execute_streaming(PropsSpec::new().retaining([
            web::GROUP_HISTORY_SEND.code,
            web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT.code,
            web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS.code,
        ])).await.map_err(|_| GroupHistorySkipReason::AccountPropsUnavailable) },
        async { client.execute(GroupPropsSpec::new(jid)).await
            .map_err(|_| GroupHistorySkipReason::GroupPropsUnavailable) },
    )?;
    resolve_history_share_limits(account, group).await
}

pub(super) async fn resolve_history_share_limits(
    account: PropsResponse,
    group: GroupPropsResponse,
) -> Result<HistoryShareLimits, GroupHistorySkipReason> {
    if account.delta_update {
        return Err(GroupHistorySkipReason::AccountPropsUnavailable);
    }
    if group.hash.as_deref().is_none_or(str::is_empty) {
        return Err(GroupHistorySkipReason::GroupPropsUnavailable);
    }
    let mut group_values = HashMap::new();
    for (code, value) in &group.experiment_props {
        if *code == 0 || group_values.insert(*code, value.as_str()).is_some_and(|old| old != value.as_str()) {
            return Err(GroupHistorySkipReason::GroupPropsUnavailable);
        }
    }
    let cache = AbPropsCache::new();
    cache.apply_props(false, account.experiment_props.into_iter()).await;
    let account = cache.snapshot().await;
    let enabled = account.get_bool(web::GROUP_HISTORY_SEND)
        .ok_or(GroupHistorySkipReason::InvalidProperties)?;
    let group_enabled = group_bool(&group_values, group::GROUP_HISTORY_SEND_GROUP_LEVEL)?;
    if !enabled && !group_enabled {
        return Err(GroupHistorySkipReason::SharingDisabled);
    }
    let account_window = account.get_int(web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS)
        .ok_or(GroupHistorySkipReason::InvalidProperties)?;
    let default_window = int_default(web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS)?;
    let window = if account_window != default_window {
        account_window
    } else {
        group_values.get(&group::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS_GROUP_LEVEL.code)
            .map_or_else(|| int_default(group::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS_GROUP_LEVEL),
                |value| value.parse().map_err(|_| GroupHistorySkipReason::InvalidProperties))?
    };
    let time_window_seconds = u64::try_from(window).ok().filter(|value| *value > 0)
        .ok_or(GroupHistorySkipReason::InvalidProperties)?;
    let max_messages = account.get_int(web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(GroupHistorySkipReason::InvalidProperties)?;
    if max_messages == 0 {
        return Err(GroupHistorySkipReason::NoEligibleMessages);
    }
    Ok(HistoryShareLimits { max_messages: max_messages.min(100), time_window_seconds: time_window_seconds.min(7 * 86400) })
}

fn group_bool(values: &HashMap<u32, &str>, prop: AbProp) -> Result<bool, GroupHistorySkipReason> {
    match values.get(&prop.code) {
        Some(value) if matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "enabled") => Ok(true),
        Some(value) if matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "disabled") => Ok(false),
        Some(_) => Err(GroupHistorySkipReason::InvalidProperties),
        None => match prop.default {
            AbDefault::Bool(value) => Ok(value),
            _ => Err(GroupHistorySkipReason::InvalidProperties),
        },
    }
}

fn int_default(prop: AbProp) -> Result<i64, GroupHistorySkipReason> {
    match prop.default {
        AbDefault::Int(value) => Ok(value),
        _ => Err(GroupHistorySkipReason::InvalidProperties),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(props: &[(AbProp, &str)]) -> PropsResponse {
        PropsResponse { experiment_props: props.iter().map(|(prop, value)| (prop.code, (*value).into())).collect(), ..Default::default() }
    }

    fn group(props: &[(AbProp, &str)]) -> GroupPropsResponse {
        GroupPropsResponse { hash: Some("synthetic-version".into()), experiment_props: props.iter().map(|(prop, value)| (prop.code, (*value).into())).collect() }
    }

    #[tokio::test]
    async fn disabled_incomplete_and_invalid_properties_refuse_history() {
        assert_eq!(resolve_history_share_limits(account(&[]), group(&[])).await, Err(GroupHistorySkipReason::SharingDisabled));
        let enabled = [(web::GROUP_HISTORY_SEND, "true")];
        assert_eq!(resolve_history_share_limits(account(&enabled), GroupPropsResponse::default()).await, Err(GroupHistorySkipReason::GroupPropsUnavailable));
        let mut delta = account(&enabled);
        delta.delta_update = true;
        assert_eq!(resolve_history_share_limits(delta, group(&[])).await, Err(GroupHistorySkipReason::AccountPropsUnavailable));
        for (prop, value) in [(web::GROUP_HISTORY_SEND, "maybe"), (web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT, "no"), (web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT, "-1"), (web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS, "0")] {
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true"), (prop, value)]), group(&[])).await, Err(GroupHistorySkipReason::InvalidProperties));
        }
        assert_eq!(resolve_history_share_limits(account(&enabled), group(&[(group::GROUP_HISTORY_SEND_GROUP_LEVEL, "maybe")])).await, Err(GroupHistorySkipReason::InvalidProperties));
        assert_eq!(resolve_history_share_limits(account(&enabled), group(&[(group::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS_GROUP_LEVEL, "0")])).await, Err(GroupHistorySkipReason::InvalidProperties));
        assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true"), (web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT, "0")]), group(&[])).await, Err(GroupHistorySkipReason::NoEligibleMessages));
    }

    #[tokio::test]
    async fn history_limits_follow_account_and_group_precedence_with_local_caps() {
        let bounded = HistoryShareLimits { max_messages: 100, time_window_seconds: 7 * 86400 };
        assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true")]), group(&[])).await, Ok(bounded));
        assert_eq!(resolve_history_share_limits(account(&[]), group(&[(group::GROUP_HISTORY_SEND_GROUP_LEVEL, "enabled")])).await, Ok(bounded));
        let small_group = [(group::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS_GROUP_LEVEL, "3600")];
        assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true"), (web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT, "12")]), group(&small_group)).await,
            Ok(HistoryShareLimits { max_messages: 12, time_window_seconds: 3600 }));
        assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true"), (web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS, "7200")]), group(&small_group)).await,
            Ok(HistoryShareLimits { max_messages: 100, time_window_seconds: 7200 }));
        assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true"), (web::GROUP_HISTORY_MESSAGE_COUNT_LIMIT, "1000"), (web::GROUP_HISTORY_MESSAGES_TIME_LIMIT_SECS, "2000000")]), group(&[])).await, Ok(bounded));
    }

    #[tokio::test]
    async fn conflicting_or_unversioned_group_properties_refuse_history() {
        let enabled = [(web::GROUP_HISTORY_SEND, "true")];
        let conflict = [(group::GROUP_HISTORY_SEND_GROUP_LEVEL, "true"), (group::GROUP_HISTORY_SEND_GROUP_LEVEL, "false")];
        assert_eq!(resolve_history_share_limits(account(&enabled), group(&conflict)).await, Err(GroupHistorySkipReason::GroupPropsUnavailable));
        let mut unversioned = group(&[]);
        unversioned.hash = Some(String::new());
        assert_eq!(resolve_history_share_limits(account(&enabled), unversioned).await, Err(GroupHistorySkipReason::GroupPropsUnavailable));
    }

    #[tokio::test]
    async fn history_boolean_parsing_matches_upstream_and_requires_only_one_enabled_sender_gate() {
        let bounded = HistoryShareLimits { max_messages: 100, time_window_seconds: 7 * 86400 };
        for value in ["1", "TRUE", "Enabled"] {
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, value)]), group(&[])).await, Ok(bounded));
            assert_eq!(resolve_history_share_limits(account(&[]), group(&[(group::GROUP_HISTORY_SEND_GROUP_LEVEL, value)])).await, Ok(bounded));
        }
        for value in ["0", "FALSE", "Disabled"] {
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, value)]), group(&[])).await, Err(GroupHistorySkipReason::SharingDisabled));
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, value)]), group(&[(group::GROUP_HISTORY_SEND_GROUP_LEVEL, "true")])).await, Ok(bounded));
        }
        for value in ["maybe", " true ", ""] {
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, value)]), group(&[])).await, Err(GroupHistorySkipReason::InvalidProperties));
            assert_eq!(resolve_history_share_limits(account(&[(web::GROUP_HISTORY_SEND, "true")]), group(&[(group::GROUP_HISTORY_SEND_GROUP_LEVEL, value)])).await, Err(GroupHistorySkipReason::InvalidProperties));
        }
    }
}
