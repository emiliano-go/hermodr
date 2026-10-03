use std::sync::Arc;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

fn current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> CommandResult<()> {
    let live = state.account_service(account).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(expected, &live) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}

#[tauri::command]
pub(crate) async fn spaces_snapshot(state: State<'_, AppState>, account_id: String) -> CommandResult<postal_core::SpaceSnapshot> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.spaces_snapshot().await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn spaces_action(state: State<'_, AppState>, account_id: String, action: postal_core::SpaceAction) -> CommandResult<postal_core::SpaceSnapshot> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.spaces_action(action).await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn resolve_spaces(state: State<'_, AppState>, account_id: String, selection: postal_core::SpaceSelection,
    keyword_counts: Option<std::collections::HashMap<String, u32>>) -> CommandResult<postal_core::SpaceResolution> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.resolve_spaces_with_keywords(selection, keyword_counts.unwrap_or_default()).await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn space_group_catalog(state: State<'_, AppState>, account_id: String) -> CommandResult<Vec<postal_core::CachedSpaceGroup>> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.space_group_catalog().await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn export_space_metadata(state: State<'_, AppState>, account_id: String) -> CommandResult<String> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.export_space_metadata().await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn import_space_metadata(state: State<'_, AppState>, account_id: String, json: String) -> CommandResult<postal_core::SpaceSnapshot> {
    let service = state.account_service(&account_id).map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.import_space_metadata(&json).await;
    current(&state, &account_id, &service)?;
    result.map_err(Into::into)
}
