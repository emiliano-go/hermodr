use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn group_join_requests(
    state: State<'_, AppState>, account: String, chat: String,
) -> Result<Vec<postal_core::GroupJoinRequest>, String> {
    let service = state.account_service(&account)?;
    service.group_join_requests(&chat).await.map_err(|error| command_error(&service, error))
}

#[tauri::command]
pub(crate) async fn change_group_join_requests(
    state: State<'_, AppState>, account: String, chat: String, jids: Vec<String>, approve: bool,
) -> Result<Vec<postal_core::ParticipantChange>, String> {
    let service = state.account_service(&account)?;
    service.change_group_join_requests(&chat, &jids, approve).await.map_err(|error| command_error(&service, error))
}
