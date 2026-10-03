use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

/// Messages reported to a group's admins.
#[tauri::command]
pub(crate) async fn admin_reports(state: State<'_, AppState>, chat: String) -> CommandResult<Vec<postal_core::AdminReport>> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.admin_reports(&chat).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_allow_admin_reports(state: State<'_, AppState>, chat: String, allow: bool) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.set_allow_admin_reports(&chat, allow).await.map_err(CommandError::from)
}

/// Group members for mention autocomplete.
#[tauri::command]
pub(crate) async fn participants(
    state: State<'_, AppState>,
    chat: String,
) -> CommandResult<Vec<postal_core::Participant>> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .participants(&chat)
        .await
        .map_err(CommandError::from)
}

/// Group subject, description and members, for the info sidebar.
#[tauri::command]
pub(crate) async fn group_info(
    state: State<'_, AppState>,
    chat: String,
) -> CommandResult<postal_core::GroupInfo> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .group_info(&chat)
        .await
        .map_err(CommandError::from)
}

/// Community parents and subgroups among the account's groups, by JID.
#[tauri::command]
pub(crate) async fn group_kinds(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::HashMap<String, postal_core::GroupKind>> {
    Ok(state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.group_kinds().await)
}

/// Adds participants to a group.
#[tauri::command]
pub(crate) async fn add_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
) -> CommandResult<Vec<postal_core::ParticipantChange>> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .add_group_participants(&chat, &jids)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn group_history_offer(state: State<'_, AppState>, account: String, chat: String) -> CommandResult<postal_core::GroupHistoryOffer> {
    Ok(state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.group_history_offer(&chat).await)
}

#[tauri::command]
pub(crate) async fn add_group_participants_with_history(state: State<'_, AppState>, account: String,
    chat: String, jids: Vec<String>, opted_in: Vec<String>) -> CommandResult<postal_core::GroupMemberAddResult> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.add_group_participants_with_history(&chat, &jids, &opted_in).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

#[tauri::command]
pub(crate) async fn retry_group_history(state: State<'_, AppState>, account: String, chat: String,
    retry_id: String) -> CommandResult<postal_core::GroupHistoryResult> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.retry_group_history(&chat, &retry_id).await.map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Removes participants from a group; a community parent also removes them
/// from its subgroups.
#[tauri::command]
pub(crate) async fn remove_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
    account: Option<String>,
) -> CommandResult<Vec<postal_core::ParticipantChange>> {
    let service = member_action_service(&state, account.as_deref(), &chat, &jids, "remove").await?;
    service
        .remove_group_participants(&chat, &jids)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Gives participants admin rights.
#[tauri::command]
pub(crate) async fn promote_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
    account: Option<String>,
) -> CommandResult<Vec<postal_core::ParticipantChange>> {
    let service = member_action_service(&state, account.as_deref(), &chat, &jids, "promote").await?;
    service
        .promote_group_participants(&chat, &jids)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Takes admin rights back.
#[tauri::command]
pub(crate) async fn demote_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
    account: Option<String>,
) -> CommandResult<Vec<postal_core::ParticipantChange>> {
    let service = member_action_service(&state, account.as_deref(), &chat, &jids, "demote").await?;
    service
        .demote_group_participants(&chat, &jids)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

async fn member_action_service(state: &AppState, account: Option<&str>, chat: &str, jids: &[String], action: &str)
    -> CommandResult<std::sync::Arc<postal_core::WhatsAppService>> {
    let service = match account { Some(account) => state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?, None => state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))? };
    if let Some(account) = account {
        for jid in jids {
            service.member_moderation_preflight(chat, jid, action, || {
                anyhow::ensure!(state.account_service(account).is_ok_and(|current| std::sync::Arc::ptr_eq(&service, &current)),
                    postal_core::message_ref::MessageRef::new("error.account_changed"));
                Ok(())
            }).await.map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
        }
    }
    Ok(service)
}

/// Sets whether members, or only admins, may add people.
#[tauri::command]
pub(crate) async fn set_members_can_add(
    state: State<'_, AppState>,
    chat: String,
    allow: bool,
) -> CommandResult<()> {
    let service = state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .set_members_can_add(&chat, allow)
        .await
        .map_err(|e| { service.note_error(&e); CommandError::from(e) })
}

/// Leaves a group.
#[tauri::command]
pub(crate) async fn leave_group(state: State<'_, AppState>, chat: String) -> CommandResult<()> {
    state.service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?.leave_group(&chat).await.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn set_member_label(
    state: State<'_, AppState>,
    chat: String,
    label: String,
) -> CommandResult<()> {
    state
        .service().map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?
        .set_member_label(&chat, label.trim())
        .await
        .map_err(CommandError::from)
}

/// The group behind an invite link, without joining it.
#[tauri::command]
pub(crate) async fn invite_info(state: State<'_, AppState>, account: String, link: String) -> CommandResult<postal_core::InviteInfo> {
    state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.invite_info(&link).await.map_err(CommandError::from)
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct Joined {
    pub(crate) jid: String,
    /// An admin still has to approve the request.
    pub(crate) pending: bool,
}

#[tauri::command]
pub(crate) async fn join_invite(state: State<'_, AppState>, account: String, link: String) -> CommandResult<Joined> {
    let (jid, pending) = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?.join_invite(&link).await.map_err(CommandError::from)?;
    Ok(Joined { jid, pending })
}
