use tauri::State;
use crate::AppState;
use crate::connection::command_error;

/// Messages reported to a group's admins.
#[tauri::command]
pub(crate) async fn admin_reports(state: State<'_, AppState>, chat: String) -> Result<Vec<postal_core::AdminReport>, String> {
    state.service()?.admin_reports(&chat).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_allow_admin_reports(state: State<'_, AppState>, chat: String, allow: bool) -> Result<(), String> {
    state.service()?.set_allow_admin_reports(&chat, allow).await.map_err(|e| e.to_string())
}

/// Group members for mention autocomplete.
#[tauri::command]
pub(crate) async fn participants(
    state: State<'_, AppState>,
    chat: String,
) -> Result<Vec<postal_core::Participant>, String> {
    state
        .service()?
        .participants(&chat)
        .await
        .map_err(|e| e.to_string())
}

/// Group subject, description and members, for the info sidebar.
#[tauri::command]
pub(crate) async fn group_info(
    state: State<'_, AppState>,
    chat: String,
) -> Result<postal_core::GroupInfo, String> {
    state
        .service()?
        .group_info(&chat)
        .await
        .map_err(|e| e.to_string())
}

/// Community parents and subgroups among the account's groups, by JID.
#[tauri::command]
pub(crate) async fn group_kinds(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, postal_core::GroupKind>, String> {
    Ok(state.service()?.group_kinds().await)
}

/// Adds participants to a group.
#[tauri::command]
pub(crate) async fn add_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
) -> Result<Vec<postal_core::ParticipantChange>, String> {
    let service = state.service()?;
    service
        .add_group_participants(&chat, &jids)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Removes participants from a group; a community parent also removes them
/// from its subgroups.
#[tauri::command]
pub(crate) async fn remove_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
) -> Result<Vec<postal_core::ParticipantChange>, String> {
    let service = state.service()?;
    service
        .remove_group_participants(&chat, &jids)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Gives participants admin rights.
#[tauri::command]
pub(crate) async fn promote_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
) -> Result<Vec<postal_core::ParticipantChange>, String> {
    let service = state.service()?;
    service
        .promote_group_participants(&chat, &jids)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Takes admin rights back.
#[tauri::command]
pub(crate) async fn demote_group_participants(
    state: State<'_, AppState>,
    chat: String,
    jids: Vec<String>,
) -> Result<Vec<postal_core::ParticipantChange>, String> {
    let service = state.service()?;
    service
        .demote_group_participants(&chat, &jids)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Sets whether members, or only admins, may add people.
#[tauri::command]
pub(crate) async fn set_members_can_add(
    state: State<'_, AppState>,
    chat: String,
    allow: bool,
) -> Result<(), String> {
    let service = state.service()?;
    service
        .set_members_can_add(&chat, allow)
        .await
        .map_err(|e| command_error(&service, e))
}

/// Leaves a group.
#[tauri::command]
pub(crate) async fn leave_group(state: State<'_, AppState>, chat: String) -> Result<(), String> {
    state.service()?.leave_group(&chat).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_member_label(
    state: State<'_, AppState>,
    chat: String,
    label: String,
) -> Result<(), String> {
    state
        .service()?
        .set_member_label(&chat, label.trim())
        .await
        .map_err(|e| e.to_string())
}

/// The group behind an invite link, without joining it.
#[tauri::command]
pub(crate) async fn invite_info(state: State<'_, AppState>, link: String) -> Result<postal_core::InviteInfo, String> {
    state.service()?.invite_info(&link).await.map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub(crate) struct Joined {
    jid: String,
    /// An admin still has to approve the request.
    pending: bool,
}

#[tauri::command]
pub(crate) async fn join_invite(state: State<'_, AppState>, link: String) -> Result<Joined, String> {
    let (jid, pending) = state.service()?.join_invite(&link).await.map_err(|e| e.to_string())?;
    Ok(Joined { jid, pending })
}
