use std::sync::Arc;
use tauri::State;
use crate::{AppState, connection::command_error};

#[tauri::command]
pub(crate) async fn blocked_contacts(state: State<'_, AppState>, account: String)
    -> Result<Vec<postal_core::service::BlockedContact>, String> {
    let service = state.account_service(&account)?;
    let contacts = service.blocked_contacts().await.map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed before operation".into()); }
    Ok(contacts)
}

#[tauri::command]
pub(crate) async fn set_contact_blocked(state: State<'_, AppState>, account: String, jid: String, blocked: bool) -> Result<(), String> {
    let service = state.account_service(&account)?;
    service.set_contact_blocked(&jid, blocked, || {
        state.account_service(&account).is_ok_and(|current| Arc::ptr_eq(&service, &current))
    }).await.map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed before operation".into()); }
    Ok(())
}
