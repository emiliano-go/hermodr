use crate::{AppState, command_error::{CommandError, CommandResult}};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn keyword_mentions(
    state: State<'_, AppState>,
    account_id: String,
    highlight: Vec<String>,
    hide: Vec<String>,
) -> CommandResult<std::collections::HashMap<String, i64>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let counts = service
        .keyword_mentions(&highlight, &hide)
        .await
        .map_err(CommandError::from)?;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(counts)
}

#[tauri::command]
pub(crate) async fn keyword_matches(
    state: State<'_, AppState>,
    account_id: String,
    chat: Option<String>,
    unread_only: bool,
    highlight: Vec<String>,
    hide: Vec<String>,
) -> CommandResult<Vec<postal_core::store::StoredMessage>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let rows = service
        .keyword_matches(chat.as_deref(), unread_only, &highlight, &hide)
        .await
        .map_err(CommandError::from)?;
    let current = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(rows)
}
