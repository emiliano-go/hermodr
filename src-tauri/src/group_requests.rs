use crate::command_error::{CommandError, CommandResult};
use tauri::State;
use crate::{AppState };

#[tauri::command]
pub(crate) async fn group_join_requests(
    state: State<'_, AppState>, account: String, chat: String,
) -> CommandResult<Vec<postal_core::GroupJoinRequest>> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.group_join_requests(&chat).await.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}

#[tauri::command]
pub(crate) async fn change_group_join_requests(
    state: State<'_, AppState>, account: String, chat: String, jids: Vec<String>, approve: bool,
) -> CommandResult<Vec<postal_core::ParticipantChange>> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    service.change_group_join_requests(&chat, &jids, approve).await.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}
