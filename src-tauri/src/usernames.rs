use std::sync::Arc;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[tauri::command]
pub(crate) async fn lookup_username(
    state: State<'_, AppState>, account_id: String, username: String, username_key: Option<String>,
) -> CommandResult<postal_core::UsernameLookupResult> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let current = || crate::account_store::active_account(&state).as_deref() == Some(account_id.as_str())
        && state.account_service(&account_id).is_ok_and(|live| Arc::ptr_eq(&service, &live));
    if !current() { return Err(CommandError::code("error.account_changed")); }
    let result = service.lookup_username(&username, username_key.as_deref(), current).await;
    if !current() { return Err(CommandError::code("error.account_changed")); }
    result.map_err(Into::into)
}
