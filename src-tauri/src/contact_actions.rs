use crate::{AppState, command_error::{CommandError, CommandResult}};
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
) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
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
        .map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let current = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_contact(
    state: State<'_, AppState>,
    account: String,
    jid: String,
) -> CommandResult<()> {
    let service = state.account_service(&account).map_err(|error| CommandError::code("error.not_connected").with_diagnostic(error))?;
    service
        .remove_contact(&jid, || {
            state
                .account_service(&account)
                .is_ok_and(|current| Arc::ptr_eq(&service, &current))
        })
        .await
        .map_err(|error| { service.note_error(&error); CommandError::from(error) })?;
    let current = state.account_service(&account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(())
}
