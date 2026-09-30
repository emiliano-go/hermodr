use std::sync::Arc;
use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn linked_devices(state: State<'_, AppState>, account: String)
    -> Result<Vec<postal_core::service::LinkedDevice>, String> {
    let service = state.account_service(&account)?;
    let devices = service.linked_devices().await.map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed before operation".into()); }
    Ok(devices)
}

#[tauri::command]
pub(crate) async fn unlink_device(state: State<'_, AppState>, account: String, jid: String) -> Result<(), String> {
    let service = state.account_service(&account)?;
    service.unlink_device(&jid, || {
        state.account_service(&account).is_ok_and(|current| Arc::ptr_eq(&service, &current))
    }).await.map_err(|error| command_error(&service, error))
}
