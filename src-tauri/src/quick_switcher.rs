use std::sync::Arc;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[tauri::command]
pub(crate) async fn switcher_catalog(state: State<'_, AppState>, account_id: String) -> CommandResult<Vec<postal_core::SearchResult>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.switcher_catalog().await.map_err(CommandError::from)?;
    if !Arc::ptr_eq(&service, &state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?) { return Err(CommandError::code("error.account_changed")); }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn switcher_messages(state: State<'_, AppState>, account_id: String, query: String, limit: Option<u32>) -> CommandResult<Vec<postal_core::store::StoredMessage>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.switcher_messages(&query, limit.unwrap_or(50)).await.map_err(CommandError::from)?;
    if !Arc::ptr_eq(&service, &state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?) { return Err(CommandError::code("error.account_changed")); }
    Ok(result)
}
