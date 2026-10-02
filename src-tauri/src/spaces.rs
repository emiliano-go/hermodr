use std::sync::Arc;
use tauri::State;
use crate::AppState;

fn current(state: &AppState, account: &str, expected: &Arc<postal_core::WhatsAppService>) -> Result<(), String> {
    let live = state.account_service(account)?;
    if Arc::ptr_eq(expected, &live) { Ok(()) } else { Err("Account changed during Space operation.".into()) }
}

#[tauri::command]
pub(crate) async fn spaces_snapshot(state: State<'_, AppState>, account_id: String) -> Result<postal_core::SpaceSnapshot, String> {
    let service = state.account_service(&account_id)?;
    let result = service.spaces_snapshot().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn spaces_action(state: State<'_, AppState>, account_id: String, action: postal_core::SpaceAction) -> Result<postal_core::SpaceSnapshot, String> {
    let service = state.account_service(&account_id)?;
    let result = service.spaces_action(action).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn resolve_spaces(state: State<'_, AppState>, account_id: String, selection: postal_core::SpaceSelection,
    keyword_counts: Option<std::collections::HashMap<String, u32>>) -> Result<postal_core::SpaceResolution, String> {
    let service = state.account_service(&account_id)?;
    let result = service.resolve_spaces_with_keywords(selection, keyword_counts.unwrap_or_default()).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn space_group_catalog(state: State<'_, AppState>, account_id: String) -> Result<Vec<postal_core::CachedSpaceGroup>, String> {
    let service = state.account_service(&account_id)?;
    let result = service.space_group_catalog().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn export_space_metadata(state: State<'_, AppState>, account_id: String) -> Result<String, String> {
    let service = state.account_service(&account_id)?;
    let result = service.export_space_metadata().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn import_space_metadata(state: State<'_, AppState>, account_id: String, json: String) -> Result<postal_core::SpaceSnapshot, String> {
    let service = state.account_service(&account_id)?;
    let result = service.import_space_metadata(&json).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| error.to_string())
}
