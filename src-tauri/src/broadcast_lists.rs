use std::sync::Arc;
use tauri::State;
use crate::AppState;
use crate::command_error::{CommandError, CommandResult};

#[tauri::command]
pub(crate) async fn broadcast_list(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> CommandResult<Option<postal_core::BroadcastList>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let result = service.broadcast_list(&chat).await;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    result.map_err(CommandError::from)
}
