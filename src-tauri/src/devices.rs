use std::sync::Arc;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[tauri::command]
pub(crate) async fn linked_devices(state: State<'_, AppState>, account: String)
    -> CommandResult<Vec<postal_core::service::LinkedDevice>> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    let devices = service.linked_devices().await.map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let current = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    Ok(devices)
}

#[tauri::command]
pub(crate) async fn unlink_device(state: State<'_, AppState>, account: String, jid: String) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service.unlink_device(&jid, || {
        state.account_service(&account).is_ok_and(|current| Arc::ptr_eq(&service, &current))
    }).await.map_err(|error| { service.note_error(&error); CommandError::from(error) })
}
