use tauri::State;
use crate::AppState;

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
