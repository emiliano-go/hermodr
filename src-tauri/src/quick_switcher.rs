use std::sync::Arc;
use tauri::State;
use crate::AppState;

#[tauri::command]
pub(crate) async fn switcher_catalog(state: State<'_, AppState>, account_id: String) -> Result<Vec<postal_core::SearchResult>, String> {
    let service = state.account_service(&account_id)?;
    let result = service.switcher_catalog().await.map_err(|error| error.to_string())?;
    if !Arc::ptr_eq(&service, &state.account_service(&account_id)?) { return Err("account changed during operation".into()); }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn switcher_messages(state: State<'_, AppState>, account_id: String, query: String, limit: Option<u32>) -> Result<Vec<postal_core::store::StoredMessage>, String> {
    let service = state.account_service(&account_id)?;
    let result = service.switcher_messages(&query, limit.unwrap_or(50)).await.map_err(|error| error.to_string())?;
    if !Arc::ptr_eq(&service, &state.account_service(&account_id)?) { return Err("account changed during operation".into()); }
    Ok(result)
}
