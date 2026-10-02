use std::sync::Arc;
use tauri::State;
use crate::AppState;

#[tauri::command]
pub(crate) async fn broadcast_list(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> Result<Option<postal_core::BroadcastList>, String> {
    let service = state.account_service(&account_id)?;
    let result = service.broadcast_list(&chat).await;
    let current = state.account_service(&account_id)?;
    if !Arc::ptr_eq(&service, &current) { return Err("account changed during operation".into()); }
    result.map_err(|error| error.to_string())
}
