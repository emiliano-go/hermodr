use crate::{AppState, connection::command_error};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn save_contact(
    state: State<'_, AppState>,
    account: String,
    jid: String,
    full_name: String,
    first_name: Option<String>,
    save_on_primary_addressbook: bool,
) -> Result<(), String> {
    let service = state.account_service(&account)?;
    service
        .save_contact(
            &jid,
            &full_name,
            first_name.as_deref(),
            save_on_primary_addressbook,
            || {
                state
                    .account_service(&account)
                    .is_ok_and(|current| Arc::ptr_eq(&service, &current))
            },
        )
        .await
        .map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) {
        return Err("account changed during contact edit".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_contact(
    state: State<'_, AppState>,
    account: String,
    jid: String,
) -> Result<(), String> {
    let service = state.account_service(&account)?;
    service
        .remove_contact(&jid, || {
            state
                .account_service(&account)
                .is_ok_and(|current| Arc::ptr_eq(&service, &current))
        })
        .await
        .map_err(|error| command_error(&service, error))?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) {
        return Err("account changed during contact edit".into());
    }
    Ok(())
}
